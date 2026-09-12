//! Le daemon `prophet-capd` répond-il vraiment ?
//!
//! Ce test lance le programme installé par l'image, et lui parle par son socket. Il ne teste pas
//! la bibliothèque — d'autres tests s'en chargent — mais le binaire, parce que c'est le binaire
//! que `systemd` lancera, et que son absence pure et simple n'avait été remarquée par personne.
//!
//! Une règle y est appliquée, tirée d'ADR-0006 : **constater une présence n'est pas vérifier**.
//! Attendre que le fichier de socket apparaisse ne prouve rien — il apparaît avant que le serveur
//! n'accepte. On attend donc qu'une connexion aboutisse *et* qu'un appel réponde.

use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use prophet_ipc::Client;
use serde_json::json;

/// Un daemon lancé pour la durée d'un test, tué à la fin quoi qu'il arrive.
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn lancer(socket: &Path, etat: &Path) -> Daemon {
    let enfant = Command::new(env!("CARGO_BIN_EXE_prophet-capd"))
        .env("PROPHET_SOCKET", socket)
        .env("STATE_DIRECTORY", etat)
        .env("RUST_LOG", "warn")
        .spawn()
        .expect("prophet-capd doit pouvoir être lancé");
    Daemon(enfant)
}

/// Attend que le daemon réponde, et non qu'il paraisse prêt.
async fn joindre(socket: &Path) -> Client {
    let limite = Instant::now() + Duration::from_secs(10);
    let mut derniere = String::new();
    while Instant::now() < limite {
        match Client::connect(socket).await {
            Ok(client) => match client.call("ping", json!({})).await {
                Ok(_) => return client,
                Err(e) => derniere = format!("connecté, mais ping a échoué : {}", e.message),
            },
            Err(e) => derniere = format!("connexion impossible : {e}"),
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("le daemon n'a pas répondu en 10 s — {derniere}");
}

#[tokio::test]
async fn le_daemon_repond_sur_son_socket() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let socket = temp.path().join("capd.sock");
    let etat = temp.path().join("etat");
    let _daemon = lancer(&socket, &etat);

    let client = joindre(&socket).await;

    let pong = client.call("ping", json!({})).await.expect("ping");
    assert_eq!(pong, json!("pong"));

    let clef = client
        .call("cap.public_key", json!({}))
        .await
        .expect("la clé publique se demande");
    let clef = clef["key"].as_str().expect("une chaîne");
    assert!(
        clef.starts_with("ed25519:"),
        "la clé doit dire son algorithme, obtenu : {clef}"
    );

    let attente = client
        .call("approval.pending", json!({}))
        .await
        .expect("la file d'attente se lit");
    assert_eq!(
        attente,
        json!([]),
        "un daemon qui vient de démarrer n'a rien à faire trancher"
    );
}

#[tokio::test]
async fn une_methode_inconnue_est_refusee_et_nommee() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let socket = temp.path().join("capd.sock");
    let _daemon = lancer(&socket, &temp.path().join("etat"));
    let client = joindre(&socket).await;

    let erreur = client
        .call("cap.tout_autoriser", json!({}))
        .await
        .expect_err("une méthode inventée ne doit pas répondre");
    assert_eq!(erreur.code, prophet_ipc::ErrorCode::MethodNotFound);
    assert!(
        erreur.message.contains("cap.tout_autoriser"),
        "l'erreur doit nommer ce qui a été demandé, obtenu : {}",
        erreur.message
    );
}

#[tokio::test]
async fn trancher_une_approbation_inconnue_ne_fabrique_rien() {
    // Le cas qui compte : une réponse complaisante à un identifiant inventé donnerait un droit
    // que personne n'a accordé.
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let socket = temp.path().join("capd.sock");
    let _daemon = lancer(&socket, &temp.path().join("etat"));
    let client = joindre(&socket).await;

    let erreur = client
        .call(
            "approval.resolve",
            json!({ "id": "apr:inexistante", "decision": "allow" }),
        )
        .await
        .expect_err("une demande inconnue ne se tranche pas");
    assert_eq!(erreur.code, prophet_ipc::ErrorCode::NotFound);

    let erreur = client
        .call(
            "approval.resolve",
            json!({ "id": "apr:x", "decision": "peut-être" }),
        )
        .await
        .expect_err("une décision qui n'en est pas une se refuse");
    assert_eq!(erreur.code, prophet_ipc::ErrorCode::InvalidParams);
}

