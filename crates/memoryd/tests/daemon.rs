//! Le daemon `prophet-memoryd` cloisonne-t-il vraiment ?
//!
//! Un magasin de mémoire est facile à écrire et facile à rendre poreux. Ce qui est testé ici est
//! le cloisonnement : ce qu'un espace contient ne se retrouve pas dans un autre, et un appel qui
//! ne dit pas où chercher ne cherche pas partout.

use prophet_daemon::essai::Daemon;
use serde_json::json;

const PROGRAMME: &str = env!("CARGO_BIN_EXE_prophet-memoryd");

#[tokio::test]
async fn ce_qui_est_appris_se_retrouve() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let daemon = Daemon::lancer(
        PROGRAMME,
        &temp.path().join("memoryd.sock"),
        &temp.path().join("etat"),
    );
    let client = daemon.joindre().await;

    let appris = client
        .call(
            "memory.remember",
            json!({
                "space": "perso",
                "text": "la gare de départ habituelle est Lyon Part-Dieu",
                "kind": "preference"
            }),
        )
        .await
        .expect("un fait s'apprend");
    assert!(appris["id"].as_str().is_some_and(|i| !i.is_empty()));

    let trouve = client
        .call(
            "memory.search",
            json!({ "space": "perso", "query": "gare de départ" }),
        )
        .await
        .expect("la mémoire se cherche");
    let trouve = trouve.as_array().expect("une liste");
    assert!(
        !trouve.is_empty(),
        "ce qu'on vient d'apprendre doit se retrouver"
    );
    assert!(
        trouve[0]["text"]
            .as_str()
            .is_some_and(|t| t.contains("Part-Dieu"))
    );
}

#[tokio::test]
async fn un_espace_ne_deborde_pas_dans_un_autre() {
    // Le cloisonnement est la seule raison pour laquelle les espaces existent. S'il fuit, ils ne
    // sont plus qu'une étiquette décorative.
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let daemon = Daemon::lancer(
        PROGRAMME,
        &temp.path().join("memoryd.sock"),
        &temp.path().join("etat"),
    );
    let client = daemon.joindre().await;

    client
        .call(
            "memory.remember",
            json!({ "space": "travail", "text": "le dépôt interne est sur gitlab" }),
        )
        .await
        .expect("apprentissage");

    let ailleurs = client
        .call(
            "memory.search",
            json!({ "space": "perso", "query": "dépôt interne gitlab" }),
        )
        .await
        .expect("recherche");
    assert_eq!(
        ailleurs.as_array().map(Vec::len),
        Some(0),
        "ce qui a été appris dans « travail » n'a rien à faire dans « perso » : {ailleurs}"
    );

    let chez_lui = client
        .call(
            "memory.search",
            json!({ "space": "travail", "query": "dépôt interne gitlab" }),
        )
        .await
        .expect("recherche");
    assert!(
        !chez_lui.as_array().expect("une liste").is_empty(),
        "et doit se retrouver chez lui"
    );
}

#[tokio::test]
async fn un_appel_sans_espace_ne_cherche_pas_partout() {
    // Le défaut dangereux serait « tous les espaces ». Un appel mal formé doit échouer, pas
    // parcourir la mémoire entière.
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let daemon = Daemon::lancer(
        PROGRAMME,
        &temp.path().join("memoryd.sock"),
        &temp.path().join("etat"),
    );
    let client = daemon.joindre().await;

    let erreur = client
        .call("memory.search", json!({ "query": "n'importe quoi" }))
        .await
        .expect_err("sans espace, la question n'a pas de sens");
    assert_eq!(erreur.code, prophet_ipc::ErrorCode::InvalidParams);
    assert!(erreur.message.contains("space"), "{}", erreur.message);
}

#[tokio::test]
async fn oublier_dit_combien_a_ete_oublie() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let daemon = Daemon::lancer(
        PROGRAMME,
        &temp.path().join("memoryd.sock"),
        &temp.path().join("etat"),
    );
    let client = daemon.joindre().await;

    for n in 0..3 {
        client
            .call(
                "memory.remember",
                json!({ "space": "jetable", "text": format!("note numéro {n}") }),
            )
            .await
            .expect("apprentissage");
    }

    let oublie = client
        .call("memory.forget_space", json!({ "space": "jetable" }))
        .await
        .expect("un espace s'oublie");
    assert_eq!(
        oublie["forgotten"], 3,
        "« oublié » sans quantité laisserait croire à une opération sans effet"
    );

    let reste = client
        .call("memory.list", json!({ "space": "jetable" }))
        .await
        .expect("liste");
    assert_eq!(reste.as_array().map(Vec::len), Some(0));
}

#[tokio::test]
async fn la_memoire_survit_a_un_redemarrage() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let socket = temp.path().join("memoryd.sock");
    let etat = temp.path().join("etat");

    {
        let daemon = Daemon::lancer(PROGRAMME, &socket, &etat);
        let client = daemon.joindre().await;
        client
            .call(
                "memory.remember",
                json!({ "space": "perso", "text": "ceci doit survivre" }),
            )
            .await
            .expect("apprentissage");
    }

    let daemon = Daemon::lancer(PROGRAMME, &socket, &etat);
    let client = daemon.joindre().await;
    let liste = client
        .call("memory.list", json!({ "space": "perso" }))
        .await
        .expect("liste");
    assert_eq!(
        liste.as_array().map(Vec::len),
        Some(1),
        "une mémoire qui s'efface au redémarrage n'en est pas une : {liste}"
    );
}
