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

/// Monte capd et le coffre, et rend le jeton plus les chemins de leurs sockets.
struct Chaine {
    _capd: Daemon,
    _coffre: Daemon,
    socket_capd: std::path::PathBuf,
    socket_coffre: std::path::PathBuf,
    jeton: serde_json::Value,
}

async fn chaine(temp: &tempfile::TempDir) -> Chaine {
    let socket_capd = temp.path().join("capd.sock");
    let socket_coffre = temp.path().join("vault.sock");

    let capd = Daemon::lancer(
        prophet_daemon::essai::binaire_voisin("prophet-capd")
            .to_str()
            .expect("chemin"),
        &socket_capd,
        &temp.path().join("etat-capd"),
    );
    let coffre = Daemon::lancer(
        prophet_daemon::essai::binaire_voisin("prophet-vault")
            .to_str()
            .expect("chemin"),
        &socket_coffre,
        &temp.path().join("etat-vault"),
    );

    let client_capd = capd.joindre().await;
    let jeton = client_capd
        .call(
            "cap.mint",
            json!({
                "manifest": manifeste(),
                "grants": [{ "res": "net", "act": "egress", "match": "127.0.0.1" }],
                "task": "task:injection",
                "user": "prophet"
            }),
        )
        .await
        .expect("jeton");

    let client_coffre = coffre.joindre().await;
    client_coffre
        .call(
            "vault.put",
            json!({
                "info": {
                    "name": "essai",
                    "domains": ["127.0.0.1"],
                    "header": "Authorization",
                    "description": "de quoi essayer"
                },
                "value": "valeur-que-le-modele-ne-voit-jamais"
            }),
        )
        .await
        .expect("le secret se dépose");

    Chaine {
        _capd: capd,
        _coffre: coffre,
        socket_capd,
        socket_coffre,
        jeton,
    }
}

#[tokio::test]
async fn sans_droit_de_reveler_la_requete_s_arrete_au_lieu_de_partir_avec_la_reference() {
    // Le test tourne sous un utilisateur qui n'est pas `egress` : le coffre refuse donc de
    // révéler, ce qui est exactement son travail. Ce qu'on vérifie ici est la conséquence — la
    // requête ne part pas avec le handle littéral. La laisser partir serait inoffensif (un handle
    // ne vaut rien sans le coffre) mais la tâche croirait son secret transmis et ne comprendrait
    // pas l'échec d'authentification qui suivrait.
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let temoin = Temoin::poser().await;
    let chaine = chaine(&temp).await;

    let socket = temp.path().join("egress.sock");
    let daemon = Daemon::lancer_avec(
        EGRESS,
        &socket,
        &temp.path().join("etat"),
        &[
            (
                "PROPHET_CAPD_SOCKET",
                chaine.socket_capd.to_str().expect("chemin"),
            ),
            (
                "PROPHET_VAULT_SOCKET",
                chaine.socket_coffre.to_str().expect("chemin"),
            ),
        ],
    );
    daemon.attendre_reponse(SONDE).await;

    let (reponse, corps) = demander_avec_corps(
        &socket,
        &format!(
            "GET http://127.0.0.1:{}/api HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Authorization: Bearer prophet-secret:essai\r\n\
             Proxy-Authorization: Prophet {}\r\n\r\n",
            temoin.port,
            base64_json(&chaine.jeton)
        ),
    )
    .await;

    assert!(
        reponse.contains("403"),
        "la substitution impossible arrête la requête, obtenu : {reponse} / {corps}"
    );
    laisser_le_temps().await;
    assert_eq!(
        temoin.jointes(),
        0,
        "et rien ne part : ni la valeur, ni la référence"
    );
}

