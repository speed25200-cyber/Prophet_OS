//! Deux agents, deux modèles, une tâche : le parent délègue, l'enfant écrit, le parent conclut.
//!
//! Vrais capd, ledger et agentd ; un moteur simulé qui joue une partition et note quel modèle
//! on lui demande à chaque tour. Le jeton de l'enfant est délégué par capd : un contexte plus
//! large que le parent est refusé, et le parent l'apprend comme une erreur d'outil.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use prophet_daemon::essai::{Daemon, binaire_voisin};
use prophet_ipc::Client;
use serde_json::{Value, json};

const AGENTD: &str = env!("CARGO_BIN_EXE_prophet-agentd");

struct Chain {
    dir: tempfile::TempDir,
    _capd: Daemon,
    _ledger: Daemon,
    _agentd: Daemon,
    client: Client,
    stop: Arc<AtomicBool>,
    model: Option<std::thread::JoinHandle<()>>,
    /// Les modèles demandés au moteur, tour après tour.
    asked: Arc<Mutex<Vec<String>>>,
}

impl Drop for Chain {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.model.take().unwrap().join().unwrap();
    }
}

fn tool_call(name: &str, arguments: Value) -> Value {
    json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"tool_calls":[{"type":"function","id":"call_1","function":{"name":name,"arguments":arguments.to_string()}}]}}],"usage":{"prompt_tokens":12,"completion_tokens":8}})
}

fn fin(text: &str) -> Value {
    json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":text}}],"usage":{"prompt_tokens":32,"completion_tokens":3}})
}

impl Chain {
    async fn new(script: Vec<Value>) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let noted = asked.clone();
        let script = Arc::new(Mutex::new(VecDeque::from(script)));
        let model = std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                let mut raw = Vec::new();
                let mut buf = [0; 4096];
                let (head_end, size) = loop {
                    let n = stream.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        break (raw.len(), 0);
                    }
                    raw.extend_from_slice(&buf[..n]);
                    if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&raw[..pos]).to_ascii_lowercase();
                        let size = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        break (pos + 4, size);
                    }
                };
                while raw.len() < head_end + size {
                    let n = stream.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buf[..n]);
                }
                let request = String::from_utf8_lossy(&raw).into_owned();
                let body = if request.starts_with("GET /v1/models") {
                    json!({"data":[{"id":"modele-controle"},{"id":"second"}]}).to_string()
                } else {
                    let corps: Value =
                        serde_json::from_slice(&raw[head_end..]).unwrap_or(Value::Null);
                    noted
                        .lock()
                        .unwrap()
                        .push(corps["model"].as_str().unwrap_or("?").to_owned());
                    script
                        .lock()
                        .unwrap()
                        .pop_front()
                        .unwrap_or_else(|| fin("Partition épuisée."))
                        .to_string()
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join("docs")).unwrap();
        let mut base: Value =
            serde_json::from_str(include_str!("../../../examples/missions/note-locale.json"))
                .unwrap();
        base["manifest"]["model"]["preferred"] = json!(["local:modele-controle", "local:second"]);
        base["manifest"]["capabilities"]["max"]["fs.read"] = json!(["~/docs/**"]);
        base["manifest"]["capabilities"]["max"]["fs.write"] = json!(["~/docs/**"]);
        base["manifest"]["capabilities"]["max"]["tool.call"] = json!(["fs.read", "fs.write"]);
        let mut chef = base["manifest"].clone();
        chef["agent"]["id"] = json!("org.prophet.chef");
        chef["capabilities"]["max"]["task.spawn"] = json!(["scribe"]);
        chef["capabilities"]["max"]["tool.call"] = json!(["fs.read", "fs.write", "task.delegate"]);
        let profiles = dir.path().join("profiles.json");
        std::fs::write(&profiles, json!([
            {"id":"chef", "name":"Chef", "description":"Confie l'écriture", "manifest":chef, "scopes":["~/docs"]},
            {"id":"scribe", "name":"Scribe", "description":"Écrit", "manifest":base["manifest"], "scopes":["~/docs"]}
        ]).to_string()).unwrap();
        let caps = dir.path().join("cap.sock");
        let logs = dir.path().join("ledger.sock");
        let capd = Daemon::lancer_avec(
            binaire_voisin("prophet-capd").to_str().unwrap(),
            &caps,
            &dir.path().join("cap-state"),
            &[("PROPHET_HOME", home.to_str().unwrap())],
        );
        drop(capd.joindre().await);
        let ledger = Daemon::lancer(
            binaire_voisin("prophet-ledger").to_str().unwrap(),
            &logs,
            &dir.path().join("ledger-state"),
        );
        drop(ledger.joindre().await);
        let egress = dir.path().join("egress.sock");
        let agentd = Daemon::lancer_avec(
            AGENTD,
            &dir.path().join("agent.sock"),
            &dir.path().join("agent-state"),
            &[
                ("PROPHET_HOME", home.to_str().unwrap()),
                ("PROPHET_CAPD_SOCKET", caps.to_str().unwrap()),
                ("PROPHET_LEDGER_SOCKET", logs.to_str().unwrap()),
                ("PROPHET_EGRESS_SOCKET", egress.to_str().unwrap()),
                ("PROPHET_LOCAL_ENDPOINT", &endpoint),
                ("PROPHET_MISSION_PROFILES", profiles.to_str().unwrap()),
            ],
        );
        let client = agentd.joindre().await;
        Self {
            dir,
            _capd: capd,
            _ledger: ledger,
            _agentd: agentd,
            client,
            stop,
            model: Some(model),
            asked,
        }
    }

    async fn run(&self, id: &str, intent: &str) -> Value {
        self.client
            .call(
                "task.prepare",
                json!({"id":id, "intent":intent, "profile":"chef", "model":"modele-controle"}),
            )
            .await
            .unwrap();
        self.client
            .call("task.start", json!({"id":id}))
            .await
            .unwrap();
        for _ in 0..300 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let info = self
                .client
                .call("task.inspect", json!({"id":id}))
                .await
                .unwrap();
            if matches!(
                info["task"]["state"].as_str(),
                Some("done" | "failed" | "cancelled")
            ) {
                return info;
            }
        }
        panic!("la mission {id} ne finit pas");
    }
}

