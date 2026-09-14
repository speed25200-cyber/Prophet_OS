//! Un client officiel comme rôle du relais (ADR 0035) : le modèle de réflexion confie une étape
//! au rôle `code`, que le contexte donne à `driver:codex` ; agentd prépare une séance sous un
//! jeton délégué par capd, le lanceur de pilotes de la session lance « Codex » sous l'identité
//! de l'humain, le client rejoint la séance, écrit, se retire, et son texte revient au parent
//! avec le compte par modèle.
//!
//! Et, depuis le 14 septembre, un client officiel comme modèle principal d'une mission (complément
//! de l'ADR 0035) : l'humain nomme `codex` comme modèle, `task.prepare` fait le plan sur lui,
//! `task.start` le lance par le lanceur, sans que le moteur local soit sollicité.
//!
//! Vrais capd, ledger, agentd, `prophet-pilotd` et CLI ; un moteur simulé pour le parent, et
//! un client de remplacement (un script) à la place de Codex, qui n'est pas installé ici et
//! dont la connexion appartient à l'humain. Le vrai Codex suit exactement le même chemin par
//! le pont `prophet-mcp` ; ce que ce test ne prouve pas, c'est son comportement à lui.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use prophet_daemon::essai::{Daemon, binaire_voisin};
use prophet_ipc::{Client, ErrorCode};
use serde_json::{Value, json};

const AGENTD: &str = env!("CARGO_BIN_EXE_prophet-agentd");

