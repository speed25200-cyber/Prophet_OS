//! Le seul transport réel vers Jev : par le proxy de sortie, jamais en direct.
//!
//! La requête part sur le socket d'egress avec deux en-têtes que ce processus ne sait pas
//! remplir lui-même : le jeton de la tâche (`Proxy-Authorization`, retiré par le proxy avant la
//! sortie, après que capd a tranché sur l'hôte) et une référence de secret
//! (`Authorization: Bearer prophet-secret:<nom>`) que seul le proxy fait résoudre par le coffre,
//! au dernier moment. Ni agentd, ni ce pilote, ni le modèle ne voient jamais la clé.
//!
//! C'est aussi ce qui fait que Jev reste optionnel : sans secret enregistré, sans grant
//! `net.egress` sur son hôte, ou sans proxy, la première décision échoue proprement et
//! l'appelant continue sans lui.

use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use prophet_types::cap::Token;

use super::{Decided, HOST, JevError, PATH, Request, Response, Transport};

/// Taille maximale d'une réponse lue sur le socket.
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
/// Délai d'attente du proxy, relais amont compris. Jev répond en moins d'une seconde ; au-delà
/// de ce délai, le pilote préfère rendre la main que bloquer une mission.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// Transport par le proxy de sortie.
pub struct EgressTransport {
    socket: PathBuf,
    token_header: String,
    secret: String,
    timeout: Duration,
}

impl std::fmt::Debug for EgressTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EgressTransport")
            .field("socket", &self.socket)
            .field("secret", &self.secret)
            .finish_non_exhaustive()
    }
}

impl EgressTransport {
    /// Prépare le transport pour une tâche. `secret` est le nom du secret dans le coffre, jamais
    /// sa valeur ; il n'est pas vérifié ici, c'est le proxy qui le fait résoudre.
    ///
    /// # Errors
    /// Nom de secret vide ou jeton non sérialisable.
    pub fn new(socket: impl Into<PathBuf>, token: &Token, secret: &str) -> Result<Self, JevError> {
        let name_ok = !secret.is_empty()
            && secret
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if !name_ok {
            return Err(JevError::Invalid(format!(
                "nom de secret invalide : {secret:?}"
            )));
        }
        let json = serde_json::to_string(token)
            .map_err(|e| JevError::Invalid(format!("jeton non sérialisable : {e}")))?;
        Ok(Self {
            socket: socket.into(),
            token_header: base64::engine::general_purpose::STANDARD.encode(json),
            secret: secret.to_owned(),
            timeout: DEFAULT_TIMEOUT,
        })
    }

    /// Change le délai d'attente.
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// La requête HTTP telle qu'elle est écrite sur le socket du proxy.
    fn raw_request(&self, body: &[u8]) -> Vec<u8> {
        let mut head = format!(
            "POST https://{HOST}{PATH} HTTP/1.1\r\nHost: {HOST}\r\n\
             Proxy-Authorization: Prophet {}\r\n\
             Authorization: Bearer prophet-secret:{}\r\n\
             Content-Type: application/json\r\nAccept: application/json\r\n\
             User-Agent: prophet-agent\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
            self.token_header,
            self.secret,
            body.len()
        )
        .into_bytes();
        head.extend_from_slice(body);
        head
    }
}

impl Transport for EgressTransport {
    fn decide(&self, request: &Request) -> Decided {
        let raw = relay(
            &self.socket,
            &self.raw_request(&request.body()),
            self.timeout,
        )
        .map_err(|e| JevError::Transport(format!("proxy de sortie : {e}")))?;
        let (status, reason, body) = parse_response(&raw)?;
        if reason == "Prophet" {
            // Le proxy a refusé avant de sortir : le corps dit pourquoi, et c'est ce qu'il faut
            // rendre à l'humain, pas un code HTTP.
            let detail: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            return Err(JevError::Refused {
                code: detail["code"].as_str().unwrap_or("Refused").to_owned(),
                detail: detail["detail"].as_str().unwrap_or("").to_owned(),
            });
        }
        match status {
            200..=299 => Response::parse(&body, request),
            401 => Err(JevError::Unauthorized),
            422 => Err(JevError::Rejected(
                String::from_utf8_lossy(&body).chars().take(512).collect(),
            )),
            429 => Err(JevError::RateLimited {
                retry_after_s: retry_after(&raw),
            }),
            529 => Err(JevError::Overloaded),
            other => Err(JevError::Transport(format!("l'API répond HTTP {other}"))),
        }
    }

