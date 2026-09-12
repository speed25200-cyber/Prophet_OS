//! La surface montre-t-elle ce qui se passe vraiment ?
//!
//! Les tests du rendu prouvent que le champ se dessine ; ceux de `depuis` prouvent qu'une tâche se
//! traduit correctement en filament. Aucun des deux ne prouve ce qui compte le plus : que ce qui
//! est affiché **vient du système**.
//!
//! Ce test démarre `capd` et `agentd`, crée une vraie tâche par le socket d'`agentd`, et vérifie
//! qu'elle apparaît dans la scène — avec son intitulé. Sans lui, la surface pourrait afficher une
//! belle scène d'exemple et personne ne verrait la différence. C'était d'ailleurs le cas jusqu'ici.
//!
//! Aucun GPU n'est nécessaire : la scène est une structure de données, et c'est elle qu'on
//! interroge. Le dessin est vérifié ailleurs.

use std::time::Duration;

use prophet_daemon::essai::{Daemon, binaire_voisin};
use serde_json::json;
use surface::fenetre::Source as _;
use surface::reel::{Reel, Sockets};

fn manifeste() -> serde_json::Value {
    json!({
        "agent": {
            "id": "org.essai.surface",
            "version": "1.0.0",
            "name": "Essai de la surface",
            "publisher_key": "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        },
        "model": { "preferred": ["local:qwen3-8b"] },
        "capabilities": { "max": { "fs.read": ["~/essai/**"] } }
    })
}

/// Laisse au fil de fond le temps de faire au moins un tour.
///
/// Il interroge toutes les 250 ms ; on attend franchement plus, parce qu'un test qui regarde trop
/// tôt échouerait pour une raison qui n'a rien à voir avec ce qu'il vérifie.
fn laisser_interroger() {
    std::thread::sleep(Duration::from_millis(800));
}

#[test]
fn une_tache_reelle_devient_un_filament() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let socket_capd = temp.path().join("capd.sock");
    let socket_agentd = temp.path().join("agentd.sock");
    let socket_ledger = temp.path().join("ledger.sock");

    let capd = Daemon::lancer(
        binaire_voisin("prophet-capd").to_str().expect("chemin"),
        &socket_capd,
        &temp.path().join("etat-capd"),
    );
    let agentd = Daemon::lancer_avec(
        binaire_voisin("prophet-agentd").to_str().expect("chemin"),
        &socket_agentd,
        &temp.path().join("etat-agentd"),
        &[
            ("PROPHET_CAPD_SOCKET", socket_capd.to_str().expect("chemin")),
            (
                "PROPHET_LEDGER_SOCKET",
                socket_ledger.to_str().expect("chemin"),
            ),
            ("PROPHET_HOME", temp.path().to_str().expect("chemin")),
        ],
    );

    let execution = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("exécution asynchrone");

    execution.block_on(async {
        drop(capd.joindre().await);
        let agents = agentd.joindre().await;
        agents
            .call(
                "task.spawn",
                json!({
                    "id": "task:visible",
                    "intent": "préparer le rapport hebdomadaire",
                    "user": "prophet",
                    "manifest": manifeste(),
                    "requested": [{ "res": "fs", "act": "read", "match": "~/essai/**" }],
                    "availability": { "local_models": ["qwen3-8b"] }
                }),
            )
            .await
            .expect("une tâche se crée");
    });

    let mut source = Reel::demarrer(Sockets {
        agentd: socket_agentd,
        capd: socket_capd,
        sandboxd: temp.path().join("sandboxd.sock"),
    });
    laisser_interroger();

    let scene = source.scene();
    assert_eq!(
        scene.courants.len(),
        1,
        "la tâche créée doit être le seul filament du champ : {:?}",
        scene.courants
    );
    let courant = &scene.courants[0];
    assert_eq!(courant.tache, "task:visible");
    assert_eq!(
        courant.intitule, "préparer le rapport hebdomadaire",
        "et le filament doit porter l'intention exprimée, pas un libellé inventé"
    );
    assert_eq!(
        courant.agent, "local:qwen3-8b",
        "le pilote retenu est ce que la surface nomme : {courant:?}"
    );
}

#[test]
fn un_agentd_qui_meurt_vide_le_champ_au_lieu_de_le_figer() {
    // Le piège serait de garder la dernière image connue : l'écran montrerait des tâches en train
    // de courir alors que plus rien ne tourne. Faux, et crédible — la pire combinaison.
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let socket_capd = temp.path().join("capd.sock");
    let socket_agentd = temp.path().join("agentd.sock");

    let capd = Daemon::lancer(
        binaire_voisin("prophet-capd").to_str().expect("chemin"),
        &socket_capd,
        &temp.path().join("etat-capd"),
    );
    let agentd = Daemon::lancer_avec(
        binaire_voisin("prophet-agentd").to_str().expect("chemin"),
        &socket_agentd,
        &temp.path().join("etat-agentd"),
        &[
            ("PROPHET_CAPD_SOCKET", socket_capd.to_str().expect("chemin")),
            (
                "PROPHET_LEDGER_SOCKET",
                temp.path().join("ledger.sock").to_str().expect("chemin"),
            ),
            ("PROPHET_HOME", temp.path().to_str().expect("chemin")),
        ],
    );

    let execution = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("exécution asynchrone");
    execution.block_on(async {
        drop(capd.joindre().await);
        let agents = agentd.joindre().await;
        agents
            .call(
                "task.spawn",
                json!({
                    "id": "task:ephemere",
                    "intent": "quelque chose",
                    "user": "prophet",
                    "manifest": manifeste(),
                    "requested": [{ "res": "fs", "act": "read", "match": "~/essai/**" }],
                    "availability": { "local_models": ["qwen3-8b"] }
                }),
            )
            .await
            .expect("une tâche se crée");
    });

    let mut source = Reel::demarrer(Sockets {
        agentd: socket_agentd,
        capd: socket_capd,
        sandboxd: temp.path().join("sandboxd.sock"),
    });
    laisser_interroger();
    assert_eq!(
        source.scene().courants.len(),
        1,
        "la tâche doit d'abord être visible, sans quoi le reste du test ne prouve rien"
    );

    // `agentd` s'arrête. Sa mémoire des tâches meurt avec lui — elle n'est pas persistée — et le
    // champ doit le refléter immédiatement.
    drop(agentd);
    laisser_interroger();

    let scene = source.scene();
    assert!(
        scene.courants.is_empty(),
        "un daemon muet ne doit pas laisser ses anciennes tâches à l'écran : {:?}",
        scene.courants
    );
    assert!(
        scene
            .isolation
            .manque
            .as_deref()
            .is_some_and(|m| m.contains("agentd")),
        "et la panne doit être nommée, obtenu : {:?}",
        scene.isolation.manque
    );
}