#[tokio::test]
async fn un_secret_destine_a_un_autre_domaine_arrete_la_requete() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let temoin = Temoin::poser().await;
    let chaine = chaine(&temp).await;

    let socket = temp.path().join("egress.sock");
    let daemon = Daemon::lancer_avec(
        EGRESS,
        &socket,
        &temp.path().join("etat"),
        &[
            (
                "PROPHET_CAPD_SOCKET",
                chaine.socket_capd.to_str().expect("chemin"),
            ),
            (
                "PROPHET_VAULT_SOCKET",
                chaine.socket_coffre.to_str().expect("chemin"),
            ),
        ],
    );
    daemon.attendre_reponse(SONDE).await;

    // Le secret « essai » n'est destiné qu'à 127.0.0.1 ; on vise ici la boucle locale par son nom,
    // que le motif ne couvre pas.
    let (reponse, corps) = demander_avec_corps(
        &socket,
        &format!(
            "GET http://localhost:{}/api HTTP/1.1\r\nHost: localhost\r\n\
             Authorization: Bearer prophet-secret:essai\r\n\
             Proxy-Authorization: Prophet {}\r\n\r\n",
            temoin.port,
            base64_json(&chaine.jeton)
        ),
    )
    .await;

    assert!(reponse.contains("403"), "obtenu : {reponse} / {corps}");
    laisser_le_temps().await;
    assert_eq!(temoin.jointes(), 0);
}

#[tokio::test]
async fn une_injection_dans_un_tunnel_est_refusee_plutot_que_sans_effet() {
    // ADR-0007 : le proxy ne voit rien dans un tunnel chiffré. Laisser passer enverrait le handle
    // littéral au serveur, et la tâche croirait son secret substitué.
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let temoin = Temoin::poser().await;
    let chaine = chaine(&temp).await;

    let socket = temp.path().join("egress.sock");
    let daemon = Daemon::lancer_avec(
        EGRESS,
        &socket,
        &temp.path().join("etat"),
        &[
            (
                "PROPHET_CAPD_SOCKET",
                chaine.socket_capd.to_str().expect("chemin"),
            ),
            (
                "PROPHET_VAULT_SOCKET",
                chaine.socket_coffre.to_str().expect("chemin"),
            ),
        ],
    );
    daemon.attendre_reponse(SONDE).await;

    let (reponse, corps) = demander_avec_corps(
        &socket,
        &format!(
            "CONNECT 127.0.0.1:{} HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Authorization: Bearer prophet-secret:essai\r\n\
             Proxy-Authorization: Prophet {}\r\n\r\n",
            temoin.port,
            base64_json(&chaine.jeton)
        ),
    )
    .await;

    assert!(reponse.contains("400"), "obtenu : {reponse} / {corps}");
    assert!(
        corps.contains("ADR-0007"),
        "le refus doit dire où la raison est écrite : {corps}"
    );
    laisser_le_temps().await;
    assert_eq!(temoin.jointes(), 0);
}

