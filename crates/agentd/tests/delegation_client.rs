//! Un client officiel connecté de l'humain comme sous-mission (ADR 0034).
//!
//! Le parent, un modèle local scripté, confie un objectif à `claude-code`. Ici, `claude` est un
//! faux client : il fait ce que fait le vrai en mode délégué — il lit la configuration MCP
//! qu'on lui donne, lance le pont `prophet-mcp`, s'attache à la mission, liste ses outils, écrit
//! une note par `fs.write`, se retire, et rend son texte final en `stream-json`. Tout le reste est
//! vrai : capd délègue le jeton, agentd tient la séance, le lanceur de la session (`prophet-supd`)
//! lance le client, et le parent reçoit le résultat.

use std::os::unix::fs::PermissionsExt as _;
use std::sync::{Arc, Mutex};

use prophet_daemon::essai::{Daemon, binaire_voisin};
use prophet_ipc::Client;
use serde_json::{Value, json};

const AGENTD: &str = env!("CARGO_BIN_EXE_prophet-agentd");

fn tool_call(name: &str, arguments: Value) -> Value {
    json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"c1","type":"function","function":{"name":name,"arguments":arguments.to_string()}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}})
}

fn fin(text: &str) -> Value {
    json!({"choices":[{"message":{"role":"assistant","content":text},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}})
}

/// Le faux client officiel : ce que fait `claude -p` en mode délégué, sans modèle.
const FAUX_CLAUDE: &str = r#"#!/usr/bin/env python3
import json, os, subprocess, sys
DECONNECTE = "@DECONNECTE@"
args = sys.argv[1:]
if args[:2] == ["auth", "status"]:
    sys.exit(1 if os.path.exists(DECONNECTE) else 0)
if "--tools" not in args or args[args.index("--tools") + 1] != "":
    print("outils intégrés non désactivés", file=sys.stderr); sys.exit(3)
if "--strict-mcp-config" not in args or "--permission-prompts" not in args:
    print("mode délégué incomplet", file=sys.stderr); sys.exit(3)
cfg = next(a[len("--mcp-config="):] for a in args if a.startswith("--mcp-config="))
intent = args[args.index("--") + 1]
serveur = json.load(open(cfg))["mcpServers"]["prophet"]
env = dict(os.environ); env.update(serveur["env"])
pont = subprocess.Popen([serveur["command"]] + serveur.get("args", []), env=env,
                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
def appel(msg):
    pont.stdin.write(json.dumps(msg) + "\n"); pont.stdin.flush()
    if "id" in msg:
        return json.loads(pont.stdout.readline())
init = appel({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"faux-claude","version":"1"}}})
if "error" in init:
    print(json.dumps(init), file=sys.stderr); sys.exit(4)
appel({"jsonrpc":"2.0","method":"notifications/initialized"})
outils = [t["name"] for t in appel({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}})["result"]["tools"]]
ecrit = appel({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"fs.write","arguments":{"path":"~/docs/note.txt","content":"preuve"}}})
pont.stdin.close(); pont.wait()
print(json.dumps({"type":"assistant","message":{"content":[{"type":"text","text":"j'écris la note"}]}}))
print(json.dumps({"type":"result","subtype":"success","result":"Note écrite par le client officiel (%d outils ; %s)." % (len(outils), intent)}))
"#;

struct Chain {
    dir: tempfile::TempDir,
    _capd: Daemon,
    _ledger: Daemon,
    _supd: Daemon,
    _agentd: Daemon,
    client: Client,
    stop: Arc<std::sync::atomic::AtomicBool>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Drop for Chain {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
    }
}

