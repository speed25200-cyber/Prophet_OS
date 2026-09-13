//! `sandbox.run` par le vrai daemon : une commande confinée au niveau 0, sa sortie et son code
//! rendus, le délai qui tue. Sans espaces de noms utilisateur, le test se tait.

use std::path::PathBuf;

use prophet_daemon::essai::Daemon;
use sandboxd::{Capabilities, SandboxSpec};
use serde_json::json;

fn helper() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join("prophet-sandbox-helper")
}

fn namespaces_disponibles() -> bool {
    Capabilities::probe().user_namespaces && helper().exists()
}

#[tokio::test]
async fn une_commande_confinee_rend_sa_sortie_et_son_code_et_le_delai_la_tue() {
    if !namespaces_disponibles() {
        eprintln!("espaces de noms indisponibles : test passé sans rien vérifier");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("sandboxd.sock");
    let daemon = Daemon::lancer_avec(
        env!("CARGO_BIN_EXE_prophet-sandboxd"),
        &socket,
        &dir.path().join("state"),
        &[("PROPHET_SANDBOX_HELPER", helper().to_str().unwrap())],
    );
    let client = daemon.joindre().await;
    let spec = SandboxSpec::new(0, "/bin/sh", "/")
        .args(["-c", "echo bonjour; echo erreur >&2; exit 3"])
        .env("PATH", "/usr/bin:/bin");
    let rendu = client
        .call(
            "sandbox.run",
            json!({"task":"essai", "spec": spec, "timeout_s": 10}),
        )
        .await
        .unwrap();
    assert_eq!(rendu["exit_code"], 3, "{rendu}");
    assert_eq!(rendu["stdout"].as_str().unwrap().trim(), "bonjour");
    assert!(rendu["stderr"].as_str().unwrap().contains("erreur"));
    assert_eq!(rendu["timed_out"], false);
    assert_eq!(rendu["level"], 0);
    // Rien ne reste vivant après une commande finie.
    let liste = client.call("sandbox.list", json!({})).await.unwrap();
    assert_eq!(liste.as_array().map(Vec::len), Some(0), "{liste}");

    let lente = SandboxSpec::new(0, "/bin/sh", "/")
        .args(["-c", "echo debut; sleep 30; echo jamais"])
        .env("PATH", "/usr/bin:/bin");
    let debut = std::time::Instant::now();
    let rendu = client
        .call(
            "sandbox.run",
            json!({"task":"lente", "spec": lente, "timeout_s": 1}),
        )
        .await
        .unwrap();
    assert_eq!(rendu["timed_out"], true, "{rendu}");
    assert!(rendu["stdout"].as_str().unwrap().contains("debut"));
    assert!(!rendu["stdout"].as_str().unwrap().contains("jamais"));
    assert!(debut.elapsed() < std::time::Duration::from_secs(10));
}