    fn name(&self) -> String {
        format!("jev:egress:{}", self.secret)
    }
}

fn relay(socket: &Path, request: &[u8], timeout: Duration) -> std::io::Result<Vec<u8>> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(request)?;
    stream.flush()?;
    let mut raw = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
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
        if raw.len() > MAX_RESPONSE_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "réponse trop volumineuse",
            ));
        }
    }
    Ok(raw)
}

fn retry_after(raw: &[u8]) -> Option<u64> {
    let head = String::from_utf8_lossy(raw);
    head.lines()
        .find_map(|l| {
            l.split_once(':')
                .filter(|(n, _)| n.trim().eq_ignore_ascii_case("retry-after"))
        })
        .and_then(|(_, v)| v.trim().parse().ok())
}

/// Sépare statut, motif et corps ; recompose un corps segmenté.
fn parse_response(raw: &[u8]) -> Result<(u16, String, Vec<u8>), JevError> {
    let malformed = |m: &str| JevError::Malformed(m.to_owned());
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| malformed("réponse sans en-têtes complets"))?;
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let mut parts = lines.next().unwrap_or_default().splitn(3, ' ');
    let _version = parts.next();
    let status: u16 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| malformed("ligne de statut illisible"))?;
    let reason = parts.next().unwrap_or("").trim().to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let mut body = raw[split + 4..].to_vec();
    let chunked = headers
        .iter()
        .any(|(n, v)| n == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked"));
    if chunked {
        body = dechunk(&body);
    } else if let Some(length) = headers
        .iter()
        .find(|(n, _)| n == "content-length")
        .and_then(|(_, v)| v.parse::<usize>().ok())
    {
        if body.len() < length {
            return Err(malformed("corps tronqué"));
        }
        body.truncate(length);
    }
    Ok((status, reason, body))
}

