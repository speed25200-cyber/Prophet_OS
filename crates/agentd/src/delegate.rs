//! `task.delegate` : un agent en fait travailler un autre.
//!
//! La sous-mission reçoit un contexte du catalogue et, au choix, un autre modèle ; ses droits
//! sont un sous-ensemble de ceux du parent, délégués par capd, et son budget est prélevé sur le
//! sien. Le parent attend et reçoit le résultat comme celui d'un outil : c'est ainsi que des
//! modèles différents avancent ensemble sur une tâche, sans qu'aucun ne puisse donner à l'autre
//! ce qu'il n'a pas lui-même (ADR 0029).

use mcp_system::protocol::{CallResult, ErrorCode, ToolMeta, ToolSpec};
use mcp_system::registry::{Tool as SystemTool, ToolContext};
use serde_json::{Value, json};

use crate::local::{Delegate, Delegation};

/// L'outil, adossé à la délégation que le service fournit.
pub struct Tool {
    delegate: Delegate,
}

impl Tool {
    /// Un outil qui délègue par ce chemin.
    #[must_use]
    pub fn new(delegate: Delegate) -> Self {
        Self { delegate }
    }
}

impl std::fmt::Debug for Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("task.delegate").finish_non_exhaustive()
    }
}

impl SystemTool for Tool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "task.delegate".into(),
            description: "Confie un objectif précis à une sous-mission menée par un autre agent, dans un contexte du catalogue (profile) et, au choix, pour un rôle (role : reflect, execute, code ou review, le service choisit alors le modèle que ce contexte admet pour ce rôle) ou avec un modèle local nommé (model). La sous-mission n'a jamais plus de droits que vous, travaille dans son propre espace, et son résultat vous est rendu ici quand elle a fini. Formulez l'objectif complet : elle ne voit pas votre conversation.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "intent": {"type": "string", "description": "Objectif complet et autonome de la sous-mission"},
                    "profile": {"type": "string", "description": "Contexte du catalogue, parmi ceux que votre mission peut confier"},
                    "role": {"type": "string", "enum": ["reflect", "execute", "code", "review"], "description": "Rôle voulu : le modèle le moins coûteux que le contexte admet pour ce rôle est choisi ; review fait juger un travail rendu par un autre regard, sans le refaire"},
                    "model": {"type": "string", "description": "Modèle local demandé explicitement ; sinon celui du rôle, sinon le vôtre"}
                },
                "required": ["intent", "profile"],
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "task.spawn".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, args: &Value, _context: &ToolContext) -> Option<String> {
        args.get("profile")
            .and_then(Value::as_str)
            .map(|p| p.trim().to_lowercase())
            .filter(|p| !p.is_empty())
    }

    fn call(&self, args: &Value, context: &ToolContext) -> CallResult {
        let request: Delegation = match serde_json::from_value(args.clone()) {
            Ok(r) => r,
            Err(e) => return CallResult::error(ErrorCode::Invalid, e.to_string()),
        };
        if request.intent.trim().is_empty() {
            return CallResult::error(ErrorCode::Invalid, "intent est requis");
        }
        match (self.delegate)(&context.task, &context.token, request) {
            Ok(value) => CallResult::structured(value),
            Err((code, message)) => CallResult::error(code, message),
        }
    }
}