#[tokio::test]
async fn un_post_vers_un_hote_d_interrogation_passe_comme_une_lecture() {
    // Une API de décision répond à un POST sans rien retenir. Pour les hôtes que l'administrateur
    // a nommés, et pour POST seulement, la requête est contrôlée comme une lecture : le jeton et
    // le grant sur l'hôte restent exigés, et un PUT vers le même hôte reste une modification.
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
                "task": "task:decision",
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
        &[
            (
                "PROPHET_CAPD_SOCKET",
                socket_capd.to_str().expect("chemin lisible"),
            ),
            ("PROPHET_EGRESS_QUERY_HOSTS", "127.0.0.1, api.typesafe.ai"),
        ],
    );
    daemon.attendre_reponse(SONDE).await;

    let corps = r#"{"state":"x","model":"jev-latest","questions":{"q":{"type":"noul","instructions":"?"}}}"#;
    let (reponse, detail) = demander_avec_corps(
        &socket,
        &format!(
            "POST http://127.0.0.1:{}/v1/systemone HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\n\
             Proxy-Authorization: Prophet {}\r\n\r\n{corps}",
            temoin.port,
            corps.len(),
            base64_json(&jeton)
        ),
    )
    .await;
    assert!(
        reponse.contains("200"),
        "un POST d'interrogation passe sans décision humaine, obtenu : {reponse} / {detail}"
    );
    laisser_le_temps().await;
    assert_eq!(temoin.jointes(), 1, "et il atteint réellement le serveur");
    assert!(
        temoin.ligne_recue().starts_with("POST /v1/systemone"),
        "{}",
        temoin.ligne_recue()
    );

    let (reponse, detail) = demander_avec_corps(
        &socket,
        &format!(
            "PUT http://127.0.0.1:{}/v1/systemone HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Content-Length: 0\r\nProxy-Authorization: Prophet {}\r\n\r\n",
            temoin.port,
            base64_json(&jeton)
        ),
    )
    .await;
    assert!(
        reponse.contains("403"),
        "PUT reste une modification : {reponse}"
    );
    assert!(detail.contains("approval_required"), "{detail}");
    laisser_le_temps().await;
    assert_eq!(temoin.jointes(), 1, "le PUT n'a pas atteint le serveur");

    let (reponse, _) = demander_avec_corps(
        &socket,
        &format!(
            "POST http://127.0.0.1:{}/v1/systemone HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Content-Length: 0\r\n\r\n",
            temoin.port,
        ),
    )
    .await;
    assert!(
        reponse.contains("407"),
        "sans jeton, un hôte d'interrogation ne passe pas non plus : {reponse}"
    );
    laisser_le_temps().await;
    assert_eq!(temoin.jointes(), 1);
}

#[tokio::test]
async fn une_liste_d_hotes_d_interrogation_illisible_arrete_le_service() {
    // « Tous les POST sont des lectures » ne doit pas pouvoir s'écrire par accident.
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let socket = temp.path().join("egress.sock");
    let mut commande = std::process::Command::new(EGRESS);
    commande
        .env("PROPHET_SOCKET", &socket)
        .env("STATE_DIRECTORY", temp.path().join("etat"))
        .env("PROPHET_CAPD_SOCKET", "/nulle/part/capd.sock")
        .env("PROPHET_EGRESS_QUERY_HOSTS", "*")
        .stderr(std::process::Stdio::null());
    let statut = commande.status().expect("le programme se lance");
    assert!(!statut.success(), "un motif `*` doit refuser de démarrer");
}

/// Un serveur TLS jetable, avec un certificat auto-signé pour 127.0.0.1 écrit en PEM pour que
/// le proxy puisse lui faire confiance par `SSL_CERT_FILE`.
struct TemoinTls {
    port: u16,
    pem: std::path::PathBuf,
    connexions: Arc<AtomicUsize>,
    recue: Arc<std::sync::Mutex<String>>,
}

impl TemoinTls {
    async fn poser(dir: &std::path::Path) -> Self {
        use tokio::io::AsyncReadExt as _;
        let mut params = rcgen::CertificateParams::default();
        params.subject_alt_names = vec![rcgen::SanType::IpAddress(std::net::IpAddr::V4(
            std::net::Ipv4Addr::LOCALHOST,
        ))];
        let cle = rcgen::KeyPair::generate().expect("clé");
        let cert = params.self_signed(&cle).expect("certificat");
        let pem = dir.join("temoin.pem");
        std::fs::write(&pem, cert.pem()).expect("pem");
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                rustls_pki_types::PrivateKeyDer::Pkcs8(cle.serialize_der().into()),
            )
            .expect("configuration TLS");
        let accepteur = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let ecoute = TcpListener::bind(("127.0.0.1", 0)).await.expect("écoute");
        let port = ecoute.local_addr().expect("adresse").port();
        let connexions = Arc::new(AtomicUsize::new(0));
        let compteur = Arc::clone(&connexions);
        let recue = Arc::new(std::sync::Mutex::new(String::new()));
        let carnet = Arc::clone(&recue);
        tokio::spawn(async move {
            while let Ok((flux, _)) = ecoute.accept().await {
                compteur.fetch_add(1, Ordering::SeqCst);
                let accepteur = accepteur.clone();
                let carnet = Arc::clone(&carnet);
                tokio::spawn(async move {
                    let Ok(mut flux) = accepteur.accept(flux).await else {
                        return;
                    };
                    let mut brut = vec![0u8; 8192];
                    let n = flux.read(&mut brut).await.unwrap_or(0);
                    if let Ok(mut carnet) = carnet.lock() {
                        *carnet = String::from_utf8_lossy(&brut[..n]).into_owned();
                    }
                    let _ = flux
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecret")
                        .await;
                    let _ = flux.shutdown().await;
                });
            }
        });
        Self {
            port,
            pem,
            connexions,
            recue,
        }
    }
}

