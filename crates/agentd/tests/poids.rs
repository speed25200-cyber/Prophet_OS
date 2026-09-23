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
    cap: std::path::PathBuf,
    egress: std::path::PathBuf,
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
            cap,
            egress: egress_socket,
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

/// Le dépôt, l'organisation et la révision d'une adresse `…/<org>/<dépôt>/resolve/<rév>/<fichier>`.
fn depot_de(url: &str) -> (String, String, String) {
    let chemin = url.strip_prefix("https://huggingface.co/").unwrap();
    let (depot, reste) = chemin.split_once("/resolve/").unwrap();
    let (revision, fichier) = reste.split_once('/').unwrap();
    (depot.to_owned(), revision.to_owned(), fichier.to_owned())
}

/// Le catalogue ne porte que des empreintes publiées : pour chaque entrée, l'API de Hugging Face,
/// lue par le vrai egress sous un jeton de capd, dit l'empreinte et la taille du fichier à la
/// révision épinglée, et elles doivent être celles du catalogue. L'essai relève aussi ce que le
/// dépôt publie des candidats à inscrire (`qwen3-8b-q4`, l'exemple du plan), pour qu'une entrée
/// n'entre qu'avec une empreinte relevée à la source.
#[tokio::test]
#[ignore = "needs_network: Hugging Face joignable"]
async fn le_catalogue_porte_les_empreintes_que_le_depot_publie() {
    let chaine = Chaine::new(None).await;
    let catalogue = providers::catalogue::Catalogue::builtin();
    let entree = catalogue.entries[0].clone();
    let manifeste = agentd::poids::manifest(&entree).unwrap();
    let capd = Client::connect(&chaine.cap).await.unwrap();
    let jeton: prophet_types::cap::Token = serde_json::from_value(
        capd.call(
            "cap.mint",
            json!({
                "manifest": manifeste,
                "grants": agentd::poids::grants(&entree),
                "task": "essai:empreintes",
                "user": "essai",
                "ttl_seconds": 600,
            }),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    let egress = providers::pull::Egress::new(chaine.egress.clone(), &jeton).unwrap();
    let essayer = move |url: String| {
        let url = providers::catalogue::Url::parse(&url).unwrap();
        providers::pull::get_json(&egress, &url, 4 * 1024 * 1024)
            .map_err(|e| format!("{} : {e}", url.full()))
    };
    let essayer = std::sync::Arc::new(essayer);
    let e = essayer.clone();
    let lire = std::sync::Arc::new(move |url: String| e(url).unwrap_or_else(|m| panic!("{m}")));
    for entree in &catalogue.entries {
        let (depot, revision, fichier) = depot_de(&entree.url);
        let l = lire.clone();
        let url = format!("https://huggingface.co/api/models/{depot}/tree/{revision}");
        let arbre = tokio::task::spawn_blocking(move || l(url)).await.unwrap();
        let publie = arbre
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"] == fichier.as_str())
            .unwrap_or_else(|| panic!("{fichier} absent de {depot}@{revision}"))
            .clone();
        eprintln!(
            "mesure : {} publié {} octets, sha256 {}",
            entree.id, publie["size"], publie["lfs"]["oid"]
        );
        assert_eq!(
            publie["lfs"]["oid"],
            entree.sha256.as_str(),
            "{}",
            entree.id
        );
        if let Some(octets) = entree.bytes {
            assert_eq!(publie["size"], octets, "{}", entree.id);
        }
    }
    // D'autres familles, à licence ouverte, pour valider le moteur au-delà de Qwen3.
    for (depot, motif) in [
        ("Qwen/Qwen3-8B-GGUF", "q4_k_m.gguf"),
        ("Qwen/Qwen3-4B-GGUF", "q4_k_m.gguf"),
        ("ibm-granite/granite-3.3-2b-instruct-GGUF", "q4_k_m.gguf"),
        ("HuggingFaceTB/SmolLM2-1.7B-Instruct-GGUF", "q4_k_m.gguf"),
        ("microsoft/Phi-3-mini-4k-instruct-gguf", "q4.gguf"),
        ("bartowski/Llama-3.2-3B-Instruct-GGUF", "q4_k_m.gguf"),
    ] {
        // Un candidat introuvable se dit ; il ne fait pas échouer la vérification du catalogue.
        let l = essayer.clone();
        let url = format!("https://huggingface.co/api/models/{depot}");
        let modele = match tokio::task::spawn_blocking(move || l(url)).await.unwrap() {
            Ok(modele) => modele,
            Err(erreur) => {
                eprintln!("mesure : candidat {depot} illisible : {erreur}");
                continue;
            }
        };
        let Some(revision) = modele["sha"].as_str().map(str::to_owned) else {
            eprintln!("mesure : candidat {depot} sans révision");
            continue;
        };
        eprintln!(
            "mesure : dépôt {depot} licence {} ; accès restreint : {}",
            modele["cardData"]["license"], modele["gated"]
        );
        let l = essayer.clone();
        let url = format!("https://huggingface.co/api/models/{depot}/tree/{revision}");
        let Ok(arbre) = tokio::task::spawn_blocking(move || l(url)).await.unwrap() else {
            eprintln!("mesure : candidat {depot} : arbre illisible");
            continue;
        };
        for f in arbre.as_array().into_iter().flatten() {
            if f["path"]
                .as_str()
                .is_some_and(|p| p.to_ascii_lowercase().ends_with(motif))
            {
                eprintln!(
                    "mesure : candidat https://huggingface.co/{depot}/resolve/{revision}/{} {} octets, sha256 {}",
                    f["path"].as_str().unwrap(),
                    f["size"],
                    f["lfs"]["oid"]
                );
            }
        }
    }
}

/// Une requête HTTP sur la boucle locale ; rend le corps de la réponse.
fn http_local(port: u16, methode: &str, chemin: &str, corps: &Value) -> Value {
    use std::io::{Read as _, Write as _};
    let corps = corps.to_string();
    let mut flux = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    flux.set_read_timeout(Some(std::time::Duration::from_secs(600)))
        .unwrap();
    write!(
        flux,
        "{methode} {chemin} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{corps}",
        corps.len()
    )
    .unwrap();
    let mut reponse = Vec::new();
    flux.read_to_end(&mut reponse).unwrap();
    let fin_tete = reponse
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("réponse sans en-tête");
    let tete = String::from_utf8_lossy(&reponse[..fin_tete]).into_owned();
    let mut corps = reponse[fin_tete + 4..].to_vec();
    assert!(
        tete.starts_with("HTTP/1.1 200"),
        "{methode} {chemin} : {tete}\n{}",
        String::from_utf8_lossy(&corps)
    );
    // Le moteur répond en morceaux (`Transfer-Encoding: chunked`) : on les recolle, en octets.
    if tete
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        let mut entier = Vec::new();
        let mut reste = corps.as_slice();
        while let Some(fin) = reste.windows(2).position(|w| w == b"\r\n") {
            let taille = String::from_utf8_lossy(&reste[..fin]);
            let n = usize::from_str_radix(taille.trim(), 16).unwrap_or(0);
            if n == 0 {
                break;
            }
            entier.extend_from_slice(&reste[fin + 2..fin + 2 + n]);
            reste = &reste[(fin + 4 + n).min(reste.len())..];
        }
        corps = entier;
    }
    serde_json::from_slice(&corps)
        .unwrap_or_else(|e| panic!("{chemin} : {e} : {}", String::from_utf8_lossy(&corps)))
}

#[test]
fn une_reponse_en_morceaux_se_recolle() {
    // La forme exacte que le routeur épinglé a rendue en CI, un caractère accentué en plus.
    let ecoute = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = ecoute.local_addr().unwrap().port();
    std::thread::spawn(move || {
        use std::io::{Read as _, Write as _};
        let (mut flux, _) = ecoute.accept().unwrap();
        // Lire la requête entière avant de répondre : fermer sur des octets non lus ferait
        // envoyer un RST au client, qui verrait « Connection reset » au lieu de la réponse.
        let mut recu = Vec::new();
        let mut tampon = [0u8; 4096];
        while !recu.ends_with(b"\r\n\r\n{}") {
            let n = flux.read(&mut tampon).unwrap();
            assert!(n > 0, "requête tronquée");
            recu.extend_from_slice(&tampon[..n]);
        }
        let a = "{\"choices\":[{\"message\":{\"content\":\"bonjour é".as_bytes();
        let b = "\"}}],\"usage\":{\"completion_tokens\":7}}".as_bytes();
        let mut r = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        // Le premier morceau coupe le « é » en deux.
        let coupe = a.len() - 1;
        r.extend(format!("{coupe:x}\r\n").as_bytes());
        r.extend(&a[..coupe]);
        r.extend(b"\r\n");
        let second = [&a[coupe..], b].concat();
        r.extend(format!("{:x}\r\n", second.len()).as_bytes());
        r.extend(&second);
        r.extend(b"\r\n0\r\n\r\n");
        flux.write_all(&r).unwrap();
    });
    let lu = http_local(port, "POST", "/v1/chat/completions", &json!({}));
    assert_eq!(lu["choices"][0]["message"]["content"], "bonjour é");
    assert_eq!(lu["usage"]["completion_tokens"], 7);
}

/// Le critère de M8-T7, par les vrais binaires : `prophet model pull qwen3-8b-q4` (agentd, capd,
/// egress, Hugging Face), le vrai routeur épinglé lancé comme l'image le lance (préréglages avec
/// section `[*]`, dossier des téléchargements), `prophet model serve qwen3-8b-q4`, puis une
/// complétion qui aboutit. 5 Go téléchargés : un travail de la CI qui a le moteur et le réseau.
#[tokio::test]
#[ignore = "needs_llama_server: PROPHET_TEST_LLAMA_SERVER (llama-server épinglé), Hugging Face joignable, 5 Go"]
async fn le_critere_pull_serve_puis_une_completion() {
    let moteur = std::env::var("PROPHET_TEST_LLAMA_SERVER")
        .expect("PROPHET_TEST_LLAMA_SERVER : le llama-server épinglé (nix build .#llama-cpp)");
    let chaine = Chaine::new(None).await;
    let cli = binaire_voisin("prophet");
    let agents = chaine.dir.path().join("agents.sock");
    let id = "qwen3-8b-q4";

    let debut = std::time::Instant::now();
    let tire = tokio::process::Command::new(&cli)
        .args(["model", "pull", id])
        .env("PROPHET_AGENTD_SOCKET", &agents)
        .output()
        .await
        .unwrap();
    let dit = String::from_utf8_lossy(&tire.stdout).into_owned();
    assert!(
        tire.status.success(),
        "{dit}\n{}",
        String::from_utf8_lossy(&tire.stderr)
    );
    assert!(dit.contains("téléchargé et vérifié"), "{dit}");
    eprintln!(
        "mesure : prophet model pull {id} en {:?} ; {}",
        debut.elapsed(),
        dit.trim()
    );

    // Le routeur, comme l'image le lance en mode relais, lit le dossier à son démarrage.
    let prereglages = chaine.dir.path().join("prereglages.ini");
    std::fs::write(
        &prereglages,
        "[*]\njinja = 1\nctx-size = 2048\nthreads = 4\nparallel = 1\nn-gpu-layers = 0\n",
    )
    .unwrap();
    let port = 18_099;
    let mut routeur = tokio::process::Command::new(&moteur)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .arg("--models-preset")
        .arg(&prereglages)
        .args(["--models-max", "1", "--models-dir"])
        .arg(chaine.poids())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let limite = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            std::time::Instant::now() < limite,
            "le routeur ne répond pas"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    let debut = std::time::Instant::now();
    let sert = tokio::process::Command::new(&cli)
        .args(["--json", "model", "serve", id, "--endpoint"])
        .arg(format!("http://127.0.0.1:{port}/v1"))
        .env("PROPHET_LOCAL_CONTEXT", "2048")
        .env("PROPHET_AGENTD_SOCKET", &agents)
        .output()
        .await
        .unwrap();
    assert!(
        sert.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&sert.stdout),
        String::from_utf8_lossy(&sert.stderr)
    );
    let servi: Value = serde_json::from_slice(&sert.stdout).unwrap();
    let nom = servi["model"].as_str().unwrap().to_owned();
    eprintln!(
        "mesure : prophet model serve {id} en {:?} ; servi sous {nom:?} ; mémoire estimée {}",
        debut.elapsed(),
        servi["memory"]
    );
    let fichier = servi["path"].as_str().unwrap_or_default().to_owned();

    let debut = std::time::Instant::now();
    let reponse = tokio::task::spawn_blocking(move || {
        http_local(
            port,
            "POST",
            "/v1/chat/completions",
            &json!({
                "model": nom,
                "messages": [{"role": "user", "content": "Réponds par un seul mot : bonjour. /no_think"}],
                "max_tokens": 16,
                "temperature": 0,
            }),
        )
    })
    .await
    .unwrap();
    let texte = reponse["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    eprintln!(
        "mesure : complétion de {id} en {:?} : {texte:?} ({})",
        debut.elapsed(),
        reponse["usage"]
    );
    assert!(
        reponse["usage"]["completion_tokens"].as_u64().unwrap_or(0) > 0,
        "{reponse}"
    );
    if let Some(r) = routeur
        .id()
        .and_then(|pid| memoire_de_l_instance(pid, &fichier))
    {
        eprintln!(
            "mesure : mémoire de {id} : estimée {} ; résidente {} (anonyme {}, fichier {}), pic {}",
            servi["memory"]["total"], r.rss, r.anonyme, r.fichier, r.pic
        );
    }
    let _ = routeur.kill().await;
}

