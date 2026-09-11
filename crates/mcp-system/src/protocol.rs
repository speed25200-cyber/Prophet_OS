//! Le protocole MCP, réduit à ce dont un système d'exploitation a besoin.
//!
//! MCP est du JSON-RPC 2.0. Prophet OS l'implémente directement plutôt que d'ajouter une
//! dépendance : c'est le même codec que l'IPC interne (ADR-0003), et la surface utilisée est
//! petite et stable (`initialize`, `tools/list`, `tools/call`).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Version du protocole annoncée.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// Description d'un outil, telle que la voit un modèle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    /// Nom `<domaine>.<verbe>`.
    pub name: String,
    /// Description courte et précise, écrite pour un modèle.
    pub description: String,
    /// Schéma JSON des arguments.
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    /// Métadonnées propres à Prophet OS, ignorées par les clients qui ne les connaissent pas.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<ToolMeta>,
}

/// Métadonnées de sécurité d'un outil.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolMeta {
    /// Capacité exigée, sous la forme `<res>.<act>`.
    pub requires: String,
    /// L'appel est-il irréversible ?
    pub irreversible: bool,
    /// A-t-il un effet hors de la machine ?
    pub external: bool,
    /// Niveau de sandbox minimal imposé, le cas échéant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_level_min: Option<u8>,
}

/// Résultat d'un appel d'outil.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallResult {
    /// Contenu rendu au modèle.
    pub content: Vec<Content>,
    /// Vrai en cas d'erreur.
    #[serde(rename = "isError", default)]
    pub is_error: bool,
    /// Résultat structuré, quand il y en a un.
    #[serde(
        rename = "structuredContent",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub structured: Option<Value>,
}

/// Bloc de contenu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Content {
    /// Texte.
    Text {
        /// Contenu textuel.
        text: String,
    },
}

impl CallResult {
    /// Résultat structuré, accompagné de sa forme textuelle.
    #[must_use]
    pub fn structured(value: Value) -> Self {
        let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_owned());
        Self {
            content: vec![Content::Text { text }],
            is_error: false,
            structured: Some(value),
        }
    }

    /// Résultat purement textuel.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![Content::Text { text: text.into() }],
            is_error: false,
            structured: None,
        }
    }

    /// Erreur typée. Le modèle doit pouvoir agir sur le code, pas deviner à partir d'une phrase.
    #[must_use]
    pub fn error(code: ErrorCode, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        let value = json!({ "code": code.as_str(), "detail": detail });
        Self {
            content: vec![Content::Text {
                text: format!("{} : {detail}", code.as_str()),
            }],
            is_error: true,
            structured: Some(value),
        }
    }
}

/// Codes d'erreur rendus aux modèles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// La politique ou le jeton refuse.
    PolicyDenied,
    /// Une approbation humaine est nécessaire.
    ApprovalRequired,
    /// Cible introuvable.
    NotFound,
    /// Erreur de sandbox.
    SandboxError,
    /// Budget épuisé.
    BudgetExceeded,
    /// Arguments invalides.
    Invalid,
    /// Échec interne.
    Internal,
}

impl ErrorCode {
    /// Forme textuelle stable.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PolicyDenied => "PolicyDenied",
            Self::ApprovalRequired => "ApprovalRequired",
            Self::NotFound => "NotFound",
            Self::SandboxError => "SandboxError",
            Self::BudgetExceeded => "BudgetExceeded",
            Self::Invalid => "Invalid",
            Self::Internal => "Internal",
        }
    }
}

/// Réponse à `initialize`.
#[must_use]
pub fn initialize_result(server_name: &str) -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": server_name, "version": env!("CARGO_PKG_VERSION") }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resultat_structure_porte_aussi_du_texte() {
        let result = CallResult::structured(json!({"a": 1}));
        assert!(!result.is_error);
        assert_eq!(result.structured, Some(json!({"a": 1})));
        let Content::Text { text } = &result.content[0];
        assert!(text.contains("\"a\""), "{text}");
    }

    #[test]
    fn erreur_typee_lisible_par_le_modele() {
        let result = CallResult::error(ErrorCode::PolicyDenied, "hors du périmètre accordé");
        assert!(result.is_error);
        assert_eq!(result.structured.unwrap()["code"], json!("PolicyDenied"));
    }

    #[test]
    fn initialisation_annonce_la_version() {
        let result = initialize_result("prophet-fs");
        assert_eq!(result["protocolVersion"], json!(PROTOCOL_VERSION));
        assert_eq!(result["serverInfo"]["name"], json!("prophet-fs"));
    }

    #[test]
    fn specification_serialisable_avec_le_nom_attendu_par_mcp() {
        let spec = ToolSpec {
            name: "fs.read".into(),
            description: "Lit un fichier.".into(),
            input_schema: json!({"type": "object"}),
            meta: Some(ToolMeta {
                requires: "fs.read".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        };
        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("\"inputSchema\""), "{json}");
    }
}
