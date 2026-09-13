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
        Self::with_env(endpoint, &[]).await
    }

    /// Comme [`Chain::new`], avec des variables supplémentaires pour agentd (navigateur…).
    async fn with_env(endpoint: &str, extra: &[(&str, &str)]) -> Self {
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
        let egress_socket = dir.path().join("egress.sock");
        let mut env: Vec<(&str, &str)> = vec![
            ("PROPHET_HOME", home.to_str().unwrap()),
            ("PROPHET_CAPD_SOCKET", cap_socket.to_str().unwrap()),
            ("PROPHET_LEDGER_SOCKET", ledger_socket.to_str().unwrap()),
            ("PROPHET_EGRESS_SOCKET", egress_socket.to_str().unwrap()),
            ("PROPHET_LOCAL_ENDPOINT", endpoint),
        ];
        env.extend_from_slice(extra);
        let agentd = Daemon::lancer_avec(
            AGENTD,
            &dir.path().join("agents.sock"),
            &dir.path().join("agent-state"),
            &env,
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
    /// Planifie une mission autorisée à lire un hôte par le proxy de sortie.
    async fn plan_web(&self, model: &str, intent: &str, host: &str) {
        self.agents.call("task.spawn",json!({
            "id":"local-test", "intent":intent, "user":"prophet",
            "manifest": {
                "agent":{"id":"org.prophet.local-test","version":"1.0.0","name":"Test local","publisher_key":"ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="},
                "model":{"preferred":[format!("local:{model}")]},
                "sandbox":{"min_level":0},
                "capabilities":{"max":{"fs.read":["~/docs/**"],"fs.write":["~/docs/**"],"net.egress":[host],"ui.read":["browser"],"ui.act":["browser"],"tool.call":["fs.read","fs.write","http.fetch","web.open","web.tree","web.act"]}},
                "budget":{"default":{"tokens":20000,"wall_time":"90s","approvals":3}}
            },
            "requested":[{"res":"fs","act":"read","match":"~/docs/**"},{"res":"fs","act":"write","match":"~/docs/**"},{"res":"net","act":"egress","match":host},{"res":"ui","act":"read","match":"browser"},{"res":"ui","act":"act","match":"browser"},{"res":"tool","act":"call","match":"fs.read"},{"res":"tool","act":"call","match":"fs.write"},{"res":"tool","act":"call","match":"http.fetch"},{"res":"tool","act":"call","match":"web.open"},{"res":"tool","act":"call","match":"web.tree"},{"res":"tool","act":"call","match":"web.act"}],
            "scopes":["~/docs"],"availability":{"local_models":[model]}
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

#[tokio::test]
async fn le_createur_publie_les_versions_examinees_puis_les_annule() {
    let model = controlled_model().await;
    let mut chain = Chain::new(&model.endpoint).await;
    chain.plan("controlled", "Écris une note").await;
    // Rien à publier avant la fin : le refus est une question d'autorisation, pas d'état SFS.
    let refused = chain
        .agents
        .call("task.apply", json!({"id":"local-test"}))
        .await
        .unwrap_err();
    assert_eq!(refused.code, prophet_ipc::ErrorCode::PolicyDenied);
    chain
        .agents
        .call("task.start", json!({"id":"local-test"}))
        .await
        .unwrap();
    model.received.await.unwrap();
    model.release.send(()).unwrap();
    let status = chain.wait_terminal().await;
    assert_eq!(status["state"], "done", "{status}");
    let note = chain.dir.path().join("home/docs/note.txt");
    assert!(!note.exists(), "la fin d'une mission ne publie rien");

    let info = inspect(&chain).await;
    assert_eq!(info["publication"], "open", "{info}");
    assert_eq!(info["can_apply"], true);
    assert_eq!(info["can_undo"], false);

    let applied = chain
        .agents
        .call("task.apply", json!({"id":"local-test"}))
        .await
        .unwrap();
    assert_eq!(applied["applied"], "local-test", "{applied}");
    assert_eq!(applied["changes"]["added"], 1);
    assert_eq!(std::fs::read_to_string(&note).unwrap(), "preuve");
    let info = inspect(&chain).await;
    assert_eq!(info["task"]["state"], "done");
    assert_eq!(info["publication"], "committed", "{info}");
    assert_eq!(info["can_apply"], false);
    assert_eq!(info["can_undo"], true);
    let again = chain
        .agents
        .call("task.apply", json!({"id":"local-test"}))
        .await
        .unwrap_err();
    assert_eq!(again.code, prophet_ipc::ErrorCode::Conflict);
    assert!(again.message.contains("déjà publiées"), "{}", again.message);

    let undone = chain
        .agents
        .call("task.undo", json!({"id":"local-test"}))
        .await
        .unwrap();
    assert_eq!(undone["undone"], "local-test", "{undone}");
    assert_eq!(undone["state"], "rolled_back");
    assert!(!note.exists(), "l'ajout publié doit être retiré");
    let info = inspect(&chain).await;
    assert_eq!(info["task"]["state"], "rolled_back", "{info}");
    assert_eq!(info["publication"], "rolled_back");
    assert_eq!(info["can_apply"], false);
    assert_eq!(info["can_undo"], false);
    let refused = chain
        .agents
        .call("task.apply", json!({"id":"local-test"}))
        .await
        .unwrap_err();
    assert!(
        refused.message.contains("annulée"),
        "un état terminal doit être nommé : {}",
        refused.message
    );

    let events = chain
        .journal
        .call("ledger.query", json!({"task":"local-test"}))
        .await
        .unwrap();
    let events = events.as_array().unwrap();
    for kind in ["fs.commit", "fs.undo", "task.rolled_back"] {
        let event = events
            .iter()
            .find(|e| e["kind"] == kind)
            .unwrap_or_else(|| panic!("{kind} absent du journal : {events:?}"));
        assert_eq!(event["actor"], "user", "{event}");
    }
    assert!(
        !events.iter().any(|e| e.to_string().contains("preuve")),
        "le contenu publié n'entre pas dans le journal"
    );

    // L'état survit au redémarrage : la tâche reste annulée, sans nouvelle commande possible.
    restart(&mut chain).await;
    let info = inspect(&chain).await;
    assert_eq!(info["task"]["state"], "rolled_back");
    assert_eq!(info["publication"], "rolled_back");
}

#[tokio::test]
async fn une_retouche_humaine_apres_publication_interdit_l_annulation() {
    let model = controlled_model().await;
    let chain = Chain::new(&model.endpoint).await;
    chain.plan("controlled", "Écris une note").await;
    chain
        .agents
        .call("task.start", json!({"id":"local-test"}))
        .await
        .unwrap();
    model.received.await.unwrap();
    model.release.send(()).unwrap();
    assert_eq!(chain.wait_terminal().await["state"], "done");
    chain
        .agents
        .call("task.apply", json!({"id":"local-test"}))
        .await
        .unwrap();
    let note = chain.dir.path().join("home/docs/note.txt");
    assert_eq!(std::fs::read_to_string(&note).unwrap(), "preuve");

    // Le document a été repris par l'humain : l'annulation le laisse tel quel et le dit.
    std::fs::write(&note, "retouche humaine").unwrap();
    let refused = chain
        .agents
        .call("task.undo", json!({"id":"local-test"}))
        .await
        .unwrap_err();
    assert_eq!(
        refused.code,
        prophet_ipc::ErrorCode::Conflict,
        "{refused:?}"
    );
    assert_eq!(std::fs::read_to_string(&note).unwrap(), "retouche humaine");
    let info = inspect(&chain).await;
    assert_eq!(info["task"]["state"], "done", "{info}");
    assert_eq!(info["publication"], "committed", "{info}");
    let events = chain
        .journal
        .call("ledger.query", json!({"task":"local-test"}))
        .await
        .unwrap();
    assert!(
        !events
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "fs.undo"),
        "un refus ne s'inscrit pas comme une annulation : {events}"
    );
}

#[tokio::test]
async fn la_cli_publie_et_annule_par_le_service() {
    let model = controlled_model().await;
    let chain = Chain::new(&model.endpoint).await;
    chain.plan("controlled", "Écris une note").await;
    chain
        .agents
        .call("task.start", json!({"id":"local-test"}))
        .await
        .unwrap();
    model.received.await.unwrap();
    model.release.send(()).unwrap();
    assert_eq!(chain.wait_terminal().await["state"], "done");
    let note = chain.dir.path().join("home/docs/note.txt");

    let show = cli(&chain, &["task", "show", "local-test"]).await;
    let show = String::from_utf8(show.stdout).unwrap();
    assert!(show.contains("non appliquées"), "{show}");
    assert!(show.contains("prophet task apply local-test"), "{show}");

    let apply = cli(&chain, &["task", "apply", "local-test"]).await;
    let apply = String::from_utf8(apply.stdout).unwrap();
    assert!(apply.contains("versions publiées"), "{apply}");
    assert!(apply.contains("1 ajout(s)"), "{apply}");
    assert_eq!(std::fs::read_to_string(&note).unwrap(), "preuve");

    let show = cli(&chain, &["task", "show", "local-test"]).await;
    let show = String::from_utf8(show.stdout).unwrap();
    assert!(show.contains("Publication : versions publiées"), "{show}");
    assert!(show.contains("prophet task undo local-test"), "{show}");

    let undo = cli(&chain, &["--json", "task", "undo", "local-test"]).await;
    let undo: Value = serde_json::from_slice(&undo.stdout).unwrap();
    assert_eq!(undo["undone"], "local-test", "{undo}");
    assert!(!note.exists());
}

async fn inspect(chain: &Chain) -> Value {
    chain
        .agents
        .call("task.inspect", json!({"id":"local-test"}))
        .await
        .unwrap()
}

#[tokio::test]
async fn une_revocation_apres_la_mission_interdit_la_publication() {
    let model = controlled_model().await;
    let chain = Chain::new(&model.endpoint).await;
    chain.plan("controlled", "Écris une note").await;
    chain
        .agents
        .call("task.start", json!({"id":"local-test"}))
        .await
        .unwrap();
    model.received.await.unwrap();
    model.release.send(()).unwrap();
    assert_eq!(chain.wait_terminal().await["state"], "done");
    let note = chain.dir.path().join("home/docs/note.txt");

    // Le propriétaire retire ses droits à la mission après coup : ce qu'elle a préparé ne
    // doit plus pouvoir atteindre ses documents, même approuvé, et le refus se lit au journal.
    Client::connect(chain.dir.path().join("cap.sock"))
        .await
        .unwrap()
        .call("cap.revoke", json!({"subject":"local-test"}))
        .await
        .unwrap();
    let refused = chain
        .agents
        .call("task.apply", json!({"id":"local-test"}))
        .await
        .unwrap_err();
    assert_eq!(
        refused.code,
        prophet_ipc::ErrorCode::PolicyDenied,
        "{refused:?}"
    );
    assert!(refused.message.contains("capd"), "{}", refused.message);
    assert!(!note.exists(), "un refus de capd ne doit rien écrire");
    let info = inspect(&chain).await;
    assert_eq!(info["publication"], "open", "{info}");
    assert_eq!(info["task"]["state"], "done");
    let events = chain
        .journal
        .call("ledger.query", json!({"task":"local-test"}))
        .await
        .unwrap();
    let events = events.as_array().unwrap();
    let deny = events
        .iter()
        .find(|e| e["kind"] == "policy.deny" && e["payload"]["stage"] == "publish")
        .unwrap_or_else(|| panic!("refus de publication absent du journal : {events:?}"));
    assert_eq!(deny["payload"]["path"], "docs/note.txt", "{deny}");
    assert!(
        !events.iter().any(|e| e["kind"] == "fs.commit"),
        "aucune publication ne doit être consignée : {events:?}"
    );
}

/// Un serveur HTTP témoin : il note ce qu'il reçoit et répond une page connue.
async fn temoin_web(recu: std::sync::Arc<std::sync::Mutex<Vec<String>>>) -> u16 {
    use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let mut reader = BufReader::new(stream);
            let mut head = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).await.unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                head.push_str(&line);
            }
            recu.lock().unwrap().push(head);
            let body = "<html><body><h1>Page témoin</h1><p>preuve-web</p></body></html>";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = reader.get_mut().write_all(response.as_bytes()).await;
        }
    });
    port
}

