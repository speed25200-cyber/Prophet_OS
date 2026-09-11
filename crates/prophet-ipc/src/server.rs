//! Serveur JSON-RPC sur socket Unix.

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::codec::{Error, ErrorCode, Request, Response};
use crate::{MAX_MESSAGE_BYTES, extract_auth};

/// Identité du pair, attestée par le noyau via `SO_PEERCRED`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerIdentity {
    /// Identifiant d'utilisateur.
    pub uid: u32,
    /// Identifiant de groupe principal.
    pub gid: u32,
    /// Identifiant de processus, si le noyau le fournit.
    pub pid: Option<i32>,
}

/// Traitement d'une méthode.
///
/// L'implémentation reçoit l'identité du pair et le jeton de capacité extrait de `params._auth`
/// (déjà retiré des paramètres), et décide elle-même de la politique à appliquer.
pub trait Handler: Send + Sync + 'static {
    /// Répond à un appel.
    fn call(
        &self,
        peer: PeerIdentity,
        auth: Option<String>,
        method: String,
        params: Value,
    ) -> impl std::future::Future<Output = Result<Value, Error>> + Send;
}

/// Serveur écoutant sur un socket Unix.
#[derive(Debug)]
pub struct Server {
    listener: UnixListener,
    path: PathBuf,
}

impl Server {
    /// Ouvre un socket, en retirant un fichier résiduel et en posant le mode `0660`.
    ///
    /// # Erreurs
    /// Si le socket ne peut pas être créé.
    pub fn bind(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660))?;
        Ok(Self { listener, path })
    }

    /// Chemin du socket.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Boucle d'acceptation. Ne rend la main qu'en cas d'erreur fatale d'`accept`.
    ///
    /// # Erreurs
    /// Si `accept` échoue de façon non récupérable.
    pub async fn serve<H: Handler>(self, handler: Arc<H>) -> std::io::Result<()> {
        loop {
            let (stream, _) = self.listener.accept().await?;
            let handler = Arc::clone(&handler);
            tokio::spawn(async move {
                if let Err(error) = handle_connection(stream, handler).await {
                    tracing::debug!(%error, "connexion terminée");
                }
            });
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

async fn handle_connection<H: Handler>(stream: UnixStream, handler: Arc<H>) -> std::io::Result<()> {
    let creds = stream.peer_cred()?;
    let peer = PeerIdentity {
        uid: creds.uid(),
        gid: creds.gid(),
        pid: creds.pid(),
    };
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let response = if line.len() > MAX_MESSAGE_BYTES {
            Response::err(
                Value::Null,
                Error::new(ErrorCode::InvalidRequest, "message trop grand"),
            )
        } else {
            dispatch(&line, peer, Arc::clone(&handler)).await
        };
        let mut bytes = serde_json::to_vec(&response).unwrap_or_else(|_| {
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"reponse non serialisable"}}"#.as_bytes().to_vec()
        });
        bytes.push(b'\n');
        write_half.write_all(&bytes).await?;
        write_half.flush().await?;
    }
    Ok(())
}

async fn dispatch<H: Handler>(line: &str, peer: PeerIdentity, handler: Arc<H>) -> Response {
    let request: Request = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(error) => {
            return Response::err(
                Value::Null,
                Error::new(ErrorCode::ParseError, error.to_string()),
            );
        }
    };
    if request.jsonrpc != crate::codec::JSONRPC_VERSION {
        return Response::err(
            request.id,
            Error::new(ErrorCode::InvalidRequest, "version jsonrpc inattendue"),
        );
    }
    let mut params = request.params;
    let auth = extract_auth(&mut params);
    match handler.call(peer, auth, request.method, params).await {
        Ok(result) => Response::ok(request.id, result),
        Err(error) => Response::err(request.id, error),
    }
}
