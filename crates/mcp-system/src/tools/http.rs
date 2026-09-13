//! Sortie réseau, exclusivement par le proxy.

use std::io::{Read as _, Write as _};
use std::path::PathBuf;
use std::time::Duration;

use base64::Engine as _;
use serde_json::{Value, json};

use crate::protocol::{CallResult, ErrorCode, ToolMeta, ToolSpec};
use crate::registry::{Tool, ToolContext};

/// Requête HTTP sortante.
///
/// L'outil ne joint jamais le réseau lui-même : il passe par le socket du proxy, seule interface
/// disponible depuis une sandbox. Un agent qui contournerait cet outil ne trouverait aucune route.
/// Le jeton de la tâche voyage dans l'en-tête interne que le proxy retire avant la sortie ; c'est
/// lui, et non l'outil, qui demande à capd si l'hôte est permis.
#[derive(Debug, Default)]
pub struct Fetch {
    socket: Option<PathBuf>,
}

impl Fetch {
    /// Outil relié au socket du proxy de sortie fourni par le service, jamais par le modèle.
    #[must_use]
    pub const fn via(socket: PathBuf) -> Self {
        Self {
            socket: Some(socket),
        }
    }
}

/// Taille de réponse rendue au modèle par défaut.
const DEFAULT_MAX_BYTES: usize = 256 * 1024;
/// Plafond absolu, quel que soit l'argument.
const HARD_MAX_BYTES: usize = 1024 * 1024;
/// Délai d'attente d'une réponse du proxy, qui comprend le relais amont.
const TIMEOUT: Duration = Duration::from_secs(45);

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
            description: "Effectue une requête HTTP vers un domaine autorisé, par le proxy de sortie. Les méthodes qui modifient un état distant demandent une approbation humaine. Rend le statut, le type de contenu et le corps textuel, tronqué à max_bytes.".into(),
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

    /// Lire (`GET`, `HEAD`) est une lecture réseau automatique, journalisée ; tout ce qui peut
    /// modifier un état distant est irréversible et externe, donc soumis à décision humaine.
    /// Le proxy applique la même distinction de son côté : l'outil ne peut pas l'assouplir.
    fn effects(&self, args: &Value, _meta: &ToolMeta) -> (bool, bool) {
        let method = args
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("GET")
            .to_ascii_uppercase();
        if matches!(method.as_str(), "GET" | "HEAD") {
            (false, false)
        } else {
            (true, true)
        }
    }

    fn call(&self, args: &Value, context: &ToolContext) -> CallResult {
        let Some(url) = args.get("url").and_then(Value::as_str) else {
            return CallResult::error(ErrorCode::Invalid, "argument `url` manquant");
        };
        let Some(host) = host_of(url) else {
            return CallResult::error(ErrorCode::Invalid, format!("adresse illisible : {url}"));
        };
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return CallResult::error(ErrorCode::Invalid, "seuls http:// et https:// sont relayés");
        }
        let method = args
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("GET")
            .to_ascii_uppercase();
        if !method.bytes().all(|b| b.is_ascii_uppercase() || b == b'-')
            || method.is_empty()
            || method.len() > 16
        {
            return CallResult::error(ErrorCode::Invalid, "méthode HTTP illisible");
        }
        let body = args.get("body").and_then(Value::as_str).unwrap_or("");
        let max_bytes = args
            .get("max_bytes")
            .and_then(Value::as_u64)
            .map_or(DEFAULT_MAX_BYTES, |n| {
                usize::try_from(n).unwrap_or(HARD_MAX_BYTES)
            })
            .clamp(1, HARD_MAX_BYTES);
        let Some(socket) = &self.socket else {
            // Le relais effectif est assuré par le daemon `egress` ; sans lui, l'échec est
            // explicite plutôt qu'une sortie directe qui contournerait l'invariant.
            return CallResult::error(
                ErrorCode::SandboxError,
                format!(
                    "proxy de sortie injoignable pour {method} {host} : aucune requête n'a été émise"
                ),
            );
        };
        let token = match serde_json::to_string(&context.token) {
            Ok(json) => base64::engine::general_purpose::STANDARD.encode(json),
            Err(e) => return CallResult::error(ErrorCode::Internal, e.to_string()),
        };
        // L'adresse absolue va au proxy, qui en extrait l'hôte contrôlé et joint ce même hôte.
        let mut request = format!(
            "{method} {url} HTTP/1.1\r\nHost: {host}\r\nProxy-Authorization: Prophet {token}\r\n\
             User-Agent: prophet-agent\r\nAccept: text/html, application/json, text/plain;q=0.9, */*;q=0.5\r\n\
             Connection: close\r\n"
        );
        if !body.is_empty() || matches!(method.as_str(), "POST" | "PUT" | "PATCH") {
            request.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        request.push_str("\r\n");
        request.push_str(body);
        match relay(socket, request.as_bytes(), max_bytes) {
            Ok(response) => response.into_result(max_bytes),
            Err(e) => CallResult::error(
                ErrorCode::SandboxError,
                format!("proxy de sortie injoignable pour {method} {host} : {e}"),
            ),
        }
    }
}

/// Réponse lue sur le socket du proxy, avant interprétation.
struct Response {
    status: u16,
    reason: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    truncated: bool,
}