fn dechunk(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut cursor = 0;
    while cursor < data.len() {
        let Some(end) = data[cursor..].windows(2).position(|w| w == b"\r\n") else {
            break;
        };
        let size_line = String::from_utf8_lossy(&data[cursor..cursor + end]);
        let Some(size) = size_line
            .split(';')
            .next()
            .and_then(|s| usize::from_str_radix(s.trim(), 16).ok())
        else {
            break;
        };
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
    use crate::jev::{DEFAULT_MODEL, Question};
    use serde_json::json;
    use std::os::unix::net::UnixListener;

    fn jeton() -> Token {
        use prophet_types::cap::{Act, Grant, Res, TokenBuilder};
        TokenBuilder::new("capd", "task:jev", "org.test.jev", "u")
            .grants(vec![Grant::new(Res::Net, Act::Egress, HOST)])
            .ttl_seconds(60)
            .build(
                &ed25519_dalek::SigningKey::from_bytes(&[3u8; 32]),
                time::OffsetDateTime::now_utc(),
                [0u8; 16],
            )
            .unwrap()
    }

    /// Un faux proxy : lit une requête, la garde, répond ce qu'on lui a dit.
    fn faux_proxy(
        reponse: &'static str,
    ) -> (PathBuf, std::thread::JoinHandle<String>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("egress.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut raw = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap();
                raw.extend_from_slice(&buf[..n]);
                if let Some(split) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&raw[..split]).to_string();
                    let length = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if raw.len() >= split + 4 + length {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            stream.write_all(reponse.as_bytes()).unwrap();
            String::from_utf8_lossy(&raw).into_owned()
        });
        (socket, handle, dir)
    }

    fn demande() -> Request {
        Request::new(
            json!({"page":"accueil"}),
            DEFAULT_MODEL,
            [(
                "next",
                Question::choice("Que faire ?", [("click:n1", "ouvrir"), ("done", "fini")])
                    .unwrap(),
            )],
        )
        .unwrap()
    }

    #[test]
    fn la_requete_porte_le_jeton_et_une_reference_jamais_une_valeur() {
        let body = json!({"model":"jev-1.13.0","answers":{"next":{"type":"choice","choice":"click:n1","probabilities":{"click:n1":0.9,"done":0.1},"confidence":0.9}},"usage":{"input_tokens":300,"output_tokens":0}}).to_string();
        let reponse: &'static str = Box::leak(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .into_boxed_str(),
        );
        let (socket, proxy, _dir) = faux_proxy(reponse);
        let transport = EgressTransport::new(&socket, &jeton(), "typesafe").unwrap();
        let decision = transport.decide(&demande()).unwrap();
        assert_eq!(
            decision.answer("next").unwrap().choice(),
            Some(("click:n1", 0.9))
        );
        assert_eq!(decision.usage.input_tokens, 300);

        let recu = proxy.join().unwrap();
        let (head, corps) = recu.split_once("\r\n\r\n").unwrap();
        assert!(
            head.starts_with("POST https://api.typesafe.ai/v1/systemone HTTP/1.1\r\n"),
            "{head}"
        );
        assert!(head.contains("\r\nHost: api.typesafe.ai\r\n"), "{head}");
        assert!(head.contains("\r\nProxy-Authorization: Prophet "), "{head}");
        assert!(
            head.contains("\r\nAuthorization: Bearer prophet-secret:typesafe\r\n"),
            "{head}"
        );
        let envoye: serde_json::Value = serde_json::from_str(corps).unwrap();
        assert_eq!(envoye["model"], "jev-latest");
        assert_eq!(envoye["questions"]["next"]["type"], "choice");
        assert_eq!(transport.name(), "jev:egress:typesafe");
    }

    #[test]
    fn un_refus_du_proxy_est_rendu_avec_son_code() {
        let corps = r#"{"code":"SecretRefused","detail":"secret inconnu : x"}"#;
        let reponse: &'static str = Box::leak(
            format!(
                "HTTP/1.1 403 Prophet\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{corps}",
                corps.len()
            )
            .into_boxed_str(),
        );
        let (socket, proxy, _dir) = faux_proxy(reponse);
        let transport = EgressTransport::new(&socket, &jeton(), "typesafe").unwrap();
        let err = transport.decide(&demande()).unwrap_err();
        assert_eq!(
            err,
            JevError::Refused {
                code: "SecretRefused".into(),
                detail: "secret inconnu : x".into()
            }
        );
        proxy.join().unwrap();
    }

    #[test]
    fn les_codes_de_l_api_deviennent_des_erreurs_nommees() {
        for (statut, attendu) in [
            (401, JevError::Unauthorized),
            (529, JevError::Overloaded),
            (
                429,
                JevError::RateLimited {
                    retry_after_s: Some(7),
                },
            ),
        ] {
            let reponse: &'static str = Box::leak(
                format!(
                    "HTTP/1.1 {statut} Whatever\r\nRetry-After: 7\r\nContent-Length: 0\r\n\r\n"
                )
                .into_boxed_str(),
            );
            let (socket, proxy, _dir) = faux_proxy(reponse);
            let transport = EgressTransport::new(&socket, &jeton(), "typesafe").unwrap();
            assert_eq!(transport.decide(&demande()).unwrap_err(), attendu);
            proxy.join().unwrap();
        }
    }

    #[test]
    fn une_reponse_segmentee_est_recomposee_puis_verifiee() {
        let corps = r#"{"answers":{"next":{"type":"choice","choice":"done","probabilities":{},"confidence":0.5}},"usage":{}}"#;
        let (a, b) = corps.split_at(37);
        let reponse: &'static str = Box::leak(
            format!(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n",
                a.len(),
                b.len()
            )
            .into_boxed_str(),
        );
        let (socket, proxy, _dir) = faux_proxy(reponse);
        let transport = EgressTransport::new(&socket, &jeton(), "typesafe").unwrap();
        let decision = transport.decide(&demande()).unwrap();
        assert_eq!(
            decision.answer("next").unwrap().choice(),
            Some(("done", 0.5))
        );
        proxy.join().unwrap();
    }

    #[test]
    fn sans_proxy_aucune_decision_et_un_nom_de_secret_est_verifie() {
        let transport =
            EgressTransport::new("/nulle/part/egress.sock", &jeton(), "typesafe").unwrap();
        assert!(matches!(
            transport.decide(&demande()),
            Err(JevError::Transport(_))
        ));
        assert!(EgressTransport::new("/x", &jeton(), "").is_err());
        assert!(EgressTransport::new("/x", &jeton(), "a b").is_err());
        assert!(EgressTransport::new("/x", &jeton(), "sk-ant-valeur;").is_err());
    }
}
