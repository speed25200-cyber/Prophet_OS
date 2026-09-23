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
#[ignore = "needs_userns: gel et arrêt d'une commande synchrone réellement confinée"]
async fn une_commande_run_est_visible_gelable_et_arretable() {
    assert!(namespaces_disponibles(), "espaces de noms et helper requis");
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("sandboxd.sock");
    let daemon = Daemon::lancer_avec(
        env!("CARGO_BIN_EXE_prophet-sandboxd"),
        &socket,
        &dir.path().join("state"),
        &[("PROPHET_SANDBOX_HELPER", helper().to_str().unwrap())],
    );
    let command_client = daemon.joindre().await;
    let control = daemon.joindre().await;
    let run =
        tokio::spawn(async move {
            command_client.call("sandbox.run", json!({
            "task": "commande", "spec": SandboxSpec::new(0, "/bin/sleep", "/").args(["20"]),
            "timeout_s": 4
        })).await
        });
    let mut pid = None;
    for _ in 0..100 {
        let list = control.call("sandbox.list", json!({})).await.unwrap();
        pid = list
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["task"] == "commande")
            .and_then(|item| item["pid"].as_u64());
        if pid.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let Some(pid) = pid else {
        let _ = run.await;
        panic!("sandbox.run doit figurer parmi les commandes contrôlables");
    };
    let conflict = control
        .call(
            "sandbox.start",
            json!({
                "task": "commande", "spec": SandboxSpec::new(0, "/bin/true", "/")
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(conflict.code, prophet_ipc::ErrorCode::Conflict);
    let frozen = control.call("sandbox.freeze_all", json!({})).await.unwrap();
    assert_eq!(frozen, json!({"frozen":["commande"],"errors":[]}));
    let mut stopped = false;
    for _ in 0..50 {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
        stopped = status
            .lines()
            .any(|line| line.starts_with("State:") && line.contains("T (stopped)"));
        if stopped {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    control
        .call("sandbox.thaw", json!({"task":"commande"}))
        .await
        .unwrap();
    control
        .call("sandbox.kill", json!({"task":"commande"}))
        .await
        .unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(3), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(stopped, "le noyau doit confirmer le gel de la commande");
    assert_eq!(result["timed_out"], false);
    assert_eq!(
        control.call("sandbox.list", json!({})).await.unwrap(),
        json!([])
    );
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
    // L'agent sait ce que l'exécution a coûté, et qu'aucune machine de réserve n'a servi.
    assert!(rendu["elapsed_ms"].as_u64().is_some(), "{rendu}");
    assert_eq!(rendu["warm_start"], false, "{rendu}");
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
