//! Préparer depuis une intention ne doit ni inférer ni exécuter ; les droits viennent du service.
use std::io::{Read as _, Write as _};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use prophet_daemon::essai::{Daemon, binaire_voisin};
use prophet_ipc::{Client, ErrorCode};
use serde_json::{Value, json};

struct Chain {
    _dir: tempfile::TempDir,
    _daemons: Vec<Daemon>,
    stop: Arc<AtomicBool>,
    model: Option<std::thread::JoinHandle<()>>,
    client: Client,
}

impl Drop for Chain {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.model.take().unwrap().join().unwrap();
    }
}

impl Chain {
    async fn new() -> Self {
        Self::with_env(&[]).await
    }

    async fn with_env(extra: &[(&str, &str)]) -> Self {
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
                    "la préparation ne lance pas d'inférence"
                );
                let body =
                    json!({"data":[{"id":"modele-controle"},{"id":"non-autorise"}]}).to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join("docs")).unwrap();
        let mut example: Value =
            serde_json::from_str(include_str!("../../../examples/missions/note-locale.json"))
                .unwrap();
        example["manifest"]["model"]["preferred"] =
            json!(["local:modele-controle", "local:hors-ligne"]);
        example["manifest"]["capabilities"]["max"]["fs.read"] = json!(["~/docs/**"]);
        example["manifest"]["capabilities"]["max"]["fs.write"] = json!(["~/docs/**"]);
        let profiles = dir.path().join("profiles.json");
        let mut web = example["manifest"].clone();
        web["capabilities"]["max"]["net.egress"] = json!(["127.0.0.1"]);
        web["capabilities"]["max"]["ui.read"] = json!(["browser"]);
        web["capabilities"]["max"]["ui.act"] = json!(["browser"]);
        web["capabilities"]["max"]["tool.call"] = json!([
            "fs.read",
            "fs.write",
            "http.fetch",
            "web.open",
            "web.tree",
            "web.act"
        ]);
        std::fs::write(&profiles, json!([
            {"id":"documents", "name":"Documents de travail", "description":"Préparer des fichiers dans docs", "manifest":example["manifest"], "scopes":["~/docs"]},
            {"id":"web", "name":"Recherche sur le web", "description":"Consulter le web", "manifest":web, "scopes":["~/docs"]}
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
            env!("CARGO_BIN_EXE_prophet-agentd"),
            &dir.path().join("agent.sock"),
            &dir.path().join("agent-state"),
            &[
                ("PROPHET_HOME", home.to_str().unwrap()),
                ("PROPHET_CAPD_SOCKET", caps.to_str().unwrap()),
                ("PROPHET_LEDGER_SOCKET", logs.to_str().unwrap()),
                ("PROPHET_LOCAL_ENDPOINT", &endpoint),
                ("PROPHET_MISSION_PROFILES", profiles.to_str().unwrap()),
            ]
            .iter()
            .chain(extra)
            .copied()
            .collect::<Vec<_>>(),
        );
        let client = agentd.joindre().await;
        Self {
            _dir: dir,
            _daemons: vec![agentd, ledger, capd],
            stop,
            model: Some(model),
            client,
        }
    }
}

