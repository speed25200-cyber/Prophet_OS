//! Le daemon `prophet-vault` tient-il l'invariant qui le définit ?
//!
//! « Aucun secret ne transite par un modèle : le Vault rend des handles, jamais des valeurs. »
//! C'est une phrase de `CLAUDE.md` ; ces tests la rendent vérifiable.
//!
//! Le test central est celui du refus. Il tourne dans un environnement où le compte du proxy de
//! sortie n'existe pas — ce qui est le cas de toute machine de développement — et vérifie que le
//! coffre refuse alors *tout le monde*, y compris le pair qui vient de déposer le secret et qui
//! est, à ce moment-là, le plus légitime des appelants.

use prophet_daemon::essai::Daemon;
use serde_json::json;

const PROGRAMME: &str = env!("CARGO_BIN_EXE_prophet-vault");

fn secret(nom: &str) -> serde_json::Value {
    json!({
        "info": {
            "name": nom,
            "domains": ["api.exemple.fr"],
            "header": "Authorization",
            "description": "de quoi essayer"
        },
        "value": "valeur-tres-secrete-42"
    })
}

#[tokio::test]
async fn ce_qu_on_obtient_est_une_reference_jamais_une_valeur() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let daemon = Daemon::lancer(
        PROGRAMME,
        &temp.path().join("vault.sock"),
        &temp.path().join("etat"),
    );
    let client = daemon.joindre().await;

    let depose = client
        .call("vault.put", secret("jeton-sncf"))
        .await
        .expect("un secret se dépose");
    assert_eq!(depose["ref"], "prophet-secret:jeton-sncf");

    let liste = client
        .call("secrets.list_refs", json!({}))
        .await
        .expect("les références se listent");
    let texte = liste.to_string();
    assert!(texte.contains("jeton-sncf"), "le nom est public : {texte}");
    assert!(
        !texte.contains("valeur-tres-secrete-42"),
        "la valeur ne doit apparaître nulle part dans une liste : {texte}"
    );
}

#[tokio::test]
async fn sans_proxy_de_sortie_personne_n_obtient_de_valeur() {
    // Le cœur de l'invariant. Le pair qui appelle ici est celui qui vient de déposer le secret :
    // s'il existait un appelant à qui le coffre devait s'ouvrir par complaisance, ce serait
    // lui. Il est refusé, parce que le compte du proxy n'existe pas sur cette machine.
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let daemon = Daemon::lancer(
        PROGRAMME,
        &temp.path().join("vault.sock"),
        &temp.path().join("etat"),
    );
    let client = daemon.joindre().await;

    client
        .call("vault.put", secret("jeton-sncf"))
        .await
        .expect("un secret se dépose");

    for methode in ["secrets.use", "vault.reveal"] {
        match client.call(methode, json!({ "name": "jeton-sncf" })).await {
            Err(erreur) => assert_eq!(
                erreur.code,
                prophet_ipc::ErrorCode::Unauthorized,
                "{methode} doit être refusée faute de proxy, obtenu : {erreur:?}"
            ),
            // Arriver ici veut dire qu'une valeur a été rendue, ce qui est exactement ce que
            // l'invariant interdit.
            Ok(rendu) => panic!("{methode} a rendu quelque chose : {rendu}"),
        }
    }
}

#[tokio::test]
async fn la_reveleation_refusee_dit_pourquoi_sans_reveler() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let daemon = Daemon::lancer(
        PROGRAMME,
        &temp.path().join("vault.sock"),
        &temp.path().join("etat"),
    );
    let client = daemon.joindre().await;
    client
        .call("vault.put", secret("jeton-sncf"))
        .await
        .expect("dépôt");

    let erreur = client
        .call("secrets.use", json!({ "name": "jeton-sncf" }))
        .await
        .expect_err("aucune valeur ne sort d'ici");

    assert_eq!(erreur.code, prophet_ipc::ErrorCode::Unauthorized);
    assert!(
        erreur.message.contains("egress"),
        "le refus doit nommer qui aurait eu le droit : {}",
        erreur.message
    );
    assert!(
        !erreur.message.contains("valeur-tres-secrete-42"),
        "un message d'erreur n'est pas un endroit où faire fuir un secret : {}",
        erreur.message
    );
}

#[tokio::test]
async fn un_secret_n_est_presente_qu_aux_domaines_qui_le_concernent() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let daemon = Daemon::lancer(
        PROGRAMME,
        &temp.path().join("vault.sock"),
        &temp.path().join("etat"),
    );
    let client = daemon.joindre().await;
    client
        .call("vault.put", secret("jeton-sncf"))
        .await
        .expect("dépôt");

    let permis = client
        .call(
            "secrets.allowed_for",
            json!({ "name": "jeton-sncf", "host": "api.exemple.fr" }),
        )
        .await
        .expect("la question se pose");
    assert_eq!(permis["allowed"], true);

    let refuse = client
        .call(
            "secrets.allowed_for",
            json!({ "name": "jeton-sncf", "host": "collecteur.example.com" }),
        )
        .await
        .expect("la question se pose");
    assert_eq!(
        refuse["allowed"], false,
        "un secret destiné à un domaine ne part pas vers un autre"
    );
}
