//! La CLI consulte agentd sans ouvrir les captures privées du service.

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixListener;
use std::process::{Command, Output};

use serde_json::{Value, json};

fn invoke(args: &[&str], method: &str, params: Value, result: Value) -> Output {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    std::fs::create_dir_all(home.join(".prophet")).unwrap();
    // Même sous root, cette sentinelle rend une lecture directe impossible. En production,
    // le répertoire est en 0700 sous l'UID d'agentd ; il ne faut pas élargir ses droits.
    std::fs::write(home.join(".prophet/tasks"), "capture privée du service").unwrap();
    let socket = temp.path().join("agentd.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(std::time::Instant::now() < deadline, "aucun appel à agentd");
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => panic!("{e}"),
            }
        };
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        writeln!(
            stream,
            "{}",
            json!({"jsonrpc":"2.0", "id":request["id"], "result":result})
        )
        .unwrap();
        request
    });
    let output = Command::new(env!("CARGO_BIN_EXE_prophet"))
        .args(args)
        .env("HOME", &home)
        .env("PROPHET_AGENTD_SOCKET", &socket)
        .output()
        .unwrap();
    let request = server.join().unwrap();
    assert_eq!(request["method"], method);
    assert_eq!(request["params"], params);
    assert_eq!(
        std::fs::read_to_string(home.join(".prophet/tasks")).unwrap(),
        "capture privée du service"
    );
    output
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn mission() -> agentd::Task {
    agentd::Task::new(
        "task:service",
        "Préparer un document",
        "org.prophet.documents",
        "pilot",
        agentd::Budget::new(agentd::Limits::default()),
        time::OffsetDateTime::UNIX_EPOCH,
    )
}

fn inspection() -> Value {
    json!(agentd::Inspection {
        task: mission(),
        plan: None,
        result: Some(
            json!({"task":"task:service", "state":"done", "text":"Document préparé.",
            "diff":{"changes":[{"path":"Documents/Prophet/rapport.txt", "kind":"added", "size_after":24}]}})
        ),
        can_start: false,
        can_cancel: false,
        start_reason: None,
        publication: None,
        can_apply: false,
        can_undo: false,
        browsing: None,
    })
}

#[test]
fn liste_du_service_lisible_sans_acces_aux_captures() {
    for result in [json!([]), json!([mission()])] {
        let output = success(invoke(
            &["task", "ls"],
            "task.list",
            json!({}),
            result.clone(),
        ));
        if result.as_array().unwrap().is_empty() {
            assert!(output.contains("aucune tâche"), "{output}");
        } else {
            assert!(output.contains("task:service"), "{output}");
            assert!(output.contains("Préparer un document"), "{output}");
        }
        assert!(!output.contains("aucun espace de travail"), "{output}");
    }
}

#[test]
fn detail_et_diff_proviennent_de_la_meme_inspection_que_la_surface() {
    for action in ["show", "diff"] {
        let output = success(invoke(
            &["task", action, "task:service"],
            "task.inspect",
            json!({"id":"task:service"}),
            inspection(),
        ));
        assert!(output.contains("Documents/Prophet/rapport.txt"), "{output}");
        assert!(output.contains("non appliqu"), "{output}");
        let output = success(invoke(
            &["--json", "task", action, "task:service"],
            "task.inspect",
            json!({"id":"task:service"}),
            inspection(),
        ));
        let value: Value = serde_json::from_str(&output).unwrap();
        if action == "show" {
            assert_eq!(value, inspection());
        } else {
            assert_eq!(value, inspection()["result"]["diff"]);
        }
    }
}

#[test]
fn les_contextes_du_service_se_lisent_et_une_mission_se_prepare_sans_manifeste_fourni() {
    let options = json!({
        "profiles":[{"id":"web","name":"Recherche sur le web","description":"Consulter le web","models":["qwen3-1.7b"],"scopes":["~/Documents/Prophet"],"grants":["net.egress sur *"],"limits":agentd::Limits::default(),"web":true}],
        "model_error":null,
        "browser":{"program":"/run/current-system/sw/bin/chromium","ready":true,"detail":"HeadlessChrome/131.0"}
    });
    let rendu = success(invoke(
        &["task", "options"],
        "task.options",
        json!({}),
        options,
    ));
    assert!(
        rendu.contains("web — Recherche sur le web (consulte le web)"),
        "{rendu}"
    );
    assert!(rendu.contains("modèles : qwen3-1.7b"), "{rendu}");
    assert!(
        rendu.contains(
            "Navigateur piloté : ✓ /run/current-system/sw/bin/chromium — HeadlessChrome/131.0"
        ),
        "{rendu}"
    );

    // La CLI ne fournit ni manifeste, ni droits : seulement l'intention et le contexte choisi.
    let rendu = success(invoke(
        &[
            "--json",
            "task",
            "prepare",
            "--profile",
            "web",
            "--model",
            "qwen3-1.7b",
            "--id",
            "essai-cli",
            "Lire une page et noter son titre",
        ],
        "task.prepare",
        json!({"id":"essai-cli","intent":"Lire une page et noter son titre","profile":"web","model":"qwen3-1.7b"}),
        json!({"task":"essai-cli"}),
    ));
    assert_eq!(
        serde_json::from_str::<Value>(&rendu).unwrap()["task"],
        "essai-cli"
    );
}

#[test]
fn la_configuration_mcp_ne_vise_qu_une_mission_preparee() {
    let mut planned = inspection();
    planned["task"]["state"] = json!("planned");
    planned["result"] = Value::Null;
    let rendu = success(invoke(
        &["task", "mcp-config", "task:service"],
        "task.inspect",
        json!({"id":"task:service"}),
        planned,
    ));
    let config: Value = serde_json::from_str(&rendu).unwrap();
    assert_eq!(
        config["mcpServers"]["prophet"]["env"]["PROPHET_TASK"],
        "task:service"
    );
    assert!(
        config["mcpServers"]["prophet"]["command"]
            .as_str()
            .unwrap()
            .ends_with("prophet-mcp"),
        "{config}"
    );

    // Une mission terminée n'accueille plus de client : la CLI le dit au lieu de configurer.
    let output = invoke(
        &["task", "mcp-config", "task:service"],
        "task.inspect",
        json!({"id":"task:service"}),
        inspection(),
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("seule une mission préparée"));
}
