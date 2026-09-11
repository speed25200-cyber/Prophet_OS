//! Boucle agentique native de Prophet OS.
//!
//! C'est elle qui pilote les modèles locaux, et c'est elle qui offre ce qu'aucun client hébergé ne
//! peut offrir : des points de reprise complets, le fork d'une tâche, et un rejeu exact.
//!
//! Le modèle est vu à travers [`ModelClient`], ce qui rend la boucle testable sans GPU : un client
//! scripté suffit à vérifier le comportement, y compris les cas d'échec.

use std::collections::HashMap;

use prophet_types::driver::{
    AuthMode, DriverCapabilities, DriverEvent, DriverKind, RunStatus, StartRequest, StartResponse,
    Supports, ToolVia,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Driver, DriverError};

/// Ce qu'un modèle rend à la boucle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelTurn {
    /// Le modèle veut appeler un outil.
    ToolCall {
        /// Nom de l'outil.
        tool: String,
        /// Arguments.
        arguments: Value,
    },
    /// Le modèle rend sa réponse finale.
    Final {
        /// Texte destiné à l'humain.
        text: String,
    },
}

/// Consommation d'un tour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Tokens en entrée.
    pub tokens_in: u64,
    /// Tokens en sortie.
    pub tokens_out: u64,
}

/// Accès au modèle.
pub trait ModelClient: Send + Sync {
    /// Produit le tour suivant à partir de l'historique.
    ///
    /// # Errors
    /// Si le modèle est injoignable ou rend une sortie inexploitable.
    fn next_turn(&mut self, history: &[Value]) -> Result<(ModelTurn, Usage), DriverError>;

    /// Nom du modèle, pour le journal.
    fn model_name(&self) -> String;
}

/// Exécution d'un outil, du point de vue de la boucle.
pub trait ToolExecutor: Send + Sync {
    /// Appelle un outil et rend son résultat structuré, ainsi qu'un indicateur de succès.
    fn call(&self, tool: &str, arguments: &Value) -> (bool, Value);
}

/// Point de reprise complet d'une exécution.
///
/// Contient tout ce qu'il faut pour reprendre ailleurs ou plus tard : l'historique, l'étape, la
/// consommation. C'est la différence concrète avec un client hébergé, qui ne rend qu'un
/// identifiant de session opaque.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Identifiant.
    pub id: String,
    /// Étape atteinte.
    pub step: u32,
    /// Historique complet.
    pub history: Vec<Value>,
    /// Consommation cumulée.
    pub tokens_in: u64,
    /// Consommation cumulée.
    pub tokens_out: u64,
}

/// État d'une exécution native.
#[derive(Debug)]
struct Run {
    history: Vec<Value>,
    step: u32,
    max_steps: u32,
    tokens_in: u64,
    tokens_out: u64,
    finished: bool,
    cancelled: bool,
    checkpoints: Vec<Checkpoint>,
}

/// Boucle agentique native.
pub struct NativeDriver {
    model: Box<dyn ModelClient>,
    executor: Box<dyn ToolExecutor>,
    runs: HashMap<String, Run>,
    next_run: u64,
    /// Nombre d'étapes entre deux points de reprise.
    checkpoint_every: u32,
}

impl std::fmt::Debug for NativeDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeDriver")
            .field("model", &self.model.model_name())
            .field("runs", &self.runs.len())
            .finish_non_exhaustive()
    }
}

impl NativeDriver {
    /// Construit la boucle avec un modèle et un exécuteur d'outils.
    #[must_use]
    pub fn new(model: Box<dyn ModelClient>, executor: Box<dyn ToolExecutor>) -> Self {
        Self {
            model,
            executor,
            runs: HashMap::new(),
            next_run: 1,
            checkpoint_every: 5,
        }
    }

    /// Fixe la fréquence des points de reprise.
    #[must_use]
    pub const fn checkpoint_every(mut self, steps: u32) -> Self {
        self.checkpoint_every = steps;
        self
    }

    /// Points de reprise d'une exécution.
    #[must_use]
    pub fn checkpoints(&self, run: &str) -> Vec<Checkpoint> {
        self.runs
            .get(run)
            .map(|r| r.checkpoints.clone())
            .unwrap_or_default()
    }

    /// Reprend une exécution à partir d'un point de reprise, dans une nouvelle exécution.
    ///
    /// C'est la primitive du fork : reprendre deux fois le même point donne deux explorations
    /// indépendantes du même état.
    #[must_use]
    pub fn fork(&mut self, checkpoint: &Checkpoint) -> String {
        let run = format!("run:native-{}", self.next_run);
        self.next_run += 1;
        self.runs.insert(
            run.clone(),
            Run {
                history: checkpoint.history.clone(),
                step: checkpoint.step,
                max_steps: u32::MAX,
                tokens_in: checkpoint.tokens_in,
                tokens_out: checkpoint.tokens_out,
                finished: false,
                cancelled: false,
                checkpoints: Vec::new(),
            },
        );
        run
    }
}