#[tokio::test]
async fn un_outil_web_sort_uniquement_par_le_proxy_et_sous_le_jeton_de_la_mission() {
    let recu = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let port = temoin_web(recu.clone()).await;
    let url = format!("http://127.0.0.1:{port}/page");
    let model = controlled_reply(Some(json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"tool_calls":[{"type":"function","id":"call_1","function":{"name":"http.fetch","arguments":json!({"url":url}).to_string()}}]}}],"usage":{"prompt_tokens":12,"completion_tokens":8}}))).await;
    let chain = Chain::new(&model.endpoint).await;
    let egress = Daemon::lancer_avec(
        binaire_voisin("prophet-egress").to_str().unwrap(),
        &chain.dir.path().join("egress.sock"),
        &chain.dir.path().join("egress-state"),
        &[(
            "PROPHET_CAPD_SOCKET",
            chain.dir.path().join("cap.sock").to_str().unwrap(),
        )],
    );
    egress
        .attendre_reponse(b"GET http://sonde.invalide/ HTTP/1.1\r\nHost: sonde.invalide\r\n\r\n")
        .await;
    chain
        .plan_web("controlled", "Lis la page témoin et résume-la", "127.0.0.1")
        .await;
    chain
        .agents
        .call("task.start", json!({"id":"local-test"}))
        .await
        .unwrap();
    model.received.await.unwrap();
    model.release.send(()).unwrap();
    let status = chain.wait_terminal().await;
    assert_eq!(status["state"], "done", "{status}");

    let requests = recu.lock().unwrap().clone();
    assert_eq!(
        requests.len(),
        1,
        "une seule requête doit atteindre le serveur : {requests:?}"
    );
    let head = requests[0].to_ascii_lowercase();
    assert!(head.starts_with("get /page http/1.1"), "{head}");
    assert!(
        !head.contains("proxy-authorization"),
        "le jeton de la mission ne doit jamais sortir : {head}"
    );
    let events = chain
        .journal
        .call("ledger.query", json!({"task":"local-test"}))
        .await
        .unwrap();
    let events = events.as_array().unwrap();
    assert!(
        events
            .iter()
            .any(|e| e["kind"] == "tool.call" && e["payload"]["tool"] == "http.fetch"),
        "{events:?}"
    );
    let result = events
        .iter()
        .find(|e| e["kind"] == "tool.result" && e["payload"]["tool"] == "http.fetch")
        .unwrap_or_else(|| panic!("résultat de http.fetch absent : {events:?}"));
    assert_eq!(result["payload"]["ok"], true, "{result}");
    assert!(
        !events.iter().any(|e| e.to_string().contains("preuve-web")),
        "le contenu lu n'entre pas dans le journal"
    );
    drop(egress);
}

