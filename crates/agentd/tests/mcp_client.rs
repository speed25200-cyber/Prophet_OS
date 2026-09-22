//! Un client MCP de l'humain travaille dans une mission préparée, par le vrai pont
//! `prophet-mcp` et les vrais capd, ledger et agentd : le jeton ne quitte pas le service, les
//! écritures vont dans le travail SFS, et le créateur examine puis publie comme pour une
//! mission native.
use std::io::{Read as _, Write as _};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use prophet_daemon::essai::{Daemon, binaire_voisin};
use prophet_ipc::{Client, ErrorCode};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

struct Chain {
    dir: tempfile::TempDir,
    _daemons: Vec<Daemon>,
    stop: Arc<AtomicBool>,
    model: Option<std::thread::JoinHandle<()>>,
    client: Client,
    socket: std::path::PathBuf,
}

impl Drop for Chain {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(model) = self.model.take() {
            let _ = model.join();
        }
    }
}

impl Chain {
    async fn new() -> Self {
        // Le catalogue exige un moteur qui annonce ses modèles ; aucune inférence n'a lieu.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let model = std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                let mut request = [0; 4096];
                let n = stream.read(&mut request).unwrap();
                assert!(
                    String::from_utf8_lossy(&request[..n]).starts_with("GET /v1/models "),
                    "une séance d'outils ne lance aucune inférence"
                );
                let body = json!({"data":[{"id":"modele-controle"}]}).to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join("docs")).unwrap();
        std::fs::create_dir_all(home.join("secret")).unwrap();
        std::fs::write(home.join("secret/cle.txt"), "jamais").unwrap();
        let mut example: Value =
            serde_json::from_str(include_str!("../../../examples/missions/note-locale.json"))
                .unwrap();
        example["manifest"]["model"]["preferred"] = json!(["local:modele-controle"]);
        example["manifest"]["capabilities"]["max"]["fs.read"] = json!(["~/docs/**"]);
        example["manifest"]["capabilities"]["max"]["fs.write"] = json!(["~/docs/**"]);
        example["manifest"]["budget"]["default"]["approvals"] = json!(3);
        let profiles = dir.path().join("profiles.json");
        std::fs::write(&profiles, json!([{"id":"documents", "name":"Documents", "description":"Préparer des fichiers dans docs", "manifest":example["manifest"], "scopes":["~/docs"]}]).to_string()).unwrap();
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
        let socket = dir.path().join("agent.sock");
        let agentd = Daemon::lancer_avec(
            env!("CARGO_BIN_EXE_prophet-agentd"),
            &socket,
            &dir.path().join("agent-state"),
            &[
                ("PROPHET_HOME", home.to_str().unwrap()),
                ("PROPHET_CAPD_SOCKET", caps.to_str().unwrap()),
                ("PROPHET_LEDGER_SOCKET", logs.to_str().unwrap()),
                ("PROPHET_LOCAL_ENDPOINT", &endpoint),
                ("PROPHET_MISSION_PROFILES", profiles.to_str().unwrap()),
            ],
        );
        let client = agentd.joindre().await;
        Self {
            dir,
            _daemons: vec![agentd, ledger, capd],
            stop,
            model: Some(model),
            client,
            socket,
        }
    }

    async fn prepare(&self, id: &str) {
        self.client
            .call(
                "task.prepare",
                json!({"id":id, "intent":"Rédiger une note avec mon client", "profile":"documents", "model":"modele-controle"}),
            )
            .await
            .unwrap();
    }

    async fn state(&self, id: &str) -> String {
        self.client
            .call("task.status", json!({"id":id}))
            .await
            .unwrap()["state"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn pont(&self, task: &str) -> tokio::process::Child {
        tokio::process::Command::new(binaire_voisin("prophet-mcp"))
            .env("PROPHET_TASK", task)
            .env("PROPHET_AGENTD_SOCKET", &self.socket)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .expect("prophet-mcp voisin")
    }
}

/// Un client MCP minimal : une ligne envoyée, une ligne lue.
struct Mcp {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
    next: u64,
}

impl Mcp {
    fn new(mut child: tokio::process::Child) -> Self {
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
            next: 1,
        }
    }
    async fn notify(&mut self, method: &str) {
        let line = json!({"jsonrpc":"2.0","method":method}).to_string();
        self.stdin
            .write_all(format!("{line}\n").as_bytes())
            .await
            .unwrap();
    }
    async fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        let line = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string();
        self.stdin
            .write_all(format!("{line}\n").as_bytes())
            .await
            .unwrap();
        let mut response = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.stdout.read_line(&mut response),
        )
        .await
        .expect("le pont répond")
        .unwrap();
        let value: Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(value["id"], json!(id), "{value}");
        value
    }
    async fn tool(&mut self, name: &str, arguments: Value) -> Value {
        let value = self
            .call("tools/call", json!({"name":name,"arguments":arguments}))
            .await;
        assert!(value.get("error").is_none(), "{value}");
        value["result"].clone()
    }
    async fn close(mut self) -> std::process::ExitStatus {
        drop(self.stdin);
        tokio::time::timeout(std::time::Duration::from_secs(30), self.child.wait())
            .await
            .expect("le pont se retire")
            .unwrap()
    }
}

