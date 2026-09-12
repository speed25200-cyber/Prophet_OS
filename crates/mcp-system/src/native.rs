//! Liaison d'une tâche native au même registre et aux mêmes contrôles que MCP.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use providers::local::LocalTool;
use providers::native::ToolExecutor;
use serde_json::{Value, json};
use time::OffsetDateTime;

use crate::registry::{Registry, ToolContext};

/// Exécuteur lié à une seule tâche. Le lanceur doit créer une instance par tâche et lui
/// fournir le contexte d'agentd, jamais des chemins ou un jeton choisis par le modèle.
pub struct RegistryExecutor {
    registry: Arc<Registry>,
    context: ToolContext,
    calls: AtomicU32,
}

impl RegistryExecutor {
    /// Lie le contrôleur à sa tâche ; les signatures et droits sont contrôlés à chaque appel.
    #[must_use]
    pub fn new(registry: Arc<Registry>, context: ToolContext) -> Self {
        Self {
            registry,
            context,
            calls: AtomicU32::new(0),
        }
    }

    /// Descriptions des outils visibles dans le registre pour ce jeton.
    #[must_use]
    pub fn tools(&self) -> Vec<LocalTool> {
        self.registry
            .visible_for(&self.context.token)
            .into_iter()
            .map(|spec| LocalTool {
                name: spec.name,
                description: spec.description,
                parameters: spec.input_schema,
            })
            .collect()
    }
}

impl ToolExecutor for RegistryExecutor {
    fn call(&self, tool: &str, arguments: &Value) -> (bool, Value) {
        let Ok(index) = self
            .calls
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        else {
            return (
                false,
                json!({"code":"BudgetExceeded", "detail":"compteur d'appels épuisé"}),
            );
        };
        let mut context = self.context.clone();
        let Some(step) = context.step.checked_add(index) else {
            return (
                false,
                json!({"code":"BudgetExceeded", "detail":"compteur d'étapes épuisé"}),
            );
        };
        context.step = step;
        let result = self
            .registry
            .call(tool, arguments, &context, OffsetDateTime::now_utc());
        (
            !result.is_error,
            result
                .structured
                .unwrap_or_else(|| json!({"content":result.content})),
        )
    }
}
