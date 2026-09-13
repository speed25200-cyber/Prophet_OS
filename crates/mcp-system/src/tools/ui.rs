//! Pilotage des applications de bureau par leur arbre sémantique.
//!
//! Les applications vivent dans la session de l'humain ; l'adaptateur d'accessibilité
//! (`prophet-supd`) y lit leurs arbres AT-SPI et y exécute des actions typées. Ces outils en
//! sont les clients, sous capd : lire l'arbre d'une application exige `ui.read` sur son nom,
//! y agir exige `ui.act`. Aucune capture d'écran, aucun clic en coordonnées : seulement ce que
//! l'application déclare elle-même, avec la confiance que mérite une lecture d'accessibilité.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use sup::session::{
    ActRequest, AppView, METHOD_ACT, METHOD_APPS, METHOD_TREE, Observation, Outcome, TreeRequest,
};
use sup::tree::Detail;

use crate::protocol::{CallResult, ErrorCode, ToolMeta, ToolSpec};
use crate::registry::{Tool, ToolContext};

/// La session de l'humain, jointe par le socket de l'adaptateur.
#[derive(Debug)]
pub struct Desktop {
    socket: PathBuf,
    runtime: tokio::runtime::Runtime,
}

impl Desktop {
    /// Les outils d'interface, tous adossés à ce socket.
    ///
    /// # Panics
    /// Si aucun exécuteur asynchrone ne peut être créé, ce qui n'arrive pas sur une machine
    /// capable de lancer le service.
    #[must_use]
    pub fn at(socket: PathBuf) -> Arc<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("exécuteur pour l'adaptateur d'accessibilité");
        Arc::new(Self { socket, runtime })
    }

    /// Les trois outils, prêts à être enregistrés.
    #[must_use]
    pub fn tools(self: &Arc<Self>) -> Vec<Arc<dyn Tool>> {
        vec![
            Arc::new(Apps(self.clone())),
            Arc::new(Observe(self.clone())),
            Arc::new(Act(self.clone())),
        ]
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, CallResult> {
        self.runtime.block_on(async {
            let client = prophet_ipc::Client::connect(&self.socket)
                .await
                .map_err(|e| {
                    CallResult::error(
                        ErrorCode::SandboxError,
                        format!(
                            "l'adaptateur d'accessibilité de la session ne répond pas ({}) : {e}",
                            self.socket.display()
                        ),
                    )
                })?;
            client.call(method, params).await.map_err(|e| {
                let code = match e.code {
                    prophet_ipc::ErrorCode::NotFound => ErrorCode::NotFound,
                    prophet_ipc::ErrorCode::InvalidParams => ErrorCode::Invalid,
                    prophet_ipc::ErrorCode::Unauthorized => ErrorCode::PolicyDenied,
                    _ => ErrorCode::SandboxError,
                };
                CallResult::error(code, e.message)
            })
        })
    }
}

fn app_of(args: &Value) -> Option<String> {
    args.get("app")
        .and_then(Value::as_str)
        .map(|a| a.trim().to_lowercase())
        .filter(|a| !a.is_empty())
}

fn detail_of(args: &Value) -> Result<Detail, CallResult> {
    match args.get("detail").and_then(Value::as_str) {
        None | Some("normal") => Ok(Detail::Normal),
        Some("summary") => Ok(Detail::Summary),
        Some("full") => Ok(Detail::Full),
        Some(other) => Err(CallResult::error(
            ErrorCode::Invalid,
            format!("detail inconnu : {other} (summary, normal, full)"),
        )),
    }
}

/// Rend une observation au niveau de détail demandé, avec sa confiance et ses réserves.
fn observation(obs: Observation, detail: Detail) -> Value {
    let root = obs.tree.root.at_detail(detail);
    let mut tree = serde_json::to_value(&obs.tree).unwrap_or(Value::Null);
    if let Some(objet) = tree.as_object_mut() {
        objet.insert(
            "root".into(),
            serde_json::to_value(root).unwrap_or(Value::Null),
        );
    }
    let mut value = json!({
        "app": obs.tree.app,
        "window": obs.tree.window,
        "title": obs.tree.title,
        "nodes": obs.tree.root.count(),
        "provenance": obs.provenance,
        "confidence": obs.confidence,
        "caveat": obs.caveat,
        "tree": tree,
    });
    if obs.truncated > 0 {
        value["truncated"] = json!(obs.truncated);
    }
    value
}

/// `ui.apps` : les applications qui exposent une interface.
#[derive(Debug)]
pub struct Apps(Arc<Desktop>);