impl Chain {
    async fn new(script: Vec<Value>, deconnecte: bool) -> Self {
        use std::io::{Read as _, Write as _};
        // Le moteur scripté du parent, comme dans les autres tests de délégation.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let script = Arc::new(Mutex::new(std::collections::VecDeque::from(script)));
        let asked = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (noted, stop_flag) = (asked.clone(), stop.clone());
        listener.set_nonblocking(true).unwrap();
        std::thread::spawn(move || {
            while !stop_flag.load(std::sync::atomic::Ordering::Acquire) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    continue;
                };
                stream.set_nonblocking(false).unwrap();
                let mut raw = Vec::new();
                let mut buf = [0u8; 4096];
                let mut head_end = 0;
                let mut size = 0;
                loop {
                    let n = stream.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buf[..n]);
                    if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        head_end = pos + 4;
                        let head = String::from_utf8_lossy(&raw[..head_end]).to_lowercase();
                        size = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        break;
                    }
                }
                while raw.len() < head_end + size {
                    let n = stream.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buf[..n]);
                }
                let request = String::from_utf8_lossy(&raw).into_owned();
                let body = if request.starts_with("GET /v1/models") {
                    json!({"data":[{"id":"modele-controle"}]}).to_string()
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
        base["manifest"]["model"]["preferred"] =
            json!(["local:modele-controle", "driver:claude-code"]);
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

        // La session de l'humain : un faux `claude` sur un chemin privé, son répertoire de
        // configuration (que personne ne lit), et le lanceur.
        let session = dir.path().join("session");
        let bin = session.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let drapeau = session.join("deconnecte");
        if deconnecte {
            std::fs::write(&drapeau, "").unwrap();
        }
        let faux = bin.join("claude");
        std::fs::write(
            &faux,
            FAUX_CLAUDE.replace("@DECONNECTE@", &drapeau.display().to_string()),
        )
        .unwrap();
        std::fs::set_permissions(&faux, std::fs::Permissions::from_mode(0o755)).unwrap();
        let racine = session.join("state");
        std::fs::create_dir_all(racine.join("providers/claude-code/pilot")).unwrap();

        let caps = dir.path().join("cap.sock");
        let logs = dir.path().join("ledger.sock");
        let agent_sock = dir.path().join("agent.sock");
        let sup_sock = dir.path().join("sup.sock");
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
        let supd = Daemon::lancer_avec(
            binaire_voisin("prophet-supd").to_str().unwrap(),
            &sup_sock,
            &dir.path().join("sup-state"),
            &[
                ("PROPHET_SUP_SOCKET", sup_sock.to_str().unwrap()),
                ("PROPHET_SUP_ALLOW_OWNER", "1"),
                ("PROPHET_SUP_CLIENT_PATH", bin.to_str().unwrap()),
                ("PROPHET_STATE_ROOT", racine.to_str().unwrap()),
                (
                    "PROPHET_MCP_BRIDGE",
                    binaire_voisin("prophet-mcp").to_str().unwrap(),
                ),
                ("PROPHET_AGENTD_SOCKET", agent_sock.to_str().unwrap()),
                ("XDG_RUNTIME_DIR", session.to_str().unwrap()),
                ("USER", "pilot"),
                ("HOME", session.to_str().unwrap()),
            ],
        );
        drop(supd.joindre().await);
        let egress = dir.path().join("egress.sock");
        let agentd = Daemon::lancer_avec(
            AGENTD,
            &agent_sock,
            &dir.path().join("agent-state"),
            &[
                ("PROPHET_HOME", home.to_str().unwrap()),
                ("PROPHET_CAPD_SOCKET", caps.to_str().unwrap()),
                ("PROPHET_LEDGER_SOCKET", logs.to_str().unwrap()),
                ("PROPHET_EGRESS_SOCKET", egress.to_str().unwrap()),
                ("PROPHET_SUP_SOCKET", sup_sock.to_str().unwrap()),
                ("PROPHET_LOCAL_ENDPOINT", &endpoint),
                ("PROPHET_MISSION_PROFILES", profiles.to_str().unwrap()),
            ],
        );
        let client = agentd.joindre().await;
        Self {
            dir,
            _capd: capd,
            _ledger: ledger,
            _supd: supd,
            _agentd: agentd,
            client,
            stop,
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
        for _ in 0..600 {
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
async fn le_parent_confie_une_note_a_un_client_officiel_connecte() {
    let chain = Chain::new(
        vec![
            tool_call(
                "task.delegate",
                json!({"intent":"Écris « preuve » dans ~/docs/note.txt","profile":"scribe","model":"claude-code"}),
            ),
            fin("Délégué au client officiel."),
        ],
        false,
    )
    .await;
    let parent = chain
        .run("delegue", "Faire écrire une note par Claude Code")
        .await;
    assert_eq!(parent["task"]["state"], "done", "{parent}");
    // Le moteur local n'a servi qu'au parent : le client apporte son propre modèle.
    assert_eq!(
        *chain.asked.lock().unwrap(),
        vec!["modele-controle", "modele-controle"]
    );
    let child = chain
        .client
        .call("task.inspect", json!({"id":"delegue.1"}))
        .await
        .unwrap();
    assert_eq!(child["task"]["state"], "done", "{child}");
    assert_eq!(child["task"]["parent"], "delegue");
    assert_eq!(child["task"]["driver"], "driver:claude-code", "{child}");
    let texte = child["result"]["text"].as_str().unwrap_or("");
    assert!(
        texte.contains("client officiel") && texte.contains("2 outils"),
        "{child}"
    );
    assert_eq!(child["result"]["execution"], "mcp-client");
    assert_eq!(child["result"]["tool_calls"], 1);
    let note = chain
        .dir
        .path()
        .join("home/.prophet/tasks/delegue.1/work/docs/note.txt");
    assert_eq!(std::fs::read_to_string(&note).unwrap(), "preuve");
}

#[tokio::test]
async fn un_client_deconnecte_ne_donne_lieu_a_aucune_sous_mission() {
    let chain = Chain::new(
        vec![
            tool_call(
                "task.delegate",
                json!({"intent":"Écris une note","profile":"scribe","model":"claude-code"}),
            ),
            fin("Le client n'est pas connecté ; j'arrête."),
        ],
        true,
    )
    .await;
    let parent = chain
        .run("seul", "Tenter une délégation à un client déconnecté")
        .await;
    assert!(
        matches!(parent["task"]["state"].as_str(), Some("done" | "failed")),
        "{parent}"
    );
    let list = chain.client.call("task.list", json!({})).await.unwrap();
    assert!(
        !list.to_string().contains("seul.1"),
        "aucune sous-mission ne doit exister : {list}"
    );
    let journal = parent.to_string();
    assert!(
        journal.contains("connecté") || journal.contains("connect"),
        "{parent}"
    );
}

#[tokio::test]
async fn un_client_que_le_contexte_n_admet_pas_est_refuse() {
    let chain = Chain::new(
        vec![
            tool_call(
                "task.delegate",
                json!({"intent":"Écris une note","profile":"scribe","model":"codex"}),
            ),
            fin("Refusé."),
        ],
        false,
    )
    .await;
    let parent = chain.run("refus", "Tenter un client non admis").await;
    assert_eq!(parent["task"]["state"], "failed", "{parent}");
    assert!(
        parent["task"]["reason"]
            .as_str()
            .unwrap_or("")
            .contains("PolicyDenied"),
        "{parent}"
    );
    let list = chain.client.call("task.list", json!({})).await.unwrap();
    assert!(!list.to_string().contains("refus.1"), "{list}");
}
