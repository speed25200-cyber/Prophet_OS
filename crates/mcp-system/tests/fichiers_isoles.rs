//! Régressions exercées sur des fichiers temporaires, sans données de l'utilisateur.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use capd::Broker;
use mcp_system::protocol::CallResult;
use mcp_system::registry::{MemoryJournal, Registry, ToolContext};
use prophet_types::cap::{Act, Grant, Res};
use prophet_types::manifest::Manifest;
use serde_json::{Value, json};
use time::OffsetDateTime;

struct FailingResultJournal;
impl mcp_system::registry::Journal for FailingResultJournal {
    fn record(&self, draft: prophet_types::ledger::Draft) -> Result<(), String> {
        if draft.kind == prophet_types::ledger::EventKind::ToolResult {
            Err("confirmation indisponible".into())
        } else {
            Ok(())
        }
    }
}

#[test]
fn une_panne_apres_ecriture_interdit_toute_repetition_automatique() {
    let m = monde(&["~/docs/**"]);
    let mut registry = Registry::new(m.broker, Arc::new(FailingResultJournal));
    registry.register(Arc::new(mcp_system::tools::Write));
    let first = registry.call(
        "fs.write",
        &json!({"path":"~/docs/note.txt","content":"première action"}),
        &m.context,
        OffsetDateTime::now_utc(),
    );
    assert!(first.is_error);
    assert_eq!(
        std::fs::read_to_string(m.work.join("docs/note.txt")).unwrap(),
        "première action"
    );
    let second = registry.call(
        "fs.write",
        &json!({"path":"~/docs/note.txt","content":"répétition interdite"}),
        &m.context,
        OffsetDateTime::now_utc(),
    );
    assert!(second.is_error);
    assert_eq!(
        std::fs::read_to_string(m.work.join("docs/note.txt")).unwrap(),
        "première action"
    );
}

struct Monde {
    _dir: tempfile::TempDir,
    home: PathBuf,
    work: PathBuf,
    outside: PathBuf,
    context: ToolContext,
    registry: Arc<Registry>,
    broker: Arc<Mutex<Broker>>,
    journal: Arc<MemoryJournal>,
}

fn monde(read: &[&str]) -> Monde {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let work = home.join(".prophet/tasks/isolated/work");
    let outside = dir.path().join("outside");
    for p in [&work, &outside, &home.join("docs")] {
        std::fs::create_dir_all(p).unwrap();
    }
    std::fs::write(outside.join("secret.txt"), "NE-PAS-DIVULGUER").unwrap();
    let manifest = Manifest::from_toml(
        r#"
[agent]
id = "org.prophet.fs-test"
version = "1.0.0"
name = "Test fichiers"
publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
[model]
preferred = ["local:test"]
[capabilities.max]
"fs.read" = ["~/**"]
"fs.write" = ["~/**"]
"fs.list" = ["~/**"]
"tool.call" = ["fs.*"]
"#,
    )
    .unwrap();
    let mut broker = Broker::new(
        ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng),
        "capd@test",
        home.display().to_string(),
    )
    .unwrap();
    let mut grants = vec![
        Grant::new(Res::Tool, Act::Call, "fs.*"),
        Grant::new(Res::Fs, Act::Write, "~/docs/**"),
        Grant::new(Res::Fs, Act::List, "~/docs/**"),
    ];
    grants.extend(read.iter().map(|p| Grant::new(Res::Fs, Act::Read, *p)));
    let token = broker
        .mint(
            &manifest,
            "isolated",
            "u",
            &grants,
            3600,
            OffsetDateTime::now_utc(),
        )
        .unwrap();
    let broker = Arc::new(Mutex::new(broker));
    let journal = Arc::new(MemoryJournal::new());
    let mut registry = Registry::new(broker.clone(), journal.clone());
    mcp_system::tools::register_all(&mut registry);
    let context = ToolContext {
        token,
        task: "isolated".into(),
        home: home.display().to_string(),
        workdir: work.display().to_string(),
        sandbox_level: 1,
        step: 1,
    };
    Monde {
        _dir: dir,
        home,
        work,
        outside,
        context,
        registry: Arc::new(registry),
        broker,
        journal,
    }
}

