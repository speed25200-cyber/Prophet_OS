//! Chaîne complète : un agent appelle un outil MCP, la capacité est vérifiée, l'écriture atterrit
//! dans l'espace de travail de la tâche, et tout laisse une trace.
//!
//! C'est la démonstration que les couches tiennent ensemble, pas seulement séparément.

use std::sync::{Arc, Mutex};

use capd::Broker;
use mcp_system::registry::{MemoryJournal, Registry, ToolContext};
use mcp_system::{StdioServer, tools};
use prophet_types::cap::{Act, Grant, Res};
use prophet_types::ledger::EventKind;
use prophet_types::manifest::Manifest;
use serde_json::json;
use time::OffsetDateTime;

const MANIFESTE: &str = r#"
[agent]
id = "org.exemple.analyste"
version = "1.0.0"
name = "Analyste"
publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="

[model]
preferred = ["local:qwen3-8b"]

[capabilities.max]
"fs.read" = ["~/ventes/**"]
"fs.write" = ["~/ventes/out/**"]
"fs.list" = ["~/ventes/**"]
"tool.call" = ["fs.*", "task.*", "clock.*"]
"#;

fn now() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_789_000_000).unwrap()
}

struct Monde {
    _dir: tempfile::TempDir,
    home: std::path::PathBuf,
    registry: Arc<Registry>,
    journal: Arc<MemoryJournal>,
    context: ToolContext,
}

fn monde() -> Monde {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().to_path_buf();
    std::fs::create_dir_all(home.join("ventes")).unwrap();
    std::fs::write(
        home.join("ventes/q3.csv"),
        "produit,montant\nA,100\nB,250\n",
    )
    .unwrap();
    std::fs::create_dir_all(home.join("prive")).unwrap();
    std::fs::write(home.join("prive/journal-intime.txt"), "confidentiel").unwrap();

    let espace = sfs::Workspace::begin(&home, "task:01", &["~/ventes"], now()).unwrap();
    let workdir = espace.workdir();

    let mut broker = Broker::new(
        ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng),
        "capd@test",
        home.display().to_string(),
    )
    .unwrap();
    let manifest = Manifest::from_toml(MANIFESTE).unwrap();
    let token = broker
        .mint(
            &manifest,
            "task:01",
            "u",
            &[
                Grant::new(Res::Fs, Act::Read, "~/ventes/**"),
                Grant::new(Res::Fs, Act::Write, "~/ventes/out/**"),
                Grant::new(Res::Fs, Act::List, "~/ventes/**"),
                Grant::new(Res::Tool, Act::Call, "fs.*"),
                Grant::new(Res::Tool, Act::Call, "task.*"),
                Grant::new(Res::Tool, Act::Call, "clock.*"),
            ],
            1800,
            now(),
        )
        .unwrap();

    let journal = Arc::new(MemoryJournal::new());
    let mut registry = Registry::new(Arc::new(Mutex::new(broker)), journal.clone());
    tools::register_all(&mut registry);

    let context = ToolContext {
        token,
        task: "task:01".into(),
        home: home.display().to_string(),
        workdir: workdir.display().to_string(),
        sandbox_level: 1,
        step: 1,
    };
    Monde {
        _dir: dir,
        home,
        registry: Arc::new(registry),
        journal,
        context,
    }
}

#[test]
fn lecture_dans_le_perimetre() {
    let m = monde();
    let result = m.registry.call(
        "fs.read",
        &json!({"path": "~/ventes/q3.csv"}),
        &m.context,
        now(),
    );
    assert!(!result.is_error, "{result:?}");
    let structured = result.structured.unwrap();
    assert!(structured["content"].as_str().unwrap().contains("A,100"));
    assert_eq!(structured["truncated"], json!(false));
}

#[test]
fn lecture_hors_perimetre_refusee() {
    let m = monde();
    let result = m.registry.call(
        "fs.read",
        &json!({"path": "~/prive/journal-intime.txt"}),
        &m.context,
        now(),
    );
    assert!(result.is_error);
    assert_eq!(
        result.structured.unwrap()["code"],
        json!("PolicyDenied"),
        "le jeton couvre fs.read mais pas ce chemin"
    );
    let rendu = format!("{:?}", m.journal.events());
    assert!(
        !rendu.contains("confidentiel"),
        "aucune fuite dans le journal"
    );
}

