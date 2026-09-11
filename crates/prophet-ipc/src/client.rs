//! Client JSON-RPC sur socket Unix.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader, WriteHalf};
use tokio::net::UnixStream;
use tokio::sync::Mutex;

use crate::codec::{Error, ErrorCode, Request, Response};

/// Client d'un daemon. Réutilise une connexion unique, sérialisée par un verrou.
#[derive(Debug)]
pub struct Client {
    inner: Mutex<Connection>,
    next_id: AtomicU64,
    auth: Option<String>,
}

#[derive(Debug)]
struct Connection {
    reader: BufReader<tokio::io::ReadHalf<UnixStream>>,
    writer: WriteHalf<UnixStream>,
}

impl Client {
    /// Se connecte à un daemon.
    ///
    /// # Erreurs
    /// Si la connexion échoue.
    pub async fn connect(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let stream = UnixStream::connect(path).await?;
        let (read_half, writer) = tokio::io::split(stream);
        Ok(Self {
            inner: Mutex::new(Connection {
                reader: BufReader::new(read_half),
                writer,
            }),
            next_id: AtomicU64::new(1),
            auth: None,
        })
    }

    /// Attache un jeton de capacité à toutes les requêtes suivantes.
    #[must_use]
    pub fn with_auth(mut self, token: impl Into<String>) -> Self {
        self.auth = Some(token.into());
        self
    }

    /// Appelle une méthode et attend la réponse.
    ///
    /// # Erreurs
    /// Erreur de transport, ou erreur JSON-RPC renvoyée par le serveur.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, Error> {
        let mut params = params;
        if let Some(token) = &self.auth
            && let Some(object) = params.as_object_mut()
        {
            object.insert("_auth".to_owned(), json!(token));
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = Request::new(id, method, params);
        let mut bytes = serde_json::to_vec(&request)
            .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))?;
        bytes.push(b'\n');

        let mut guard = self.inner.lock().await;
        guard
            .writer
            .write_all(&bytes)
            .await
            .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))?;
        guard
            .writer
            .flush()
            .await
            .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))?;

        let mut line = String::new();
        let read = guard
            .reader
            .read_line(&mut line)
            .await
            .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))?;
        if read == 0 {
            return Err(Error::new(ErrorCode::InternalError, "connexion fermée"));
        }
        let response: Response = serde_json::from_str(&line)
            .map_err(|e| Error::new(ErrorCode::ParseError, e.to_string()))?;
        if let Some(error) = response.error {
            return Err(error);
        }
        Ok(response.result.unwrap_or(Value::Null))
    }
}