impl Monde {
    fn call(&self, tool: &str, args: Value) -> CallResult {
        self.registry
            .call(tool, &args, &self.context, OffsetDateTime::now_utc())
    }
}

#[test]
fn un_lien_symbolique_final_ou_ancetre_ne_divulgue_rien() {
    let m = monde(&["~/docs/**"]);
    std::fs::create_dir_all(m.work.join("docs")).unwrap();
    std::os::unix::fs::symlink(m.outside.join("secret.txt"), m.work.join("docs/lien.txt")).unwrap();
    std::os::unix::fs::symlink(&m.outside, m.work.join("docs/dossier")).unwrap();
    for path in ["~/docs/lien.txt", "~/docs/dossier/secret.txt"] {
        for tool in ["fs.read", "fs.stat"] {
            let r = m.call(tool, json!({"path":path}));
            assert!(r.is_error, "{tool} a suivi {path} : {r:?}");
            assert!(!format!("{r:?}").contains("NE-PAS-DIVULGUER"));
        }
    }
}

#[test]
fn un_lien_de_travail_invalide_ne_declenche_pas_un_repli_vers_le_home() {
    let m = monde(&["~/docs/**"]);
    std::fs::create_dir_all(m.work.join("docs")).unwrap();
    std::fs::write(m.home.join("docs/note.txt"), "VERSION-PRIVEE").unwrap();
    std::os::unix::fs::symlink(m.outside.join("absent"), m.work.join("docs/note.txt")).unwrap();
    let r = m.call("fs.read", json!({"path":"~/docs/note.txt"}));
    assert!(
        r.is_error,
        "le lien cassé ne doit pas révéler la version réelle : {r:?}"
    );
}

#[test]
fn ecrire_ne_modifie_ni_lien_symbolique_ni_lien_physique_externe() {
    let m = monde(&["~/docs/**"]);
    std::fs::create_dir_all(m.work.join("docs")).unwrap();
    std::os::unix::fs::symlink(&m.outside, m.work.join("docs/echappe")).unwrap();
    let r = m.call(
        "fs.write",
        json!({"path":"~/docs/echappe/secret.txt","content":"MODIFIE"}),
    );
    assert!(r.is_error);
    std::fs::hard_link(m.outside.join("secret.txt"), m.work.join("docs/dur.txt")).unwrap();
    let _ = m.call(
        "fs.write",
        json!({"path":"~/docs/dur.txt","content":"NOUVEAU"}),
    );
    assert_eq!(
        std::fs::read_to_string(m.outside.join("secret.txt")).unwrap(),
        "NE-PAS-DIVULGUER"
    );
}

#[test]
fn la_recherche_verifie_chaque_descendant_et_rend_des_chemins_logiques() {
    let m = monde(&["~/docs", "~/docs/public.txt"]);
    std::fs::write(m.home.join("docs/public.txt"), "visible").unwrap();
    std::fs::write(m.home.join("docs/prive.txt"), "secret").unwrap();
    let r = m.call("fs.search", json!({"root":"~/docs"}));
    assert!(!r.is_error, "{r:?}");
    let data = r.structured.unwrap();
    let results = data["results"].as_array().unwrap();
    assert_eq!(results.len(), 1, "{data}");
    assert_eq!(results[0]["path"], json!(m.home.join("docs/public.txt")));
}

#[test]
fn un_plafond_fourni_par_le_modele_ne_supprime_pas_la_borne_de_lecture() {
    let m = monde(&["~/docs/**"]);
    std::fs::write(m.home.join("docs/grand.txt"), vec![b'a'; 600_000]).unwrap();
    let r = m.call(
        "fs.read",
        json!({"path":"~/docs/grand.txt","max_bytes":u64::MAX}),
    );
    assert!(!r.is_error, "{r:?}");
    let d = r.structured.unwrap();
    assert!(d["content"].as_str().unwrap().len() <= 256 * 1024);
    assert_eq!(d["total_bytes"], 600_000);
    assert_eq!(d["truncated"], true);
}

