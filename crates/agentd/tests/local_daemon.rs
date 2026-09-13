//! Parcours installé du runtime, avec capd et ledger en processus séparés.
use prophet_daemon::essai::{Daemon, binaire_voisin};
use prophet_ipc::Client;
use serde_json::{Value, json};

const AGENTD: &str = env!("CARGO_BIN_EXE_prophet-agentd");

#[tokio::test]
async fn l_inspection_retourne_le_plan_sans_exposer_le_jeton() {
    let chain = Chain::new("http://127.0.0.1:1/v1").await;
    chain.plan("absent", "Plan à examiner").await;
    let info = chain
        .agents
        .call("task.inspect", json!({"id":"local-test"}))
        .await
        .unwrap();
    assert_eq!(info["task"]["id"], "local-test");
    assert_eq!(info["plan"]["scopes"], json!(["~/docs"]));
    assert_eq!(info["can_start"], true);
    assert!(info["result"].is_null());
    let refused = chain
        .agents
        .call(
            "task.change",
            json!({"id":"local-test","path":"docs/note.txt"}),
        )
        .await
        .unwrap_err();
    assert_eq!(refused.code, prophet_ipc::ErrorCode::PolicyDenied);
    assert!(!info.to_string().contains("signature"));
    assert!(!info.to_string().contains("jeton"));
    chain
        .agents
        .call("task.cancel", json!({"id":"local-test"}))
        .await
        .unwrap();
    let info = chain
        .agents
        .call("task.inspect", json!({"id":"local-test"}))
        .await
        .unwrap();
    assert_eq!(info["task"]["state"], "cancelled");
    assert_eq!(info["can_start"], false);
}

struct Chain {
    dir: tempfile::TempDir,
    endpoint: String,
    _capd: Daemon,
    _ledger: Option<Daemon>,
    agentd: Option<Daemon>,
    agents: Client,
    journal: Client,
}