#[tokio::test]
async fn la_cle_survit_a_un_redemarrage_et_reste_illisible_par_les_autres() {
    use std::os::unix::fs::PermissionsExt as _;

    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let socket = temp.path().join("capd.sock");
    let etat = temp.path().join("etat");

    let premiere = {
        let _daemon = lancer(&socket, &etat);
        let client = joindre(&socket).await;
        client.call("cap.public_key", json!({})).await.expect("clé")["key"]
            .as_str()
            .expect("une chaîne")
            .to_owned()
    };

    let mode = std::fs::metadata(etat.join("signing.key"))
        .expect("la clé est écrite dans l'état du service")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "la clé de signature ne se partage pas");

    // Le socket du premier daemon a été retiré à sa mort ; le second en pose un neuf.
    let _daemon = lancer(&socket, &etat);
    let client = joindre(&socket).await;
    let seconde = client.call("cap.public_key", json!({})).await.expect("clé")["key"]
        .as_str()
        .expect("une chaîne")
        .to_owned();

    assert_eq!(
        premiere, seconde,
        "une clé qui change au redémarrage invaliderait tous les jetons déjà émis"
    );
}

/// Un jeton signé par une clé qui n'est **pas** celle du daemon.
///
/// C'est la contrefaçon la plus évidente, et donc celle qu'il faut prouver refusée : sans cela,
/// n'importe qui pourrait s'accorder n'importe quel droit en écrivant le JSON qui l'arrange.
fn jeton_forge() -> serde_json::Value {
    use prophet_types::cap::{Act, Grant, Res, TokenBuilder};
    let clef_etrangere = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
    let jeton = TokenBuilder::new("capd", "task:forge", "agent.forge", "prophet")
        .grant(Grant::new(Res::Net, Act::Egress, "*"))
        .build(&clef_etrangere, time::OffsetDateTime::now_utc(), [0u8; 16])
        .expect("le faussaire sait construire un jeton bien formé");
    serde_json::to_value(jeton).expect("sérialisable")
}

#[tokio::test]
async fn un_jeton_signe_par_un_autre_n_accorde_rien() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let socket = temp.path().join("capd.sock");
    let _daemon = lancer(&socket, &temp.path().join("etat"));
    let client = joindre(&socket).await;

    let decision = client
        .call(
            "cap.check",
            json!({
                "token": jeton_forge(),
                "res": "net",
                "act": "egress",
                "target": "collecteur.example.com",
                "external": true
            }),
        )
        .await
        .expect("un refus est une réponse, pas une erreur de protocole");

    assert_eq!(
        decision["decision"], "deny",
        "un jeton signé ailleurs n'accorde rien : {decision}"
    );
    assert_eq!(
        decision["reason"], "bad_signature",
        "et le motif doit nommer la signature, pas autre chose : {decision}"
    );
}

#[tokio::test]
async fn un_controle_sans_jeton_ne_passe_pas_pour_un_refus_ordinaire() {
    // La distinction compte : « refusé » est une décision du broker, « paramètre manquant » est
    // une faute de l'appelant. Les confondre ferait passer un appel malformé pour une politique
    // appliquée.
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let socket = temp.path().join("capd.sock");
    let _daemon = lancer(&socket, &temp.path().join("etat"));
    let client = joindre(&socket).await;

    let erreur = client
        .call(
            "cap.check",
            json!({ "res": "net", "act": "egress", "target": "x" }),
        )
        .await
        .expect_err("sans jeton, il n'y a rien à contrôler");
    assert_eq!(erreur.code, prophet_ipc::ErrorCode::InvalidParams);
}
