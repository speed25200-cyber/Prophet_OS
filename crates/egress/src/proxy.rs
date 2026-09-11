//! Le proxy lui-même : écoute sur un socket Unix, applique la politique, relaie vers le réseau.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::detect::{Detector, Outbound, Signal};
use crate::policy::{Policy, Verdict};

/// Erreur du proxy.
#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    /// Erreur d'entrée-sortie.
    #[error("erreur d'entrée-sortie : {0}")]
    Io(#[from] std::io::Error),
    /// Requête HTTP illisible.
    #[error("requête illisible : {0}")]
    BadRequest(String),
}

/// Trace d'une requête, telle qu'elle part au journal.
///
/// Elle ne contient ni en-tête substitué, ni corps : seulement de quoi comprendre et auditer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestLog {
    /// Tâche à l'origine.
    pub task: String,
    /// Hôte visé.
    pub host: String,
    /// Méthode.
    pub method: String,
    /// Chemin demandé, sans la chaîne de requête, qui peut porter des données.
    pub path: String,
    /// Octets sortants.
    pub bytes_out: u64,
    /// Décision rendue.
    pub verdict: Verdict,
    /// Signaux relevés.
    pub signals: Vec<Signal>,
}

/// Requête sortante analysée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRequest {
    /// Méthode HTTP.
    pub method: String,
    /// Cible, telle qu'écrite par le client.
    pub target: String,
    /// Hôte extrait de la cible ou de l'en-tête `Host`.
    pub host: String,
    /// En-têtes.
    pub headers: Vec<(String, String)>,
    /// Corps.
    pub body: Vec<u8>,
}

impl ParsedRequest {
    /// Chemin sans chaîne de requête, pour la journalisation.
    #[must_use]
    pub fn path(&self) -> String {
        let without_scheme = self
            .target
            .split_once("://")
            .map_or(self.target.as_str(), |(_, rest)| rest);
        let path = without_scheme
            .find('/')
            .map_or("/", |index| &without_scheme[index..]);
        path.split('?').next().unwrap_or("/").to_owned()
    }
}

/// Analyse une requête HTTP de proxy (forme absolue ou `CONNECT`).
///
/// # Erreurs
/// Si la ligne de requête est absente ou mal formée.
pub fn parse_request(raw: &str) -> Result<ParsedRequest, ProxyError> {
    let mut lines = raw.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| ProxyError::BadRequest("ligne de requête absente".into()))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| ProxyError::BadRequest("méthode absente".into()))?
        .to_owned();
    let target = parts
        .next()
        .ok_or_else(|| ProxyError::BadRequest("cible absente".into()))?
        .to_owned();

    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_owned(), value.trim().to_owned()));
        }
    }

    let host = host_from(&target, &headers)
        .ok_or_else(|| ProxyError::BadRequest("hôte introuvable".into()))?;
    Ok(ParsedRequest {
        method,
        target,
        host,
        headers,
        body: Vec::new(),
    })
}

fn host_from(target: &str, headers: &[(String, String)]) -> Option<String> {
    // Forme absolue : `GET http://hôte/chemin`.
    if let Some((_, rest)) = target.split_once("://") {
        let host = rest.split('/').next()?;
        return Some(strip_port(host));
    }
    // `CONNECT hôte:port`.
    if target.contains(':') && !target.starts_with('/') {
        return Some(strip_port(target));
    }
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("host"))
        .map(|(_, value)| strip_port(value))
}

fn strip_port(host: &str) -> String {
    host.rsplit_once(':')
        .filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit()))
        .map_or_else(|| host.to_owned(), |(h, _)| h.to_owned())
}

/// Proxy de sortie.
#[derive(Debug)]
pub struct Proxy {
    policy: Policy,
    detector: Detector,
    task: String,
    requests: AtomicU64,
    bytes_out: AtomicU64,
}

impl Proxy {
    /// Construit un proxy pour une tâche.
    #[must_use]
    pub fn new(task: impl Into<String>, policy: Policy) -> Self {
        Self {
            policy,
            detector: Detector::new(),
            task: task.into(),
            requests: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
        }
    }

