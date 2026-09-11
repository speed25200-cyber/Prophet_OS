//! Messages JSON-RPC 2.0 et codes d'erreur de Prophet OS.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Version du protocole, littérale dans chaque message.
pub const JSONRPC_VERSION: &str = "2.0";

/// Requête JSON-RPC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// Toujours `"2.0"`.
    pub jsonrpc: String,
    /// Identifiant de corrélation.
    pub id: Value,
    /// Méthode `<domaine>.<verbe>`.
    pub method: String,
    /// Paramètres, objet JSON.
    #[serde(default)]
    pub params: Value,
}

impl Request {
    /// Construit une requête.
    #[must_use]
    pub fn new(id: impl Into<Value>, method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: id.into(),
            method: method.into(),
            params,
        }
    }
}

/// Notification JSON-RPC (sans `id`, sans réponse attendue).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    /// Toujours `"2.0"`.
    pub jsonrpc: String,
    /// Méthode.
    pub method: String,
    /// Paramètres.
    #[serde(default)]
    pub params: Value,
}

impl Notification {
    /// Construit une notification.
    #[must_use]
    pub fn new(method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            method: method.into(),
            params,
        }
    }
}

/// Erreur JSON-RPC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[error("{code:?} ({}): {message}", i32::from(*code))]
pub struct Error {
    /// Code stable.
    pub code: ErrorCode,
    /// Message lisible.
    pub message: String,
    /// Données complémentaires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl Error {
    /// Erreur sans données.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Erreur avec données.
    #[must_use]
    pub fn with_data(code: ErrorCode, message: impl Into<String>, data: Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }
}

/// Codes d'erreur du protocole et de Prophet OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// JSON illisible.
    ParseError,
    /// Requête mal formée ou trop grande.
    InvalidRequest,
    /// Méthode inconnue.
    MethodNotFound,
    /// Paramètres invalides.
    InvalidParams,
    /// Erreur interne.
    InternalError,
    /// Pair non autorisé ou jeton absent.
    Unauthorized,
    /// Refus de politique.
    PolicyDenied,
    /// Approbation humaine requise.
    ApprovalRequired,
    /// Budget dépassé.
    BudgetExceeded,
    /// Erreur de sandbox.
    SandboxError,
    /// Ressource introuvable.
    NotFound,
    /// État incompatible.
    Conflict,
}

impl From<ErrorCode> for i32 {
    fn from(code: ErrorCode) -> Self {
        match code {
            ErrorCode::ParseError => -32700,
            ErrorCode::InvalidRequest => -32600,
            ErrorCode::MethodNotFound => -32601,
            ErrorCode::InvalidParams => -32602,
            ErrorCode::InternalError => -32603,
            ErrorCode::Unauthorized => -32001,
            ErrorCode::PolicyDenied => -32002,
            ErrorCode::ApprovalRequired => -32003,
            ErrorCode::BudgetExceeded => -32004,
            ErrorCode::SandboxError => -32005,
            ErrorCode::NotFound => -32006,
            ErrorCode::Conflict => -32007,
        }
    }
}

impl TryFrom<i32> for ErrorCode {
    type Error = i32;

    fn try_from(value: i32) -> Result<Self, i32> {
        Ok(match value {
            -32700 => Self::ParseError,
            -32600 => Self::InvalidRequest,
            -32601 => Self::MethodNotFound,
            -32602 => Self::InvalidParams,
            -32603 => Self::InternalError,
            -32001 => Self::Unauthorized,
            -32002 => Self::PolicyDenied,
            -32003 => Self::ApprovalRequired,
            -32004 => Self::BudgetExceeded,
            -32005 => Self::SandboxError,
            -32006 => Self::NotFound,
            -32007 => Self::Conflict,
            other => return Err(other),
        })
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i32(i32::from(*self))
    }
}

impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = i32::deserialize(deserializer)?;
        Self::try_from(raw)
            .map_err(|code| serde::de::Error::custom(format!("code d'erreur inconnu : {code}")))
    }
}

/// Réponse JSON-RPC : résultat ou erreur, jamais les deux.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// Toujours `"2.0"`.
    pub jsonrpc: String,
    /// Identifiant de la requête.
    pub id: Value,
    /// Résultat en cas de succès.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Erreur en cas d'échec.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Error>,
}

impl Response {
    /// Réponse de succès.
    #[must_use]
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Réponse d'erreur.
    #[must_use]
    pub fn err(id: Value, error: Error) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// Extrait et retire le jeton `params._auth`.
///
/// Retourne `None` si le champ est absent ou si `params` n'est pas un objet.
pub fn extract_auth(params: &mut Value) -> Option<String> {
    let object = params.as_object_mut()?;
    match object.remove("_auth")? {
        Value::String(token) => Some(token),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn codes_d_erreur_aller_retour() {
        for code in [
            ErrorCode::ParseError,
            ErrorCode::Unauthorized,
            ErrorCode::PolicyDenied,
            ErrorCode::Conflict,
        ] {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(serde_json::from_str::<ErrorCode>(&json).unwrap(), code);
        }
        assert!(serde_json::from_str::<ErrorCode>("-42").is_err());
    }

    #[test]
    fn reponse_de_succes_sans_champ_error() {
        let json = serde_json::to_string(&Response::ok(json!(1), json!({"a": 1}))).unwrap();
        assert!(!json.contains("error"), "{json}");
    }

    #[test]
    fn extraction_du_jeton() {
        let mut params = json!({"path": "/x", "_auth": "jeton"});
        assert_eq!(extract_auth(&mut params).as_deref(), Some("jeton"));
        assert_eq!(params, json!({"path": "/x"}));
        assert_eq!(extract_auth(&mut params), None);
        assert_eq!(extract_auth(&mut json!([1, 2])), None);
    }

    #[test]
    fn message_d_erreur_lisible() {
        let e = Error::new(ErrorCode::PolicyDenied, "refus");
        assert!(e.to_string().contains("-32002"), "{e}");
    }
}