#[tokio::test]
async fn un_amont_https_est_joint_en_tls_termine_par_le_proxy() {
    // Une API de décision parle HTTPS. Un tunnel ne laisserait rien substituer (ADR-0007) ; le
    // proxy termine donc TLS lui-même pour une requête en forme absolue `https://`, avec les
    // racines de la machine. Un certificat qu'elle ne reconnaît pas ferme la sortie : elle ne se
    // dégrade jamais en clair.
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let temoin = TemoinTls::poser(temp.path()).await;

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
                "task": "task:tls",
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
        &[
            (
                "PROPHET_CAPD_SOCKET",
                socket_capd.to_str().expect("chemin lisible"),
            ),
            ("SSL_CERT_FILE", temoin.pem.to_str().expect("chemin")),
        ],
    );
    daemon.attendre_reponse(SONDE).await;

    let (reponse, corps) = demander_avec_corps(
        &socket,
        &format!(
            "GET https://127.0.0.1:{}/protege HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Authorization: Bearer valeur-en-clair\r\n\
             Proxy-Authorization: Prophet {}\r\n\r\n",
            temoin.port,
            base64_json(&jeton)
        ),
    )
    .await;
    assert!(reponse.contains("200"), "obtenu : {reponse} / {corps}");
    assert_eq!(corps, "secret");
    laisser_le_temps().await;
    let recue = temoin.recue.lock().map(|r| r.clone()).unwrap_or_default();
    assert!(recue.starts_with("GET /protege HTTP/1.1"), "{recue}");
    assert!(
        recue.contains("Authorization: Bearer valeur-en-clair"),
        "{recue}"
    );
    assert!(
        !recue.to_ascii_lowercase().contains("proxy-authorization"),
        "{recue}"
    );
    assert_eq!(temoin.connexions.load(Ordering::SeqCst), 1);

    // Sans la racine du témoin, la poignée de main échoue et rien ne part en clair.
    let autre = tempfile::tempdir().expect("répertoire temporaire");
    let socket_mefiant = autre.path().join("egress.sock");
    let vide = autre.path().join("aucune-racine.pem");
    std::fs::write(&vide, "").expect("fichier vide");
    let mefiant = Daemon::lancer_avec(
        EGRESS,
        &socket_mefiant,
        &autre.path().join("etat"),
        &[
            (
                "PROPHET_CAPD_SOCKET",
                socket_capd.to_str().expect("chemin lisible"),
            ),
            ("SSL_CERT_FILE", vide.to_str().expect("chemin")),
        ],
    );
    mefiant.attendre_reponse(SONDE).await;
    let (reponse, corps) = demander_avec_corps(
        &socket_mefiant,
        &format!(
            "GET https://127.0.0.1:{}/protege HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Proxy-Authorization: Prophet {}\r\n\r\n",
            temoin.port,
            base64_json(&jeton)
        ),
    )
    .await;
    assert!(reponse.contains("502"), "obtenu : {reponse} / {corps}");
    assert!(corps.contains("TlsFailed"), "{corps}");
    laisser_le_temps().await;
    let recue = temoin.recue.lock().map(|r| r.clone()).unwrap_or_default();
    assert!(
        recue.starts_with("GET /protege"),
        "la seule requête reçue reste la première : {recue}"
    );
}

