//! Les approbations de bout en bout (ADR 0041) : une action refusée faute de décision humaine
//! est soumise à l'humain par le registre lui-même, le modèle attend la décision avec
//! `approval.wait`, l'humain tranche — ici par le même capd, comme le fait le panneau de la
//! surface —, et le même appel passe (ou reste refusé). Une décision « une fois » ne vaut que
//! pour la demande identique qui suit ; une décision de tâche vaut pour toute la tâche.

use std::sync::{Arc, Mutex};

use capd::{ApprovalDecision, ApprovalScope, Broker};
use mcp_system::protocol::{CallResult, ToolMeta, ToolSpec};
use mcp_system::registry::{MemoryJournal, Registry, Tool, ToolContext};
use prophet_types::cap::{Act, Grant, Res};
use prophet_types::manifest::Manifest;
use serde_json::{Value, json};
use time::OffsetDateTime;

/// Un outil engageant : irréversible et externe, comme un envoi hors de la machine.
#[derive(Debug)]
struct Envoi;

impl Tool for Envoi {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "essai.envoi".into(),
            description: "Envoie hors de la machine (essai)".into(),
            input_schema: json!({"type": "object"}),
            meta: Some(ToolMeta {
                requires: "tool.call".into(),
                irreversible: true,
                external: true,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, _args: &Value, _context: &ToolContext) -> Option<String> {
        None
    }

    fn call(&self, _args: &Value, _context: &ToolContext) -> CallResult {
        CallResult::text("envoyé")
    }
}

fn monde(dir: &std::path::Path) -> (ToolContext, Registry, Arc<Mutex<Broker>>) {
    let home = dir.join("home");
    std::fs::create_dir_all(home.join("docs")).unwrap();
    let manifest = Manifest::from_toml(
        r#"
[agent]
id = "org.prophet.approbations-test"
version = "1.0.0"
name = "Test approbations"
publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
[model]
preferred = ["local:test"]
[sandbox]
min_level = 0
[capabilities.max]
"fs.read" = ["~/docs/**"]
"tool.call" = ["essai.envoi", "approval.wait"]
"#,
    )
    .unwrap();
    let mut broker = Broker::new(
        ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng),
        "capd@test",
        home.display().to_string(),
    )
    .unwrap();
    let grants = vec![
        Grant::new(Res::Tool, Act::Call, "essai.envoi"),
        Grant::new(Res::Tool, Act::Call, "approval.wait"),
        Grant::new(Res::Fs, Act::Read, "~/docs/**"),
    ];
    let token = broker
        .mint(
            &manifest,
            "approb",
            "u",
            &grants,
            3600,
            OffsetDateTime::now_utc(),
        )
        .unwrap();
    let broker = Arc::new(Mutex::new(broker));
    let mut registry = Registry::new(broker.clone(), Arc::new(MemoryJournal::new()));
    mcp_system::tools::register_all(&mut registry);
    registry.register(Arc::new(Envoi));
    (
        ToolContext {
            token,
            task: "approb".into(),
            home: home.display().to_string(),
            workdir: dir.join("work").display().to_string(),
            sandbox_level: 0,
            step: 1,
        },
        registry,
        broker,
    )
}

fn appel(registry: &Registry, context: &ToolContext) -> CallResult {
    registry.call(
        "essai.envoi",
        &json!({}),
        context,
        OffsetDateTime::now_utc(),
    )
}

fn attendre(registry: &Registry, context: &ToolContext, id: &str, timeout_s: u64) -> Value {
    let r = registry.call(
        "approval.wait",
        &json!({"id": id, "timeout_s": timeout_s}),
        context,
        OffsetDateTime::now_utc(),
    );
    assert!(!r.is_error, "{r:?}");
    r.structured.unwrap()
}

#[test]
fn une_action_engageante_attend_l_humain_puis_passe_une_fois_accordee() {
    let dir = tempfile::tempdir().unwrap();
    let (context, registry, broker) = monde(dir.path());

    // Premier appel : refusé faute de décision, la demande est créée et son identifiant rendu.
    let r = appel(&registry, &context);
    assert!(r.is_error, "{r:?}");
    let structure = r.structured.clone().unwrap();
    assert_eq!(structure["code"], "ApprovalRequired", "{r:?}");
    let id = structure["approval"].as_str().unwrap().to_owned();
    assert_eq!(structure["summary"], "Appeler essai.envoi");
    assert_eq!(broker.lock().unwrap().approvals().pending().len(), 1);

    // Attendre : toujours en attente au bout d'une seconde.
    let etat = attendre(&registry, &context, &id, 1);
    assert_eq!(etat["state"], "pending", "{etat}");

    // L'humain tranche, une fois, sur un autre fil : l'attente le voit.
    let juge = broker.clone();
    let id_jugee = id.clone();
    let humain = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(800));
        juge.lock()
            .unwrap()
            .approvals_mut()
            .resolve(
                &id_jugee,
                ApprovalDecision::Allow,
                ApprovalScope::Once,
                OffsetDateTime::now_utc(),
            )
            .unwrap();
    });
    let etat = attendre(&registry, &context, &id, 10);
    assert_eq!(etat["state"], "allowed", "{etat}");
    humain.join().unwrap();

    // Le même appel passe, une fois ; le suivant repasse devant l'humain.
    let r = appel(&registry, &context);
    assert!(!r.is_error, "{r:?}");
    let r = appel(&registry, &context);
    assert!(r.is_error, "{r:?}");
    assert_eq!(r.structured.unwrap()["code"], "ApprovalRequired");

    // Une demande inconnue ne se laisse pas attendre.
    let r = registry.call(
        "approval.wait",
        &json!({"id": "inconnue"}),
        &context,
        OffsetDateTime::now_utc(),
    );
    assert!(r.is_error, "{r:?}");
}

#[test]
fn une_decision_de_tache_vaut_pour_toute_la_tache_et_un_refus_est_definitif() {
    let dir = tempfile::tempdir().unwrap();
    let (context, registry, broker) = monde(dir.path());
    let r = appel(&registry, &context);
    let id = r.structured.unwrap()["approval"]
        .as_str()
        .unwrap()
        .to_owned();
    broker
        .lock()
        .unwrap()
        .approvals_mut()
        .resolve(
            &id,
            ApprovalDecision::Allow,
            ApprovalScope::Task,
            OffsetDateTime::now_utc(),
        )
        .unwrap();
    // Toute la tâche : les appels suivants passent sans nouvelle demande.
    for _ in 0..3 {
        let r = appel(&registry, &context);
        assert!(!r.is_error, "{r:?}");
    }
    assert!(broker.lock().unwrap().approvals().pending().is_empty());

    // Un autre monde, un refus : l'appel est refusé, la demande consommée.
    let dir = tempfile::tempdir().unwrap();
    let (context, registry, broker) = monde(dir.path());
    let r = appel(&registry, &context);
    let id = r.structured.unwrap()["approval"]
        .as_str()
        .unwrap()
        .to_owned();
    broker
        .lock()
        .unwrap()
        .approvals_mut()
        .resolve(
            &id,
            ApprovalDecision::Deny,
            ApprovalScope::Once,
            OffsetDateTime::now_utc(),
        )
        .unwrap();
    let etat = attendre(&registry, &context, &id, 1);
    assert_eq!(etat["state"], "denied", "{etat}");
    let r = appel(&registry, &context);
    assert!(r.is_error, "{r:?}");
    assert_eq!(r.structured.unwrap()["code"], "PolicyDenied");
}