impl Chain {
    async fn new(endpoint: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join("docs")).unwrap();
        let cap_socket = dir.path().join("cap.sock");
        let ledger_socket = dir.path().join("ledger.sock");
        let capd = Daemon::lancer_avec(
            binaire_voisin("prophet-capd").to_str().unwrap(),
            &cap_socket,
            &dir.path().join("cap-state"),
            &[("PROPHET_HOME", home.to_str().unwrap())],
        );
        drop(capd.joindre().await);
        let ledger = Daemon::lancer(
            binaire_voisin("prophet-ledger").to_str().unwrap(),
            &ledger_socket,
            &dir.path().join("ledger-state"),
        );
        let journal = ledger.joindre().await;
        let agentd = Daemon::lancer_avec(
            AGENTD,
            &dir.path().join("agents.sock"),
            &dir.path().join("agent-state"),
            &[
                ("PROPHET_HOME", home.to_str().unwrap()),
                ("PROPHET_CAPD_SOCKET", cap_socket.to_str().unwrap()),
                ("PROPHET_LEDGER_SOCKET", ledger_socket.to_str().unwrap()),
                ("PROPHET_LOCAL_ENDPOINT", endpoint),
            ],
        );
        let agents = agentd.joindre().await;
        Self {
            dir,
            endpoint: endpoint.into(),
            _capd: capd,
            _ledger: Some(ledger),
            agentd: Some(agentd),
            agents,
            journal,
        }
    }
    async fn plan(&self, model: &str, intent: &str) {
        self.plan_budget(model, intent, 20000).await;
    }
    async fn plan_budget(&self, model: &str, intent: &str, tokens: u64) {
        self.plan_scopes(model, intent, tokens, &["~/docs"]).await;
    }
    async fn plan_scopes(&self, model: &str, intent: &str, tokens: u64, scopes: &[&str]) {
        self.agents.call("task.spawn",json!({
            "id":"local-test", "intent":intent, "user":"prophet",
            "manifest": {
                "agent":{"id":"org.prophet.local-test","version":"1.0.0","name":"Test local","publisher_key":"ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="},
                "model":{"preferred":[format!("local:{model}")]},
                "sandbox":{"min_level":0},
                "capabilities":{"max":{"fs.read":["~/docs/**"],"fs.write":["~/docs/**"],"tool.call":["fs.read","fs.write"]}},
                "budget":{"default":{"tokens":tokens,"wall_time":"90s","approvals":3}}
            },
            "requested":[{"res":"fs","act":"read","match":"~/docs/**"},{"res":"fs","act":"write","match":"~/docs/**"},{"res":"tool","act":"call","match":"fs.read"},{"res":"tool","act":"call","match":"fs.write"}],
            "scopes":scopes,"availability":{"local_models":[model]}
        })).await.unwrap();
    }
    async fn wait_terminal(&self) -> Value {
        tokio::time::timeout(std::time::Duration::from_secs(100), async {
            loop {
                let status = self
                    .agents
                    .call("task.status", json!({"id":"local-test"}))
                    .await
                    .unwrap();
                if matches!(
                    status["state"].as_str(),
                    Some("done" | "failed" | "cancelled")
                ) {
                    return status;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn un_moteur_absent_echoue_sans_bloquer_les_autres_commandes() {
    let chain = Chain::new("http://127.0.0.1:1/v1").await;
    chain
        .plan("absent", "Écris une note dans ~/docs/note.txt")
        .await;
    chain
        .agents
        .call("task.start", json!({"id":"local-test"}))
        .await
        .expect("le lancement existe et rend la main");
    let status = chain.wait_terminal().await;
    assert_eq!(status["state"], "failed");
    assert!(status["reason"].as_str().is_some_and(|r| !r.is_empty()));
    assert_eq!(chain.agents.call("ping", json!({})).await.unwrap(), "pong");
    assert!(!chain.dir.path().join("home/docs/note.txt").exists());
}

#[tokio::test]
#[ignore = "needs_local_model: PROPHET_TEST_MODEL et PROPHET_TEST_ENDPOINT"]
async fn une_mission_reelle_traverse_agentd_capd_ledger_et_survit_au_redemarrage() {
    let endpoint = std::env::var("PROPHET_TEST_ENDPOINT").unwrap();
    let model = std::env::var("PROPHET_TEST_MODEL").unwrap();
    let mut chain = Chain::new(&endpoint).await;
    let nonce = format!("mission-{}", rand::random::<u32>());
    chain.plan(&model,&format!("Use fs.write to save exactly {nonce} into ~/docs/note.txt. After staged=true, answer Done. /no_think")).await;
    chain
        .agents
        .call("task.start", json!({"id":"local-test"}))
        .await
        .unwrap();
    let status = chain.wait_terminal().await;
    assert_eq!(status["state"], "done", "{status}");
    assert!(status["budget"]["spent"]["tokens"].as_u64().unwrap() > 0);
    let result = chain
        .agents
        .call("task.result", json!({"id":"local-test"}))
        .await
        .unwrap();
    assert_eq!(result["diff"]["changes"][0]["path"], "docs/note.txt");
    assert_eq!(
        std::fs::read_to_string(
            chain
                .dir
                .path()
                .join("home/.prophet/tasks/local-test/work/docs/note.txt")
        )
        .unwrap(),
        nonce
    );
    assert!(!chain.dir.path().join("home/docs/note.txt").exists());
    let events = chain
        .journal
        .call("ledger.query", json!({"task":"local-test"}))
        .await
        .unwrap();
    for kind in ["provider.started", "tool.call", "tool.result", "task.done"] {
        assert!(
            events.as_array().unwrap().iter().any(|e| e["kind"] == kind),
            "{events}"
        );
    }
    assert!(
        !events.to_string().contains(&nonce),
        "le contenu produit ne doit pas entrer dans le journal"
    );
    drop(chain.agentd.take());
    chain.agentd = Some(Daemon::lancer_avec(
        AGENTD,
        &chain.dir.path().join("restarted.sock"),
        &chain.dir.path().join("agent-state"),
        &[
            (
                "PROPHET_HOME",
                chain.dir.path().join("home").to_str().unwrap(),
            ),
            ("PROPHET_LOCAL_ENDPOINT", &endpoint),
        ],
    ));
    chain.agents = chain.agentd.as_ref().unwrap().joindre().await;
    let restored = chain
        .agents
        .call("task.result", json!({"id":"local-test"}))
        .await
        .unwrap();
    assert_eq!(restored, result);
    println!("mission réelle terminée et résultat relu après redémarrage : {model}");
}

struct ModelServer {
    endpoint: String,
    received: tokio::sync::oneshot::Receiver<()>,
    release: tokio::sync::oneshot::Sender<()>,
    worker: tokio::task::JoinHandle<()>,
}

async fn controlled_model() -> ModelServer {
    controlled_reply(None).await
}

async fn controlled_reply(first: Option<Value>) -> ModelServer {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let (received, rx) = tokio::sync::oneshot::channel();
    let (release, gate) = tokio::sync::oneshot::channel();
    let worker = tokio::spawn(async move {
        let mut received = Some(received);
        let mut gate = Some(gate);
        for turn in 0..2 {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            let mut size = 0;
            loop {
                let mut line = String::new();
                if stream.read_line(&mut line).await.unwrap() == 0 {
                    return;
                }
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    size = value.trim().parse::<usize>().unwrap();
                }
            }
            assert!(size < 128 * 1024);
            let mut body = vec![0; size];
            stream.read_exact(&mut body).await.unwrap();
            if turn == 0 {
                let _ = received.take().unwrap().send(());
                if gate.take().unwrap().await.is_err() {
                    return;
                }
            }
            let response = if turn == 0 {
                first.clone().unwrap_or_else(|| json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"tool_calls":[{"type":"function","id":"call_1","function":{"name":"fs.write","arguments":"{\"path\":\"~/docs/note.txt\",\"content\":\"preuve\"}"}}]}}],"usage":{"prompt_tokens":12,"completion_tokens":8}}))
            } else {
                json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"Terminé."}}],"usage":{"prompt_tokens":32,"completion_tokens":3}})
            };
            let body = response.to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.get_mut().write_all(response.as_bytes()).await;
        }
    });
    ModelServer {
        endpoint,
        received: rx,
        release,
        worker,
    }
}