#[tokio::test]
async fn sans_proxy_de_sortie_aucune_requete_ne_part() {
    let recu = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let port = temoin_web(recu.clone()).await;
    let url = format!("http://127.0.0.1:{port}/page");
    let model = controlled_reply(Some(json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"tool_calls":[{"type":"function","id":"call_1","function":{"name":"http.fetch","arguments":json!({"url":url}).to_string()}}]}}],"usage":{"prompt_tokens":12,"completion_tokens":8}}))).await;
    let chain = Chain::new(&model.endpoint).await;
    chain
        .plan_web("controlled", "Lis la page témoin", "127.0.0.1")
        .await;
    chain
        .agents
        .call("task.start", json!({"id":"local-test"}))
        .await
        .unwrap();
    model.received.await.unwrap();
    model.release.send(()).unwrap();
    let status = chain.wait_terminal().await;
    assert!(status["state"].as_str().is_some(), "{status}");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        recu.lock().unwrap().is_empty(),
        "sans proxy, l'outil ne doit trouver aucune route directe"
    );
    let events = chain
        .journal
        .call("ledger.query", json!({"task":"local-test"}))
        .await
        .unwrap();
    let result = events
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "tool.result" && e["payload"]["tool"] == "http.fetch")
        .cloned()
        .unwrap_or_else(|| panic!("résultat de http.fetch absent : {events}"));
    assert_eq!(result["payload"]["ok"], false, "{result}");
}