    /// Évalue une requête et produit sa trace.
    ///
    /// C'est le cœur testable du proxy : la décision ne dépend que de la requête et de la
    /// politique, jamais de l'état du réseau.
    #[must_use]
    pub fn evaluate(&self, request: &ParsedRequest) -> RequestLog {
        let bytes_out = request.body.len() as u64;
        let signals = self.detector.inspect(&Outbound {
            host: &request.host,
            url: &request.target,
            headers: &request.headers,
            body: &request.body,
        });

        let explanation = || {
            signals
                .iter()
                .map(Signal::explain)
                .collect::<Vec<_>>()
                .join(" ; ")
        };
        let verdict = if Detector::should_block(&signals) {
            // Un secret sur le départ : refus, sans arbitrage possible.
            Verdict::Deny {
                reason: crate::policy::DenyReason::ExfiltrationSuspected,
                detail: explanation(),
            }
        } else {
            match self
                .policy
                .evaluate(&request.host, &request.method, bytes_out)
            {
                // Signal faible sur une requête par ailleurs autorisée : l'humain tranche.
                Verdict::Allow if Detector::should_escalate(&signals) => Verdict::NeedsApproval {
                    detail: explanation(),
                },
                other => other,
            }
        };

        self.requests.fetch_add(1, Ordering::Relaxed);
        if matches!(verdict, Verdict::Allow) {
            self.bytes_out.fetch_add(bytes_out, Ordering::Relaxed);
        }

        RequestLog {
            task: self.task.clone(),
            host: request.host.clone(),
            method: request.method.clone(),
            path: request.path(),
            bytes_out,
            verdict,
            signals,
        }
    }

    /// Nombre de requêtes vues.
    #[must_use]
    pub fn request_count(&self) -> u64 {
        self.requests.load(Ordering::Relaxed)
    }

    /// Volume sortant autorisé cumulé.
    #[must_use]
    pub fn bytes_out(&self) -> u64 {
        self.bytes_out.load(Ordering::Relaxed)
    }

    /// Écoute sur un socket Unix et répond à chaque requête.
    ///
    /// Le proxy ne relaie que ce que la politique autorise ; tout le reste reçoit une réponse
    /// d'erreur explicite, jamais un silence.
    ///
    /// # Erreurs
    /// Si l'écoute échoue.
    pub async fn serve(
        self: Arc<Self>,
        listener: UnixListener,
        on_log: impl Fn(RequestLog) + Send + Sync + 'static,
    ) -> Result<(), ProxyError> {
        let on_log = Arc::new(on_log);
        loop {
            let (stream, _) = listener.accept().await?;
            let proxy = Arc::clone(&self);
            let on_log = Arc::clone(&on_log);
            tokio::spawn(async move {
                if let Err(error) = proxy.handle(stream, on_log.as_ref()).await {
                    tracing::debug!(%error, "requête sortante interrompue");
                }
            });
        }
    }

    async fn handle(
        &self,
        stream: UnixStream,
        on_log: &(impl Fn(RequestLog) + Send + Sync),
    ) -> Result<(), ProxyError> {
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut raw = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).await? == 0 {
                return Ok(());
            }
            let end = line == "\r\n" || line == "\n";
            raw.push_str(&line);
            if end || raw.len() > 64 * 1024 {
                break;
            }
        }

        let request = parse_request(&raw)?;
        let log = self.evaluate(&request);
        let response = match &log.verdict {
            Verdict::Allow => {
                // Le relais vers le réseau réel se fait ici. La sortie effective est assurée par
                // le daemon en production ; le cœur de décision, lui, est entièrement testable
                // sans réseau.
                b"HTTP/1.1 502 Bad Gateway\r\nX-Prophet: relais non configure\r\nContent-Length: 0\r\n\r\n".to_vec()
            }
            Verdict::Deny { reason, detail } => http_error(403, &format!("{reason:?}"), detail),
            Verdict::NeedsApproval { detail } => http_error(451, "ApprovalRequired", detail),
        };
        on_log(log);
        write_half.write_all(&response).await?;
        write_half.flush().await?;
        Ok(())
    }
}