#[tokio::test]
async fn une_annulation_interrompt_l_inference_sans_action_tardive() {
    let model = controlled_model().await;
    let chain = Chain::new(&model.endpoint).await;
    chain.plan("controlled", "Écris une note").await;
    chain
        .agents
        .call("task.start", json!({"id":"local-test"}))
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), model.received)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(
        std::time::Duration::from_millis(500),
        chain.agents.call("task.list", json!({})),
    )
    .await
    .unwrap()
    .unwrap();
    chain
        .agents
        .call("task.cancel", json!({"id":"local-test"}))
        .await
        .unwrap();
    let status = tokio::time::timeout(std::time::Duration::from_secs(2), chain.wait_terminal())
        .await
        .unwrap();
    assert_eq!(status["state"], "cancelled", "{status}");
    let _ = model.release.send(());
    assert!(
        !chain
            .dir
            .path()
            .join("home/.prophet/tasks/local-test/work/docs/note.txt")
            .exists()
    );
    model.worker.abort();
}

#[tokio::test]
async fn une_revocation_entre_inference_et_action_est_appliquee_par_le_vrai_capd() {
    let model = controlled_model().await;
    let chain = Chain::new(&model.endpoint).await;
    chain.plan("controlled", "Écris une note").await;
    chain
        .agents
        .call("task.start", json!({"id":"local-test"}))
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), model.received)
        .await
        .unwrap()
        .unwrap();
    Client::connect(chain.dir.path().join("cap.sock"))
        .await
        .unwrap()
        .call("cap.revoke", json!({"subject":"local-test"}))
        .await
        .unwrap();
    model.release.send(()).unwrap();
    let status = chain.wait_terminal().await;
    assert_eq!(status["state"], "failed", "{status}");
    assert!(
        !chain
            .dir
            .path()
            .join("home/.prophet/tasks/local-test/work/docs/note.txt")
            .exists()
    );
    let events = chain
        .journal
        .call("ledger.query", json!({"task":"local-test"}))
        .await
        .unwrap();
    assert!(
        events
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "policy.deny"),
        "{events}"
    );
    model.worker.abort();
}

#[tokio::test]
async fn le_budget_est_controle_avant_l_action_du_modele() {
    let model = controlled_model().await;
    let chain = Chain::new(&model.endpoint).await;
    chain.plan_budget("controlled", "Écris une note", 10).await;
    chain
        .agents
        .call("task.start", json!({"id":"local-test"}))
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), model.received)
        .await
        .unwrap()
        .unwrap();
    model.release.send(()).unwrap();
    let status = chain.wait_terminal().await;
    assert_eq!(status["state"], "failed", "{status}");
    assert_eq!(status["budget"]["spent"]["tokens"], 20);
    assert!(
        !chain
            .dir
            .path()
            .join("home/.prophet/tasks/local-test/work/docs/note.txt")
            .exists()
    );
    model.worker.abort();
}

