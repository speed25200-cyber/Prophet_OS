//! Le pont entre la session humaine et le service : l'adaptateur d'accessibilité.
//!
//! Les applications de bureau vivent dans la session de l'humain, sur son bus D-Bus et son bus
//! d'accessibilité, que rien d'extérieur à cette session ne peut joindre. L'adaptateur
//! (`prophet-supd`) tourne donc dans la session, lit les arbres AT-SPI, les rend en SUP et
//! exécute des actions typées ; `agentd` est son seul client, et c'est `agentd` qui fait
//! trancher capd avant chaque appel. Ce module fixe le vocabulaire de ce socket : les noms de
//! méthodes et la forme des paramètres et des réponses, partagés par les deux côtés.

use serde::{Deserialize, Serialize};

use crate::adapter::Provenance;
use crate::tree::Tree;

/// Chemin par défaut du socket de l'adaptateur, dans le répertoire d'exécution des services.
///
/// Le répertoire est accessible au groupe système de Prophet, dont l'humain fait partie : la
/// session y crée le socket, et `agentd`, membre du même groupe, s'y connecte.
pub const DEFAULT_SOCKET: &str = "/run/prophet/sup.sock";

/// `sup.apps` : les applications qui exposent une interface, sans leurs titres.
pub const METHOD_APPS: &str = "sup.apps";
/// `sup.tree` : l'arbre SUP d'une fenêtre d'application.
pub const METHOD_TREE: &str = "sup.tree";
/// `sup.act` : une action typée sur un élément, puis l'arbre résultant.
pub const METHOD_ACT: &str = "sup.act";
/// `sup.status` : l'adaptateur répond-il, et le bus d'accessibilité est-il joignable ?
pub const METHOD_STATUS: &str = "sup.status";
/// `client.status` : un client officiel de l'humain est-il installé et connecté ?
pub const METHOD_CLIENT_STATUS: &str = "client.status";
/// `client.run` : lancer un client officiel de l'humain sur une mission préparée, sans autre
/// outil que le pont MCP de Prophet, et attendre qu'il ait fini (ADR 0034).
pub const METHOD_CLIENT_RUN: &str = "client.run";

/// Paramètres de `client.status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientStatusRequest {
    /// Pilote : `claude-code`, `codex`, `gemini`.
    pub driver: String,
}

/// Réponse de `client.status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientStatus {
    /// Pilote interrogé.
    pub driver: String,
    /// Le client est installé sur la session.
    pub available: bool,
    /// Le client rapporte une session ouverte ; l'adaptateur ne lit jamais ses fichiers.
    pub logged_in: bool,
    /// État lisible.
    pub detail: String,
}

/// Paramètres de `client.run`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientRunRequest {
    /// Mission préparée que le client rejoindra par le pont MCP (`PROPHET_TASK`).
    pub task: String,
    /// Pilote à lancer.
    pub driver: String,
    /// Objectif complet donné au client.
    pub intent: String,
    /// Délai au-delà duquel le client est arrêté.
    #[serde(default = "default_client_timeout")]
    pub timeout_s: u64,
}

fn default_client_timeout() -> u64 {
    1200
}

/// Issue de `client.run` : ce que le client a fait de sa sortie, pas ce qu'il a fait dans la
/// mission — cela, agentd le sait par la séance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientRunOutcome {
    /// Code de retour du client, s'il a fini.
    pub exit_code: Option<i32>,
    /// Le délai l'a arrêté.
    pub timed_out: bool,
    /// Texte final rendu par le client, borné.
    pub text: String,
    /// Fin de sa sortie d'erreur, bornée, pour le diagnostic.
    pub stderr: String,
}

/// Une application vue par l'adaptateur.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppView {
    /// Identifiant de l'application, tel que capd le tranche (`ui.read` / `ui.act`).
    pub app: String,
    /// Nombre de fenêtres ouvertes.
    pub windows: usize,
}

/// Paramètres de `sup.tree`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeRequest {
    /// Identifiant de l'application.
    pub app: String,
    /// Fenêtre visée, par identifiant ; la fenêtre active sinon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<String>,
}

/// Paramètres de `sup.act`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActRequest {
    /// Identifiant de l'application.
    pub app: String,
    /// Fenêtre visée, par identifiant ; la fenêtre active sinon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<String>,
    /// Action SUP : `click`, `set_field`, `toggle`.
    pub action: String,
    /// Élément visé, par son identifiant dans l'arbre.
    pub node: String,
    /// Valeur, pour `set_field`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// Ce que l'adaptateur rend pour un arbre : l'arbre, et la confiance qu'on peut lui accorder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    /// L'arbre SUP de la fenêtre.
    pub tree: Tree,
    /// Origine de l'arbre.
    pub provenance: Provenance,
    /// Confiance indicative, de 0 à 1.
    pub confidence: f32,
    /// Avertissement destiné au modèle.
    pub caveat: String,
    /// Nombre de nœuds d'accessibilité laissés de côté par la borne de lecture, s'il y en a.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub truncated: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// Résultat de `sup.act` : ce qui s'est passé, et l'état qui en résulte.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    /// Ce que l'action a fait, en une phrase.
    pub message: String,
    /// L'arbre après l'action, si la fenêtre existe encore.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation: Option<Observation>,
}

/// Réponse de `sup.status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// Le bus d'accessibilité répond.
    pub ready: bool,
    /// Détail lisible : nombre d'applications, ou la raison d'une indisponibilité.
    pub detail: String,
}

impl Observation {
    /// Une observation d'accessibilité, avec la confiance et l'avertissement de cette origine.
    #[must_use]
    pub fn accessibility(tree: Tree, truncated: usize) -> Self {
        let provenance = Provenance::Accessibility;
        Self {
            tree,
            provenance,
            confidence: provenance.confidence(),
            caveat: provenance.caveat().to_owned(),
            truncated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{Node, Role};

    #[test]
    fn une_observation_dit_sa_confiance_et_sa_troncature() {
        let tree = Tree::new(
            "mousepad",
            "1",
            "Sans titre",
            Node::new("1", Role::Group, "f"),
        );
        let obs = Observation::accessibility(tree, 0);
        let json = serde_json::to_value(&obs).unwrap();
        assert_eq!(json["provenance"], "accessibility");
        assert!(json.get("truncated").is_none());
        assert!(obs.confidence < 1.0);
        let tronque = Observation::accessibility(obs.tree.clone(), 12);
        assert_eq!(serde_json::to_value(&tronque).unwrap()["truncated"], 12);
    }

    #[test]
    fn les_parametres_se_lisent_avec_leurs_options_absentes() {
        let req: ActRequest =
            serde_json::from_str(r#"{"app":"mousepad","action":"click","node":"23"}"#).unwrap();
        assert_eq!(req.window, None);
        assert_eq!(req.value, None);
        let tree: TreeRequest = serde_json::from_str(r#"{"app":"mousepad"}"#).unwrap();
        assert_eq!(tree.window, None);
    }
}