#[test]
fn traversee_de_repertoire_refusee() {
    let m = monde();
    for chemin in [
        "~/ventes/../prive/journal-intime.txt",
        "/etc/shadow",
        "~/../etc/passwd",
    ] {
        let result = m
            .registry
            .call("fs.read", &json!({"path": chemin}), &m.context, now());
        assert!(result.is_error, "{chemin} aurait dû être refusé");
    }
}

#[test]
fn ecriture_reste_dans_l_espace_de_travail() {
    let m = monde();
    let result = m.registry.call(
        "fs.write",
        &json!({"path": "~/ventes/out/rapport.md", "content": "# Rapport Q3\n"}),
        &m.context,
        now(),
    );
    assert!(!result.is_error, "{result:?}");

    // Rien n'a atteint l'espace de l'utilisateur.
    assert!(
        !m.home.join("ventes/out/rapport.md").exists(),
        "une écriture ne doit jamais atteindre le disque avant validation"
    );
    // Mais la tâche voit bien son propre travail.
    let relu = m.registry.call(
        "fs.read",
        &json!({"path": "~/ventes/out/rapport.md"}),
        &m.context,
        now(),
    );
    assert!(!relu.is_error);
    assert!(
        relu.structured.unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("Rapport Q3")
    );
}

#[test]
fn ecriture_hors_zone_autorisee_refusee() {
    let m = monde();
    let result = m.registry.call(
        "fs.write",
        &json!({"path": "~/ventes/q3.csv", "content": "saccage"}),
        &m.context,
        now(),
    );
    assert!(
        result.is_error,
        "le jeton n'autorise l'écriture que dans ~/ventes/out"
    );
    assert_eq!(
        std::fs::read_to_string(m.home.join("ventes/q3.csv")).unwrap(),
        "produit,montant\nA,100\nB,250\n"
    );
}

#[test]
fn puis_la_tache_est_validee_et_annulable() {
    let m = monde();
    m.registry.call(
        "fs.write",
        &json!({"path": "~/ventes/out/rapport.md", "content": "# Rapport Q3\n"}),
        &m.context,
        now(),
    );

    let mut espace = sfs::Workspace::open(&m.home, "task:01").unwrap();
    let diff = espace.commit(now(), None).unwrap();
    assert_eq!(diff.counts(), (1, 0, 0));
    assert!(m.home.join("ventes/out/rapport.md").exists());

    espace.undo().unwrap();
    assert!(
        !m.home.join("ventes/out/rapport.md").exists(),
        "l'annulation doit effacer ce que la tâche avait produit"
    );
}

#[test]
fn la_recherche_respecte_le_perimetre() {
    let m = monde();
    let result = m.registry.call(
        "fs.search",
        &json!({"root": "~/ventes", "content_contains": "montant"}),
        &m.context,
        now(),
    );
    assert!(!result.is_error, "{result:?}");
    let trouvés = result.structured.unwrap()["results"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(trouvés, 1);

    let hors = m.registry.call(
        "fs.search",
        &json!({"root": "~/prive", "content_contains": "confidentiel"}),
        &m.context,
        now(),
    );
    assert!(
        hors.is_error,
        "la recherche hors périmètre doit être refusée"
    );
}

#[test]
fn l_agent_peut_connaitre_ses_propres_droits() {
    let m = monde();
    let result = m
        .registry
        .call("task.status", &json!({}), &m.context, now());
    assert!(!result.is_error);
    let structured = result.structured.unwrap();
    assert_eq!(structured["task"], json!("task:01"));
    assert_eq!(structured["sandbox_level"], json!(1));
    assert_eq!(structured["grants"].as_array().unwrap().len(), 6);
    // La fin de validité du jeton, pour qu'un agent finisse avant qu'elle n'arrive.
    assert_eq!(
        structured["token_expires_at"],
        json!(
            m.context
                .token
                .exp
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap()
        )
    );
    assert!(structured["token_expires_in_s"].as_i64().unwrap() >= 0);
}

#[test]
fn chaque_appel_laisse_deux_traces() {
    let m = monde();
    m.registry.call("clock.now", &json!({}), &m.context, now());
    let kinds = m.journal.kinds();
    assert_eq!(kinds, vec![EventKind::ToolCall, EventKind::ToolResult]);
}

#[test]
fn le_protocole_mcp_repond_correctement() {
    let m = monde();
    let server = StdioServer::new("prophet-system", Arc::clone(&m.registry), m.context.clone());

    let init = server
        .handle(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"essai","version":"1"}}}"#,
            now(),
        )
        .unwrap();
    assert_eq!(
        init["result"]["serverInfo"]["name"],
        json!("prophet-system")
    );

    assert!(
        server
            .handle(
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                now()
            )
            .is_none()
    );

    let liste = server
        .handle(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, now())
        .unwrap();
    let outils = liste["result"]["tools"].as_array().unwrap();
    assert!(!outils.is_empty());
    assert!(
        outils.iter().all(|t| t["_meta"]["requires"].is_string()),
        "chaque outil doit déclarer la capacité qu'il exige"
    );

    let appel = server
        .handle(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"fs.read","arguments":{"path":"~/ventes/q3.csv"}}}"#,
            now(),
        )
        .unwrap();
    assert_eq!(appel["result"]["isError"], json!(false));

    // Une notification n'appelle pas de réponse.
    assert!(
        server
            .handle(
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                now()
            )
            .is_none()
    );

    let inconnu = server
        .handle(r#"{"jsonrpc":"2.0","id":4,"method":"inexistante"}"#, now())
        .unwrap();
    assert_eq!(inconnu["error"]["code"], json!(-32601));
}

