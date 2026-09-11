//! Serveur MCP sur entrée et sortie standard.
//!
//! C'est la forme qu'attendent les clients officiels des éditeurs : ils lancent le programme et
//! dialoguent par lignes JSON-RPC. Le même registre sert aussi l'IPC interne, sans traduction.

use std::sync::Arc;

use serde_json::{Value, json};
use time::OffsetDateTime;

use crate::protocol::initialize_result;
use crate::registry::{Registry, ToolContext};

/// Serveur MCP dialoguant par lignes.
#[derive(Debug)]
pub struct StdioServer {
    name: String,
    registry: Arc<Registry>,
    context: ToolContext,
}

impl StdioServer {
    /// Construit un serveur pour une tâche donnée.
    #[must_use]
    pub fn new(name: impl Into<String>, registry: Arc<Registry>, context: ToolContext) -> Self {
        Self {
            name: name.into(),
            registry,
            context,
        }
    }

    /// Traite un message et rend la réponse, ou `None` pour une notification.
    ///
    /// Séparer le traitement de la boucle d'entrée-sortie rend le protocole entièrement testable
    /// sans processus ni tube.
    #[must_use]
    pub fn handle(&self, line: &str, now: OffsetDateTime) -> Option<Value> {
        let request: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(error) => {
                return Some(json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": {"code": -32700, "message": error.to_string()}
                }));
            }
        };
        let id = request.get("id").cloned();
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

        // Une notification n'attend pas de réponse.
        let id = id?;

        let result = match method {
            "initialize" => Ok(initialize_result(&self.name)),
            "tools/list" => Ok(json!({
                "tools": self.registry.visible_for(&self.context.token)
            })),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let result = self.registry.call(name, &args, &self.context, now);
                Ok(serde_json::to_value(result).unwrap_or_else(|_| json!({})))
            }
            "ping" => Ok(json!({})),
            other => Err(format!("méthode inconnue : {other}")),
        };

        Some(match result {
            Ok(value) => json!({"jsonrpc": "2.0", "id": id, "result": value}),
            Err(message) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": message}
            }),
        })
    }

    /// Boucle de service sur l'entrée standard.
    ///
    /// # Errors
    /// Si la lecture ou l'écriture échouent.
    pub fn serve_stdio(&self) -> std::io::Result<()> {
        use std::io::{BufRead as _, Write as _};
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout();
        for line in stdin.lock().lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(response) = self.handle(&line, OffsetDateTime::now_utc()) {
                serde_json::to_writer(&mut stdout, &response)?;
                stdout.write_all(b"\n")?;
                stdout.flush()?;
            }
        }
        Ok(())
    }
}