impl Response {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Distingue une réponse du proxy lui-même — un refus, jamais du contenu — d'une réponse
    /// relayée, rendue telle quelle avec son statut.
    fn into_result(self, max_bytes: usize) -> CallResult {
        if self.reason == "Prophet" {
            let detail: Value = serde_json::from_slice(&self.body).unwrap_or(Value::Null);
            let code = detail["code"].as_str().unwrap_or("Refused");
            let explanation = detail["detail"].as_str().unwrap_or("");
            let message = format!("sortie refusée par le proxy ({code}) : {explanation}");
            return CallResult::error(
                match self.status {
                    403 | 407 => ErrorCode::PolicyDenied,
                    400 | 413 => ErrorCode::Invalid,
                    502 => ErrorCode::NotFound,
                    _ => ErrorCode::SandboxError,
                },
                message,
            );
        }
        let content_type = self.header("content-type").unwrap_or("").to_owned();
        let mut body = self.body;
        let mut truncated = self.truncated;
        if body.len() > max_bytes {
            body.truncate(max_bytes);
            truncated = true;
        }
        let text = String::from_utf8_lossy(&body).into_owned();
        CallResult::structured(json!({
            "status": self.status,
            "content_type": content_type,
            "body": text,
            "bytes": body.len(),
            "truncated": truncated,
        }))
    }
}

/// Envoie la requête au proxy et lit sa réponse, bornée.
///
/// Le proxy relaie l'amont octet pour octet ; un corps segmenté (`chunked`) est recomposé ici,
/// et la lecture s'arrête au plafond plutôt que de laisser un serveur remplir la mémoire.
fn relay(socket: &std::path::Path, request: &[u8], max_bytes: usize) -> std::io::Result<Response> {
    let mut stream = std::os::unix::net::UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    stream.write_all(request)?;
    stream.flush()?;
    let limit = max_bytes.saturating_add(64 * 1024);
    let mut raw = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    let mut truncated = false;
    loop {
        let n = match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                if raw.is_empty() {
                    return Err(e);
                }
                break;
            }
        };
        raw.extend_from_slice(&chunk[..n]);
        if raw.len() >= limit {
            truncated = true;
            break;
        }
    }
    parse_response(&raw, truncated)
}

fn parse_response(raw: &[u8], truncated: bool) -> std::io::Result<Response> {
    let invalid = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, m.to_owned());
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| invalid("réponse sans en-têtes complets"))?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let mut parts = status_line.splitn(3, ' ');
    let _version = parts.next();
    let status: u16 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| invalid("ligne de statut illisible"))?;
    let reason = parts.next().unwrap_or("").trim().to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(n, v)| (n.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let mut body = raw[split + 4..].to_vec();
    let chunked = headers.iter().any(|(n, v)| {
        n.eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked")
    });
    if chunked && !truncated {
        body = dechunk(&body);
    } else if let Some(length) = headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
    {
        body.truncate(length);
    }
    Ok(Response {
        status,
        reason,
        headers,
        body,
        truncated,
    })
}

/// Recompose un corps `Transfer-Encoding: chunked`. Un cadrage illisible rend ce qui a pu être lu.
fn dechunk(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut cursor = 0;
    while cursor < data.len() {
        let Some(end) = data[cursor..].windows(2).position(|w| w == b"\r\n") else {
            break;
        };
        let size_line = String::from_utf8_lossy(&data[cursor..cursor + end]);
        let size = size_line
            .split(';')
            .next()
            .and_then(|s| usize::from_str_radix(s.trim(), 16).ok());
        let Some(size) = size else { break };
        cursor += end + 2;
        if size == 0 {
            break;
        }
        let stop = cursor.saturating_add(size).min(data.len());
        out.extend_from_slice(&data[cursor..stop]);
        cursor = stop + 2;
    }
    out
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

    #[test]
    fn une_reponse_segmentee_est_recomposee() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: text/plain\r\n\r\n4\r\nProp\r\n3\r\nhet\r\n0\r\n\r\n";
        let response = parse_response(raw, false).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"Prophet");
        assert_eq!(response.header("content-type"), Some("text/plain"));
    }

    #[test]
    fn un_refus_du_proxy_devient_une_erreur_nommee() {
        let raw = b"HTTP/1.1 403 Prophet\r\nContent-Type: application/json\r\nContent-Length: 45\r\n\r\n{\"code\":\"NoGrant\",\"detail\":\"hors des droits\"}";
        let response = parse_response(raw, false).unwrap();
        let result = response.into_result(1024);
        assert!(result.is_error, "{result:?}");
        let structured = result.structured.unwrap();
        assert_eq!(structured["code"], "PolicyDenied");
        assert!(
            structured["detail"].as_str().unwrap().contains("NoGrant"),
            "{structured}"
        );
    }

    #[test]
    fn sans_proxy_rien_n_est_emis() {
        let tool = Fetch::default();
        let context = crate::registry::ToolContext {
            token: prophet_types::cap::TokenBuilder::new("capd", "task:x", "agent.x", "u")
                .grants(vec![prophet_types::cap::Grant::new(
                    prophet_types::cap::Res::Net,
                    prophet_types::cap::Act::Egress,
                    "exemple.fr",
                )])
                .ttl_seconds(60)
                .build(
                    &ed25519_dalek::SigningKey::from_bytes(&[1u8; 32]),
                    time::OffsetDateTime::now_utc(),
                    [0u8; 16],
                )
                .unwrap(),
            task: "task:x".into(),
            home: "/tmp".into(),
            workdir: "/tmp".into(),
            sandbox_level: 0,
            step: 1,
        };
        let result = tool.call(&json!({"url":"http://exemple.fr/"}), &context);
        assert!(result.is_error);
        assert_eq!(result.structured.unwrap()["code"], "SandboxError");
    }
}
