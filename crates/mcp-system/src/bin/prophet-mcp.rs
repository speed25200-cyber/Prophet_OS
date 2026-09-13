//! Pont MCP sur entrée et sortie standard vers la séance d'outils d'une mission tenue par agentd.
//!
//! C'est la forme qu'attendent les clients d'éditeurs (Claude Code, Codex…) : ils lancent le
//! programme et dialoguent par lignes JSON-RPC. Le pont ne tient aucun jeton et n'exécute
//! aucun outil : `initialize` attache la mission (`task.attach`), `tools/list` et `tools/call`
//! sont relayés (`task.tools`, `task.call`), et la fin de l'entrée retire le client
//! (`task.detach`), ce qui scelle les versions pour l'examen du créateur.
//!
//! Usage : `PROPHET_TASK=<mission> prophet-mcp`. La mission doit avoir été préparée par le même
//! utilisateur (`prophet task new`, ou « Nouvel objectif » de la surface) ; `prophet task
//! mcp-config <mission>` rend la configuration à donner au client. `PROPHET_AGENTD_SOCKET`
//! désigne un autre socket que celui du système.

use std::io::{BufRead as _, Write as _};
use std::path::PathBuf;

use mcp_system::protocol::initialize_result;
use mcp_system::server::MAX_INPUT_BYTES;
use serde_json::{Value, json};

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("prophet-mcp : {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    New,
    AwaitingInitialized,
    Ready,
}

struct Pont {
    socket: PathBuf,
    task: String,
    runtime: tokio::runtime::Runtime,
    phase: Phase,
    attached: bool,
}

impl Pont {
    fn rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        self.runtime.block_on(async {
            let client = prophet_ipc::Client::connect(&self.socket)
                .await
                .map_err(|e| format!("agentd injoignable ({}) : {e}", self.socket.display()))?;
            client.call(method, params).await.map_err(|e| e.message)
        })
    }

    /// Traite un message et rend la réponse, ou `None` pour une notification.
    fn handle(&mut self, line: &str) -> Option<Value> {
        if line.len() > MAX_INPUT_BYTES {
            return Some(rpc_error(Value::Null, -32600, "message MCP trop grand"));
        }
        let request: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(error) => return Some(rpc_error(Value::Null, -32700, &error.to_string())),
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
        let Some(id) = id else {
            if method == "notifications/initialized" && self.phase == Phase::AwaitingInitialized {
                self.phase = Phase::Ready;
            }
            return None;
        };
        let result = match method {
            "initialize" => {
                if self.phase != Phase::New {
                    return Some(rpc_error(id, -32600, "session déjà initialisée"));
                }
                if !params.get("protocolVersion").is_some_and(Value::is_string)
                    || !params.get("capabilities").is_some_and(Value::is_object)
                    || !params["clientInfo"]["name"].is_string()
                    || !params["clientInfo"]["version"].is_string()
                {
                    return Some(rpc_error(id, -32602, "paramètres initialize incomplets"));
                }
                let client = format!(
                    "{} {}",
                    params["clientInfo"]["name"].as_str().unwrap_or_default(),
                    params["clientInfo"]["version"].as_str().unwrap_or_default()
                );
                match self.rpc("task.attach", json!({"id": self.task, "client": client})) {
                    Ok(_) => {
                        self.attached = true;
                        self.phase = Phase::AwaitingInitialized;
                        Ok(initialize_result("prophet"))
                    }
                    Err(raison) => Err(format!("mission {} non attachée : {raison}", self.task)),
                }
            }
            "tools/list" | "tools/call" if self.phase != Phase::Ready => {
                return Some(rpc_error(id, -32002, "session MCP non initialisée"));
            }
            "tools/list" if params.get("cursor").is_some() => {
                return Some(rpc_error(
                    id,
                    -32602,
                    "curseur inconnu : cette liste n'est pas paginée",
                ));
            }
            "tools/list" => self.rpc("task.tools", json!({"id": self.task})),
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
                self.rpc(
                    "task.call",
                    json!({"id": self.task, "name": name, "arguments": args}),
                )
            }
            "ping" => Ok(json!({})),
            other => {
                return Some(rpc_error(
                    id,
                    -32601,
                    &format!("méthode inconnue : {other}"),
                ));
            }
        };
        Some(match result {
            Ok(value) => json!({"jsonrpc": "2.0", "id": id, "result": value}),
            Err(message) => rpc_error(id, -32603, &message),
        })
    }

    /// Le client s'en va : la mission est conclue et ses versions scellées.
    fn detach(&mut self) {
        if !self.attached {
            return;
        }
        self.attached = false;
        if let Err(raison) = self.rpc("task.detach", json!({"id": self.task})) {
            eprintln!(
                "prophet-mcp : retrait de la mission {} : {raison}",
                self.task
            );
        }
    }
}

fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let task = std::env::var("PROPHET_TASK")
        .ok()
        .filter(|t| !t.is_empty())
        .ok_or("variable PROPHET_TASK absente : quelle mission servir ?")?;
    let socket = std::env::var_os("PROPHET_AGENTD_SOCKET")
        .map_or_else(|| prophet_ipc::socket_path("agentd"), PathBuf::from);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut pont = Pont {
        socket,
        task,
        runtime,
        phase: Phase::New,
        attached: false,
    };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("prophet-mcp : entrée illisible : {error}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = pont.handle(&line) {
            writeln!(stdout, "{response}")?;
            stdout.flush()?;
        }
    }
    pont.detach();
    Ok(())
}