#[tokio::test]
async fn une_panne_du_journal_avant_l_action_interdit_l_ecriture() {
    let model = controlled_model().await;
    let mut chain = Chain::new(&model.endpoint).await;
    chain.plan("controlled", "Écris une note").await;
    chain
        .agents
        .call("task.start", json!({"id":"local-test"}))
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), model.received)
        .await
        .unwrap()
        .unwrap();
    drop(chain._ledger.take());
    model.release.send(()).unwrap();
    let status = chain.wait_terminal().await;
    assert_eq!(status["state"], "failed", "{status}");
    assert!(
        !chain
            .dir
            .path()
            .join("home/.prophet/tasks/local-test/work/docs/note.txt")
            .exists()
    );
    model.worker.abort();
}

#[tokio::test]
async fn une_generation_tronquee_est_comptee_sans_executer_son_appel() {
    let model = controlled_reply(Some(json!({
        "choices":[{"finish_reason":"length","message":{"tool_calls":[{
            "type":"function","function":{"name":"fs.write","arguments":"{\"path\":\"~/docs/note.txt\",\"content\":\"interdit\"}"}
        }]}}],"usage":{"prompt_tokens":376,"completion_tokens":2048}
    }))).await;
    let chain = Chain::new(&model.endpoint).await;
    chain.plan("controlled", "Écris une note").await;
    chain
        .agents
        .call("task.start", json!({"id":"local-test"}))
        .await
        .unwrap();
    model.received.await.unwrap();
    model.release.send(()).unwrap();
    let status = chain.wait_terminal().await;
    assert_eq!(status["state"], "failed", "{status}");
    assert_eq!(status["budget"]["spent"]["tokens"], 2424);
    assert!(
        !chain
            .dir
            .path()
            .join("home/.prophet/tasks/local-test/work/docs/note.txt")
            .exists()
    );
    model.worker.abort();
}

#[tokio::test]
async fn les_droits_du_jeton_ne_permettent_pas_de_deborder_du_perimetre_du_plan() {
    let model = controlled_model().await;
    let chain = Chain::new(&model.endpoint).await;
    chain
        .plan_scopes("controlled", "Écris une note", 20000, &[])
        .await;
    chain
        .agents
        .call("task.start", json!({"id":"local-test"}))
        .await
        .unwrap();
    model.received.await.unwrap();
    model.release.send(()).unwrap();
    let status = chain.wait_terminal().await;
    assert_eq!(status["state"], "failed", "{status}");
    assert!(
        !chain
            .dir
            .path()
            .join("home/.prophet/tasks/local-test/work/docs/note.txt")
            .exists()
    );
    model.worker.abort();
}