struct Chain {
    dir: tempfile::TempDir,
    _daemons: Vec<Daemon>,
    client: Client,
    stop: Arc<AtomicBool>,
    model: Option<std::thread::JoinHandle<()>>,
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

/// Le faux Codex : un script qui rejoint la mission par la CLI, écrit, se retire ; il reçoit
/// de `prophet-pilotd` exactement l'environnement d'un vrai client.
fn faux_codex(dir: &std::path::Path, cli: &std::path::Path) -> std::path::PathBuf {
    let script = dir.join("faux-codex.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nset -e\nP={cli}\n\
             [ -n \"$PROPHET_TASK\" ] || exit 3\n\
             [ -n \"$CODEX_HOME\" ] || exit 4\n\
             [ -f \"$PROPHET_MCP_CONFIG\" ] || exit 5\n\
             \"$P\" task attach \"$PROPHET_TASK\" --client codex >/dev/null\n\
             case \"$1\" in *attends*) sleep 30 & wait ;; esac\n\
             \"$P\" task call \"$PROPHET_TASK\" fs.write '{{\"path\":\"~/docs/code.txt\",\"content\":\"fn main() {{}}\"}}' >/dev/null\n\
             texte=\"Code écrit par le faux Codex : $1\"\n\
             case \"$1\" in *relire*)\n\
               relecture=$(\"$P\" task call \"$PROPHET_TASK\" task.delegate '{{\"intent\":\"Relis ~/docs/code.txt\",\"profile\":\"atelier\",\"role\":\"review\"}}' --json)\n\
               texte=\"$texte ; relecture : $relecture\" ;;\n\
             esac\n\
             \"$P\" task detach \"$PROPHET_TASK\" --text \"$texte\" >/dev/null\n\
             echo '{{\"item\":{{\"type\":\"agent_message\",\"text\":\"Code écrit par le faux Codex.\"}}}}'\n",
            cli = cli.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    script
}

/// Le faux Claude Code : rejoint la mission qu'on lui confie, lit le fichier, rend son avis ;
/// il reçoit de `prophet-pilotd` l'environnement d'un vrai Claude Code et parle comme lui.
fn faux_claude(dir: &std::path::Path, cli: &std::path::Path) -> std::path::PathBuf {
    let script = dir.join("faux-claude.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nset -e\nP={cli}\n\
             [ -n \"$PROPHET_TASK\" ] || exit 3\n\
             [ -n \"$CLAUDE_CONFIG_DIR\" ] || exit 4\n\
             \"$P\" task attach \"$PROPHET_TASK\" --client claude-code >/dev/null\n\
             lu=$(\"$P\" task call \"$PROPHET_TASK\" fs.read '{{\"path\":\"~/docs/code.txt\"}}')\n\
             \"$P\" task call \"$PROPHET_TASK\" fs.write '{{\"path\":\"~/docs/relecture.txt\",\"content\":\"Relu.\"}}' >/dev/null\n\
             \"$P\" task detach \"$PROPHET_TASK\" --text \"Relu par le faux Claude Code : $1 ; lu : $lu\" >/dev/null\n\
             echo '{{\"type\":\"result\",\"result\":\"Relu par le faux Claude Code.\"}}'\n",
            cli = cli.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    script
}

impl Chain {
    async fn new(script: Vec<Value>, with_pilot: bool) -> Self {
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
        base["manifest"]["model"]["preferred"] = json!(["local:modele-controle", "driver:codex"]);
        base["manifest"]["model"]["privacy"] = json!("local-preferred");
        base["manifest"]["capabilities"]["max"]["fs.read"] = json!(["~/docs/**"]);
        base["manifest"]["capabilities"]["max"]["fs.write"] = json!(["~/docs/**"]);
        base["manifest"]["capabilities"]["max"]["tool.call"] = json!(["fs.read", "fs.write"]);
        let mut chef = base["manifest"].clone();
        chef["agent"]["id"] = json!("org.prophet.chef");
        chef["model"]["roles"] = json!({"reflect": ["local:modele-controle"]});
        chef["capabilities"]["max"]["task.spawn"] = json!(["atelier"]);
        chef["capabilities"]["max"]["tool.call"] = json!(["fs.read", "fs.write", "task.delegate"]);
        let mut atelier = base["manifest"].clone();
        atelier["agent"]["id"] = json!("org.prophet.atelier");
        // L'atelier admet les deux clients : le catalogue exige qu'un modèle de rôle soit
        // aussi un modèle admis.
        atelier["model"]["preferred"] = json!([
            "local:modele-controle",
            "driver:codex",
            "driver:claude-code"
        ]);
        // Comme dans l'image, l'atelier peut se confier des sous-missions : un client qui le
        // mène délègue un rôle à l'autre.
        atelier["capabilities"]["max"]["task.spawn"] = json!(["atelier"]);
        atelier["capabilities"]["max"]["tool.call"] =
            json!(["fs.read", "fs.write", "task.delegate"]);
        // Le code revient à Codex s'il est connecté, sinon au modèle local.
        atelier["model"]["roles"] = json!({
            "code": ["driver:codex", "local:modele-controle"],
            "review": ["driver:claude-code", "local:modele-controle"],
            "execute": ["local:modele-controle"]
        });
        let profiles = dir.path().join("profiles.json");
        std::fs::write(&profiles, json!([
            {"id":"chef", "name":"Chef", "description":"Réfléchit et confie", "manifest":chef, "scopes":["~/docs"]},
            {"id":"atelier", "name":"Atelier", "description":"Code", "manifest":atelier, "scopes":["~/docs"]}
        ]).to_string()).unwrap();
        let caps = dir.path().join("cap.sock");
        let logs = dir.path().join("ledger.sock");
        let agent_socket = dir.path().join("agent.sock");
        let pilot_socket = dir.path().join("pilot.sock");
        let mut daemons = Vec::new();
        let capd = Daemon::lancer_avec(
            binaire_voisin("prophet-capd").to_str().unwrap(),
            &caps,
            &dir.path().join("cap-state"),
            &[("PROPHET_HOME", home.to_str().unwrap())],
        );
        drop(capd.joindre().await);
        daemons.push(capd);
        let ledger = Daemon::lancer(
            binaire_voisin("prophet-ledger").to_str().unwrap(),
            &logs,
            &dir.path().join("ledger-state"),
        );
        drop(ledger.joindre().await);
        daemons.push(ledger);
        if with_pilot {
            let cli = binaire_voisin("prophet");
            let codex = faux_codex(dir.path(), &cli);
            let claude = faux_claude(dir.path(), &cli);
            let clients = json!({
                "codex": {"program": codex.display().to_string(), "args": ["{intent}"]},
                "claude-code": {"program": claude.display().to_string(), "args": ["{intent}"]}
            })
            .to_string();
            let pilot = Daemon::lancer_avec(
                binaire_voisin("prophet-pilotd").to_str().unwrap(),
                &pilot_socket,
                &dir.path().join("pilot-state"),
                &[
                    ("PROPHET_PILOT_SOCKET", pilot_socket.to_str().unwrap()),
                    ("PROPHET_PILOT_ALLOW_OWNER", "1"),
                    ("PROPHET_PILOT_CLIENTS", &clients),
                    (
                        "PROPHET_PILOT_STATE",
                        dir.path().join("pilot-state").to_str().unwrap(),
                    ),
                    ("PROPHET_AGENTD_SOCKET", agent_socket.to_str().unwrap()),
                    ("HOME", home.to_str().unwrap()),
                    ("XDG_RUNTIME_DIR", dir.path().join("run").to_str().unwrap()),
                ],
            );
            drop(pilot.joindre().await);
            daemons.push(pilot);
        }
        let mut env = vec![
            ("PROPHET_HOME", home.to_str().unwrap().to_owned()),
            ("PROPHET_CAPD_SOCKET", caps.to_str().unwrap().to_owned()),
            ("PROPHET_LEDGER_SOCKET", logs.to_str().unwrap().to_owned()),
            (
                "PROPHET_EGRESS_SOCKET",
                dir.path().join("egress.sock").to_str().unwrap().to_owned(),
            ),
            ("PROPHET_LOCAL_ENDPOINT", endpoint),
            (
                "PROPHET_MISSION_PROFILES",
                profiles.to_str().unwrap().to_owned(),
            ),
        ];
        if with_pilot {
            env.push((
                "PROPHET_PILOT_SOCKET",
                pilot_socket.to_str().unwrap().to_owned(),
            ));
        }
        let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let agentd = Daemon::lancer_avec(
            AGENTD,
            &agent_socket,
            &dir.path().join("agent-state"),
            &env_refs,
        );
        let client = agentd.joindre().await;
        daemons.push(agentd);
        Self {
            dir,
            _daemons: daemons,
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
        self.attendre(id).await
    }

    async fn attendre(&self, id: &str) -> Value {
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
async fn le_role_code_lance_le_client_officiel_dans_une_seance_et_son_texte_revient_au_parent() {
    let chain = Chain::new(
        vec![
            tool_call(
                "task.delegate",
                json!({"intent":"Écris un programme minimal dans ~/docs/code.txt","profile":"atelier","role":"code"}),
            ),
            fin("Codex a écrit le code ; je conclus."),
        ],
        true,
    )
    .await;
    // Le catalogue annonce Codex comme rôle « code » de l'atelier, parce que le lanceur le dit prêt.
    let options = chain.client.call("task.options", json!({})).await.unwrap();
    let atelier = options["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "atelier")
        .unwrap();
    assert_eq!(
        atelier["roles"]["code"],
        json!(["driver:codex", "modele-controle"]),
        "{atelier}"
    );
    assert_eq!(options["pilot"]["drivers"][1]["driver"], "codex");
    assert_eq!(options["pilot"]["drivers"][1]["connection"], "simulated");

    let parent = chain
        .run("relais-codex", "Faire écrire du code par Codex")
        .await;
    assert_eq!(parent["task"]["state"], "done", "{parent}");
    // Le moteur simulé n'a servi que le parent : l'enfant est un client, pas un modèle du moteur.
    assert_eq!(
        *chain.asked.lock().unwrap(),
        vec!["modele-controle", "modele-controle"]
    );
    let child = chain
        .client
        .call("task.inspect", json!({"id":"relais-codex.1"}))
        .await
        .unwrap();
    assert_eq!(child["task"]["state"], "done", "{child}");
    assert_eq!(child["task"]["driver"], "driver:codex");
    assert_eq!(child["task"]["role"], "code");
    assert_eq!(child["task"]["parent"], "relais-codex");
    // Le client a travaillé par sa séance : un appel d'outil, compté sous son nom, sans tokens.
    assert_eq!(
        child["task"]["usage"]["client:codex"]["turns"], 1,
        "{child}"
    );
    assert_eq!(child["task"]["usage"]["client:codex"]["tokens_in"], 0);
    assert_eq!(
        child["result"]["text"],
        "Code écrit par le faux Codex : Écris un programme minimal dans ~/docs/code.txt"
    );
    assert_eq!(child["result"]["execution"], "mcp-client");
    let code = chain
        .dir
        .path()
        .join("home/.prophet/tasks/relais-codex.1/work/docs/code.txt");
    assert_eq!(std::fs::read_to_string(&code).unwrap(), "fn main() {}");
    // Le code de Codex est revenu dans l'espace du parent, qui publiera le tout (ADR 0039).
    let chez_le_parent = chain
        .dir
        .path()
        .join("home/.prophet/tasks/relais-codex/work/docs/code.txt");
    assert_eq!(
        std::fs::read_to_string(&chez_le_parent).unwrap(),
        "fn main() {}"
    );
    // Le parent porte le compte de l'enfant, modèle local et client confondus.
    assert_eq!(
        parent["task"]["usage"]["client:codex"]["turns"], 1,
        "{parent}"
    );
    assert_eq!(parent["task"]["usage"]["local:modele-controle"]["turns"], 2);
}

#[tokio::test]
async fn une_mission_demarre_directement_sur_le_client_officiel_connecte() {
    let chain = Chain::new(vec![], true).await;
    // Le catalogue propose Codex comme modèle de l'atelier, avant le modèle local du service.
    let options = chain.client.call("task.options", json!({})).await.unwrap();
    let atelier = options["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "atelier")
        .unwrap();
    assert_eq!(
        atelier["models"],
        json!(["codex", "claude-code", "modele-controle"]),
        "{atelier}"
    );
    // Un client que le profil n'admet pas reste refusé, connecté ou non.
    let refus = chain
        .client
        .call(
            "task.prepare",
            json!({"id":"direct-gemini","intent":"Écris ~/docs/code.txt","profile":"atelier","model":"gemini"}),
        )
        .await
        .unwrap_err();
    assert_eq!(refus.code, ErrorCode::PolicyDenied, "{refus:?}");
    // Le nom nu ou préfixé désigne le même client.
    let plan = chain
        .client
        .call(
            "task.prepare",
            json!({"id":"direct-codex-prefixe","intent":"Écris ~/docs/code.txt","profile":"atelier","model":"driver:codex"}),
        )
        .await
        .unwrap();
    assert_eq!(plan["choice"]["reference"], "driver:codex", "{plan}");
    let plan = chain
        .client
        .call(
            "task.prepare",
            json!({"id":"direct-codex","intent":"Écris ~/docs/code.txt","profile":"atelier","model":"codex"}),
        )
        .await
        .unwrap();
    assert_eq!(plan["choice"]["reference"], "driver:codex", "{plan}");
    let avant = chain
        .client
        .call("task.inspect", json!({"id":"direct-codex"}))
        .await
        .unwrap();
    assert_eq!(avant["can_start"], true, "{avant}");
    assert_eq!(avant["task"]["driver"], "driver:codex");
    let lancee = chain
        .client
        .call("task.start", json!({"id":"direct-codex"}))
        .await
        .unwrap();
    assert_eq!(lancee["driver"], "driver:codex", "{lancee}");
    assert_eq!(lancee["launched"], true);
    let info = chain.attendre("direct-codex").await;
    assert_eq!(info["task"]["state"], "done", "{info}");
    assert_eq!(info["task"]["driver"], "driver:codex");
    assert_eq!(info["task"]["parent"], Value::Null);
    // Le client a travaillé par sa séance : un appel d'outil, compté sous son nom, sans tokens.
    assert_eq!(info["task"]["usage"]["client:codex"]["turns"], 1, "{info}");
    assert_eq!(
        info["result"]["text"],
        "Code écrit par le faux Codex : Écris ~/docs/code.txt"
    );
    assert_eq!(info["result"]["execution"], "mcp-client");
    let code = chain
        .dir
        .path()
        .join("home/.prophet/tasks/direct-codex/work/docs/code.txt");
    assert_eq!(std::fs::read_to_string(&code).unwrap(), "fn main() {}");
    // Le moteur du service n'a jamais été sollicité : le client est le modèle.
    assert!(chain.asked.lock().unwrap().is_empty(), "{:?}", chain.asked);
    // La mission finie, une autre peut se lancer sur le même client.
    chain
        .client
        .call(
            "task.prepare",
            json!({"id":"direct-codex-2","intent":"Écris ~/docs/code.txt","profile":"atelier","model":"codex"}),
        )
        .await
        .unwrap();
    chain
        .client
        .call("task.start", json!({"id":"direct-codex-2"}))
        .await
        .unwrap();
    let info = chain.attendre("direct-codex-2").await;
    assert_eq!(info["task"]["state"], "done", "{info}");
}

/// Les deux cerveaux ensemble, sans modèle local : l'humain confie la mission à Codex, qui
/// écrit le code puis confie sa relecture à Claude Code par `task.delegate {role: "review"}` ;
/// Claude Code rejoint sa sous-mission par le pont, lit, rend son avis, et Codex conclut avec
/// cet avis. Chaque client dans sa propre mission contrôlée, chaque appel compté sous son nom.
#[tokio::test]
async fn codex_mene_la_mission_et_confie_la_relecture_a_claude_code() {
    let chain = Chain::new(vec![], true).await;
    let options = chain.client.call("task.options", json!({})).await.unwrap();
    let atelier = options["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "atelier")
        .unwrap();
    assert_eq!(
        atelier["roles"]["review"],
        json!(["driver:claude-code", "modele-controle"]),
        "{atelier}"
    );
    assert_eq!(options["pilot"]["drivers"][0]["driver"], "claude-code");
    assert_eq!(options["pilot"]["drivers"][0]["connection"], "simulated");
    chain
        .client
        .call(
            "task.prepare",
            json!({"id":"duo","intent":"Écris ~/docs/code.txt et fais-le relire","profile":"atelier","model":"codex"}),
        )
        .await
        .unwrap();
    chain
        .client
        .call("task.start", json!({"id":"duo"}))
        .await
        .unwrap();
    let parent = chain.attendre("duo").await;
    assert_eq!(parent["task"]["state"], "done", "{parent}");
    assert_eq!(parent["task"]["driver"], "driver:codex");
    let texte = parent["result"]["text"].as_str().unwrap();
    assert!(
        texte.starts_with("Code écrit par le faux Codex : Écris ~/docs/code.txt et fais-le relire"),
        "{texte}"
    );
    // Le relecteur a lu le code que Codex venait d'écrire : la sous-mission part de l'espace
    // de travail du parent (ADR 0039).
    assert!(
        texte.contains("Relu par le faux Claude Code : Relis ~/docs/code.txt ; lu : "),
        "{texte}"
    );
    assert!(texte.contains("fn main() {}"), "{texte}");
    let child = chain
        .client
        .call("task.inspect", json!({"id":"duo.1"}))
        .await
        .unwrap();
    assert_eq!(child["task"]["state"], "done", "{child}");
    assert_eq!(child["task"]["driver"], "driver:claude-code");
    assert_eq!(child["task"]["role"], "review");
    assert_eq!(child["task"]["parent"], "duo");
    assert!(
        child["result"]["text"]
            .as_str()
            .unwrap()
            .starts_with("Relu par le faux Claude Code : Relis ~/docs/code.txt ; lu : "),
        "{child}"
    );
    // Le verdict écrit par le relecteur est revenu dans l'espace de Codex, qui publiera le
    // tout ; la sous-mission, elle, ne se publie pas seule.
    assert_eq!(child["can_apply"], false, "{child}");
    let racine = chain.dir.path().join("home/.prophet/tasks");
    assert_eq!(
        std::fs::read_to_string(racine.join("duo.1/work/docs/code.txt")).unwrap(),
        "fn main() {}"
    );
    assert_eq!(
        std::fs::read_to_string(racine.join("duo/work/docs/relecture.txt")).unwrap(),
        "Relu."
    );
    assert!(!chain.dir.path().join("home/docs/relecture.txt").exists());
    // Chaque client est compté sous son nom (le relecteur a lu, puis écrit), et le parent
    // porte le compte de l'enfant.
    assert_eq!(
        child["task"]["usage"]["client:claude-code"]["turns"], 2,
        "{child}"
    );
    assert_eq!(
        parent["task"]["usage"]["client:claude-code"]["turns"], 2,
        "{parent}"
    );
    assert!(
        parent["task"]["usage"]["client:codex"]["turns"]
            .as_u64()
            .unwrap()
            >= 2,
        "{parent}"
    );
    // Le moteur local n'a jamais été sollicité : deux clients, aucun modèle du service.
    assert!(chain.asked.lock().unwrap().is_empty(), "{:?}", chain.asked);
}

/// Un client officiel se nomme comme un modèle dans `task.delegate {model}` ; un client que le
/// contexte n'admet pas est une erreur d'argument que le modèle corrige, pas un refus.
#[tokio::test]
async fn un_modele_nomme_dans_une_delegation_peut_etre_un_client_officiel() {
    let chain = Chain::new(
        vec![
            tool_call(
                "task.delegate",
                json!({"intent":"Écris ~/docs/code.txt","profile":"atelier","model":"codex"}),
            ),
            fin("Codex, nommé, a écrit ; je conclus."),
        ],
        true,
    )
    .await;
    let parent = chain
        .run("modele-codex", "Faire écrire du code par Codex")
        .await;
    assert_eq!(parent["task"]["state"], "done", "{parent}");
    let child = chain
        .client
        .call("task.inspect", json!({"id":"modele-codex.1"}))
        .await
        .unwrap();
    assert_eq!(child["task"]["state"], "done", "{child}");
    assert_eq!(child["task"]["driver"], "driver:codex");
    assert_eq!(child["task"]["role"], "code");
    assert_eq!(
        child["result"]["text"],
        "Code écrit par le faux Codex : Écris ~/docs/code.txt"
    );
    assert_eq!(
        *chain.asked.lock().unwrap(),
        vec!["modele-controle", "modele-controle"]
    );

    let chain = Chain::new(
        vec![
            tool_call(
                "task.delegate",
                json!({"intent":"Écris ~/docs/code.txt","profile":"atelier","model":"gemini"}),
            ),
            fin("Gemini n'est pas admis ; je conclus sans lui."),
        ],
        true,
    )
    .await;
    let parent = chain.run("modele-gemini", "Essayer Gemini").await;
    assert_eq!(parent["task"]["state"], "done", "{parent}");
    let refus = chain
        .client
        .call("task.inspect", json!({"id":"modele-gemini.1"}))
        .await
        .unwrap_err();
    assert_eq!(refus.code, ErrorCode::NotFound, "{refus:?}");
}

/// L'humain annule une mission menée par un client : la séance est conclue et le client est
/// tué sur-le-champ par le lanceur, avec ce qu'il a lancé ; la place est libre pour la suite.
#[tokio::test]
async fn annuler_une_mission_menee_par_un_client_le_tue_sur_le_champ() {
    let chain = Chain::new(vec![], true).await;
    chain
        .client
        .call(
            "task.prepare",
            json!({"id":"lente","intent":"Écris ~/docs/code.txt mais attends d'abord","profile":"atelier","model":"codex"}),
        )
        .await
        .unwrap();
    chain
        .client
        .call("task.start", json!({"id":"lente"}))
        .await
        .unwrap();
    // Dès le lancement, la mission est en main : elle ne se relance pas, elle s'annule.
    let info = chain
        .client
        .call("task.inspect", json!({"id":"lente"}))
        .await
        .unwrap();
    assert_eq!(info["can_start"], false, "{info}");
    assert_eq!(info["can_cancel"], true, "{info}");
    // Le client rejoint la mission (elle passe en cours), puis s'attarde.
    let mut rejointe = false;
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let info = chain
            .client
            .call("task.inspect", json!({"id":"lente"}))
            .await
            .unwrap();
        if info["task"]["state"] == "running" {
            rejointe = true;
            break;
        }
    }
    assert!(rejointe, "le client n'a pas rejoint la mission");
    let debut = std::time::Instant::now();
    let reponse = chain
        .client
        .call("task.cancel", json!({"id":"lente"}))
        .await
        .unwrap();
    assert_eq!(reponse["cancel_requested"], "lente", "{reponse}");
    let info = chain.attendre("lente").await;
    assert_eq!(info["task"]["state"], "cancelled", "{info}");
    // Le client tué, sa place est libre bien avant ses trente secondes d'attente : une autre
    // mission sur le même client se lance et finit.
    chain
        .client
        .call(
            "task.prepare",
            json!({"id":"suivante","intent":"Écris ~/docs/code.txt","profile":"atelier","model":"codex"}),
        )
        .await
        .unwrap();
    chain
        .client
        .call("task.start", json!({"id":"suivante"}))
        .await
        .unwrap();
    let info = chain.attendre("suivante").await;
    assert_eq!(info["task"]["state"], "done", "{info}");
    assert!(
        debut.elapsed() < std::time::Duration::from_secs(20),
        "{:?}",
        debut.elapsed()
    );
    assert!(chain.asked.lock().unwrap().is_empty());
}

#[tokio::test]
async fn sans_lanceur_le_client_n_est_pas_propose_et_une_mission_sur_lui_est_refusee() {
    let chain = Chain::new(vec![], false).await;
    let options = chain.client.call("task.options", json!({})).await.unwrap();
    let atelier = options["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "atelier")
        .unwrap();
    assert_eq!(atelier["models"], json!(["modele-controle"]), "{atelier}");
    let refus = chain
        .client
        .call(
            "task.prepare",
            json!({"id":"sans-lanceur","intent":"Écris ~/docs/code.txt","profile":"atelier","model":"codex"}),
        )
        .await
        .unwrap_err();
    assert_eq!(refus.code, ErrorCode::Conflict, "{refus:?}");
    assert!(refus.message.contains("lanceur"), "{}", refus.message);
}

#[tokio::test]
async fn sans_lanceur_de_pilotes_le_role_code_revient_au_modele_local() {
    let chain = Chain::new(
        vec![
            tool_call(
                "task.delegate",
                json!({"intent":"Écris ~/docs/code.txt","profile":"atelier","role":"code"}),
            ),
            tool_call(
                "fs.write",
                json!({"path":"~/docs/code.txt","content":"local"}),
            ),
            fin("Écrit en local."),
            fin("Conclu."),
        ],
        false,
    )
    .await;
    let options = chain.client.call("task.options", json!({})).await.unwrap();
    assert!(options.get("pilot").is_none(), "{options}");
    let atelier = options["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "atelier")
        .unwrap();
    // Sans lanceur, Codex n'est pas proposé ; le rôle garde son modèle local.
    assert_eq!(
        atelier["roles"]["code"],
        json!(["modele-controle"]),
        "{atelier}"
    );
    let parent = chain.run("sans-pilote", "Faire écrire du code").await;
    assert_eq!(parent["task"]["state"], "done", "{parent}");
    let child = chain
        .client
        .call("task.inspect", json!({"id":"sans-pilote.1"}))
        .await
        .unwrap();
    assert_eq!(child["task"]["driver"], "local:modele-controle", "{child}");
    assert_eq!(child["task"]["role"], "code");
    assert_eq!(
        *chain.asked.lock().unwrap(),
        vec![
            "modele-controle",
            "modele-controle",
            "modele-controle",
            "modele-controle"
        ]
    );
}
