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

use prophet_daemon::essai::Daemon;
use serde_json::json;

const PROGRAMME: &str = env!("CARGO_BIN_EXE_prophet-capd");

fn lancer(socket: &Path, etat: &Path) -> Daemon {
    Daemon::lancer(PROGRAMME, socket, etat)
}

#[tokio::test]
async fn le_daemon_repond_sur_son_socket() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let socket = temp.path().join("capd.sock");
    let etat = temp.path().join("etat");
    let daemon = lancer(&socket, &etat);

    let client = daemon.joindre().await;

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
    let daemon = lancer(&socket, &temp.path().join("etat"));
    let client = daemon.joindre().await;

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
    let daemon = lancer(&socket, &temp.path().join("etat"));
    let client = daemon.joindre().await;

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
        let daemon = lancer(&socket, &etat);
        let client = daemon.joindre().await;
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
    let daemon = lancer(&socket, &etat);
    let client = daemon.joindre().await;
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
    let daemon = lancer(&socket, &temp.path().join("etat"));
    let client = daemon.joindre().await;

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
    let daemon = lancer(&socket, &temp.path().join("etat"));
    let client = daemon.joindre().await;

    let erreur = client
        .call(
            "cap.check",
            json!({ "res": "net", "act": "egress", "target": "x" }),
        )
        .await
        .expect_err("sans jeton, il n'y a rien à contrôler");
    assert_eq!(erreur.code, prophet_ipc::ErrorCode::InvalidParams);
}

/// Une révocation survit au redémarrage de capd : sans cela, le jeton racine d'une mission
/// révoquée redevenait valide jusqu'à son expiration dès que le service repartait.
#[tokio::test]
async fn une_revocation_survit_au_redemarrage() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let socket = temp.path().join("capd.sock");
    let etat = temp.path().join("etat");
    let manifeste = json!({
        "agent": {
            "id": "org.essai.revocation",
            "version": "1.0.0",
            "name": "Essai de révocation",
            "publisher_key": "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        },
        "model": { "preferred": ["local:qwen3-8b"] },
        "capabilities": { "max": { "net.egress": ["127.0.0.1"] } }
    });
    let controle = |jeton: &serde_json::Value| json!({ "token": jeton, "res": "net", "act": "egress", "target": "127.0.0.1" });
    let (jeton, temoin) = {
        let daemon = lancer(&socket, &etat);
        let client = daemon.joindre().await;
        let emettre = |tache: &str| {
            client.call(
                "cap.mint",
                json!({
                    "manifest": manifeste,
                    "grants": [{ "res": "net", "act": "egress", "match": "127.0.0.1" }],
                    "task": tache,
                    "user": "prophet",
                    "ttl_seconds": 3600
                }),
            )
        };
        let jeton = emettre("task:revoquee").await.expect("jeton émis");
        let temoin = emettre("task:temoin").await.expect("jeton témoin émis");
        let permis = client.call("cap.check", controle(&jeton)).await.unwrap();
        assert_eq!(permis["decision"], "allow", "{permis}");
        client
            .call("cap.revoke", json!({ "subject": "task:revoquee" }))
            .await
            .expect("révoqué");
        // Révoquer deux fois ne dédouble rien et ne se refuse pas.
        client
            .call("cap.revoke", json!({ "subject": "task:revoquee" }))
            .await
            .expect("révoqué une seconde fois");
        let refus = client.call("cap.check", controle(&jeton)).await.unwrap();
        assert_eq!(refus["decision"], "deny", "{refus}");
        (jeton, temoin)
    };

    let daemon = lancer(&socket, &etat);
    let client = daemon.joindre().await;
    let refus = client.call("cap.check", controle(&jeton)).await.unwrap();
    assert_eq!(
        refus["decision"], "deny",
        "le jeton d'une mission révoquée ne revit pas au redémarrage : {refus}"
    );
    let permis = client.call("cap.check", controle(&temoin)).await.unwrap();
    assert_eq!(
        permis["decision"], "allow",
        "un jeton non révoqué reste valide après le redémarrage : {permis}"
    );
    let inscrites = std::fs::read_to_string(etat.join("revocations.jsonl"))
        .expect("les révocations sont inscrites dans l'état du service");
    assert_eq!(inscrites.lines().count(), 1, "{inscrites}");
    use std::os::unix::fs::PermissionsExt as _;
    let mode = std::fs::metadata(etat.join("revocations.jsonl"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}