#[tokio::test]
async fn le_parent_delegue_a_un_autre_modele_et_recoit_son_resultat() {
    let chain = Chain::new(vec![
        tool_call(
            "task.delegate",
            json!({"intent":"Écris « preuve » dans ~/docs/note.txt","profile":"scribe","model":"second"}),
        ),
        tool_call("fs.write", json!({"path":"~/docs/note.txt","content":"preuve"})),
        fin("Note écrite."),
        fin("Délégué avec succès."),
    ])
    .await;
    let parent = chain.run("delegue", "Faire écrire une note").await;
    assert_eq!(parent["task"]["state"], "done", "{parent}");
    assert!(
        parent["result"]["text"]
            .as_str()
            .unwrap_or("")
            .contains("Délégué"),
        "{parent}"
    );
    // Le moteur a vu les deux modèles, dans l'ordre : parent, enfant, enfant, parent.
    assert_eq!(
        *chain.asked.lock().unwrap(),
        vec!["modele-controle", "second", "second", "modele-controle"]
    );
    // L'enfant est une mission à part entière, rattachée à son parent, au même propriétaire.
    let child = chain
        .client
        .call("task.inspect", json!({"id":"delegue.1"}))
        .await
        .unwrap();
    assert_eq!(child["task"]["state"], "done", "{child}");
    assert_eq!(child["task"]["parent"], "delegue");
    assert_eq!(child["task"]["depth"], 1);
    assert_eq!(child["result"]["text"], "Note écrite.");
    let note = chain
        .dir
        .path()
        .join("home/.prophet/tasks/delegue.1/work/docs/note.txt");
    assert_eq!(std::fs::read_to_string(&note).unwrap(), "preuve");
    // Son budget est prélevé sur celui du parent, pas ajouté.
    let parent_tokens = parent["task"]["budget"]["limits"]["tokens"]
        .as_u64()
        .unwrap();
    let child_tokens = child["task"]["budget"]["limits"]["tokens"]
        .as_u64()
        .unwrap();
    assert!(
        child_tokens <= parent_tokens / 2,
        "{child_tokens} vs {parent_tokens}"
    );
    // La liste du service montre les deux.
    let list = chain.client.call("task.list", json!({})).await.unwrap();
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["id"].as_str())
        .collect();
    assert!(
        ids.contains(&"delegue") && ids.contains(&"delegue.1"),
        "{ids:?}"
    );
}

#[tokio::test]
async fn un_contexte_non_confie_est_refuse_par_capd_sans_creer_de_sous_mission() {
    let chain = Chain::new(vec![
        tool_call(
            "task.delegate",
            json!({"intent":"Prends la main","profile":"chef"}),
        ),
        fin("Refusé, je m'arrête."),
    ])
    .await;
    let parent = chain.run("seul", "Tenter une délégation interdite").await;
    // Un refus de capd interrompt la boucle native, comme tout refus d'outil : la mission le
    // dit, et aucune sous-mission n'a existé.
    assert_eq!(parent["task"]["state"], "failed", "{parent}");
    assert!(
        parent["task"]["reason"]
            .as_str()
            .unwrap_or("")
            .contains("PolicyDenied"),
        "{parent}"
    );
    assert_eq!(*chain.asked.lock().unwrap(), vec!["modele-controle"]);
    let list = chain.client.call("task.list", json!({})).await.unwrap();
    assert!(
        !list.to_string().contains("seul.1"),
        "aucune sous-mission ne doit exister : {list}"
    );
    let refus = chain
        .client
        .call("task.inspect", json!({"id":"seul.1"}))
        .await
        .unwrap_err();
    assert_eq!(refus.code, prophet_ipc::ErrorCode::NotFound);
}