/// Le navigateur des essais, s'il y en a un ; `PROPHET_EXIGER_NAVIGATEUR=1` rend son absence
/// fatale, pour qu'un vert veuille dire vrai en intégration continue.
fn navigateur_des_essais() -> Option<String> {
    for candidat in [
        "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/usr/bin/google-chrome",
    ] {
        if std::path::Path::new(candidat).is_file() {
            return Some(candidat.to_owned());
        }
    }
    let depuis_environnement = std::env::var("PROPHET_BROWSER").ok();
    assert!(
        !(depuis_environnement.is_none()
            && std::env::var("PROPHET_EXIGER_NAVIGATEUR").as_deref() == Ok("1")),
        "aucun navigateur trouvé alors que PROPHET_EXIGER_NAVIGATEUR=1"
    );
    depuis_environnement
}

fn reponse_web_open(url: &str) -> Value {
    json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"tool_calls":[{"type":"function","id":"call_1","function":{"name":"web.open","arguments":json!({"url":url}).to_string()}}]}}],"usage":{"prompt_tokens":12,"completion_tokens":8}})
}

#[tokio::test]
async fn le_navigateur_pilote_ne_sort_que_par_egress() {
    let Some(navigateur) = navigateur_des_essais() else {
        eprintln!("aucun navigateur : test sans effet");
        return;
    };
    let recu = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let port = temoin_web(recu.clone()).await;
    let url = format!("http://127.0.0.1:{port}/page");
    let model = controlled_reply(Some(reponse_web_open(&url))).await;
    let chain = Chain::with_env(&model.endpoint, &[("PROPHET_BROWSER", &navigateur)]).await;
    let egress = Daemon::lancer_avec(
        binaire_voisin("prophet-egress").to_str().unwrap(),
        &chain.dir.path().join("egress.sock"),
        &chain.dir.path().join("egress-state"),
        &[(
            "PROPHET_CAPD_SOCKET",
            chain.dir.path().join("cap.sock").to_str().unwrap(),
        )],
    );
    egress
        .attendre_reponse(b"GET http://sonde.invalide/ HTTP/1.1\r\nHost: sonde.invalide\r\n\r\n")
        .await;
    chain
        .plan_web("controlled", "Ouvre la page témoin", "127.0.0.1")
        .await;
    chain
        .agents
        .call("task.start", json!({"id":"local-test"}))
        .await
        .unwrap();
    model.received.await.unwrap();
    model.release.send(()).unwrap();
    let status = chain.wait_terminal().await;
    assert_eq!(status["state"], "done", "{status}");

    let requests = recu.lock().unwrap().clone();
    assert!(
        requests
            .iter()
            .any(|head| head.to_ascii_lowercase().starts_with("get /page http/1.1")),
        "la page doit avoir été demandée par le proxy : {requests:?}"
    );
    for head in &requests {
        assert!(
            !head.to_ascii_lowercase().contains("proxy-authorization"),
            "le jeton ne sort jamais : {head}"
        );
    }
    let events = chain
        .journal
        .call("ledger.query", json!({"task":"local-test"}))
        .await
        .unwrap();
    let events = events.as_array().unwrap();
    let call = events
        .iter()
        .find(|e| e["kind"] == "tool.call" && e["payload"]["tool"] == "web.open")
        .unwrap_or_else(|| panic!("appel web.open absent : {events:?}"));
    assert_eq!(call["payload"]["target"], "127.0.0.1", "{call}");
    let result = events
        .iter()
        .find(|e| e["kind"] == "tool.result" && e["payload"]["tool"] == "web.open")
        .unwrap();
    assert_eq!(result["payload"]["ok"], true, "{result}");
    drop(egress);
}

#[tokio::test]
async fn sans_egress_le_navigateur_pilote_n_a_aucune_route() {
    let Some(navigateur) = navigateur_des_essais() else {
        eprintln!("aucun navigateur : test sans effet");
        return;
    };
    let recu = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let port = temoin_web(recu.clone()).await;
    let url = format!("http://127.0.0.1:{port}/page");
    let model = controlled_reply(Some(reponse_web_open(&url))).await;
    let chain = Chain::with_env(&model.endpoint, &[("PROPHET_BROWSER", &navigateur)]).await;
    chain
        .plan_web("controlled", "Ouvre la page témoin", "127.0.0.1")
        .await;
    chain
        .agents
        .call("task.start", json!({"id":"local-test"}))
        .await
        .unwrap();
    model.received.await.unwrap();
    model.release.send(()).unwrap();
    let status = chain.wait_terminal().await;
    assert!(status["state"].as_str().is_some(), "{status}");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(
        recu.lock().unwrap().is_empty(),
        "sans proxy de sortie, le navigateur ne doit trouver aucune route directe"
    );
}