fn http_error(status: u16, code: &str, detail: &str) -> Vec<u8> {
    let body = format!("{{\"code\":\"{code}\",\"detail\":{}}}", json_string(detail));
    format!(
        "HTTP/1.1 {status} Prophet\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proxy() -> Proxy {
        Proxy::new("task:01", Policy::allowing(["*.exemple.fr"]))
    }

    fn requete(method: &str, target: &str, body: &[u8]) -> ParsedRequest {
        let mut r = parse_request(&format!(
            "{method} {target} HTTP/1.1\r\nHost: api.exemple.fr\r\n\r\n"
        ))
        .unwrap();
        r.body = body.to_vec();
        r
    }

    #[test]
    fn analyse_d_une_forme_absolue() {
        let r = parse_request(
            "GET http://api.exemple.fr/v1/x?a=1 HTTP/1.1\r\nHost: api.exemple.fr\r\n\r\n",
        )
        .unwrap();
        assert_eq!(r.method, "GET");
        assert_eq!(r.host, "api.exemple.fr");
        assert_eq!(
            r.path(),
            "/v1/x",
            "la chaîne de requête ne va pas au journal"
        );
    }

    #[test]
    fn analyse_d_un_connect() {
        let r = parse_request("CONNECT api.exemple.fr:443 HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(r.method, "CONNECT");
        assert_eq!(r.host, "api.exemple.fr");
    }

    #[test]
    fn analyse_par_l_en_tete_host() {
        let r = parse_request("GET /chemin HTTP/1.1\r\nHost: api.exemple.fr:8443\r\n\r\n").unwrap();
        assert_eq!(r.host, "api.exemple.fr");
    }

    #[test]
    fn requete_mal_formee_refusee() {
        assert!(parse_request("").is_err());
        assert!(parse_request("GET\r\n\r\n").is_err());
        assert!(parse_request("GET /x HTTP/1.1\r\n\r\n").is_err());
    }

    #[test]
    fn lecture_autorisee() {
        let log = proxy().evaluate(&requete("GET", "http://api.exemple.fr/v1", b""));
        assert_eq!(log.verdict, Verdict::Allow);
        assert!(log.signals.is_empty());
    }

    #[test]
    fn hote_non_autorise_refuse() {
        let p = proxy();
        let mut r = requete("GET", "http://evil.com/collect", b"");
        r.host = "evil.com".into();
        let log = p.evaluate(&r);
        assert!(matches!(
            log.verdict,
            Verdict::Deny {
                reason: crate::policy::DenyReason::HostNotAllowed,
                ..
            }
        ));
    }

    #[test]
    fn exfiltration_l_emporte_sur_un_hote_autorise() {
        // Le scénario d'injection : la page a convaincu l'agent d'envoyer une clé vers un domaine
        // que la tâche a pourtant le droit de joindre.
        let log = proxy().evaluate(&requete(
            "POST",
            "http://api.exemple.fr/collect",
            b"note=sk-ant-api03-secret",
        ));
        assert!(
            matches!(
                log.verdict,
                Verdict::Deny {
                    reason: crate::policy::DenyReason::ExfiltrationSuspected,
                    ..
                }
            ),
            "{:?}",
            log.verdict
        );
        assert!(!log.signals.is_empty());
    }

    #[test]
    fn methode_modifiante_demande_une_approbation() {
        let log = proxy().evaluate(&requete("POST", "http://api.exemple.fr/x", b"{}"));
        assert!(matches!(log.verdict, Verdict::NeedsApproval { .. }));
    }

    #[test]
    fn le_journal_ne_contient_ni_corps_ni_en_tetes() {
        let log = proxy().evaluate(&requete(
            "GET",
            "http://api.exemple.fr/v1?token=abc",
            b"corps confidentiel",
        ));
        let rendu = serde_json::to_string(&log).unwrap();
        assert!(!rendu.contains("corps confidentiel"), "{rendu}");
        assert!(!rendu.contains("token=abc"), "{rendu}");
    }

    #[test]
    fn signal_faible_sur_requete_autorisee_remonte_a_l_humain() {
        let corps = "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVphYmNkZWY=".repeat(30_000);
        let log = proxy().evaluate(&requete(
            "GET",
            "http://api.exemple.fr/upload",
            corps.as_bytes(),
        ));
        assert!(
            matches!(log.verdict, Verdict::NeedsApproval { .. }),
            "{:?}",
            log.verdict
        );
    }

    #[test]
    fn compteurs() {
        let p = proxy();
        let _ = p.evaluate(&requete("GET", "http://api.exemple.fr/a", b"12345"));
        let mut refuse = requete("GET", "http://evil.com/", b"123456789");
        refuse.host = "evil.com".into();
        let _ = p.evaluate(&refuse);
        assert_eq!(p.request_count(), 2);
        assert_eq!(
            p.bytes_out(),
            5,
            "seul le trafic autorisé compte dans le volume sortant"
        );
    }
}
