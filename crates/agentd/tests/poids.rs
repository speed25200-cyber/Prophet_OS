//! Les poids gérés de bout en bout (M8-T7, ADR 0046) : agentd télécharge une entrée du
//! catalogue par le vrai proxy de sortie, sous un jeton que le vrai capd émet, la vérifie, la
//! pose, la journalise ; une empreinte fausse ne pose rien ; le retrait efface et journalise.
use std::sync::{Arc, Mutex};

use prophet_daemon::essai::{Daemon, binaire_voisin};
use prophet_ipc::Client;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

const AGENTD: &str = env!("CARGO_BIN_EXE_prophet-agentd");

/// Un en-tête GGUF minimal valide, suivi de `remplissage` octets.
fn gguf(remplissage: usize) -> Vec<u8> {
    let mut v = b"GGUF".to_vec();
    v.extend(3u32.to_le_bytes());
    v.extend(0u64.to_le_bytes());
    v.extend(1u64.to_le_bytes());
    let cle = b"general.architecture";
    v.extend((cle.len() as u64).to_le_bytes());
    v.extend(cle);
    v.extend(8u32.to_le_bytes());
    v.extend(5u64.to_le_bytes());
    v.extend(b"qwen3");
    v.extend(std::iter::repeat_n(3u8, remplissage));
    v
}

fn hex(octets: &[u8]) -> String {
    octets.iter().map(|o| format!("{o:02x}")).collect()
}

/// Le dépôt de poids : `/r/<fichier>` redirige vers `/blob/<fichier>`, qui sert le contenu.
async fn depot(contenu: Vec<u8>, recu: Arc<Mutex<Vec<String>>>) -> u16 {
    let ecoute = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = ecoute.local_addr().unwrap().port();
    let contenu = Arc::new(contenu);
    tokio::spawn(async move {
        loop {
            let Ok((flux, _)) = ecoute.accept().await else {
                return;
            };
            let recu = recu.clone();
            let contenu = contenu.clone();
            tokio::spawn(async move {
                let mut lecteur = BufReader::new(flux);
                let mut tete = String::new();
                loop {
                    let mut ligne = String::new();
                    if lecteur.read_line(&mut ligne).await.unwrap_or(0) == 0 || ligne == "\r\n" {
                        break;
                    }
                    tete.push_str(&ligne);
                }
                if tete.is_empty() {
                    return;
                }
                let chemin = tete.split_whitespace().nth(1).unwrap_or("/").to_owned();
                recu.lock().unwrap().push(tete);
                let reponse = if let Some(fichier) = chemin.strip_prefix("/r/") {
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: /blob/{fichier}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .into_bytes()
                } else if chemin.starts_with("/blob/") {
                    let mut r = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        contenu.len()
                    )
                    .into_bytes();
                    r.extend(contenu.iter());
                    r
                } else {
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_vec()
                };
                let _ = lecteur.get_mut().write_all(&reponse).await;
            });
        }
    });
    port
}

struct Chaine {
    dir: tempfile::TempDir,
    _capd: Daemon,
    _ledger: Daemon,
    _egress: Daemon,
    _agentd: Daemon,
    agents: Client,
    journal: Client,
}

