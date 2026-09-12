//! Le daemon `prophet-egress` laisse-t-il vraiment passer seulement ce qu'il doit ?
//!
//! Ces tests montent un serveur HTTP jetable sur la machine locale et regardent **s'il reçoit
//! quelque chose**. C'est le seul témoignage qui vaille : un proxy qui répond « refusé » tout en
//! ayant déjà transmis la requête aurait exactement l'apparence d'un proxy qui refuse.
//!
//! Le serveur compte donc ses connexions, et c'est ce compte qui est vérifié — pas le code de
//! retour rendu à l'appelant.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use prophet_daemon::essai::Daemon;
use serde_json::json;
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::{TcpListener, UnixStream};

const EGRESS: &str = env!("CARGO_BIN_EXE_prophet-egress");

/// De quoi savoir que le proxy sert, sans rien faire sortir : une requête sans jeton, refusée
/// avant que le relais ne soit seulement envisagé.
const SONDE: &[u8] = b"GET http://sonde.invalide/ HTTP/1.1\r\nHost: sonde.invalide\r\n\r\n";

/// Un serveur qui ne sert à rien sauf à dire s'il a été joint.
struct Temoin {
    port: u16,
    connexions: Arc<AtomicUsize>,
    /// La première ligne de requête reçue. Compter les connexions dit *si* quelque chose est
    /// sorti ; la retenir dit *quoi* — et c'est la seule façon de voir qu'une requête est arrivée
    /// amputée.
    recue: Arc<std::sync::Mutex<String>>,
}

impl Temoin {
    async fn poser() -> Self {
        let ecoute = TcpListener::bind(("127.0.0.1", 0)).await.expect("écoute");
        let port = ecoute.local_addr().expect("adresse").port();
        let connexions = Arc::new(AtomicUsize::new(0));
        let compteur = Arc::clone(&connexions);
        let recue = Arc::new(std::sync::Mutex::new(String::new()));
        let carnet = Arc::clone(&recue);
        tokio::spawn(async move {
            while let Ok((flux, _)) = ecoute.accept().await {
                compteur.fetch_add(1, Ordering::SeqCst);
                let carnet = Arc::clone(&carnet);
                tokio::spawn(async move {
                    let (lecture, mut ecriture) = flux.into_split();
                    let mut lecteur = BufReader::new(lecture);
                    let mut ligne = String::new();
                    if lecteur.read_line(&mut ligne).await.is_ok()
                        && let Ok(mut carnet) = carnet.lock()
                    {
                        *carnet = ligne.trim_end().to_owned();
                    }
                    let _ = ecriture
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                        .await;
                });
            }
        });
        Self {
            port,
            connexions,
            recue,
        }
    }

    fn ligne_recue(&self) -> String {
        self.recue.lock().map(|l| l.clone()).unwrap_or_default()
    }

    fn jointes(&self) -> usize {
        self.connexions.load(Ordering::SeqCst)
    }
}

/// Envoie une requête brute au proxy et rend sa réponse.
async fn demander(socket: &std::path::Path, requete: &str) -> String {
    let mut flux = UnixStream::connect(socket).await.expect("le proxy écoute");
    flux.write_all(requete.as_bytes()).await.expect("envoi");
    flux.flush().await.expect("vidage");
    let mut lecteur = BufReader::new(flux);
    let mut premiere = String::new();
    lecteur.read_line(&mut premiere).await.expect("réponse");
    premiere
}

/// Comme [`demander`], mais rend aussi ce qui suit : un refus explique pourquoi, et c'est cette
/// explication qu'on veut lire quand un test échoue.
async fn demander_avec_corps(socket: &std::path::Path, requete: &str) -> (String, String) {
    use tokio::io::AsyncReadExt as _;
    let mut flux = UnixStream::connect(socket).await.expect("le proxy écoute");
    flux.write_all(requete.as_bytes()).await.expect("envoi");
    flux.flush().await.expect("vidage");
    let mut tout = String::new();
    let _ = flux.read_to_string(&mut tout).await;
    let (tete, corps) = tout.split_once("\r\n\r\n").unwrap_or((tout.as_str(), ""));
    (tete.to_owned(), corps.to_owned())
}

