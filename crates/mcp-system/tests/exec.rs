//! `proc.exec` : ce qu'une commande devient sous sandbox, sans en lancer aucune.
//!
//! La liste blanche tourne confinée sur place, tout le reste en microVM ; le home n'est lisible
//! que selon le jeton et jamais inscriptible ; l'espace de travail est le seul lieu d'écriture.
//! L'exécution réelle passe par sandboxd, exercée dans ses propres tests.

use std::sync::{Arc, Mutex};

use capd::Broker;
use mcp_system::registry::{MemoryJournal, Registry, ToolContext};
use mcp_system::tools::{Exec, is_safe_binary, required_level_for};
use prophet_types::cap::{Act, Grant, Res};
use prophet_types::manifest::Manifest;
use serde_json::json;
use time::OffsetDateTime;

fn contexte(dir: &std::path::Path) -> (ToolContext, Registry) {
    let home = dir.join("home");
    let work = home.join(".prophet/tasks/exec/work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(home.join("docs")).unwrap();
    let manifest = Manifest::from_toml(
        r#"
[agent]
id = "org.prophet.exec-test"
version = "1.0.0"
name = "Test commandes"
publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
[model]
preferred = ["local:test"]
[sandbox]
min_level = 0
code_execution = "microvm"
[capabilities.max]
"fs.read" = ["~/docs/**"]
"fs.write" = ["~/docs/**"]
"proc.exec" = ["cat", "wc", "sh"]
"tool.call" = ["proc.exec"]
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
        Grant::new(Res::Tool, Act::Call, "proc.exec"),
        Grant::new(Res::Fs, Act::Read, "~/docs/**"),
        Grant::new(Res::Fs, Act::Write, "~/docs/**"),
        Grant::new(Res::Proc, Act::Exec, "cat"),
        Grant::new(Res::Proc, Act::Exec, "wc"),
        Grant::new(Res::Proc, Act::Exec, "sh"),
    ];
    let token = broker
        .mint(
            &manifest,
            "exec",
            "u",
            &grants,
            3600,
            OffsetDateTime::now_utc(),
        )
        .unwrap();
    let mut registry = Registry::new(Arc::new(Mutex::new(broker)), Arc::new(MemoryJournal::new()));
    mcp_system::tools::register_all(&mut registry);
    (
        ToolContext {
            token,
            task: "exec".into(),
            home: home.display().to_string(),
            workdir: work.display().to_string(),
            sandbox_level: 0,
            step: 1,
        },
        registry,
    )
}

#[test]
fn la_liste_blanche_tourne_sur_place_et_le_reste_en_microvm() {
    assert!(is_safe_binary("cat") && is_safe_binary("wc"));
    assert!(!is_safe_binary("/nix/store/abc-coreutils/bin/wc"));
    assert!(!is_safe_binary("sh") && !is_safe_binary("python3"));
    assert_eq!(required_level_for("cat", None), 0);
    assert_eq!(required_level_for("cat", Some(1)), 1);
    assert_eq!(required_level_for("sh", None), 2);
    assert_eq!(required_level_for("sh", Some(0)), 2);
}

#[test]
fn le_plan_confine_dans_l_espace_de_travail_et_ne_laisse_jamais_ecrire_le_home() {
    let dir = tempfile::tempdir().unwrap();
    let (context, _) = contexte(dir.path());
    let (spec, name) =
        Exec::plan(&json!({"program":"cat","args":["-n","docs/x"]}), &context).unwrap();
    assert_eq!(name, "cat");
    assert_eq!(spec.level, 0);
    assert!(
        spec.program.starts_with('/') && spec.program.ends_with("/cat"),
        "{}",
        spec.program
    );
    assert_eq!(spec.args, vec!["-n", "docs/x"]);
    assert_eq!(spec.workdir, context.workdir);
    let home = context.home.clone();
    for rule in &spec.rules.paths {
        if rule.path == context.workdir {
            assert!(rule.read && rule.write);
        } else {
            assert!(rule.path.starts_with(&home) && !rule.write, "{rule:?}");
        }
    }
    assert!(
        spec.rules
            .paths
            .iter()
            .any(|r| r.path == format!("{home}/docs") && r.read)
    );
    assert!(
        spec.env
            .iter()
            .any(|(k, v)| k == "HOME" && *v == context.workdir)
    );
    // Un programme hors liste blanche part en microVM, même si l'agent demande moins.
    let (spec, _) = Exec::plan(&json!({"program":"sh","level":0}), &context).unwrap();
    assert_eq!(spec.level, 2);
    // Un binaire nommé `cat` mais désigné par son chemin n'est pas l'utilitaire du PATH :
    // microVM, et la cible jugée par capd reste le chemin.
    let faux = dir.path().join("cat");
    std::fs::write(&faux, "#!/bin/sh\n").unwrap();
    let chemin = faux.display().to_string();
    let (spec, cible) = Exec::plan(&json!({"program":chemin,"level":0}), &context).unwrap();
    assert_eq!(spec.level, 2);
    assert_eq!(cible, chemin);
    assert_eq!(spec.program, chemin);
}

#[test]
fn un_programme_absent_relatif_ou_remontant_est_refuse() {
    let dir = tempfile::tempdir().unwrap();
    let (context, _) = contexte(dir.path());
    for program in [
        "prophet-inexistant-xyz",
        "./cat",
        "../bin/cat",
        "/tmp/../bin/cat",
        "",
    ] {
        let err = Exec::plan(&json!({"program":program}), &context).unwrap_err();
        assert!(err.is_error, "{program}");
    }
    let err = Exec::plan(&json!({"program":"cat","args":"x"}), &context).unwrap_err();
    assert!(err.is_error);
}

#[test]
fn sans_sandboxd_l_outil_le_dit_et_ne_lance_rien() {
    let dir = tempfile::tempdir().unwrap();
    let (context, registry) = contexte(dir.path());
    let r = registry.call(
        "proc.exec",
        &json!({"program":"cat"}),
        &context,
        OffsetDateTime::now_utc(),
    );
    assert!(r.is_error, "{r:?}");
    assert!(
        serde_json::to_string(&r).unwrap().contains("sandboxd"),
        "{r:?}"
    );
}