impl Chaine {
    /// La chaîne, avec ce catalogue à la place de celui du système s'il est donné.
    async fn new(catalogue: Option<&Value>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let cap = dir.path().join("cap.sock");
        let ledger_socket = dir.path().join("ledger.sock");
        let egress_socket = dir.path().join("egress.sock");
        let capd = Daemon::lancer_avec(
            binaire_voisin("prophet-capd").to_str().unwrap(),
            &cap,
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
        let egress = Daemon::lancer_avec(
            binaire_voisin("prophet-egress").to_str().unwrap(),
            &egress_socket,
            &dir.path().join("egress-state"),
            &[("PROPHET_CAPD_SOCKET", cap.to_str().unwrap())],
        );
        egress
            .attendre_reponse(
                b"GET http://sonde.invalide/ HTTP/1.1\r\nHost: sonde.invalide\r\n\r\n",
            )
            .await;
        let chemin_catalogue = dir.path().join("catalogue.json");
        let poids = dir.path().join("poids").join("catalogue");
        let mut env = vec![
            ("PROPHET_HOME", home.to_str().unwrap()),
            ("PROPHET_CAPD_SOCKET", cap.to_str().unwrap()),
            ("PROPHET_LEDGER_SOCKET", ledger_socket.to_str().unwrap()),
            ("PROPHET_EGRESS_SOCKET", egress_socket.to_str().unwrap()),
            ("PROPHET_PULL_DIR", poids.to_str().unwrap()),
        ];
        if let Some(catalogue) = catalogue {
            std::fs::write(&chemin_catalogue, catalogue.to_string()).unwrap();
            env.push(("PROPHET_MODEL_CATALOG", chemin_catalogue.to_str().unwrap()));
        }
        let agentd = Daemon::lancer_avec(
            AGENTD,
            &dir.path().join("agents.sock"),
            &dir.path().join("agent-state"),
            &env,
        );
        let agents = agentd.joindre().await;
        Self {
            dir,
            _capd: capd,
            _ledger: ledger,
            _egress: egress,
            _agentd: agentd,
            agents,
            journal,
        }
    }

    fn poids(&self) -> std::path::PathBuf {
        self.dir.path().join("poids").join("catalogue")
    }

    /// Attend la fin du téléchargement d'une entrée, dix secondes au plus, et rend son suivi.
    async fn attendre(&self, id: &str) -> Value {
        self.attendre_au_plus(id, std::time::Duration::from_secs(10))
            .await
    }

    async fn attendre_au_plus(&self, id: &str, delai: std::time::Duration) -> Value {
        let limite = std::time::Instant::now() + delai;
        while std::time::Instant::now() < limite {
            let suivis = self.agents.call("model.pulls", json!({})).await.unwrap();
            if let Some(suivi) = suivis.as_array().unwrap().iter().find(|s| s["id"] == id)
                && suivi["state"] != "running"
            {
                return suivi.clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("le téléchargement de {id} ne finit pas");
    }

    async fn evenements(&self, kind: &str) -> Vec<Value> {
        self.journal
            .call("ledger.query", json!({"kinds": [kind]}))
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .clone()
    }
}

fn entree(id: &str, port: u16, sha256: &str) -> Value {
    json!({
        "id": id,
        "name": format!("Essai {id}"),
        "file": format!("{id}.gguf"),
        "url": format!("http://127.0.0.1:{port}/r/{id}.gguf"),
        "sha256": sha256,
        "hosts": ["127.0.0.1"],
    })
}

#[tokio::test]
async fn un_poids_du_catalogue_arrive_par_le_proxy_verifie_et_journalise() {
    let contenu = gguf(700_000);
    let empreinte = hex(&Sha256::digest(&contenu));
    let recu = Arc::new(Mutex::new(Vec::new()));
    let port = depot(contenu.clone(), recu.clone()).await;
    let faux = "0".repeat(64);
    let chaine = Chaine::new(Some(&json!({
        "version": 1,
        "entries": [entree("essai", port, &empreinte), entree("faux", port, &faux)],
    })))
    .await;

    let catalogue = chaine
        .agents
        .call("model.catalog", json!({}))
        .await
        .unwrap();
    let entrees = catalogue["entries"].as_array().unwrap();
    assert_eq!(entrees.len(), 2, "{catalogue}");
    assert_eq!(entrees[0]["installed"], false);

    let depart = chaine
        .agents
        .call("model.pull", json!({"id": "essai"}))
        .await
        .unwrap();
    assert_eq!(depart["state"], "running", "{depart}");
    let fin = chaine.attendre("essai").await;
    assert_eq!(fin["state"], "done", "{fin}");
    assert_eq!(fin["received"], contenu.len());
    let pose = chaine.poids().join("essai.gguf");
    assert_eq!(std::fs::read(&pose).unwrap(), contenu);

    // Le dépôt a vu la redirection suivie, et jamais le jeton.
    let tetes = recu.lock().unwrap().clone();
    assert_eq!(tetes.len(), 2, "{tetes:?}");
    assert!(tetes[0].starts_with("GET /r/essai.gguf "), "{tetes:?}");
    assert!(tetes[1].starts_with("GET /blob/essai.gguf "), "{tetes:?}");
    assert!(
        tetes
            .iter()
            .all(|t| !t.to_ascii_lowercase().contains("proxy-authorization")),
        "le jeton ne sort pas : {tetes:?}"
    );

    let poses = chaine.evenements("model.pulled").await;
    assert_eq!(poses.len(), 1, "{poses:?}");
    assert_eq!(poses[0]["payload"]["id"], "essai");
    assert_eq!(poses[0]["payload"]["sha256"], empreinte);
    assert_eq!(poses[0]["payload"]["bytes"], contenu.len());

    let catalogue = chaine
        .agents
        .call("model.catalog", json!({}))
        .await
        .unwrap();
    assert_eq!(catalogue["entries"][0]["installed"], true, "{catalogue}");

    // Une empreinte qui ne correspond pas : rien n'est posé, ni gardé, ni journalisé.
    chaine
        .agents
        .call("model.pull", json!({"id": "faux"}))
        .await
        .unwrap();
    let fin = chaine.attendre("faux").await;
    assert_eq!(fin["state"], "failed", "{fin}");
    assert!(
        fin["error"].as_str().unwrap().contains("empreinte"),
        "{fin}"
    );
    assert!(!chaine.poids().join("faux.gguf").exists());
    assert!(!chaine.poids().join(".faux.gguf.part").exists());
    assert_eq!(chaine.evenements("model.pulled").await.len(), 1);

    // Hors du catalogue : refusé, avec ce qui est connu.
    let inconnu = chaine
        .agents
        .call("model.pull", json!({"id": "autre"}))
        .await
        .unwrap_err();
    assert_eq!(inconnu.code, prophet_ipc::ErrorCode::NotFound);
    assert!(inconnu.message.contains("essai"), "{}", inconnu.message);

    // Retirer efface le fichier et le journalise.
    let retrait = chaine
        .agents
        .call("model.remove", json!({"id": "essai"}))
        .await
        .unwrap();
    assert_eq!(retrait["removed"], true);
    assert!(!pose.exists());
    let retires = chaine.evenements("model.removed").await;
    assert_eq!(retires.len(), 1, "{retires:?}");
    assert_eq!(retires[0]["payload"]["file"], "essai.gguf");
}

/// Le vrai chemin, de bout en bout : une entrée du catalogue du système arrive de Hugging Face
/// par le vrai egress, TLS terminé par le proxy, redirections du dépôt vers son CDN comprises,
/// sous le jeton que capd émet pour les hôtes de l'entrée — et l'empreinte publiée correspond.
/// C'est aussi ce qui dit si les adresses signées du CDN passent la détection d'exfiltration
/// d'egress (ADR 0046). 640 Mo : réservé aux machines qui joignent le dépôt.
#[tokio::test]
#[ignore = "needs_network: Hugging Face joignable, 640 Mo téléchargés"]
async fn un_vrai_poids_du_catalogue_arrive_de_hugging_face_par_egress() {
    let chaine = Chaine::new(None).await;
    let id = "qwen3-0.6b-q8";
    let debut = std::time::Instant::now();
    let depart = chaine
        .agents
        .call("model.pull", json!({"id": id}))
        .await
        .unwrap();
    assert_eq!(depart["state"], "running", "{depart}");
    let fin = chaine
        .attendre_au_plus(id, std::time::Duration::from_secs(900))
        .await;
    let duree = debut.elapsed();
    assert_eq!(fin["state"], "done", "{fin}");
    let octets = fin["received"].as_u64().unwrap();
    eprintln!(
        "mesure : {id} téléchargé et vérifié par egress, {octets} octets en {duree:?} ({:.0} Mo/s)",
        octets as f64 / 1e6 / duree.as_secs_f64()
    );
    let poses = chaine.evenements("model.pulled").await;
    assert_eq!(poses.len(), 1, "{poses:?}");
    assert_eq!(
        poses[0]["payload"]["sha256"],
        "9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031"
    );
}
