//! Appels bornés aux services de confiance, à utiliser depuis un thread de travail.
use std::path::{Path, PathBuf};
use std::time::Duration;

use capd::CheckRequest;
use prophet_types::cap::{Decision, DenyReason, Token};
use prophet_types::ledger::Draft;
use serde_json::{Value, json};
use time::OffsetDateTime;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::registry::{Authority, Journal};

/// Les sockets sont fournis par le service, jamais par un modèle ni par ses arguments.
#[derive(Debug, Clone)]
pub struct Services {
    capd: PathBuf,
    ledger: PathBuf,
}

impl Services {
    /// Configure les deux autorités. Aucun jeton n'est émis ou recopié ici.
    #[must_use]
    pub fn new(capd: PathBuf, ledger: PathBuf) -> Self {
        Self { capd, ledger }
    }

    /// Écrit dans le vrai journal. Une panne est rendue à l'appelant, jamais absorbée.
    pub fn append(&self, draft: &Draft) -> Result<(), String> {
        rpc(
            &self.ledger,
            "ledger.append",
            json!({
                "kind":draft.kind,"task":draft.task,"step":draft.step,
                "actor":draft.actor.0,"payload":draft.payload
            }),
        )?;
        Ok(())
    }
}

impl Authority for Services {
    fn check(&self, token: &Token, request: &CheckRequest, _now: OffsetDateTime) -> Decision {
        rpc(&self.capd,"cap.check",json!({"token":token,"res":request.res,"act":request.act,
            "target":request.target,"sandbox_level":request.sandbox_level,"irreversible":request.irreversible,
            "external":request.external,"context":request.context}))
            .ok().and_then(|v|serde_json::from_value(v).ok())
            .unwrap_or_else(||Decision::deny(DenyReason::PolicyDenied))
    }
}

impl Journal for Services {
    fn record(&self, draft: Draft) -> Result<(), String> {
        self.append(&draft)
    }
}

// Une connexion par appel empêche de réutiliser une réponse après une expiration incertaine.
// Aucun rejeu automatique des écritures : le daemon peut avoir écrit avant une perte de réponse.
fn rpc(path: &Path, method: &str, params: Value) -> Result<Value, String> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err("les services synchrones exigent un thread de travail".into());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut stream = tokio::net::UnixStream::connect(path)
                .await
                .map_err(|e| e.to_string())?;
            let mut bytes = serde_json::to_vec(&prophet_ipc::Request::new(1, method, params))
                .map_err(|e| e.to_string())?;
            if bytes.len() > prophet_ipc::MAX_MESSAGE_BYTES {
                return Err("requête de service trop grande".into());
            }
            bytes.push(b'\n');
            stream.write_all(&bytes).await.map_err(|e| e.to_string())?;
            let mut line = Vec::new();
            BufReader::new(stream.take(prophet_ipc::MAX_MESSAGE_BYTES as u64 + 1))
                .read_until(b'\n', &mut line)
                .await
                .map_err(|e| e.to_string())?;
            if line.len() > prophet_ipc::MAX_MESSAGE_BYTES || !line.ends_with(b"\n") {
                return Err("réponse de service tronquée ou trop grande".into());
            }
            let response: prophet_ipc::Response = serde_json::from_slice(&line)
                .map_err(|_| "réponse de service invalide".to_owned())?;
            if response.jsonrpc != "2.0" || response.id != json!(1) {
                return Err("réponse de service non corrélée".into());
            }
            if let Some(error) = response.error {
                return Err(format!("service : {:?}", error.code));
            }
            response
                .result
                .ok_or_else(|| "résultat du service absent".into())
        })
        .await
        .map_err(|_| "service sans réponse dans le délai".to_owned())?
    })
}