#[test]
fn un_gros_fichier_se_lit_par_morceaux_sans_couper_un_caractere() {
    // Un agent dont la fenêtre ne tient pas 256 Kio lit la suite là où la lecture s'est
    // arrêtée : `next_offset` le dit, et aucun morceau ne commence ni ne finit au milieu d'un
    // caractère, même quand le plafond tombe dedans.
    let m = monde(&["~/docs/**"]);
    let texte = "Été à Noël, ça dure. ".repeat(400);
    std::fs::write(m.home.join("docs/long.txt"), &texte).unwrap();
    let mut relu = String::new();
    let mut offset = 0u64;
    let mut morceaux = 0;
    loop {
        let r = m.call(
            "fs.read",
            json!({"path":"~/docs/long.txt","offset":offset,"max_bytes":1001}),
        );
        assert!(!r.is_error, "{r:?}");
        let d = r.structured.unwrap();
        assert_eq!(d["total_bytes"], texte.len());
        assert_eq!(d["offset"], offset);
        let morceau = d["content"].as_str().unwrap();
        assert!(morceau.len() <= 1001);
        assert!(
            !morceau.contains('\u{fffd}'),
            "caractère coupé : {morceau:?}"
        );
        relu.push_str(morceau);
        morceaux += 1;
        if d["truncated"] == false {
            assert!(d.get("next_offset").is_none() || d["next_offset"].is_null());
            break;
        }
        let suivant = d["next_offset"].as_u64().unwrap();
        assert!(suivant > offset);
        offset = suivant;
        assert!(morceaux < 100);
    }
    assert_eq!(relu, texte);
    assert!(morceaux > 5);
    // Un décalage au milieu d'un caractère reprend au caractère suivant, et le dit.
    let milieu = texte.find('É').unwrap() as u64 + 1;
    let d = m
        .call(
            "fs.read",
            json!({"path":"~/docs/long.txt","offset":milieu,"max_bytes":16}),
        )
        .structured
        .unwrap();
    assert_eq!(d["offset"], milieu + 1);
    assert!(d["content"].as_str().unwrap().starts_with("té à"));
    // Au-delà de la fin : rien, sans erreur.
    let d = m
        .call(
            "fs.read",
            json!({"path":"~/docs/long.txt","offset":texte.len() + 10}),
        )
        .structured
        .unwrap();
    assert_eq!(d["content"], "");
    assert_eq!(d["truncated"], false);
    // Un décalage qui n'est pas un entier est refusé.
    assert!(
        m.call("fs.read", json!({"path":"~/docs/long.txt","offset":-1}),)
            .is_error
    );
}

