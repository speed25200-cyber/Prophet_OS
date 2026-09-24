//! Introspection de la tâche courante.

use serde_json::{Value, json};

use crate::protocol::{CallResult, ErrorCode, ToolMeta, ToolSpec};
use crate::registry::{Tool, ToolContext};

/// État de la tâche courante.
#[derive(Debug)]
pub struct Status;

impl Tool for Status {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "task.status".into(),
            description: "Donne l'état de la tâche courante : identifiant, agent, étape, niveau d'isolation, capacités accordées et fin de validité du jeton.".into(),
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
        None
    }

    fn call(&self, _args: &Value, context: &ToolContext) -> CallResult {
        // Les capacités sont rendues telles qu'accordées : un agent qui sait ce qu'il peut faire
        // perd moins de tours à tenter ce qui sera refusé.
        let grants: Vec<Value> = context
            .token
            .grants
            .iter()
            .map(|g| {
                json!({
                    "res": format!("{:?}", g.res).to_lowercase(),
                    "act": format!("{:?}", g.act).to_lowercase(),
                    "match": g.pattern
                })
            })
            .collect();
        // Quand les droits cessent : une mission longue finit ou se résume avant, au lieu de
        // découvrir l'expiration à son prochain refus.
        let restant = (context.token.exp - time::OffsetDateTime::now_utc())
            .whole_seconds()
            .max(0);
        CallResult::structured(json!({
            "task": context.task,
            "agent": context.token.agent,
            "step": context.step,
            "sandbox_level": context.sandbox_level,
            "workdir": context.workdir,
            "grants": grants,
            "token_expires_at": context
                .token
                .exp
                .format(&time::format_description::well_known::Rfc3339)
                .ok(),
            "token_expires_in_s": restant,
        }))
    }
}

/// Changements accumulés par la tâche.
#[derive(Debug)]
pub struct Diff;

/// Changements montrés au plus par `task.diff` ; les comptes restent entiers.
const DIFF_LIGNES_MAX: usize = 200;

impl Tool for Diff {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "task.diff".into(),
            description: "Liste les fichiers que la tâche a ajoutés, modifiés ou supprimés depuis son début, sans rien appliquer.".into(),
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
        None
    }

    fn call(&self, _args: &Value, context: &ToolContext) -> CallResult {
        match sfs::Workspace::open(std::path::Path::new(&context.home), &context.task) {
            Ok(workspace) => match workspace.diff() {
                Ok(diff) => {
                    let (added, modified, deleted) = diff.counts();
                    CallResult::structured(json!({
                        "added": added,
                        "modified": modified,
                        "deleted": deleted,
                        "bytes_written": diff.bytes_written(),
                        "rendering": diff.render_limited(DIFF_LIGNES_MAX),
                        "truncated": added + modified + deleted > DIFF_LIGNES_MAX
                    }))
                }
                Err(error) => CallResult::error(ErrorCode::Internal, error.to_string()),
            },
            Err(_) => CallResult::error(
                ErrorCode::NotFound,
                "aucun espace de travail ouvert pour cette tâche",
            ),
        }
    }
}