/// Laisse au serveur le temps de constater une connexion qui n'aurait pas dû avoir lieu.
///
/// Sans cette attente, un test « rien n'est sorti » passerait aussi bien parce que rien n'est
/// sorti que parce qu'on a regardé trop tôt.
async fn laisser_le_temps() {
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
}

#[tokio::test]
async fn sans_jeton_rien_ne_sort() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let temoin = Temoin::poser().await;
    let socket = temp.path().join("egress.sock");
    let daemon = Daemon::lancer_avec(
        EGRESS,
        &socket,
        &temp.path().join("etat"),
        // Un capd volontairement inexistant : si la requête arrivait jusqu'au broker, elle
        // échouerait là plutôt qu'ici, et le test ne dirait plus ce qu'il prétend dire.
        &[("PROPHET_CAPD_SOCKET", "/nulle/part/capd.sock")],
    );
    daemon.attendre_reponse(SONDE).await;

    let reponse = demander(
        &socket,
        &format!(
            "GET http://127.0.0.1:{}/exfiltration HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            temoin.port
        ),
    )
    .await;

    assert!(
        reponse.contains("407"),
        "le proxy doit dire qu'il ne sait pas pour qui il travaille, obtenu : {reponse}"
    );
    laisser_le_temps().await;
    assert_eq!(
        temoin.jointes(),
        0,
        "aucun octet ne doit avoir atteint le serveur"
    );
}

#[tokio::test]
async fn un_broker_injoignable_ferme_la_sortie_au_lieu_de_l_ouvrir() {
    // Le défaut dangereux serait « en cas de doute, laisser passer ». Un `capd` mort rendrait
    // alors la machine entièrement ouverte, et c'est précisément le moment où elle ne doit pas
    // l'être.
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let temoin = Temoin::poser().await;
    let socket = temp.path().join("egress.sock");
    let daemon = Daemon::lancer_avec(
        EGRESS,
        &socket,
        &temp.path().join("etat"),
        &[("PROPHET_CAPD_SOCKET", "/nulle/part/capd.sock")],
    );
    daemon.attendre_reponse(SONDE).await;

    let jeton = base64_json(&json!({ "v": 0, "sub": "task:x" }));
    let reponse = demander(
        &socket,
        &format!(
            "GET http://127.0.0.1:{}/x HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Proxy-Authorization: Prophet {jeton}\r\n\r\n",
            temoin.port
        ),
    )
    .await;

    assert!(
        reponse.contains("503"),
        "un broker injoignable est un « je ne sais pas », pas un « oui » : {reponse}"
    );
    laisser_le_temps().await;
    assert_eq!(temoin.jointes(), 0);
}

#[tokio::test]
async fn un_jeton_que_capd_refuse_ne_sort_pas_non_plus() {
    // Cette fois le broker existe et répond. Le jeton est forgé : signé par une clé qui n'est pas
    // la sienne. Le refus vient donc d'une décision, pas d'une panne.
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let temoin = Temoin::poser().await;

    let socket_capd = temp.path().join("capd.sock");
    let chemin_capd = prophet_daemon::essai::binaire_voisin("prophet-capd");
    let capd = Daemon::lancer(
        chemin_capd.to_str().expect("chemin lisible"),
        &socket_capd,
        &temp.path().join("etat-capd"),
    );
    drop(capd.joindre().await);

    let socket = temp.path().join("egress.sock");
    let daemon = Daemon::lancer_avec(
        EGRESS,
        &socket,
        &temp.path().join("etat"),
        &[(
            "PROPHET_CAPD_SOCKET",
            socket_capd.to_str().expect("chemin lisible"),
        )],
    );
    daemon.attendre_reponse(SONDE).await;

    let reponse = demander(
        &socket,
        &format!(
            "GET http://127.0.0.1:{}/x HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Proxy-Authorization: Prophet {}\r\n\r\n",
            temoin.port,
            base64_json(&jeton_forge())
        ),
    )
    .await;

    assert!(
        reponse.contains("403"),
        "un jeton signé ailleurs n'ouvre rien : {reponse}"
    );
    laisser_le_temps().await;
    assert_eq!(temoin.jointes(), 0);
}