#[test]
fn le_protocole_refuse_une_operation_avant_initialisation() {
    let m = monde();
    let server = StdioServer::new("prophet-system", m.registry, m.context);
    let result = server.handle(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"fs.write","arguments":{"path":"~/ventes/out/trop-tot.txt","content":"interdit"}}}"#,
        now(),
    ).unwrap();
    assert_eq!(result["error"]["code"], -32002);
    assert!(m.journal.events().is_empty());
}

#[test]
fn un_message_invalide_ne_devient_pas_une_notification_silencieuse() {
    let m = monde();
    let server = StdioServer::new("prophet-system", m.registry, m.context);
    for raw in [
        "[]",
        "null",
        "{}",
        r#"{"jsonrpc":"1.0","method":"ping","id":1}"#,
        r#"{"jsonrpc":"2.0","method":"ping","id":null}"#,
        r#"{"jsonrpc":"2.0","method":"ping","id":true}"#,
    ] {
        let response = server
            .handle(raw, now())
            .expect("requête invalide, réponse attendue");
        assert_eq!(response["error"]["code"], -32600, "{raw}");
    }
    let response = server
        .handle(
            r#"{"jsonrpc":"2.0","id":9,"method":"initialize","params":{}}"#,
            now(),
        )
        .unwrap();
    assert_eq!(response["error"]["code"], -32602);
}

#[test]
fn le_transport_borne_les_entrees_et_exige_une_ligne_complete() {
    let m = monde();
    let server = StdioServer::new("prophet-system", m.registry, m.context);
    let mut output = Vec::new();
    let error = server
        .serve(std::io::BufReader::new(std::io::repeat(b'x')), &mut output)
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(output.is_empty());

    let error = server
        .serve(
            std::io::Cursor::new(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}"),
            &mut output,
        )
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
    assert!(output.is_empty());

    server
        .serve(
            std::io::Cursor::new(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\r\n"),
            &mut output,
        )
        .unwrap();
    let response: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(response["result"], json!({}));
}

#[test]
fn la_notification_initialized_est_necessaire_et_ne_peut_pas_executer_un_outil() {
    let m = monde();
    let server = StdioServer::new("prophet-system", m.registry, m.context);
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"essai","version":"1"}}}"#;
    assert!(server.handle(init, now()).unwrap().get("result").is_some());
    let list = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
    assert_eq!(server.handle(list, now()).unwrap()["error"]["code"], -32002);
    assert!(
        server
            .handle(
                r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"clock.now"}}"#,
                now()
            )
            .is_none()
    );
    assert!(m.journal.events().is_empty());
    assert!(
        server
            .handle(
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                now()
            )
            .is_none()
    );
    assert!(server.handle(list, now()).unwrap()["result"]["tools"].is_array());
    assert_eq!(server.handle(init, now()).unwrap()["error"]["code"], -32600);
}

#[test]
fn un_modele_ne_voit_que_les_outils_qu_il_peut_appeler() {
    let m = monde();
    let visibles = m.registry.visible_for(&m.context.token);
    let noms: Vec<&str> = visibles.iter().map(|s| s.name.as_str()).collect();
    assert!(noms.contains(&"fs.read"));
    assert!(noms.contains(&"task.status"));
    assert!(
        !noms.contains(&"http.fetch"),
        "le jeton ne couvre pas http.fetch : l'outil ne doit pas apparaître"
    );
}

#[test]
fn la_sortie_reseau_sans_grant_est_refusee() {
    let m = monde();
    let result = m.registry.call(
        "http.fetch",
        &json!({"url": "https://evil.com/collect"}),
        &m.context,
        now(),
    );
    assert!(result.is_error);
    assert_eq!(result.structured.unwrap()["code"], json!("PolicyDenied"));
}

