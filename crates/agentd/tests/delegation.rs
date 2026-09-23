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
    /// Pour chaque tour, la consigne de système reçue par le moteur, s'il y en a une.
    briefed: Arc<Mutex<Vec<Option<String>>>>,
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
        Self::build(script, false).await
    }

    /// Le même catalogue, avec un relais de rôles : le chef réfléchit sur `modele-controle`,
    /// le scribe exécute sur `second` (ADR 0034).
    async fn relay(script: Vec<Value>) -> Self {
        Self::build(script, true).await
    }

    async fn build(script: Vec<Value>, relay: bool) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let noted = asked.clone();
        let briefed = Arc::new(Mutex::new(Vec::new()));
        let briefings = briefed.clone();
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
                    briefings.lock().unwrap().push(
                        corps["messages"]
                            .as_array()
                            .and_then(|m| m.first())
                            .filter(|m| m["role"] == "system")
                            .and_then(|m| m["content"].as_str())
                            .map(str::to_owned),
                    );
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
        if relay {
            chef["model"]["roles"] = json!({"reflect": ["local:modele-controle"]});
            // Le scribe admet un modèle que le moteur ne sert pas : le rôle doit passer
            // outre et prendre le premier modèle réellement servi.
            base["manifest"]["model"]["preferred"] =
                json!(["local:modele-controle", "local:absent", "local:second"]);
            base["manifest"]["model"]["roles"] = json!({
                "reflect": ["local:modele-controle"],
                "execute": ["local:absent", "local:second"]
            });
        }
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
            briefed,
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
    // Le refus de capd revient au modèle, qui conclut (ADR 0050) ; aucune sous-mission n'a
    // existé.
    assert_eq!(parent["task"]["state"], "done", "{parent}");
    assert_eq!(parent["result"]["text"], "Refusé, je m'arrête.", "{parent}");
    // Seul le modèle du parent a été interrogé : avant et après le refus.
    assert_eq!(
        *chain.asked.lock().unwrap(),
        vec!["modele-controle", "modele-controle"]
    );
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