fn base64_json(valeur: &serde_json::Value) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(valeur.to_string())
}

fn jeton_forge() -> serde_json::Value {
    use prophet_types::cap::{Act, Grant, Res, TokenBuilder};
    let clef_etrangere = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
    let jeton = TokenBuilder::new("capd", "task:forge", "agent.forge", "prophet")
        .grant(Grant::new(Res::Net, Act::Egress, "*"))
        .build(&clef_etrangere, time::OffsetDateTime::now_utc(), [0u8; 16])
        .expect("un faussaire sait construire un jeton bien formé");
    serde_json::to_value(jeton).expect("sérialisable")
}

/// Un manifeste minimal qui autorise la sortie vers la machine locale.
fn manifeste() -> serde_json::Value {
    json!({
        "agent": {
            "id": "org.essai.relais",
            "version": "1.0.0",
            "name": "Essai du relais",
            "publisher_key": "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        },
        "model": { "preferred": ["local:qwen3-8b"] },
        "capabilities": { "max": { "net.egress": ["127.0.0.1"] } }
    })
}

#[tokio::test]
async fn un_jeton_legitime_fait_vraiment_sortir_la_requete() {
    // Les trois tests précédents prouvent des refus. Celui-ci prouve qu'il reste quelque chose à
    // refuser : sans lui, un proxy qui bloquerait tout les passerait tous.
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let temoin = Temoin::poser().await;

    let socket_capd = temp.path().join("capd.sock");
    let capd = Daemon::lancer(
        prophet_daemon::essai::binaire_voisin("prophet-capd")
            .to_str()
            .expect("chemin lisible"),
        &socket_capd,
        &temp.path().join("etat-capd"),
    );
    let client_capd = capd.joindre().await;

    let jeton = client_capd
        .call(
            "cap.mint",
            json!({
                "manifest": manifeste(),
                "grants": [{ "res": "net", "act": "egress", "match": "127.0.0.1" }],
                "task": "task:relais",
                "user": "prophet"
            }),
        )
        .await
        .expect("capd émet un jeton pour une tâche dont le manifeste le permet");

    let socket = temp.path().join("egress.sock");
    let daemon = Daemon::lancer_avec(
        EGRESS,
        &socket,
        &temp.path().join("etat"),
        &[(
            "PROPHET_CAPD_SOCKET",
            socket_capd.to_str().expect("chemin lisible"),
        )],
    );
    daemon.attendre_reponse(SONDE).await;

    let (reponse, corps) = demander_avec_corps(
        &socket,
        &format!(
            "GET http://127.0.0.1:{}/legitime HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Proxy-Authorization: Prophet {}\r\n\r\n",
            temoin.port,
            base64_json(&jeton)
        ),
    )
    .await;

    assert!(
        reponse.contains("200"),
        "une requête autorisée doit aboutir, obtenu : {reponse} / corps : {corps}"
    );
    laisser_le_temps().await;
    assert_eq!(
        temoin.jointes(),
        1,
        "et le serveur doit l'avoir réellement reçue"
    );
}

#[tokio::test]
async fn une_methode_modifiante_exige_une_approbation() {
    // La seconde ligne du tableau de `docs/PLAN.md` : envoyer, payer, poster ou supprimer à
    // distance ne passe pas tout seul, même avec le jeton qui autorise le domaine. Le même jeton
    // qui fait passer un GET doit buter ici — sans quoi la distinction ne serait qu'un commentaire.
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let temoin = Temoin::poser().await;

    let socket_capd = temp.path().join("capd.sock");
    let capd = Daemon::lancer(
        prophet_daemon::essai::binaire_voisin("prophet-capd")
            .to_str()
            .expect("chemin lisible"),
        &socket_capd,
        &temp.path().join("etat-capd"),
    );
    let client_capd = capd.joindre().await;
    let jeton = client_capd
        .call(
            "cap.mint",
            json!({
                "manifest": manifeste(),
                "grants": [{ "res": "net", "act": "egress", "match": "127.0.0.1" }],
                "task": "task:relais",
                "user": "prophet"
            }),
        )
        .await
        .expect("le même jeton que celui du test précédent");

    let socket = temp.path().join("egress.sock");
    let daemon = Daemon::lancer_avec(
        EGRESS,
        &socket,
        &temp.path().join("etat"),
        &[(
            "PROPHET_CAPD_SOCKET",
            socket_capd.to_str().expect("chemin lisible"),
        )],
    );
    daemon.attendre_reponse(SONDE).await;

    let (reponse, corps) = demander_avec_corps(
        &socket,
        &format!(
            "POST http://127.0.0.1:{}/payer HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Content-Length: 0\r\nProxy-Authorization: Prophet {}\r\n\r\n",
            temoin.port,
            base64_json(&jeton)
        ),
    )
    .await;

    assert!(
        reponse.contains("403"),
        "un POST ne part pas sans décision humaine, obtenu : {reponse}"
    );
    assert!(
        corps.contains("approval_required"),
        "et le motif doit dire qu'il manque une approbation, pas autre chose : {corps}"
    );
    laisser_le_temps().await;
    assert_eq!(
        temoin.jointes(),
        0,
        "rien ne doit avoir atteint le serveur avant la décision"
    );
}

#[tokio::test]
async fn une_requete_arrive_entiere_avec_sa_chaine_de_requete() {
    // Le relais transmettait `path()`, qui retire la chaîne de requête — bon pour le journal, qui
    // n'a pas à garder ce qu'elle peut porter, mais `/chercher?q=x` serait parti en `/chercher`.
    // Compter les connexions ne l'aurait jamais montré : il faut regarder ce qui arrive.
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let temoin = Temoin::poser().await;

    let socket_capd = temp.path().join("capd.sock");
    let capd = Daemon::lancer(
        prophet_daemon::essai::binaire_voisin("prophet-capd")
            .to_str()
            .expect("chemin lisible"),
        &socket_capd,
        &temp.path().join("etat-capd"),
    );
    let client_capd = capd.joindre().await;
    let jeton = client_capd
        .call(
            "cap.mint",
            json!({
                "manifest": manifeste(),
                "grants": [{ "res": "net", "act": "egress", "match": "127.0.0.1" }],
                "task": "task:relais",
                "user": "prophet"
            }),
        )
        .await
        .expect("jeton");

    let socket = temp.path().join("egress.sock");
    let daemon = Daemon::lancer_avec(
        EGRESS,
        &socket,
        &temp.path().join("etat"),
        &[(
            "PROPHET_CAPD_SOCKET",
            socket_capd.to_str().expect("chemin lisible"),
        )],
    );
    daemon.attendre_reponse(SONDE).await;

    demander(
        &socket,
        &format!(
            "GET http://127.0.0.1:{}/chercher?q=important&page=2 HTTP/1.1\r\n\
             Host: 127.0.0.1\r\nProxy-Authorization: Prophet {}\r\n\r\n",
            temoin.port,
            base64_json(&jeton)
        ),
    )
    .await;
    laisser_le_temps().await;

    let recue = temoin.ligne_recue();
    assert!(
        recue.contains("/chercher?q=important&page=2"),
        "la requête doit arriver entière, obtenu : {recue}"
    );
}

#[tokio::test]
async fn un_cadrage_ambigu_ne_sort_pas() {
    // Deux `Content-Length` permettent au proxy et au serveur de lire deux corps différents dans
    // les mêmes octets. On refuse plutôt que de faire de son mieux.
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let temoin = Temoin::poser().await;
    let socket = temp.path().join("egress.sock");
    let daemon = Daemon::lancer_avec(
        EGRESS,
        &socket,
        &temp.path().join("etat"),
        &[("PROPHET_CAPD_SOCKET", "/nulle/part/capd.sock")],
    );
    daemon.attendre_reponse(SONDE).await;

    let reponse = demander(
        &socket,
        &format!(
            "POST http://127.0.0.1:{}/x HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Content-Length: 0\r\nContent-Length: 9\r\n\r\n",
            temoin.port
        ),
    )
    .await;

    assert!(
        reponse.contains("400"),
        "un cadrage ambigu se refuse, obtenu : {reponse}"
    );
    laisser_le_temps().await;
    assert_eq!(temoin.jointes(), 0);
}