fn initialize() -> Value {
    json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"client-essai","version":"1.0"}})
}

#[tokio::test]
async fn un_client_mcp_travaille_dans_la_mission_preparee_et_le_createur_publie() {
    let chain = Chain::new().await;
    chain.prepare("seance").await;
    // Sans client attaché, la mission n'a pas de séance : ni outils, ni appels.
    assert_eq!(
        chain
            .client
            .call("task.tools", json!({"id":"seance"}))
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let mut mcp = Mcp::new(chain.pont("seance"));
    let refus = mcp.call("tools/list", json!({})).await;
    assert_eq!(refus["error"]["code"], json!(-32002), "{refus}");
    let init = mcp.call("initialize", initialize()).await;
    assert_eq!(init["result"]["serverInfo"]["name"], "prophet", "{init}");
    assert_eq!(chain.state("seance").await, "running");
    mcp.notify("notifications/initialized").await;
    let tools = mcp.call("tools/list", json!({})).await;
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    // Seuls les outils que le jeton couvre sont proposés : le profil d'exemple n'accorde que
    // fs.read, fs.write et l'introspection de la mission ; ni liste, ni réseau, ni navigateur
    // n'apparaissent.
    assert_eq!(
        names,
        ["fs.read", "fs.write", "task.diff", "task.status"],
        "{names:?}"
    );
    let written = mcp
        .tool(
            "fs.write",
            json!({"path":"docs/note.txt","content":"bonjour depuis le client"}),
        )
        .await;
    assert_eq!(written["isError"], json!(false), "{written}");
    let lu = mcp.tool("fs.read", json!({"path":"docs/note.txt"})).await;
    assert!(lu.to_string().contains("bonjour depuis le client"), "{lu}");
    // Hors périmètre : l'outil refuse, le jeton ne couvre pas ~/secret.
    let refuse = mcp.tool("fs.read", json!({"path":"secret/cle.txt"})).await;
    assert_eq!(refuse["isError"], json!(true), "{refuse}");
    assert!(!refuse.to_string().contains("jamais"), "{refuse}");
    // Le home n'a pas bougé : l'écriture est dans le travail de la mission.
    assert!(!chain.dir.path().join("home/docs/note.txt").exists());
    // Le retrait du client conclut la mission et scelle ses versions.
    assert!(mcp.close().await.success());
    assert_eq!(chain.state("seance").await, "done");
    let info = chain
        .client
        .call("task.inspect", json!({"id":"seance"}))
        .await
        .unwrap();
    assert_eq!(info["result"]["execution"], "mcp-client", "{info}");
    assert_eq!(info["result"]["tool_calls"], json!(3), "{info}");
    assert!(
        info["result"]["diff"].to_string().contains("docs/note.txt"),
        "{info}"
    );
    assert_eq!(info["can_apply"], json!(true), "{info}");
    let review = chain
        .client
        .call("task.change", json!({"id":"seance","path":"docs/note.txt"}))
        .await
        .unwrap();
    assert_eq!(
        review["file"]["after"]["content"]["text"],
        "bonjour depuis le client"
    );
    chain
        .client
        .call("task.apply", json!({"id":"seance"}))
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(chain.dir.path().join("home/docs/note.txt")).unwrap(),
        "bonjour depuis le client"
    );
    // Une séance conclue ne se rouvre pas : la mission est terminée.
    assert_eq!(
        chain
            .client
            .call("task.attach", json!({"id":"seance"}))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[tokio::test]
async fn une_annulation_pendant_la_seance_arrete_le_client_et_conclut_la_mission() {
    let chain = Chain::new().await;
    chain.prepare("interrompue").await;
    let mut mcp = Mcp::new(chain.pont("interrompue"));
    mcp.call("initialize", initialize()).await;
    mcp.notify("notifications/initialized").await;
    let liste = mcp.call("tools/list", json!({})).await;
    assert!(liste.get("result").is_some(), "{liste}");
    chain
        .client
        .call("task.cancel", json!({"id":"interrompue"}))
        .await
        .unwrap();
    assert_eq!(chain.state("interrompue").await, "cancelled");
    let refus = mcp
        .call(
            "tools/call",
            json!({"name":"fs.list","arguments":{"path":"docs"}}),
        )
        .await;
    assert!(refus.get("error").is_some(), "{refus}");
    assert!(mcp.close().await.success());
    assert_eq!(chain.state("interrompue").await, "cancelled");
    assert!(
        chain
            .client
            .call("task.list", json!({}))
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len()
            == 1
    );
}

#[tokio::test]
async fn une_mission_deja_lancee_ou_inconnue_n_accueille_aucun_client() {
    let chain = Chain::new().await;
    let mut mcp = Mcp::new(chain.pont("absente"));
    let init = mcp.call("initialize", initialize()).await;
    assert_eq!(init["error"]["code"], json!(-32603), "{init}");
    assert!(mcp.close().await.success());
    assert!(
        chain
            .client
            .call("task.list", json!({}))
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
}