/// Le relais par rôle (ADR 0034) : le parent, modèle de réflexion, confie une étape au rôle
/// d'exécution sans nommer de modèle ; le service choisit celui que le contexte admet pour ce
/// rôle parmi ceux que le moteur sert, briefe chacun sur son rôle, et le compte par modèle dit
/// ensuite ce que chacun a coûté.
#[tokio::test]
async fn un_role_designe_le_modele_du_contexte_et_le_compte_par_modele_le_dit() {
    let chain = Chain::relay(vec![
        tool_call(
            "task.delegate",
            json!({"intent":"Écris « relais » dans ~/docs/note.txt","profile":"scribe","role":"execute"}),
        ),
        tool_call("fs.write", json!({"path":"~/docs/note.txt","content":"relais"})),
        fin("Note écrite."),
        fin("Relais terminé."),
    ])
    .await;
    // Le catalogue annonce les rôles et ne garde, pour chacun, que les modèles servis.
    let options = chain.client.call("task.options", json!({})).await.unwrap();
    let scribe = options["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "scribe")
        .unwrap();
    assert_eq!(scribe["roles"]["execute"], json!(["second"]), "{scribe}");
    assert_eq!(scribe["roles"]["reflect"], json!(["modele-controle"]));

    let parent = chain
        .run("relais", "Faire écrire une note par le rôle d'exécution")
        .await;
    assert_eq!(parent["task"]["state"], "done", "{parent}");
    // « absent » n'est pas servi : le rôle prend « second », le premier modèle réellement servi.
    assert_eq!(
        *chain.asked.lock().unwrap(),
        vec!["modele-controle", "second", "second", "modele-controle"]
    );
    // Chaque mission connaît son rôle.
    assert_eq!(parent["task"]["role"], "reflect", "{parent}");
    let child = chain
        .client
        .call("task.inspect", json!({"id":"relais.1"}))
        .await
        .unwrap();
    assert_eq!(child["task"]["state"], "done", "{child}");
    assert_eq!(child["task"]["role"], "execute", "{child}");
    // Chacun a reçu la consigne de son rôle, à chaque tour ; la consigne du parent nomme le
    // contexte qu'il peut confier et les rôles que celui-ci sait jouer.
    let briefed = chain.briefed.lock().unwrap().clone();
    assert_eq!(briefed.len(), 4);
    let parent_brief = briefed[0].as_deref().expect("consigne du parent");
    assert!(parent_brief.contains("réflexion"), "{parent_brief}");
    assert!(
        parent_brief.contains("scribe (reflect, execute)"),
        "{parent_brief}"
    );
    let child_brief = briefed[1].as_deref().expect("consigne de l'enfant");
    assert!(child_brief.contains("exécution"), "{child_brief}");
    assert!(!child_brief.contains("task.delegate"), "{child_brief}");
    assert_eq!(briefed[3], briefed[0]);
    // Le compte par modèle : l'enfant a coûté deux tours de « second » (12+8, 32+3) ; le parent
    // porte ses deux tours de « modele-controle » et, imputés, ceux de l'enfant.
    assert_eq!(child["task"]["usage"]["local:second"]["turns"], 2);
    assert_eq!(child["task"]["usage"]["local:second"]["tokens_in"], 44);
    assert_eq!(child["task"]["usage"]["local:second"]["tokens_out"], 11);
    assert!(child["task"]["usage"]["local:modele-controle"].is_null());
    assert_eq!(parent["task"]["usage"]["local:modele-controle"]["turns"], 2);
    assert_eq!(parent["task"]["usage"]["local:second"]["turns"], 2);
    assert_eq!(parent["task"]["budget"]["spent"]["tokens"], 110, "{parent}");
    // Le résultat durable le porte aussi, avec le pilote de la mission.
    let result = chain
        .client
        .call("task.result", json!({"id":"relais"}))
        .await
        .unwrap();
    assert_eq!(result["usage"]["local:second"]["turns"], 2, "{result}");
    assert_eq!(result["driver"], "local:modele-controle");
    assert_eq!(result["role"], "reflect");
}

/// Un rôle que le contexte ne définit pas est une erreur d'outil nommée, pas une sous-mission :
/// le parent l'apprend et poursuit.
#[tokio::test]
async fn un_role_que_le_contexte_ne_definit_pas_est_refuse_sans_sous_mission() {
    let chain = Chain::relay(vec![
        tool_call(
            "task.delegate",
            json!({"intent":"Écris du code","profile":"scribe","role":"code"}),
        ),
        fin("Le contexte ne code pas ; je conclus sans déléguer."),
    ])
    .await;
    let parent = chain.run("sans-code", "Tenter un rôle absent").await;
    assert_eq!(parent["task"]["state"], "done", "{parent}");
    assert_eq!(
        *chain.asked.lock().unwrap(),
        vec!["modele-controle", "modele-controle"]
    );
    let list = chain.client.call("task.list", json!({})).await.unwrap();
    assert!(
        !list.to_string().contains("sans-code.1"),
        "aucune sous-mission ne doit exister : {list}"
    );
    // Le journal porte le refus de l'outil, avec le rôle en cause.
    let journal = chain
        .client
        .call("task.inspect", json!({"id":"sans-code"}))
        .await
        .unwrap();
    assert!(journal["task"]["usage"]["local:second"].is_null());
    assert_eq!(
        journal["task"]["usage"]["local:modele-controle"]["turns"],
        2
    );
}

/// Deux vrais modèles sur un vrai moteur : le modèle de réflexion (`PROPHET_TEST_REFLECT`)
/// reçoit un objectif qu'il doit confier au rôle d'exécution (`PROPHET_TEST_EXECUTE`), sur
/// `PROPHET_TEST_ENDPOINT` (un llama-server en mode routeur sert les deux). Ce que le test
/// prouve : le relais tourne de bout en bout avec les vrais capd, ledger et agentd, la
/// sous-mission a bien été jouée par le modèle d'exécution, et le compte par modèle le dit.
/// Ce qu'il ne prouve pas : qu'un petit modèle suit toujours la consigne ; le rapport note
/// le résultat de chaque essai.
#[tokio::test]
#[ignore = "needs_local_models: PROPHET_TEST_ENDPOINT, PROPHET_TEST_REFLECT et PROPHET_TEST_EXECUTE"]
async fn deux_modeles_reels_se_relaient_et_le_compte_le_dit() {
    let endpoint = std::env::var("PROPHET_TEST_ENDPOINT").unwrap();
    let reflect = std::env::var("PROPHET_TEST_REFLECT").unwrap();
    let execute = std::env::var("PROPHET_TEST_EXECUTE").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join("docs")).unwrap();
    let mut base: Value =
        serde_json::from_str(include_str!("../../../examples/missions/note-locale.json")).unwrap();
    base["manifest"]["model"]["preferred"] =
        json!([format!("local:{reflect}"), format!("local:{execute}")]);
    base["manifest"]["model"]["roles"] = json!({
        "reflect": [format!("local:{reflect}")],
        "execute": [format!("local:{execute}")]
    });
    base["manifest"]["capabilities"]["max"]["fs.read"] = json!(["~/docs/**"]);
    base["manifest"]["capabilities"]["max"]["fs.write"] = json!(["~/docs/**"]);
    base["manifest"]["capabilities"]["max"]["tool.call"] = json!(["fs.read", "fs.write"]);
    base["manifest"]["budget"]["default"]["tokens"] = json!(40_000);
    base["manifest"]["budget"]["default"]["wall_time"] = json!("600s");
    let mut chef = base["manifest"].clone();
    chef["agent"]["id"] = json!("org.prophet.chef");
    chef["capabilities"]["max"]["task.spawn"] = json!(["scribe"]);
    // Le chef garde tous les droits du scribe : capd ne délègue qu'un sous-ensemble du jeton
    // parent, et un enfant qui pourrait plus que son parent serait refusé.
    chef["capabilities"]["max"]["tool.call"] = json!(["fs.read", "fs.write", "task.delegate"]);
    let profiles = dir.path().join("profiles.json");
    std::fs::write(&profiles, json!([
        {"id":"chef", "name":"Chef", "description":"Réfléchit et confie", "manifest":chef, "scopes":["~/docs"]},
        {"id":"scribe", "name":"Scribe", "description":"Exécute", "manifest":base["manifest"], "scopes":["~/docs"]}
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
    let agentd = Daemon::lancer_avec(
        AGENTD,
        &dir.path().join("agent.sock"),
        &dir.path().join("agent-state"),
        &[
            ("PROPHET_HOME", home.to_str().unwrap()),
            ("PROPHET_CAPD_SOCKET", caps.to_str().unwrap()),
            ("PROPHET_LEDGER_SOCKET", logs.to_str().unwrap()),
            (
                "PROPHET_EGRESS_SOCKET",
                dir.path().join("egress.sock").to_str().unwrap(),
            ),
            ("PROPHET_LOCAL_ENDPOINT", &endpoint),
            ("PROPHET_MISSION_PROFILES", profiles.to_str().unwrap()),
        ],
    );
    let client = agentd.joindre().await;
    let options = client.call("task.options", json!({})).await.unwrap();
    let scribe = options["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "scribe")
        .unwrap();
    assert_eq!(scribe["roles"]["execute"], json!([execute]), "{options}");

    let debut = std::time::Instant::now();
    client
        .call(
            "task.prepare",
            json!({"id":"relais-reel", "intent":"Tu ne dois pas écrire toi-même. Confie par l'outil task.delegate, au contexte scribe avec role=execute, cet objectif exact : « Écris le texte PROPHET_RELAIS_OK dans le fichier ~/docs/relais.txt ». Quand le résultat te revient, réponds en une phrase.", "profile":"chef", "model":reflect}),
        )
        .await
        .unwrap();
    client
        .call("task.start", json!({"id":"relais-reel"}))
        .await
        .unwrap();
    let mut info = Value::Null;
    for _ in 0..6000 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        info = client
            .call("task.inspect", json!({"id":"relais-reel"}))
            .await
            .unwrap();
        if matches!(
            info["task"]["state"].as_str(),
            Some("done" | "failed" | "cancelled")
        ) {
            break;
        }
    }
    let duree = debut.elapsed();
    let list = client.call("task.list", json!({})).await.unwrap();
    let enfants: Vec<&Value> = list
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["parent"] == "relais-reel")
        .collect();
    let note = home.join(".prophet/tasks/relais-reel.1/work/docs/relais.txt");
    let contenu = std::fs::read_to_string(&note).unwrap_or_default();
    eprintln!(
        "relais réel : état {} en {:.1} s ; {} sous-mission(s) ; compte {} ; fichier « {} »",
        info["task"]["state"],
        duree.as_secs_f64(),
        enfants.len(),
        info["task"]["usage"],
        contenu.trim()
    );
    assert_eq!(info["task"]["state"], "done", "{info}");
    assert_eq!(info["task"]["role"], "reflect");
    assert!(
        !enfants.is_empty(),
        "le modèle de réflexion n'a rien confié : {info}"
    );
    assert_eq!(enfants[0]["role"], "execute", "{list}");
    assert_eq!(enfants[0]["driver"], format!("local:{execute}"), "{list}");
    let usage = &info["task"]["usage"];
    assert!(
        usage[format!("local:{execute}")]["turns"]
            .as_u64()
            .unwrap_or(0)
            >= 1,
        "{usage}"
    );
    assert!(
        usage[format!("local:{reflect}")]["turns"]
            .as_u64()
            .unwrap_or(0)
            >= 2,
        "{usage}"
    );
    assert!(contenu.contains("PROPHET_RELAIS_OK"), "{contenu:?}");
}
