//! Le daemon `prophet-agentd` fait-il vraiment tourner la chaîne ?
//!
//! Ce test est le premier du dépôt où trois daemons dialoguent : `agentd` demande un jeton à
//! `capd`, planifie une tâche, et pousse ses événements vers `ledger`. C'est cette chaîne qui
//! *est* Prophet OS ; chacun des trois pris séparément ne prouve rien sur elle.
//!
//! Deux propriétés y sont vérifiées, et ce sont celles qui manquaient :
//!
//! - **Aucune tâche sans jeton.** Si `capd` est injoignable, rien n'est planifié. Une tâche qui
//!   démarrerait sans jeton agirait sans qu'aucune capacité ne la borne.
//! - **Le journal ne se répète pas.** Les événements sont retirés quand ils sont écrits. Sans
//!   cela, chaque planification rejouerait tout l'historique, et un journal qui raconte deux fois
//!   la même chose ne raconte plus rien de fiable.

use prophet_daemon::essai::{Daemon, binaire_voisin};
use serde_json::{Value, json};

const AGENTD: &str = env!("CARGO_BIN_EXE_prophet-agentd");

/// Un manifeste minimal, dont le pilote préféré est un modèle local.
fn manifeste() -> Value {
    json!({
        "agent": {
            "id": "org.essai.chaine",
            "version": "1.0.0",
            "name": "Essai de la chaîne",
            "publisher_key": "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        },
        "model": { "preferred": ["local:qwen3-8b"] },
        "capabilities": { "max": { "fs.read": ["~/essai/**"] } }
    })
}

fn demande(id: &str) -> Value {
    json!({
        "id": id,
        "intent": "lire les fichiers d'essai et en faire un résumé",
        "user": "prophet",
        "manifest": manifeste(),
        "requested": [{ "res": "fs", "act": "read", "match": "~/essai/**" }],
        "scopes": ["~/essai"],
        "availability": { "local_models": ["qwen3-8b"] }
    })
}

/// Monte la chaîne complète et rend les clients des trois daemons.
struct Chaine {
    _temp: tempfile::TempDir,
    _capd: Daemon,
    _ledger: Daemon,
    _agentd: Daemon,
    agents: prophet_ipc::Client,
    journal: prophet_ipc::Client,
}

impl Chaine {
    async fn monter() -> Self {
        let temp = tempfile::tempdir().expect("répertoire temporaire");
        let socket_capd = temp.path().join("capd.sock");
        let socket_ledger = temp.path().join("ledger.sock");

        let capd = Daemon::lancer(
            binaire_voisin("prophet-capd").to_str().expect("chemin"),
            &socket_capd,
            &temp.path().join("etat-capd"),
        );
        let ledger = Daemon::lancer(
            binaire_voisin("prophet-ledger").to_str().expect("chemin"),
            &socket_ledger,
            &temp.path().join("etat-ledger"),
        );
        drop(capd.joindre().await);
        let journal = ledger.joindre().await;

        let agentd = Daemon::lancer_avec(
            AGENTD,
            &temp.path().join("agentd.sock"),
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
        let agents = agentd.joindre().await;

        Self {
            _temp: temp,
            _capd: capd,
            _ledger: ledger,
            _agentd: agentd,
            agents,
            journal,
        }
    }

    async fn evenements(&self) -> Vec<Value> {
        self.journal
            .call("ledger.query", json!({}))
            .await
            .expect("le journal se relit")
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

#[tokio::test]
async fn une_tache_planifiee_traverse_les_trois_daemons() {
    let chaine = Chaine::monter().await;

    let plan = chaine
        .agents
        .call("task.spawn", demande("task:essai"))
        .await
        .expect("une tâche se planifie");

    assert_eq!(plan["task"], "task:essai");
    assert_eq!(
        plan["choice"]["reference"], "local:qwen3-8b",
        "le pilote retenu doit être celui que le manifeste préfère : {plan}"
    );
    assert!(
        plan["choice"]["reason"]
            .as_str()
            .is_some_and(|r| !r.is_empty()),
        "et le plan doit expliquer pourquoi, puisqu'un humain doit pouvoir dire non : {plan}"
    );

    // La tâche existe, et se relit.
    let tache = chaine
        .agents
        .call("task.status", json!({ "id": "task:essai" }))
        .await
        .expect("la tâche se relit");
    assert_eq!(tache["state"], "planned");

    // Et le journal l'a vue passer, chez le daemon qui en est le seul écrivain.
    let evenements = chaine.evenements().await;
    let types: Vec<&str> = evenements
        .iter()
        .filter_map(|e| e["kind"].as_str())
        .collect();
    assert!(
        types.contains(&"task.created") && types.contains(&"task.planned"),
        "la création et la planification doivent être journalisées, obtenu : {types:?}"
    );
}

#[tokio::test]
async fn le_journal_ne_se_repete_pas() {
    // Le défaut que ce test existe pour attraper : si les événements ne sont pas retirés après
    // avoir été écrits, la deuxième planification réécrit ceux de la première.
    let chaine = Chaine::monter().await;

    chaine
        .agents
        .call("task.spawn", demande("task:une"))
        .await
        .expect("première tâche");
    let apres_une = chaine.evenements().await.len();

    chaine
        .agents
        .call("task.spawn", demande("task:deux"))
        .await
        .expect("seconde tâche");
    let apres_deux = chaine.evenements().await.len();

    assert_eq!(
        apres_deux - apres_une,
        apres_une,
        "la seconde tâche doit ajouter autant d'événements que la première, pas davantage — \
         {apres_une} puis {apres_deux}"
    );

    // Et chaque tâche n'apparaît qu'avec ses propres événements.
    let evenements = chaine.evenements().await;
    let pour_une = evenements
        .iter()
        .filter(|e| e["task"] == "task:une")
        .count();
    assert_eq!(
        pour_une, apres_une,
        "les événements de la première tâche ne doivent pas avoir été réécrits"
    );
}

#[tokio::test]
async fn sans_capd_aucune_tache_n_est_planifiee() {
    // Une tâche qui démarrerait sans jeton agirait sans qu'aucune capacité ne la borne. Le refus
    // est donc le comportement correct, et il doit nommer la cause.
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let agentd = Daemon::lancer_avec(
        AGENTD,
        &temp.path().join("agentd.sock"),
        &temp.path().join("etat"),
        &[
            ("PROPHET_CAPD_SOCKET", "/nulle/part/capd.sock"),
            ("PROPHET_LEDGER_SOCKET", "/nulle/part/ledger.sock"),
            ("PROPHET_HOME", temp.path().to_str().expect("chemin")),
        ],
    );
    let agents = agentd.joindre().await;

    let erreur = agents
        .call("task.spawn", demande("task:orpheline"))
        .await
        .expect_err("sans broker, aucune tâche ne doit naître");
    assert!(
        erreur.message.contains("capd"),
        "le refus doit nommer ce qui manque, obtenu : {}",
        erreur.message
    );

    let liste = agents
        .call("task.list", json!({}))
        .await
        .expect("la liste se lit");
    assert_eq!(
        liste.as_array().map(Vec::len),
        Some(0),
        "et aucune tâche à demi créée ne doit rester : {liste}"
    );
}

#[tokio::test]
async fn annuler_une_tache_inconnue_ne_fabrique_rien() {
    let chaine = Chaine::monter().await;
    let erreur = chaine
        .agents
        .call("task.cancel", json!({ "id": "task:fantome" }))
        .await
        .expect_err("une tâche inexistante ne s'annule pas");
    assert_eq!(erreur.code, prophet_ipc::ErrorCode::NotFound);
}
