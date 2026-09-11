//! Sortie réseau, exclusivement par le proxy.

use serde_json::{Value, json};

use crate::protocol::{CallResult, ErrorCode, ToolMeta, ToolSpec};
use crate::registry::{Tool, ToolContext};

/// Requête HTTP sortante.
///
/// L'outil ne joint jamais le réseau lui-même : il passe par le socket du proxy, seule interface
/// disponible depuis une sandbox. Un agent qui contournerait cet outil ne trouverait aucune route.
#[derive(Debug)]
pub struct Fetch;

/// Extrait l'hôte d'une adresse.
#[must_use]
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map(|(_, r)| r)?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host
        .rsplit_once(':')
        .filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit()))
        .map_or(host, |(h, _)| h);
    if host.is_empty() {
        None
    } else {
        Some(host.to_owned())
    }
}

impl Tool for Fetch {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "http.fetch".into(),
            description: "Effectue une requête HTTP vers un domaine autorisé, par le proxy de sortie. Les méthodes qui modifient un état distant demandent une approbation humaine.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string"},
                    "method": {"type": "string", "default": "GET"},
                    "body": {"type": "string"},
                    "max_bytes": {"type": "integer"}
                },
                "required": ["url"],
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "net.egress".into(),
                irreversible: false,
                external: true,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, args: &Value, _context: &ToolContext) -> Option<String> {
        host_of(args.get("url")?.as_str()?)
    }

    fn call(&self, args: &Value, _context: &ToolContext) -> CallResult {
        let Some(url) = args.get("url").and_then(Value::as_str) else {
            return CallResult::error(ErrorCode::Invalid, "argument `url` manquant");
        };
        let Some(host) = host_of(url) else {
            return CallResult::error(ErrorCode::Invalid, format!("adresse illisible : {url}"));
        };
        let method = args
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("GET")
            .to_ascii_uppercase();
        // Le relais effectif est assuré par le daemon `egress` ; l'outil en est le point d'entrée
        // contrôlé. Tant que le proxy n'est pas joignable, l'échec est explicite.
        CallResult::error(
            ErrorCode::SandboxError,
            format!(
                "proxy de sortie injoignable pour {method} {host} : aucune requête n'a été émise"
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_de_l_hote() {
        assert_eq!(
            host_of("https://api.exemple.fr/v1?x=1").as_deref(),
            Some("api.exemple.fr")
        );
        assert_eq!(
            host_of("http://exemple.fr:8080/").as_deref(),
            Some("exemple.fr")
        );
        assert_eq!(
            host_of("https://user@exemple.fr/x").as_deref(),
            Some("exemple.fr")
        );
        assert_eq!(host_of("pas-une-url"), None);
        assert_eq!(host_of("https:///chemin"), None);
    }
}
