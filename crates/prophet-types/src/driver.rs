//! Contrat des pilotes d'agents.
//!
//! Voir `docs/specs/agent-driver.md`. Un pilote enveloppe soit un client officiel d'éditeur
//! connecté par abonnement (Claude Code, Codex CLI, Gemini CLI), soit la boucle agentique native
//! sur un modèle local ou une API. Le reste du système ne connaît que ce contrat.

use serde::{Deserialize, Serialize};

/// Nature du pilote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DriverKind {
    /// Client officiel d'un éditeur, lancé sans modification.
    OfficialClient,
    /// Boucle agentique native de Prophet OS.
    Native,
}

/// Mode d'authentification du pilote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthMode {
    /// Abonnement grand public, via le flux de connexion du client officiel.
    Subscription,
    /// Aucune authentification (modèle local).
    None,
    /// Clé d'API (classe C, optionnelle).
    ApiKey,
}

/// Ce qu'un pilote sait faire. Déclaré par `driver.capabilities`, vérifié par la suite de
/// conformité.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Supports {
    /// Reprise d'une session précédente.
    pub resume: bool,
    /// Points de reprise complets (contexte sérialisé).
    pub checkpoint: bool,
    /// Duplication d'une tâche à un point donné.
    pub fork: bool,
    /// Comptage de tokens.
    pub token_usage: bool,
    /// Estimation du quota d'abonnement restant.
    pub quota_estimate: bool,
    /// Coût monétaire par appel.
    pub cost: bool,
    /// Flux d'événements en continu.
    pub streaming_events: bool,
    /// Délégation des demandes de permission à l'OS.
    pub permission_delegation: bool,
}

/// Réponse à `driver.capabilities`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverCapabilities {
    /// Nom du pilote.
    pub driver: String,
    /// Nature.
    pub kind: DriverKind,
    /// Mode d'authentification.
    pub auth: AuthMode,
    /// Fonctions disponibles.
    pub supports: Supports,
    /// Vrai si une session utilisateur est active.
    pub logged_in: bool,
    /// Modèles proposés.
    #[serde(default)]
    pub models: Vec<String>,
}

/// Niveau et profil de sandbox demandés pour une exécution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxRequest {
    /// Niveau d'isolation (0, 1 ou 2).
    pub level: u8,
    /// Profil de rootfs (`base`, `python`, `node`, `browser`).
    pub profile: String,
}

/// Bornes d'exécution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    /// Durée maximale en secondes.
    pub wall_time_s: u64,
    /// Nombre maximal d'étapes.
    pub max_steps: u32,
}

/// Requête `driver.start`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartRequest {
    /// Pilote visé.
    pub driver: String,
    /// Identifiant de tâche.
    pub task: String,
    /// Intention exprimée.
    pub intent: String,
    /// Répertoire de travail (sous-volume de la tâche).
    pub workdir: String,
    /// Chemin du fichier de configuration MCP généré pour cette tâche.
    pub mcp_config: String,
    /// Jeton de capacité encodé.
    pub token: String,
    /// Sandbox demandée.
    pub sandbox: SandboxRequest,
    /// Bornes.
    pub limits: Limits,
    /// Session à reprendre, le cas échéant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<String>,
}

/// Réponse à `driver.start`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartResponse {
    /// Identifiant d'exécution.
    pub run: String,
    /// Référence opaque de session, réutilisable pour une reprise.
    pub session_ref: String,
}

/// Issue d'une exécution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    /// Terminée normalement.
    Ok,
    /// Échec.
    Failed,
    /// Annulée.
    Cancelled,
}

/// Événement émis par un pilote pendant une exécution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DriverEvent {
    /// Nouvelle étape de la boucle.
    Step {
        /// Numéro d'étape.
        n: u32,
        /// Résumé destiné à l'humain.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
    /// Appel d'outil émis par le modèle.
    ToolCall {
        /// Nom de l'outil.
        tool: String,
        /// Empreinte des arguments.
        args_digest: String,
        /// Voie d'appel.
        via: ToolVia,
    },
    /// Résultat d'un appel d'outil.
    ToolResult {
        /// Nom de l'outil.
        tool: String,
        /// Succès.
        ok: bool,
        /// Code d'erreur éventuel.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Texte destiné à l'humain.
    Text {
        /// Rôle émetteur.
        role: String,
        /// Contenu.
        text: String,
    },
    /// Demande de permission remontée par le client.
    PermissionRequest {
        /// Identifiant de la demande.
        id: String,
        /// Outil concerné.
        tool: String,
        /// Empreinte des arguments.
        args_digest: String,
        /// Justification éventuelle.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Consommation observée.
    Usage {
        /// Tokens en entrée.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tokens_in: Option<u64>,
        /// Tokens en sortie.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tokens_out: Option<u64>,
        /// Coût en euros.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost_eur: Option<f64>,
        /// Pourcentage de quota d'abonnement consommé.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        quota_pct: Option<f64>,
    },
    /// Point de reprise créé.
    Checkpoint {
        /// Référence du point de reprise.
        checkpoint: String,
    },
    /// Fin d'exécution.
    Done {
        /// Issue.
        status: RunStatus,
        /// Raison en cas d'échec.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        /// Référence de session réutilisable.
        session_ref: String,
    },
}

/// Voie par laquelle un outil a été appelé.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolVia {
    /// Serveur MCP système.
    Mcp,
    /// Outil interne au client.
    Builtin,
}

impl DriverEvent {
    /// Vrai si l'événement clôt l'exécution.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Done { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evenements_serialises_avec_un_champ_type() {
        let event = DriverEvent::ToolCall {
            tool: "fs.read".into(),
            args_digest: "blake3:aa".into(),
            via: ToolVia::Mcp,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"tool_call""#), "{json}");
        let back: DriverEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    #[test]
    fn done_est_terminal() {
        let done = DriverEvent::Done {
            status: RunStatus::Ok,
            reason: None,
            session_ref: "s1".into(),
        };
        assert!(done.is_terminal());
        assert!(
            !DriverEvent::Step {
                n: 1,
                summary: None
            }
            .is_terminal()
        );
    }

    #[test]
    fn capacites_aller_retour() {
        let caps = DriverCapabilities {
            driver: "claude-code".into(),
            kind: DriverKind::OfficialClient,
            auth: AuthMode::Subscription,
            supports: Supports {
                resume: true,
                token_usage: true,
                quota_estimate: true,
                streaming_events: true,
                permission_delegation: true,
                ..Supports::default()
            },
            logged_in: false,
            models: vec!["default".into()],
        };
        let json = serde_json::to_string(&caps).unwrap();
        assert!(json.contains("official-client"), "{json}");
        assert_eq!(
            serde_json::from_str::<DriverCapabilities>(&json).unwrap(),
            caps
        );
    }
}
