//! Serveur MCP sur entrée et sortie standard.
//!
//! C'est la forme qu'attendent les clients officiels des éditeurs : ils lancent le programme et
//! dialoguent par lignes JSON-RPC. Le même registre sert aussi l'IPC interne, sans traduction.

use std::io::{self, BufRead, Read, Write};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use time::OffsetDateTime;

use crate::protocol::initialize_result;
use crate::registry::{Registry, ToolContext};

/// Taille maximale d'un message MCP entrant, délimiteur compris.
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    New,
    AwaitingInitialized,
    Ready,
}

/// Serveur MCP dialoguant par lignes.
#[derive(Debug)]
pub struct StdioServer {
    name: String,
    registry: Arc<Registry>,
    context: ToolContext,
    phase: Mutex<Phase>,
}

impl StdioServer {
    /// Construit un serveur pour une tâche donnée.
    #[must_use]
    pub fn new(name: impl Into<String>, registry: Arc<Registry>, context: ToolContext) -> Self {
        Self {
            name: name.into(),
            registry,
            context,
            phase: Mutex::new(Phase::New),
        }
    }

    /// Traite un message et rend la réponse, ou `None` pour une notification.
    ///
    /// Séparer le traitement de la boucle d'entrée-sortie rend le protocole entièrement testable
    /// sans processus ni tube.
    #[must_use]
    pub fn handle(&self, line: &str, now: OffsetDateTime) -> Option<Value> {
        if line.len() > MAX_INPUT_BYTES {
            return Some(rpc_error(Value::Null, -32600, "message MCP trop grand"));
        }
        let request: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(error) => {
                return Some(rpc_error(Value::Null, -32700, &error.to_string()));
            }
        };
        let id = request.get("id").cloned();
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        if !request.is_object()
            || request.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || method.is_empty()
            || id
                .as_ref()
                .is_some_and(|value| !value.is_string() && !value.is_i64() && !value.is_u64())
        {
            return Some(rpc_error(Value::Null, -32600, "requête JSON-RPC invalide"));
        }
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        if !params.is_object() {
            return id.map(|id| rpc_error(id, -32602, "params doit être un objet"));
        }
        let Ok(mut phase) = self.phase.lock() else {
            return id.map(|id| rpc_error(id, -32603, "session MCP indisponible"));
        };
        // Une notification n'exécute jamais un outil et n'attend pas de réponse.
        let Some(id) = id else {
            if method == "notifications/initialized" && *phase == Phase::AwaitingInitialized {
                *phase = Phase::Ready;
            }
            return None;
        };

        let result = match method {
            "initialize" => {
                if *phase != Phase::New {
                    return Some(rpc_error(id, -32600, "session déjà initialisée"));
                }
                if !params.get("protocolVersion").is_some_and(Value::is_string)
                    || !params.get("capabilities").is_some_and(Value::is_object)
                    || !params["clientInfo"]["name"].is_string()
                    || !params["clientInfo"]["version"].is_string()
                {
                    return Some(rpc_error(id, -32602, "paramètres initialize incomplets"));
                }
                // Si le client propose une autre version, il décide s'il prend en charge la
                // version annoncée dans la réponse avant d'envoyer initialized.
                *phase = Phase::AwaitingInitialized;
                Ok(initialize_result(&self.name))
            }
            "tools/list" | "tools/call" if *phase != Phase::Ready => {
                return Some(rpc_error(id, -32002, "session MCP non initialisée"));
            }
            "tools/list" if params.get("cursor").is_some() => {
                return Some(rpc_error(
                    id,
                    -32602,
                    "curseur inconnu : cette liste n'est pas paginée",
                ));
            }
            "tools/list" => Ok(json!({
                "tools": self.registry.visible_for(&self.context.token)
            })),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                if name.is_empty() || !args.is_object() {
                    return Some(rpc_error(
                        id,
                        -32602,
                        "nom d'outil et arguments objet attendus",
                    ));
                }
                let result = self.registry.call(name, &args, &self.context, now);
                match serde_json::to_value(result) {
                    Ok(value) => Ok(value),
                    Err(_) => {
                        return Some(rpc_error(id, -32603, "résultat impossible à sérialiser"));
                    }
                }
            }
            "ping" => Ok(json!({})),
            other => Err(format!("méthode inconnue : {other}")),
        };

        Some(match result {
            Ok(value) => json!({"jsonrpc": "2.0", "id": id, "result": value}),
            Err(message) => rpc_error(id, -32601, &message),
        })
    }

    /// Boucle de service sur l'entrée standard.
    ///
    /// # Errors
    /// Si la lecture ou l'écriture échouent.
    pub fn serve_stdio(&self) -> std::io::Result<()> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        self.serve(stdin.lock(), stdout.lock())
    }

    /// Sert des flux délimités par des sauts de ligne, sans charger une entrée non bornée.
    ///
    /// # Errors
    /// Si un flux échoue, si un message dépasse 1 Mio ou si EOF arrive au milieu d'une ligne.
    pub fn serve(&self, mut input: impl BufRead, mut output: impl Write) -> io::Result<()> {
        loop {
            let mut bytes = Vec::new();
            let read = input
                .by_ref()
                .take((MAX_INPUT_BYTES + 1) as u64)
                .read_until(b'\n', &mut bytes)?;
            if read == 0 {
                return Ok(());
            }
            if read > MAX_INPUT_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "message MCP supérieur à 1 Mio",
                ));
            }
            if bytes.last() != Some(&b'\n') {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "message MCP sans fin de ligne",
                ));
            }
            let line = std::str::from_utf8(&bytes)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(response) = self.handle(line, OffsetDateTime::now_utc()) {
                serde_json::to_writer(&mut output, &response)?;
                output.write_all(b"\n")?;
                output.flush()?;
            }
        }
    }
}

fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}