#[test]
fn la_liste_fusionne_les_entrees_du_home_et_les_ecritures_de_la_tache() {
    let m = monde(&["~/docs/**"]);
    std::fs::write(m.home.join("docs/original.txt"), "original").unwrap();
    let r = m.call(
        "fs.write",
        json!({"path":"~/docs/nouveau.txt","content":"nouveau"}),
    );
    assert!(!r.is_error, "{r:?}");
    let r = m.call("fs.list", json!({"path":"~/docs"}));
    assert!(!r.is_error, "{r:?}");
    let data = r.structured.unwrap();
    let names: Vec<_> = data["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["nouveau.txt", "original.txt"]);
    assert!(!m.home.join("docs/nouveau.txt").exists());
}

#[test]
fn les_metadonnees_et_la_recherche_de_la_version_de_travail_fonctionnent() {
    let m = monde(&["~/docs/**"]);
    assert!(
        !m.call(
            "fs.write",
            json!({"path":"~/docs/note.txt","content":"version de travail"})
        )
        .is_error
    );
    let stat = m.call("fs.stat", json!({"path":"~/docs/note.txt"}));
    assert!(!stat.is_error, "{stat:?}");
    assert_eq!(stat.structured.unwrap()["size"], 18);
    let search = m.call(
        "fs.search",
        json!({"root":"~/docs","content_contains":"travail"}),
    );
    assert!(!search.is_error, "{search:?}");
    let data = search.structured.unwrap();
    assert_eq!(
        data["results"][0]["path"],
        json!(m.home.join("docs/note.txt"))
    );
    assert!(!data.to_string().contains(".prophet/tasks"));
}

#[test]
fn liens_physiques_et_fichiers_speciaux_ne_sont_pas_lus() {
    let m = monde(&["~/docs/**"]);
    std::fs::hard_link(m.outside.join("secret.txt"), m.home.join("docs/dur.txt")).unwrap();
    rustix::fs::mknodat(
        rustix::fs::CWD,
        m.home.join("docs/tube"),
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        0,
    )
    .unwrap();
    for path in ["~/docs/dur.txt", "~/docs/tube"] {
        assert!(m.call("fs.read", json!({"path":path})).is_error);
    }
}

#[test]
fn le_contexte_ne_peut_pas_transformer_le_home_en_zone_d_ecriture() {
    let mut m = monde(&["~/docs/**"]);
    m.context.workdir = m.home.display().to_string();
    assert!(
        m.call(
            "fs.write",
            json!({"path":"~/docs/interdit","content":"non"})
        )
        .is_error
    );
    assert!(!m.home.join("docs/interdit").exists());
}

#[test]
fn une_recherche_partielle_ne_se_declare_pas_exhaustive() {
    let m = monde(&["~/docs/**"]);
    std::fs::write(m.home.join("docs/grand.txt"), vec![b'a'; 600_000]).unwrap();
    let r = m.call(
        "fs.search",
        json!({"root":"~/docs","content_contains":"absent"}),
    );
    assert!(!r.is_error, "{r:?}");
    assert_eq!(r.structured.unwrap()["truncated"], true);
}

#[test]
fn un_remplacement_concurrent_par_un_lien_ne_sort_pas_du_perimetre() {
    let m = monde(&["~/docs/**"]);
    std::fs::create_dir_all(m.work.join("docs/pivot")).unwrap();
    std::fs::write(m.work.join("docs/pivot/secret.txt"), "AUTORISE").unwrap();
    let work = m.work.clone();
    let outside = m.outside.clone();
    let thread = std::thread::spawn(move || {
        for _ in 0..300 {
            std::fs::rename(work.join("docs/pivot"), work.join("docs/sauve")).unwrap();
            std::os::unix::fs::symlink(&outside, work.join("docs/pivot")).unwrap();
            std::thread::yield_now();
            std::fs::remove_file(work.join("docs/pivot")).unwrap();
            std::fs::rename(work.join("docs/sauve"), work.join("docs/pivot")).unwrap();
        }
    });
    let mut leaked = false;
    for _ in 0..300 {
        let r = m.call("fs.read", json!({"path":"~/docs/pivot/secret.txt"}));
        leaked |= format!("{r:?}").contains("NE-PAS-DIVULGUER");
    }
    thread.join().unwrap();
    assert!(!leaked);
    assert_eq!(
        std::fs::read_to_string(m.outside.join("secret.txt")).unwrap(),
        "NE-PAS-DIVULGUER"
    );
}

#[test]
fn la_boucle_native_utilise_les_droits_et_reverifie_la_revocation() {
    use mcp_system::native::RegistryExecutor;
    use prophet_types::ledger::EventKind;
    use providers::native::ToolExecutor;

    let m = monde(&["~/docs/**"]);
    let executor = RegistryExecutor::new(m.registry.clone(), m.context.clone());
    assert!(executor.tools().iter().all(|t| t.name.starts_with("fs.")));
    let args = json!({"path":"~/docs/note.txt","content":"autorise"});
    assert!(executor.call("fs.write", &args).0);
    assert!(
        !executor
            .call("fs.write", &json!({"path":"~/interdit","content":"non"}))
            .0
    );
    m.broker.lock().unwrap().revoke("isolated");
    assert!(
        !executor
            .call(
                "fs.write",
                &json!({"path":"~/docs/note.txt","content":"revoque"})
            )
            .0
    );
    assert_eq!(
        std::fs::read_to_string(m.work.join("docs/note.txt")).unwrap(),
        "autorise"
    );
    assert!(!m.home.join("docs/note.txt").exists());
    assert!(!m.work.join("interdit").exists());
    let calls: Vec<_> = m
        .journal
        .events()
        .into_iter()
        .filter(|e| e.kind == EventKind::ToolCall)
        .collect();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls.iter().map(|e| e.step).collect::<Vec<_>>(),
        vec![Some(1), Some(2), Some(3)]
    );
}