impl Tool for Apps {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "ui.apps".into(),
            description: "Liste les applications de bureau ouvertes qui exposent une interface pilotable, avec leur nombre de fenêtres. Les titres et le contenu ne se lisent qu'avec ui.tree, application par application.".into(),
            input_schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
            meta: Some(ToolMeta {
                requires: "tool.call".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, _args: &Value, _context: &ToolContext) -> Option<String> {
        Some("ui.apps".into())
    }

    fn call(&self, _args: &Value, _context: &ToolContext) -> CallResult {
        match self.0.call(METHOD_APPS, json!({})) {
            Ok(value) => {
                let apps: Vec<AppView> = serde_json::from_value(value).unwrap_or_default();
                CallResult::structured(json!({"apps": apps}))
            }
            Err(e) => e,
        }
    }
}

/// `ui.tree` : l'arbre sémantique d'une fenêtre d'application.
#[derive(Debug)]
pub struct Observe(Arc<Desktop>);

impl Tool for Observe {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "ui.tree".into(),
            description: "Rend l'arbre sémantique de la fenêtre active d'une application de bureau (ou d'une fenêtre désignée), au niveau de détail demandé : rôles, noms, valeurs, éléments actionnables et actions typées disponibles. L'arbre vient de l'accessibilité de l'application : vérifiez l'effet de chaque action en relisant.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "app": {"type": "string", "description": "Identifiant rendu par ui.apps"},
                    "window": {"type": "string", "description": "Identifiant ou titre d'une fenêtre ; la fenêtre active sinon"},
                    "detail": {"type": "string", "enum": ["summary", "normal", "full"], "default": "normal"}
                },
                "required": ["app"],
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "ui.read".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, args: &Value, _context: &ToolContext) -> Option<String> {
        app_of(args)
    }

    fn call(&self, args: &Value, _context: &ToolContext) -> CallResult {
        let Some(app) = app_of(args) else {
            return CallResult::error(ErrorCode::Invalid, "app est requis");
        };
        let detail = match detail_of(args) {
            Ok(d) => d,
            Err(e) => return e,
        };
        let request = TreeRequest {
            app,
            window: args
                .get("window")
                .and_then(Value::as_str)
                .map(str::to_owned),
        };
        match self.0.call(
            METHOD_TREE,
            serde_json::to_value(request).unwrap_or(Value::Null),
        ) {
            Ok(value) => match serde_json::from_value::<Observation>(value) {
                Ok(obs) => CallResult::structured(observation(obs, detail)),
                Err(e) => CallResult::error(ErrorCode::SandboxError, e.to_string()),
            },
            Err(e) => e,
        }
    }
}

/// `ui.act` : une action typée sur un élément, avec l'arbre résultant.
#[derive(Debug)]
pub struct Act(Arc<Desktop>);

impl Tool for Act {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "ui.act".into(),
            description: "Agit dans une application de bureau : click active un élément (bouton, entrée de menu), set_field renseigne un champ modifiable (value), toggle bascule une case. Les éléments sont désignés par leur identifiant dans l'arbre de ui.tree. Rend ce qui s'est passé et le nouvel arbre de la fenêtre active. Aucune capture d'écran, aucun clic en coordonnées : seules les actions que l'application déclare sont possibles.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "app": {"type": "string"},
                    "window": {"type": "string"},
                    "action": {"type": "string", "enum": ["click", "set_field", "toggle"]},
                    "node": {"type": "string"},
                    "value": {"type": "string"},
                    "detail": {"type": "string", "enum": ["summary", "normal", "full"], "default": "normal"}
                },
                "required": ["app", "action", "node"],
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "ui.act".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, args: &Value, _context: &ToolContext) -> Option<String> {
        app_of(args)
    }

    fn call(&self, args: &Value, _context: &ToolContext) -> CallResult {
        let Some(app) = app_of(args) else {
            return CallResult::error(ErrorCode::Invalid, "app est requis");
        };
        let detail = match detail_of(args) {
            Ok(d) => d,
            Err(e) => return e,
        };
        let (Some(action), Some(node)) = (
            args.get("action").and_then(Value::as_str),
            args.get("node").and_then(Value::as_str),
        ) else {
            return CallResult::error(ErrorCode::Invalid, "action et node sont requis");
        };
        let request = ActRequest {
            app,
            window: args
                .get("window")
                .and_then(Value::as_str)
                .map(str::to_owned),
            action: action.to_owned(),
            node: node.to_owned(),
            value: args.get("value").and_then(Value::as_str).map(str::to_owned),
        };
        match self.0.call(
            METHOD_ACT,
            serde_json::to_value(request).unwrap_or(Value::Null),
        ) {
            Ok(value) => match serde_json::from_value::<Outcome>(value) {
                Ok(outcome) => {
                    let mut rendu = json!({"ok": true, "message": outcome.message});
                    if let Some(obs) = outcome.observation {
                        rendu["observation"] = observation(obs, detail);
                    }
                    CallResult::structured(rendu)
                }
                Err(e) => CallResult::error(ErrorCode::SandboxError, e.to_string()),
            },
            Err(e) => e,
        }
    }
}