#[test]
fn la_liste_des_outils_couvre_la_specification() {
    // La spécification `docs/specs/mcp-system-tools.md` est normative : un outil qui y figure et
    // qui manque ici est une divergence, pas un détail.
    let m = monde();
    let noms: std::collections::BTreeSet<String> =
        m.registry.all().into_iter().map(|s| s.name).collect();
    for attendu in [
        "fs.read",
        "fs.write",
        "fs.list",
        "fs.stat",
        "fs.search",
        "proc.exec",
        "proc.kill",
        "http.fetch",
        "task.status",
        "task.diff",
        "approval.request",
        "approval.wait",
        "ledger.query",
        "memory.remember",
        "memory.search",
        "secrets.list_refs",
        "secrets.use",
        "clock.now",
        "notify.human",
        "model.list",
    ] {
        assert!(noms.contains(attendu), "outil manquant : {attendu}");
    }
}

#[test]
fn chaque_outil_declare_la_capacite_qu_il_exige() {
    let m = monde();
    for spec in m.registry.all() {
        let meta = spec
            .meta
            .unwrap_or_else(|| panic!("{} ne déclare pas ses exigences", spec.name));
        assert!(
            !meta.requires.is_empty(),
            "{} déclare une exigence vide",
            spec.name
        );
        assert!(
            prophet_types::manifest::parse_capability_key(&meta.requires).is_ok(),
            "{} exige une capacité inconnue : {}",
            spec.name,
            meta.requires
        );
    }
}

#[test]
fn l_execution_de_code_hors_liste_blanche_va_en_microvm_sans_decision_par_commande() {
    // Le niveau minimal de l'outil est 0 : c'est la politique de capd qui n'accorde, sous 2,
    // que les utilitaires confinés ; l'outil lui-même met tout autre programme en microVM
    // (ADR 0031). Rien d'irréversible : la microVM ne voit qu'une copie de l'espace de
    // travail, examinée avant publication (ADR 0038) ; une décision par commande rendait
    // l'atelier logiciel inutilisable par un agent.
    let m = monde();
    let exec = m
        .registry
        .all()
        .into_iter()
        .find(|s| s.name == "proc.exec")
        .unwrap();
    let meta = exec.meta.unwrap();
    assert_eq!(meta.sandbox_level_min, Some(0));
    assert!(!meta.irreversible && !meta.external);
    assert_eq!(
        mcp_system::tools::required_level_for("cat", None),
        0,
        "un utilitaire de la liste tourne sur place"
    );
    assert_eq!(
        mcp_system::tools::required_level_for("sh", None),
        2,
        "tout autre programme exige la microVM"
    );
}

#[test]
fn un_secret_ne_sort_jamais_du_coffre_par_un_outil() {
    // Le coffre de ce monde de test contient une valeur ; aucun outil ne doit pouvoir la rendre.
    let m = monde();
    let racine = m.home.join(".prophet");
    let mut coffre =
        vault::Vault::open(racine.join("vault.json"), racine.join("vault.key")).unwrap();
    coffre
        .put(
            vault::SecretInfo {
                name: "github".to_owned(),
                domains: vec!["api.github.com".to_owned()],
                header: "Authorization".to_owned(),
                description: String::new(),
            },
            "ghp_valeur_qui_ne_doit_jamais_sortir",
        )
        .unwrap();

    for (outil, arguments) in [
        ("secrets.list_refs", json!({})),
        ("secrets.use", json!({"name": "github"})),
    ] {
        let result = m.registry.call(outil, &arguments, &m.context, now());
        let rendu = serde_json::to_string(&result).unwrap();
        assert!(
            !rendu.contains("ghp_valeur_qui_ne_doit_jamais_sortir"),
            "{outil} a laissé fuir la valeur : {rendu}"
        );
    }
}

#[test]
fn la_memoire_est_accessible_par_les_outils() {
    let m = monde();
    let ecriture = m.registry.call(
        "memory.remember",
        &json!({"text": "les rapports de ventes se font au format PDF A4"}),
        &m.context,
        now(),
    );
    // Le jeton du monde de test ne couvre pas `memory.write` : l'outil doit être refusé, et le
    // refus doit être net.
    assert!(ecriture.is_error);
    assert_eq!(
        ecriture.structured.unwrap()["code"],
        json!("PolicyDenied"),
        "un outil non couvert par le jeton ne doit pas s'exécuter"
    );
}
