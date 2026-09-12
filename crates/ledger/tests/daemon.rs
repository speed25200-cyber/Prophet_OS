//! Le daemon `prophet-ledger` tient-il vraiment un journal ?
//!
//! Comme pour `capd`, ce test lance le programme que systemd lancera, et lui parle par son socket.
//! Ce qu'il vérifie est ce qui distingue un journal d'un fichier : on y ajoute, on ne le réécrit
//! pas, et il sait le prouver.

use prophet_daemon::essai::Daemon;
use serde_json::json;

const PROGRAMME: &str = env!("CARGO_BIN_EXE_prophet-ledger");

#[tokio::test]
async fn ce_qui_est_ecrit_se_relit() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let daemon = Daemon::lancer(
        PROGRAMME,
        &temp.path().join("ledger.sock"),
        &temp.path().join("etat"),
    );
    let client = daemon.joindre().await;

    let ecrit = client
        .call(
            "ledger.append",
            json!({
                "kind": "task.created",
                "task": "task:essai",
                "actor": "daemon:essai",
                "payload": { "intent": "vérifier que le journal journalise" }
            }),
        )
        .await
        .expect("un événement s'ajoute");

    assert_eq!(ecrit["kind"], "task.created");
    assert_eq!(ecrit["seq"], 0, "la chaîne est numérotée à partir de zéro");
    assert!(
        ecrit["hash"]
            .as_str()
            .is_some_and(|h| h.starts_with("blake3:")),
        "chaque événement porte son empreinte, obtenu : {}",
        ecrit["hash"]
    );

    let relu = client
        .call("ledger.query", json!({ "task": "task:essai" }))
        .await
        .expect("le journal se relit");
    let relu = relu.as_array().expect("une liste");
    assert_eq!(relu.len(), 1);
    assert_eq!(relu[0]["hash"], ecrit["hash"]);
}

#[tokio::test]
async fn la_chaine_se_verifie_elle_meme() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let daemon = Daemon::lancer(
        PROGRAMME,
        &temp.path().join("ledger.sock"),
        &temp.path().join("etat"),
    );
    let client = daemon.joindre().await;

    for n in 0..5 {
        client
            .call(
                "ledger.append",
                json!({ "kind": "tool.call", "task": "task:essai", "payload": { "n": n } }),
            )
            .await
            .expect("un événement s'ajoute");
    }

    let sceau = client
        .call("ledger.seal", json!({}))
        .await
        .expect("la chaîne se scelle");
    assert_eq!(sceau["kind"], "ledger.seal");

    let rapport = client
        .call("ledger.verify", json!({}))
        .await
        .expect("le journal se vérifie");
    assert_eq!(rapport["ok"], true, "rapport : {rapport}");
    assert_eq!(
        rapport["seals"], 1,
        "le sceau posé doit être celui qui est vérifié"
    );
}

#[tokio::test]
async fn un_appelant_ne_peut_pas_reecrire_l_histoire() {
    // Le cas qui justifie que `Draft` ne soit pas désérialisable : `seq`, `prev` et `hash`
    // appartiennent au journal. Un appelant qui pourrait les poser choisirait sa place dans la
    // chaîne, et le chaînage ne prouverait plus rien.
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let daemon = Daemon::lancer(
        PROGRAMME,
        &temp.path().join("ledger.sock"),
        &temp.path().join("etat"),
    );
    let client = daemon.joindre().await;

    client
        .call("ledger.append", json!({ "kind": "task.created" }))
        .await
        .expect("premier");

    let truque = client
        .call(
            "ledger.append",
            json!({
                "kind": "task.done",
                // Un numéro qui ne peut pas coïncider avec le suivant légitime : sans quoi le
                // test passerait par accident, en confondant « refusé » et « demandé pareil ».
                "seq": 99,
                "prev": "blake3:0000000000000000000000000000000000000000000000000000000000000000",
                "hash": "blake3:1111111111111111111111111111111111111111111111111111111111111111"
            }),
        )
        .await
        .expect("l'écriture est acceptée : ces champs sont simplement ignorés");

    assert_eq!(
        truque["seq"], 1,
        "le journal numérote à la suite ; l'appelant ne choisit pas sa place"
    );
    assert_ne!(
        truque["hash"], "blake3:1111111111111111111111111111111111111111111111111111111111111111",
        "l'empreinte est calculée, pas reçue"
    );

    let rapport = client
        .call("ledger.verify", json!({}))
        .await
        .expect("vérif");
    assert_eq!(rapport["ok"], true, "rapport : {rapport}");
}

#[tokio::test]
async fn un_type_d_evenement_invente_est_refuse() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let daemon = Daemon::lancer(
        PROGRAMME,
        &temp.path().join("ledger.sock"),
        &temp.path().join("etat"),
    );
    let client = daemon.joindre().await;

    let erreur = client
        .call("ledger.append", json!({ "kind": "task.tout_va_bien" }))
        .await
        .expect_err("le vocabulaire du journal est fermé");
    assert_eq!(erreur.code, prophet_ipc::ErrorCode::InvalidParams);
}