#[test]
#[ignore = "needs_local_model: PROPHET_TEST_MODEL et PROPHET_TEST_ENDPOINT"]
fn un_modele_reel_ecrit_via_le_registre_et_produit_un_diff_sfs() {
    use mcp_system::native::RegistryExecutor;
    use prophet_types::driver::{DriverEvent, RunStatus};
    use providers::Driver;
    use providers::conformance::sample_request;
    use providers::local::LocalModel;
    use providers::native::NativeDriver;
    use std::time::{Duration, Instant};

    let endpoint = std::env::var("PROPHET_TEST_ENDPOINT").expect("adresse du moteur réel requise");
    let model = std::env::var("PROPHET_TEST_MODEL").expect("modèle réel requis");
    let m = monde(&["~/docs/**"]);
    let workspace =
        sfs::Workspace::begin(&m.home, "isolated", &["~/docs"], OffsetDateTime::now_utc()).unwrap();
    // Le registre expose uniquement l'outil nécessaire à cette tâche d'acceptation.
    let mut registry = Registry::new(m.broker.clone(), m.journal.clone());
    registry.register(Arc::new(mcp_system::tools::Write));
    let executor = RegistryExecutor::new(Arc::new(registry), m.context.clone());
    let client = LocalModel::new(&endpoint, &model, Duration::from_secs(120))
        .unwrap()
        .with_max_tokens(1024)
        .unwrap()
        .with_tools(executor.tools())
        .unwrap();
    assert!(client.models().unwrap().contains(&model));
    let mut driver = NativeDriver::new(Box::new(client), Box::new(executor));
    let nonce = format!("prophet-mcp-{}", rand::random::<u32>());
    let mut request = sample_request("prophet-agent");
    request.task = m.context.task.clone();
    request.workdir = m.context.workdir.clone();
    request.intent = format!(
        "Use fs.write to write exactly this text: {nonce} into ~/docs/note.txt. When the tool reports staged=true, answer Done. /no_think"
    );
    request.limits.max_steps = 5;
    let started = Instant::now();
    let run = driver.start(&request).unwrap().run;
    let mut events = Vec::new();
    for _ in 0..6 {
        let next = driver.poll(&run).unwrap();
        let terminal = next.iter().any(DriverEvent::is_terminal);
        events.extend(next);
        if terminal {
            break;
        }
    }
    assert!(
        events.iter().any(
            |e| matches!(e, DriverEvent::ToolResult { tool, ok:true, .. } if tool == "fs.write")
        ),
        "{events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            DriverEvent::Done {
                status: RunStatus::Ok,
                ..
            }
        )),
        "{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, DriverEvent::Usage { tokens_out:Some(n), .. } if *n > 0))
    );
    assert_eq!(
        std::fs::read_to_string(m.work.join("docs/note.txt")).unwrap(),
        nonce
    );
    assert!(!m.home.join("docs/note.txt").exists());
    let diff = workspace.diff().unwrap();
    assert_eq!(diff.counts(), (1, 0, 0));
    assert_eq!(diff.changes[0].path, PathBuf::from("docs/note.txt"));
    assert_eq!(diff.bytes_written(), nonce.len() as u64);
    println!(
        "moteur={model}; durée={:.2}s; événements={}; journal={}; diff={}",
        started.elapsed().as_secs_f32(),
        events.len(),
        m.journal.events().len(),
        diff.render()
    );
}