/// Le routeur épinglé, lancé comme l'image le lance, sur le dossier des téléchargements.
async fn routeur(moteur: &str, chaine: &Chaine, port: u16) -> tokio::process::Child {
    let prereglages = chaine.dir.path().join("prereglages.ini");
    std::fs::write(
        &prereglages,
        "[*]\njinja = 1\nctx-size = 2048\nthreads = 4\nparallel = 1\nn-gpu-layers = 0\n",
    )
    .unwrap();
    let enfant = tokio::process::Command::new(moteur)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .arg("--models-preset")
        .arg(&prereglages)
        .args(["--models-max", "1", "--models-dir"])
        .arg(chaine.poids())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let limite = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            std::time::Instant::now() < limite,
            "le routeur ne répond pas"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    enfant
}

/// La mémoire que le noyau compte à un processus (`/proc/<pid>/status`), en octets.
#[derive(Debug, Clone, Copy)]
struct Residente {
    rss: u64,
    anonyme: u64,
    fichier: u64,
    pic: u64,
}

/// L'instance que le routeur a lancée pour servir `fichier` : un descendant du routeur dont la
/// ligne de commande nomme le fichier. Sa mémoire résidente, une fois qu'elle a répondu, est ce
/// que l'estimation de `providers::memory` prétend prévoir.
fn memoire_de_l_instance(routeur: u32, fichier: &str) -> Option<Residente> {
    let nom = std::path::Path::new(fichier)
        .file_name()?
        .to_str()?
        .to_owned();
    // Le parent de chaque processus : le deuxième champ après le nom, qui peut contenir des
    // espaces.
    let parents: std::collections::HashMap<u32, u32> = std::fs::read_dir("/proc")
        .ok()?
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|pid| {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            let (_, reste) = stat.rsplit_once(')')?;
            Some((pid, reste.split_whitespace().nth(1)?.parse().ok()?))
        })
        .collect();
    let descend_du_routeur = |mut pid: u32| {
        for _ in 0..8 {
            match parents.get(&pid) {
                Some(&parent) if parent == routeur => return true,
                Some(&parent) if parent > 1 => pid = parent,
                _ => return false,
            }
        }
        false
    };
    let pid = parents.keys().copied().find(|&pid| {
        descend_du_routeur(pid)
            && String::from_utf8_lossy(
                &std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default(),
            )
            .contains(&nom)
    })?;
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let champ = |cle: &str| {
        status.lines().find_map(|l| {
            let kib: u64 = l
                .strip_prefix(cle)?
                .strip_prefix(':')?
                .trim()
                .trim_end_matches("kB")
                .trim()
                .parse()
                .ok()?;
            Some(kib * 1024)
        })
    };
    Some(Residente {
        rss: champ("VmRSS")?,
        anonyme: champ("RssAnon")?,
        fichier: champ("RssFile")?,
        pic: champ("VmHWM")?,
    })
}

