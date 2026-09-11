//! Horloge de tâche.

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::protocol::{CallResult, ToolMeta, ToolSpec};
use crate::registry::{Tool, ToolContext};

/// Heure courante, telle que la tâche doit la voir.
///
/// Passer par un outil plutôt que par l'horloge du système rend une tâche rejouable : au rejeu,
/// l'horloge renvoie ce qu'elle avait renvoyé, et la tâche se déroule à l'identique.
#[derive(Debug)]
pub struct Now;

impl Tool for Now {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "clock.now".into(),
            description: "Donne la date et l'heure courantes en UTC, au format RFC 3339.".into(),
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

    fn call(&self, _args: &Value, _context: &ToolContext) -> CallResult {
        let now = OffsetDateTime::now_utc();
        CallResult::structured(json!({
            "utc": now.format(&Rfc3339).unwrap_or_default(),
            "unix": now.unix_timestamp()
        }))
    }
}