impl Driver for NativeDriver {
    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            driver: "prophet-agent".into(),
            kind: DriverKind::Native,
            auth: AuthMode::None,
            supports: Supports {
                resume: true,
                checkpoint: true,
                fork: true,
                token_usage: true,
                quota_estimate: false,
                cost: false,
                streaming_events: true,
                permission_delegation: true,
            },
            logged_in: true,
            models: vec![self.model.model_name()],
        }
    }

    fn start(&mut self, request: &StartRequest) -> Result<StartResponse, DriverError> {
        let run = format!("run:native-{}", self.next_run);
        self.next_run += 1;
        self.runs.insert(
            run.clone(),
            Run {
                history: vec![json!({"role": "user", "content": request.intent})],
                step: 0,
                max_steps: request.limits.max_steps,
                tokens_in: 0,
                tokens_out: 0,
                finished: false,
                cancelled: false,
                checkpoints: Vec::new(),
            },
        );
        Ok(StartResponse {
            run: run.clone(),
            session_ref: run,
        })
    }

    fn poll(&mut self, run_id: &str) -> Result<Vec<DriverEvent>, DriverError> {
        let session_ref = run_id.to_owned();
        let run = self
            .runs
            .get_mut(run_id)
            .ok_or_else(|| DriverError::UnknownRun(run_id.to_owned()))?;
        if run.cancelled {
            return Ok(vec![DriverEvent::Done {
                status: RunStatus::Cancelled,
                reason: Some("annulée".into()),
                session_ref,
            }]);
        }
        if run.finished {
            return Ok(Vec::new());
        }
        if run.step >= run.max_steps {
            run.finished = true;
            return Ok(vec![DriverEvent::Done {
                status: RunStatus::Failed,
                reason: Some(format!("plafond de {} étapes atteint", run.max_steps)),
                session_ref,
            }]);
        }

        run.step += 1;
        let mut events = vec![DriverEvent::Step {
            n: run.step,
            summary: None,
        }];

        let (turn, usage) = match self.model.next_turn(&run.history) {
            Ok(result) => result,
            Err(error) => {
                run.finished = true;
                events.push(DriverEvent::Done {
                    status: RunStatus::Failed,
                    reason: Some(error.to_string()),
                    session_ref,
                });
                return Ok(events);
            }
        };
        run.tokens_in += usage.tokens_in;
        run.tokens_out += usage.tokens_out;
        events.push(DriverEvent::Usage {
            tokens_in: Some(usage.tokens_in),
            tokens_out: Some(usage.tokens_out),
            cost_eur: None,
            quota_pct: None,
        });

        match turn {
            ModelTurn::ToolCall { tool, arguments } => {
                let digest = format!(
                    "blake3:{}",
                    blake3::hash(arguments.to_string().as_bytes()).to_hex()
                );
                events.push(DriverEvent::ToolCall {
                    tool: tool.clone(),
                    args_digest: digest,
                    via: ToolVia::Mcp,
                });
                let (ok, result) = self.executor.call(&tool, &arguments);
                events.push(DriverEvent::ToolResult {
                    tool: tool.clone(),
                    ok,
                    error: if ok {
                        None
                    } else {
                        result
                            .get("code")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned)
                    },
                });
                run.history.push(json!({
                    "role": "assistant",
                    "tool_call": {"tool": tool, "arguments": arguments}
                }));
                run.history
                    .push(json!({"role": "tool", "ok": ok, "result": result}));
            }
            ModelTurn::Final { text } => {
                run.history
                    .push(json!({"role": "assistant", "content": text.clone()}));
                events.push(DriverEvent::Text {
                    role: "assistant".into(),
                    text,
                });
                run.finished = true;
                events.push(DriverEvent::Done {
                    status: RunStatus::Ok,
                    reason: None,
                    session_ref: session_ref.clone(),
                });
            }
        }

        if run.step % self.checkpoint_every == 0 && !run.finished {
            let checkpoint = Checkpoint {
                id: format!("{session_ref}@{}", run.step),
                step: run.step,
                history: run.history.clone(),
                tokens_in: run.tokens_in,
                tokens_out: run.tokens_out,
            };
            run.checkpoints.push(checkpoint.clone());
            events.push(DriverEvent::Checkpoint {
                checkpoint: checkpoint.id,
            });
        }
        Ok(events)
    }

    fn resolve_permission(
        &mut self,
        run: &str,
        _id: &str,
        _allowed: bool,
    ) -> Result<(), DriverError> {
        // Les permissions de la boucle native sont tranchées par `capd` avant l'appel d'outil :
        // il n'y a donc rien à transmettre ici, mais l'exécution doit exister.
        if self.runs.contains_key(run) {
            Ok(())
        } else {
            Err(DriverError::UnknownRun(run.to_owned()))
        }
    }

    fn cancel(&mut self, run: &str) -> Result<(), DriverError> {
        let state = self
            .runs
            .get_mut(run)
            .ok_or_else(|| DriverError::UnknownRun(run.to_owned()))?;
        state.cancelled = true;
        Ok(())
    }
}

/// Modèle scripté, pour les tests et le rejeu.
#[derive(Debug)]
pub struct ScriptedModel {
    turns: Vec<(ModelTurn, Usage)>,
    position: usize,
    name: String,
}

impl ScriptedModel {
    /// Modèle rejouant la suite de tours donnée.
    #[must_use]
    pub fn new(name: impl Into<String>, turns: Vec<(ModelTurn, Usage)>) -> Self {
        Self {
            turns,
            position: 0,
            name: name.into(),
        }
    }
}

impl ModelClient for ScriptedModel {
    fn next_turn(&mut self, _history: &[Value]) -> Result<(ModelTurn, Usage), DriverError> {
        let turn = self
            .turns
            .get(self.position)
            .cloned()
            .ok_or_else(|| DriverError::BadModelOutput("script épuisé".into()))?;
        self.position += 1;
        Ok(turn)
    }

    fn model_name(&self) -> String {
        self.name.clone()
    }
}