#[tokio::test]
async fn une_intention_devient_un_plan_sans_execution_et_sans_droits_fournis_par_le_client() {
    let chain = Chain::new().await;
    let options = chain.client.call("task.options", json!({})).await.unwrap();
    assert_eq!(options["profiles"][0]["models"], json!(["modele-controle"]));
    assert_eq!(options["profiles"][0]["scopes"], json!(["~/docs"]));
    assert!(options["profiles"][0].get("manifest").is_none());
    let input = json!({"id":"depuis-interface", "intent":"Rédiger une note", "profile":"documents", "model":"modele-controle"});
    let plan = chain
        .client
        .call("task.prepare", input.clone())
        .await
        .unwrap();
    assert_eq!(plan["task"], "depuis-interface");
    assert_eq!(plan["scopes"], json!(["~/docs"]));
    assert_eq!(plan["choice"]["reference"], "local:modele-controle");
    let info = chain
        .client
        .call("task.inspect", json!({"id":"depuis-interface"}))
        .await
        .unwrap();
    assert_eq!(info["task"]["state"], "planned");
    assert_eq!(
        info["task"]["user"],
        format!("uid:{}", prophet_daemon::uid_propre().unwrap())
    );
    assert!(info["result"].is_null());
    assert!(
        !chain
            ._dir
            .path()
            .join("home/.prophet/tasks/depuis-interface/work")
            .exists()
    );
    assert_eq!(
        chain
            .client
            .call("task.prepare", input)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        chain
            .client
            .call("task.list", json!({}))
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn les_parametres_absents_invalides_ou_les_elargissements_ne_creent_pas_de_mission() {
    let chain = Chain::new().await;
    let input = json!({"id":"refus", "intent":"Rédiger une note", "profile":"documents", "model":"modele-controle"});
    for (key, value) in [
        ("user", json!("root")),
        ("scopes", json!(["~/"])),
        ("manifest", json!({})),
        ("model", json!("non-autorise")),
        ("model", json!("hors-ligne")),
        ("profile", json!("absent")),
        ("intent", json!("   ")),
        ("intent", json!("x".repeat(16385))),
        ("id", json!("../echappe")),
    ] {
        let mut bad = input.clone();
        bad[key] = value;
        let error = chain.client.call("task.prepare", bad).await.unwrap_err();
        assert!(
            matches!(
                error.code,
                ErrorCode::InvalidParams | ErrorCode::PolicyDenied | ErrorCode::Conflict
            ),
            "champ {key} : {error:?}"
        );
    }
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

#[tokio::test]
async fn un_contexte_web_annonce_son_navigateur_et_n_est_pas_prepare_sans_lui() {
    let chain = Chain::new().await;
    let options = chain.client.call("task.options", json!({})).await.unwrap();
    assert_eq!(options["profiles"][0]["web"], json!(false));
    assert_eq!(options["profiles"][1]["id"], "web");
    assert_eq!(options["profiles"][1]["web"], json!(true));
    let grants = options["profiles"][1]["grants"].to_string();
    assert!(
        grants.contains("net.egress sur 127.0.0.1") && grants.contains("ui.act sur browser"),
        "{grants}"
    );
    // Aucun PROPHET_BROWSER pour ce service : l'état est absent, pas « en panne ».
    assert!(options["browser"].is_null(), "{options}");
    let error = chain
        .client
        .call(
            "task.prepare",
            json!({"id":"sur-le-web", "intent":"Lire une page", "profile":"web", "model":"modele-controle"}),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PolicyDenied);
    assert!(error.message.contains("navigateur"), "{error:?}");
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

/// Le navigateur des essais, comme dans `local_daemon.rs` ; sans lui le test se tait, sauf si
/// `PROPHET_EXIGER_NAVIGATEUR=1` transforme cette absence en échec.
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

#[tokio::test]
async fn avec_un_navigateur_qui_repond_un_contexte_web_est_prepare_sans_execution() {
    let Some(navigateur) = navigateur_des_essais() else {
        return;
    };
    let chain = Chain::with_env(&[("PROPHET_BROWSER", &navigateur)]).await;
    // La sonde tourne après l'ouverture du socket : le catalogue répond tout de suite, avec
    // « sonde en cours », puis avec le verdict.
    let mut browser = Value::Null;
    for _ in 0..300 {
        let options = chain.client.call("task.options", json!({})).await.unwrap();
        browser = options["browser"].clone();
        assert_eq!(browser["program"], navigateur, "{browser}");
        if browser["detail"] != "sonde en cours" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(browser["ready"], json!(true), "{browser}");
    assert!(
        browser["detail"].as_str().unwrap().contains("Chrome"),
        "{browser}"
    );
    let plan = chain
        .client
        .call(
            "task.prepare",
            json!({"id":"sur-le-web", "intent":"Lire une page", "profile":"web", "model":"modele-controle"}),
        )
        .await
        .unwrap();
    assert_eq!(plan["task"], "sur-le-web");
    let info = chain
        .client
        .call("task.inspect", json!({"id":"sur-le-web"}))
        .await
        .unwrap();
    assert_eq!(info["task"]["state"], "planned");
    assert!(info["result"].is_null());
}

#[test]
fn le_catalogue_refuse_les_profils_hors_perimetre_et_accepte_une_ecriture_plus_etroite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profiles.json");
    let example: Value =
        serde_json::from_str(include_str!("../../../examples/missions/note-locale.json")).unwrap();
    let mut profile = json!({"id":"docs","name":"Documents","description":"Notes","manifest":example["manifest"],"scopes":["~/Documents/Prophet"]});
    profile["manifest"]["capabilities"]["max"]["fs.write"] = json!(["~/Documents/Prophet/out/**"]);
    std::fs::write(&path, json!([profile]).to_string()).unwrap();
    assert!(agentd::preparation::load(&path).is_ok());
    // Le web relayé par egress est admis : hôtes, navigateur piloté et outils réseau nommés.
    let mut web = profile.clone();
    web["manifest"]["capabilities"]["max"]["net.egress"] = json!(["*.exemple.fr", "*"]);
    web["manifest"]["capabilities"]["max"]["ui.read"] = json!(["browser"]);
    web["manifest"]["capabilities"]["max"]["ui.act"] = json!(["browser"]);
    web["manifest"]["capabilities"]["max"]["tool.call"] = json!([
        "fs.read",
        "fs.write",
        "http.fetch",
        "web.open",
        "web.tree",
        "web.act"
    ]);
    std::fs::write(&path, json!([web]).to_string()).unwrap();
    let loaded = agentd::preparation::load(&path).unwrap();
    assert!(loaded[0].uses_browser());
    assert!(loaded[0].view(&[]).web);
    let mut fetch_only = profile.clone();
    fetch_only["manifest"]["capabilities"]["max"]["net.egress"] = json!(["exemple.fr"]);
    fetch_only["manifest"]["capabilities"]["max"]["tool.call"] =
        json!(["fs.read", "fs.write", "http.fetch"]);
    std::fs::write(&path, json!([fetch_only]).to_string()).unwrap();
    let loaded = agentd::preparation::load(&path).unwrap();
    assert!(!loaded[0].uses_browser() && !loaded[0].view(&[]).web);
    for bad in [
        {
            let mut p = profile.clone();
            p["scopes"] = json!(["~/Documents/../secrets"]);
            p
        },
        {
            let mut p = profile.clone();
            p["manifest"]["capabilities"]["max"]["fs.read"] = json!(["~/**"]);
            p
        },
        {
            // Un outil réseau sans aucun hôte ne ferait que des refus.
            let mut p = profile.clone();
            p["manifest"]["capabilities"]["max"]["tool.call"] = json!(["fs.read", "http.fetch"]);
            p
        },
        {
            let mut p = profile.clone();
            p["manifest"]["capabilities"]["max"]["net.egress"] = json!(["exemple.fr"]);
            p["manifest"]["capabilities"]["max"]["tool.call"] = json!(["fs.read", "web.open"]);
            p["manifest"]["capabilities"]["max"]["ui.read"] = json!(["screen"]);
            p
        },
        {
            let mut p = profile.clone();
            p["manifest"]["capabilities"]["max"]["net.egress"] = json!(["exemple.fr"]);
            p["manifest"]["capabilities"]["max"]["tool.call"] = json!(["fs.read", "proc.exec"]);
            p
        },
        {
            let mut p = profile.clone();
            p["manifest"]["capabilities"]["max"]["tool.call"] = json!(["*"]);
            p
        },
        {
            let mut p = profile.clone();
            p["manifest"]["sandbox"]["min_level"] = json!(2);
            p
        },
    ] {
        std::fs::write(&path, json!([bad]).to_string()).unwrap();
        assert!(agentd::preparation::load(&path).is_err());
    }
    std::fs::write(&path, json!([profile, profile]).to_string()).unwrap();
    assert!(agentd::preparation::load(&path).is_err());
}
