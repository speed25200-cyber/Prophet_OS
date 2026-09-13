//! Le relais entre un navigateur piloté et le proxy de sortie.
//!
//! Chromium ne sait parler qu'à un mandataire TCP ; egress n'écoute que sur un socket Unix.
//! Ce relais écoute sur l'adresse de bouclage, reçoit chaque requête et chaque tunnel du
//! navigateur, y pose le jeton de la tâche dans l'en-tête interne que le proxy retire, et
//! remet le tout au socket d'egress. Il ne décide de rien : c'est egress, et capd derrière
//! lui, qui tranchent sur l'hôte. Sans egress joignable, le navigateur n'a aucune route.

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Taille maximale d'une tête de requête acceptée du navigateur.
const HEAD_MAX: usize = 64 * 1024;
/// Délai d'inactivité au-delà duquel une connexion est abandonnée.
const IDLE: Duration = Duration::from_secs(120);

/// Le relais d'une session de navigation : un port local, un jeton, un socket d'egress.
pub(crate) struct Relay {
    port: u16,
}

impl Relay {
    /// Écoute sur un port libre de bouclage et relaie vers `egress` avec le jeton partagé.
    pub(crate) fn start(
        egress: PathBuf,
        token: Arc<Mutex<Option<String>>>,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        std::thread::Builder::new()
            .name(format!("relais-navigateur-{port}"))
            .spawn(move || {
                for client in listener.incoming() {
                    let Ok(client) = client else { break };
                    let egress = egress.clone();
                    let token = token.clone();
                    let _ = std::thread::Builder::new()
                        .name("relais-connexion".into())
                        .spawn(move || serve(client, &egress, &token));
                }
            })?;
        Ok(Self { port })
    }

    /// Adresse à donner au navigateur comme mandataire.
    pub(crate) fn address(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }
}

/// Une connexion du navigateur : lire la tête, y poser le jeton, remettre à egress, copier.
fn serve(mut client: TcpStream, egress: &std::path::Path, token: &Arc<Mutex<Option<String>>>) {
    let _ = client.set_read_timeout(Some(IDLE));
    let _ = client.set_write_timeout(Some(IDLE));
    let Some((head, rest)) = read_head(&mut client) else {
        return;
    };
    let Some(token) = token.lock().ok().and_then(|t| t.clone()) else {
        // Aucune tâche n'a encore ouvert de page : rien ne part sans jeton.
        let _ = client.write_all(refusal(407, "aucun jeton de tâche").as_bytes());
        return;
    };
    let Ok(mut upstream) = UnixStream::connect(egress) else {
        // Le proxy est la seule route ; s'il manque, le navigateur reçoit une erreur, pas
        // une sortie directe.
        let _ = client.write_all(refusal(503, "proxy de sortie injoignable").as_bytes());
        return;
    };
    let _ = upstream.set_read_timeout(Some(IDLE));
    let _ = upstream.set_write_timeout(Some(IDLE));
    let rewritten = rewrite_head(&head, &token);
    if upstream.write_all(rewritten.as_bytes()).is_err() || upstream.write_all(&rest).is_err() {
        return;
    }
    let Ok(mut upstream_reader) = upstream.try_clone() else {
        return;
    };
    let Ok(mut client_writer) = client.try_clone() else {
        return;
    };
    // Deux sens, deux fils : le navigateur peut continuer d'écrire (corps, tunnel) pendant
    // que la réponse revient.
    let back = std::thread::spawn(move || {
        let _ = std::io::copy(&mut upstream_reader, &mut client_writer);
        let _ = client_writer.shutdown(std::net::Shutdown::Write);
    });
    let _ = std::io::copy(&mut client, &mut upstream);
    let _ = upstream.shutdown(std::net::Shutdown::Write);
    let _ = back.join();
}

/// Lit la tête HTTP (jusqu'à la ligne vide) et rend ce qui a été lu au-delà.
fn read_head(client: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = client.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..n]);
        if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buffer[..end + 4]).into_owned();
            let rest = buffer[end + 4..].to_vec();
            return Some((head, rest));
        }
        if buffer.len() > HEAD_MAX {
            return None;
        }
    }
}

/// Pose le jeton de la tâche, retire tout jeton que le navigateur aurait pu porter et
/// demande une connexion par requête : egress sert une requête par connexion.
fn rewrite_head(head: &str, token: &str) -> String {
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut out = format!("{request_line}\r\n");
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("proxy-authorization:")
            || lower.starts_with("proxy-connection:")
            || lower.starts_with("connection:")
        {
            continue;
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    out.push_str(&format!("Proxy-Authorization: Prophet {token}\r\n"));
    out.push_str("Connection: close\r\n\r\n");
    out
}

fn refusal(status: u16, detail: &str) -> String {
    let body = format!("{{\"code\":\"Relay\",\"detail\":\"{detail}\"}}");
    format!(
        "HTTP/1.1 {status} Prophet\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_tete_reecrite_porte_le_jeton_et_rien_d_autre_du_navigateur() {
        let head = "GET http://exemple.fr/ HTTP/1.1\r\nHost: exemple.fr\r\nProxy-Authorization: Basic abc\r\nProxy-Connection: keep-alive\r\nAccept: */*\r\n\r\n";
        let out = rewrite_head(head, "JETON");
        assert!(out.starts_with("GET http://exemple.fr/ HTTP/1.1\r\n"));
        assert!(out.contains("Host: exemple.fr\r\n"));
        assert!(out.contains("Accept: */*\r\n"));
        assert!(out.contains("Proxy-Authorization: Prophet JETON\r\n"));
        assert!(!out.contains("Basic abc"));
        assert!(!out.contains("keep-alive"));
        assert!(out.ends_with("Connection: close\r\n\r\n"));
    }

    #[test]
    fn sans_egress_le_navigateur_recoit_un_refus_et_rien_ne_sort() {
        let token = Arc::new(Mutex::new(Some("JETON".to_owned())));
        let relay = Relay::start(PathBuf::from("/nulle/part/egress.sock"), token).unwrap();
        let mut client = TcpStream::connect(relay.address()).unwrap();
        client
            .write_all(b"GET http://exemple.fr/ HTTP/1.1\r\nHost: exemple.fr\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 503 Prophet"), "{response}");
    }

    #[test]
    fn sans_jeton_le_relais_refuse_avant_de_joindre_egress() {
        let token = Arc::new(Mutex::new(None));
        let relay = Relay::start(PathBuf::from("/nulle/part/egress.sock"), token).unwrap();
        let mut client = TcpStream::connect(relay.address()).unwrap();
        client
            .write_all(b"CONNECT exemple.fr:443 HTTP/1.1\r\nHost: exemple.fr:443\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 407 Prophet"), "{response}");
    }
}