#[tokio::test]
async fn le_cli_lance_et_relit_un_resultat_persistant() {
    let model = controlled_model().await;
    let mut chain = Chain::new(&model.endpoint).await;
    let mut request: Value =
        serde_json::from_str(include_str!("../../../examples/missions/note-locale.json")).unwrap();
    request["id"] = json!("local-test");
    request["manifest"]["model"]["preferred"] = json!(["local:controlled"]);
    request["availability"]["local_models"] = json!(["controlled"]);
    request["manifest"]["capabilities"]["max"]["fs.read"] = json!(["~/docs/**"]);
    request["manifest"]["capabilities"]["max"]["fs.write"] = json!(["~/docs/**"]);
    request["requested"][0]["match"] = json!("~/docs/**");
    request["requested"][1]["match"] = json!("~/docs/**");
    request["scopes"] = json!(["~/docs"]);
    let file = chain.dir.path().join("mission.json");
    std::fs::write(&file, request.to_string()).unwrap();
    let plan = cli(&chain, &["--json", "task", "new", file.to_str().unwrap()]).await;
    assert_eq!(
        serde_json::from_slice::<Value>(&plan.stdout).unwrap()["task"],
        "local-test"
    );
    let start = cli(&chain, &["--json", "task", "start", "local-test"]).await;
    assert_eq!(
        serde_json::from_slice::<Value>(&start.stdout).unwrap()["started"],
        "local-test"
    );
    model.received.await.unwrap();
    model.release.send(()).unwrap();
    let status = chain.wait_terminal().await;
    assert_eq!(status["state"], "done", "{status}");
    assert_eq!(status["budget"]["spent"]["tokens"], 55);
    let result = cli(&chain, &["--json", "task", "result", "local-test"]).await;
    let result: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(result["diff"]["changes"][0]["path"], "docs/note.txt");
    let review = chain
        .agents
        .call(
            "task.change",
            json!({"id":"local-test","path":"docs/note.txt"}),
        )
        .await
        .unwrap();
    assert_eq!(review["task"], "local-test");
    assert!(review["file"]["before"].is_null());
    assert_eq!(review["file"]["after"]["content"]["text"], "preuve");
    assert_eq!(
        std::fs::read_to_string(
            chain
                .dir
                .path()
                .join("home/.prophet/tasks/local-test/work/docs/note.txt")
        )
        .unwrap(),
        "preuve"
    );
    assert!(!chain.dir.path().join("home/docs/note.txt").exists());
    restart(&mut chain).await;
    assert_eq!(
        chain
            .agents
            .call("task.result", json!({"id":"local-test"}))
            .await
            .unwrap(),
        result
    );
    let human = cli(&chain, &["task", "result", "local-test"]).await;
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(
        text.contains("Terminé.") && text.contains("docs/note.txt"),
        "{text}"
    );
    assert_eq!(
        chain
            .agents
            .call(
                "task.change",
                json!({"id":"local-test","path":"docs/note.txt"})
            )
            .await
            .unwrap(),
        review
    );
    for path in ["../docs/note.txt", "/etc/passwd", "docs/absent.txt"] {
        assert!(
            chain
                .agents
                .call("task.change", json!({"id":"local-test","path":path}))
                .await
                .is_err()
        );
    }
    std::fs::write(
        chain
            .dir
            .path()
            .join("home/.prophet/tasks/local-test/work/docs/note.txt"),
        "altéré",
    )
    .unwrap();
    assert!(
        chain
            .agents
            .call(
                "task.change",
                json!({"id":"local-test","path":"docs/note.txt"})
            )
            .await
            .is_err()
    );
    model.worker.abort();
}

async fn cli(chain: &Chain, args: &[&str]) -> std::process::Output {
    let output = tokio::process::Command::new(binaire_voisin("prophet"))
        .args(args)
        .env(
            "PROPHET_AGENTD_SOCKET",
            chain.dir.path().join("agents.sock"),
        )
        .env("PROPHET_HOME", chain.dir.path().join("home"))
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

async fn restart(chain: &mut Chain) {
    drop(chain.agentd.take());
    chain.agentd = Some(Daemon::lancer_avec(
        AGENTD,
        &chain.dir.path().join("agents.sock"),
        &chain.dir.path().join("agent-state"),
        &[
            (
                "PROPHET_HOME",
                chain.dir.path().join("home").to_str().unwrap(),
            ),
            (
                "PROPHET_CAPD_SOCKET",
                chain.dir.path().join("cap.sock").to_str().unwrap(),
            ),
            (
                "PROPHET_LEDGER_SOCKET",
                chain.dir.path().join("ledger.sock").to_str().unwrap(),
            ),
            ("PROPHET_LOCAL_ENDPOINT", &chain.endpoint),
        ],
    ));
    chain.agents = chain.agentd.as_ref().unwrap().joindre().await;
}

#[tokio::test]
async fn une_mission_interrompue_par_redemarrage_ne_reste_pas_en_cours() {
    let model = controlled_model().await;
    let mut chain = Chain::new(&model.endpoint).await;
    chain.plan("controlled", "Écris une note").await;
    chain
        .agents
        .call("task.start", json!({"id":"local-test"}))
        .await
        .unwrap();
    model.received.await.unwrap();
    restart(&mut chain).await;
    let status = chain
        .agents
        .call("task.status", json!({"id":"local-test"}))
        .await
        .unwrap();
    assert_eq!(status["state"], "failed", "{status}");
    assert!(status["reason"].as_str().unwrap().contains("redémarré"));
    assert!(!chain.dir.path().join("home/docs/note.txt").exists());
    model.worker.abort();
}