/// Chaque décision s'inscrit au journal de la mission du jeton (ADR 0056) : une sortie relayée
/// en `net.request` avec son hôte, son statut et ses octets ; un refus de capd en `net.deny`. Un
/// jeton forgé, lui, n'écrit rien : il ne ferait qu'imiter une autre mission.
#[tokio::test]
async fn chaque_decision_s_inscrit_au_journal_de_la_mission() {
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
    let socket_journal = temp.path().join("ledger.sock");
    let journal = Daemon::lancer(
        prophet_daemon::essai::binaire_voisin("prophet-ledger")
            .to_str()
            .expect("chemin lisible"),
        &socket_journal,
        &temp.path().join("etat-journal"),
    );
    let client_journal = journal.joindre().await;
    let jeton = client_capd
        .call(
            "cap.mint",
            json!({
                "manifest": manifeste(),
                "grants": [{ "res": "net", "act": "egress", "match": "127.0.0.1" }],
                "task": "task:journal",
                "user": "prophet"
            }),
        )
        .await
        .expect("jeton émis");
    let socket = temp.path().join("egress.sock");
    let daemon = Daemon::lancer_avec(
        EGRESS,
        &socket,
        &temp.path().join("etat"),
        &[
            ("PROPHET_CAPD_SOCKET", socket_capd.to_str().expect("chemin")),
            (
                "PROPHET_LEDGER_SOCKET",
                socket_journal.to_str().expect("chemin"),
            ),
        ],
    );
    daemon.attendre_reponse(SONDE).await;

    let (reponse, _) = demander_avec_corps(
        &socket,
        &format!(
            "GET http://127.0.0.1:{}/page HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Proxy-Authorization: Prophet {}\r\n\r\n",
            temoin.port,
            base64_json(&jeton)
        ),
    )
    .await;
    assert!(reponse.contains("200"), "{reponse}");
    // Un hôte que le jeton ne couvre pas : capd refuse, et le refus s'inscrit aussi.
    let (refus, _) = demander_avec_corps(
        &socket,
        &format!(
            "GET http://exemple.invalide/ HTTP/1.1\r\nHost: exemple.invalide\r\n\
             Proxy-Authorization: Prophet {}\r\n\r\n",
            base64_json(&jeton)
        ),
    )
    .await;
    assert!(refus.contains("403"), "{refus}");
    // Un jeton forgé au nom d'une autre mission : refusé, et rien n'est écrit pour elle.
    let _ = demander(
        &socket,
        &format!(
            "GET http://127.0.0.1:{}/forge HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Proxy-Authorization: Prophet {}\r\n\r\n",
            temoin.port,
            base64_json(&jeton_forge())
        ),
    )
    .await;

    let mut evenements = Vec::new();
    for _ in 0..50 {
        evenements = client_journal
            .call("ledger.query", json!({"task": "task:journal"}))
            .await
            .expect("journal lisible")
            .as_array()
            .cloned()
            .unwrap_or_default();
        if evenements.len() >= 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let sortie = evenements
        .iter()
        .find(|e| e["kind"] == "net.request")
        .unwrap_or_else(|| panic!("net.request attendu : {evenements:?}"));
    assert_eq!(sortie["payload"]["host"], "127.0.0.1");
    assert_eq!(sortie["payload"]["method"], "GET");
    assert_eq!(sortie["payload"]["status"], 200);
    assert!(
        sortie["payload"]["bytes_in"].as_u64().unwrap_or(0) > 0,
        "{sortie}"
    );
    assert_eq!(sortie["actor"], "egress");
    let refus = evenements
        .iter()
        .find(|e| e["kind"] == "net.deny")
        .unwrap_or_else(|| panic!("net.deny attendu : {evenements:?}"));
    assert_eq!(refus["payload"]["host"], "exemple.invalide");
    let forge = client_journal
        .call("ledger.query", json!({"task": "task:forge"}))
        .await
        .expect("journal lisible");
    assert_eq!(forge, json!([]), "un jeton forgé n'écrit rien");
}