/// La CLI réelle sur la chaîne ; rend sa sortie, ou échoue en la disant. La fenêtre des
/// estimations de mémoire est celle des routeurs de ces essais.
async fn prophet(chaine: &Chaine, args: &[&str]) -> String {
    let sortie = tokio::process::Command::new(binaire_voisin("prophet"))
        .args(args)
        .env("PROPHET_LOCAL_CONTEXT", "2048")
        .env(
            "PROPHET_AGENTD_SOCKET",
            chaine.dir.path().join("agents.sock"),
        )
        .output()
        .await
        .unwrap();
    let dit = String::from_utf8_lossy(&sortie.stdout).into_owned();
    assert!(
        sortie.status.success(),
        "prophet {args:?} : {dit}\n{}",
        String::from_utf8_lossy(&sortie.stderr)
    );
    dit
}

/// FRONTIER, moteurs locaux : « valider des modèles de plusieurs familles ». Chaque famille du
/// catalogue hors Qwen3 — Granite, SmolLM2, Phi-3, Llama 3.2 — est tirée par egress, servie par
/// le routeur épinglé, interrogée, puis retirée ; la réponse et sa vitesse sont relevées.
#[tokio::test]
#[ignore = "needs_llama_server: PROPHET_TEST_LLAMA_SERVER (llama-server épinglé), Hugging Face joignable, 7 Go"]
async fn plusieurs_familles_se_servent_et_repondent() {
    let moteur = std::env::var("PROPHET_TEST_LLAMA_SERVER")
        .expect("PROPHET_TEST_LLAMA_SERVER : le llama-server épinglé (nix build .#llama-cpp)");
    let chaine = Chaine::new(None).await;
    let mut reussies = Vec::new();
    for (rang, id) in [
        "granite-3.3-2b-q4",
        "smollm2-1.7b-q4",
        "phi-3-mini-q4",
        "llama-3.2-3b-q4",
    ]
    .into_iter()
    .enumerate()
    {
        let debut = std::time::Instant::now();
        prophet(&chaine, &["model", "pull", id]).await;
        let tire = debut.elapsed();
        // Le routeur lit le dossier à son démarrage : un par poids.
        let port = 18_110 + u16::try_from(rang).unwrap();
        let mut routeur = routeur(&moteur, &chaine, port).await;
        let servi: Value = serde_json::from_str(
            &prophet(
                &chaine,
                &[
                    "--json",
                    "model",
                    "serve",
                    id,
                    "--endpoint",
                    &format!("http://127.0.0.1:{port}/v1"),
                ],
            )
            .await,
        )
        .unwrap();
        let nom = servi["model"].as_str().unwrap().to_owned();
        let fichier = servi["path"].as_str().unwrap_or_default().to_owned();
        let estimee = servi["memory"]["total"]
            .as_u64()
            .unwrap_or_else(|| panic!("{id} : l'en-tête doit permettre l'estimation : {servi}"));
        let debut = std::time::Instant::now();
        let reponse = tokio::task::spawn_blocking(move || {
            http_local(
                port,
                "POST",
                "/v1/chat/completions",
                &json!({
                    "model": nom,
                    "messages": [{"role": "user", "content": "What is the capital of France? Answer in one word."}],
                    "max_tokens": 16,
                    "temperature": 0,
                }),
            )
        })
        .await
        .unwrap();
        let texte = reponse["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_owned();
        eprintln!(
            "mesure : famille {id} tirée en {tire:?}, réponse en {:?} : {texte:?} ({} tokens/s en génération)",
            debut.elapsed(),
            reponse["timings"]["predicted_per_second"]
        );
        assert!(
            reponse["usage"]["completion_tokens"].as_u64().unwrap_or(0) > 0,
            "{id} : {reponse}"
        );
        if texte.to_ascii_lowercase().contains("paris") {
            reussies.push(id);
        }
        // L'estimation de mémoire face à ce que le noyau compte à l'instance qui a répondu :
        // elle ne doit pas manquer ce qui ne se récupère pas (la mémoire anonyme : cache KV,
        // calcul, poids recopiés), ni prédire bien plus que ce que le moteur tient.
        let r = routeur
            .id()
            .and_then(|pid| memoire_de_l_instance(pid, &fichier))
            .unwrap_or_else(|| panic!("{id} : l'instance du routeur est introuvable"));
        eprintln!(
            "mesure : mémoire de {id} : estimée {estimee} ; résidente {} (anonyme {}, fichier {}), pic {} ; rapport {:.2}",
            r.rss,
            r.anonyme,
            r.fichier,
            r.pic,
            estimee as f64 / r.rss.max(1) as f64
        );
        assert!(
            estimee >= r.anonyme,
            "{id} : estimée {estimee}, mais {} de mémoire anonyme",
            r.anonyme
        );
        assert!(
            (estimee as f64) <= 1.5 * r.rss.max(r.anonyme) as f64,
            "{id} : estimée {estimee}, bien au-delà des {} résidents",
            r.rss
        );
        let _ = routeur.kill().await;
        // Retirer avant le suivant : la place du coureur est comptée.
        prophet(&chaine, &["model", "rm", id]).await;
    }
    eprintln!("mesure : réponse juste (« Paris ») pour {reussies:?}");
}
