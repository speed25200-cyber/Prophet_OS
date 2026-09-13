//! Mission locale exécutée sur son thread ; le service reste disponible pendant l'inférence.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use capd::CheckRequest;
use mcp_system::native::RegistryExecutor;
use mcp_system::registry::{Authority, Journal, Registry, ToolContext};
use mcp_system::services::Services;
use prophet_types::cap::{Act, Decision, DenyReason, Res, Token};
use prophet_types::driver::{DriverEvent, Limits, RunStatus, SandboxRequest, StartRequest};
use prophet_types::ledger::{Actor, Draft, EventKind};
use providers::local::AsyncLocalModel;
use providers::native::{ModelClient, ModelTurn, NativeDriver, Usage};
use providers::{Driver, DriverError};
use serde_json::{Value, json};
use time::OffsetDateTime;

use crate::{State, Task, TaskPlan};

/// Publie l'état et le résultat sous le verrou de persistance du service.
pub type Publish = Arc<dyn Fn(Task, Option<Value>) -> Result<(), String> + Send + Sync>;

/// Contexte construit exclusivement par agentd à partir de la tâche planifiée.
pub struct Mission {
    /// Tâche réservée en état Running.
    pub task: Task,
    /// Jeton rendu par capd.
    pub token: Token,
    /// Plan conservé, comprenant les périmètres.
    pub plan: TaskPlan,
    /// Home configuré du service.
    pub home: PathBuf,
    /// Moteur local configuré par l'administrateur.
    pub endpoint: String,
    /// Services de droits et de journal.
    pub services: Services,
    /// Socket du proxy de sortie, seule route réseau offerte aux outils.
    pub egress: PathBuf,
    /// Programme du navigateur piloté, si le service en configure un ; sinon aucun outil web.
    pub browser: Option<PathBuf>,
    /// Répertoire privé des profils de navigation, un par tâche.
    pub browser_root: PathBuf,
    /// Signal d'annulation. Le résultat final confirme l'arrêt.
    pub stop: Arc<AtomicBool>,
}

struct Control {
    task: Mutex<Task>,
    stop: Arc<AtomicBool>,
    services: Services,
    started: Instant,
    until: Instant,
    fatal: Mutex<Option<String>>,
    publish: Publish,
    scopes: Vec<PathBuf>,
}

impl Control {
    fn current(&self) -> Task {
        self.task.lock().expect("état de mission").clone()
    }
    fn check_live(&self) -> Result<(), String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("annulation demandée".into());
        }
        if Instant::now() >= self.until {
            return Err("durée maximale de la mission atteinte".into());
        }
        if let Some(error) = self
            .fatal
            .lock()
            .map_err(|_| "état de mission indisponible")?
            .clone()
        {
            return Err(error);
        }
        Ok(())
    }
    fn charge(&self, usage: Usage) -> Result<(), String> {
        let mut task = self
            .task
            .lock()
            .map_err(|_| "état de mission indisponible")?;
        task.budget.spent.steps = task.budget.spent.steps.saturating_add(1);
        task.budget.spent.tokens = task
            .budget
            .spent
            .tokens
            .saturating_add(usage.tokens_in)
            .saturating_add(usage.tokens_out);
        task.budget.spent.wall_time_s = self.started.elapsed().as_secs();
        let updated = task.clone();
        drop(task);
        (self.publish)(updated.clone(), None)?;
        if updated.budget.spent.tokens >= updated.budget.limits.tokens {
            return Err("plafond de tokens atteint avant l'action".into());
        }
        self.check_live()
    }
    fn append(&self, kind: EventKind, payload: Value) -> Result<(), String> {
        let task = self.current();
        self.services.append(
            &Draft::new(
                OffsetDateTime::now_utc(),
                Actor::daemon("agentd"),
                kind,
                payload,
            )
            .task(&task.id)
            .step(task.budget.spent.steps),
        )
    }
}

impl Authority for Control {
    fn check(&self, token: &Token, request: &CheckRequest, now: OffsetDateTime) -> Decision {
        let target = std::path::Path::new(&request.target);
        if self.check_live().is_err()
            || (request.res == Res::Fs
                && (target
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
                    || !self.scopes.iter().any(|scope| target.starts_with(scope))))
        {
            return Decision::deny(DenyReason::PolicyDenied);
        }
        let decision = self.services.check(token, request, now);
        if self.check_live().is_err() {
            Decision::deny(DenyReason::PolicyDenied)
        } else {
            decision
        }
    }
}
impl Journal for Control {
    fn record(&self, draft: Draft) -> Result<(), String> {
        let result = self.services.append(&draft);
        if let Err(error) = &result {
            *self
                .fatal
                .lock()
                .map_err(|_| "état de mission indisponible")? = Some(error.clone());
        }
        result
    }
}

struct Model {
    client: AsyncLocalModel,
    runtime: tokio::runtime::Runtime,
    control: Arc<Control>,
    name: String,
}
impl ModelClient for Model {
    fn model_name(&self) -> String {
        self.name.clone()
    }
    fn next_turn(&mut self, history: &[Value]) -> Result<(ModelTurn, Usage), DriverError> {
        self.control.check_live().map_err(DriverError::Io)?;
        let task = self.control.current();
        if task.budget.spent.steps >= task.budget.limits.steps {
            return Err(DriverError::BudgetExceeded(
                "plafond d'étapes atteint".into(),
            ));
        }
        let result=self.runtime.block_on(async {
            tokio::select! {
                result=self.client.next_turn(history)=>result,
                error=async {loop {if let Err(error)=self.control.check_live(){break error;} tokio::time::sleep(Duration::from_millis(20)).await;}}=>Err(DriverError::Io(error)),
            }
        })?;
        self.control
            .charge(result.usage)
            .map_err(DriverError::BudgetExceeded)?;
        result.turn.map(|turn| (turn, result.usage))
    }
}

impl Mission {
    /// Exécute et publie un résultat. À appeler sur un thread dédié, hors de Tokio.
    pub fn run(self, publish: Publish) {
        let started = Instant::now();
        let until = started
            .checked_add(Duration::from_secs(self.task.budget.limits.wall_time_s))
            .unwrap_or(started);
        let control = Arc::new(Control {
            task: Mutex::new(self.task.clone()),
            stop: self.stop.clone(),
            services: self.services.clone(),
            started,
            until,
            fatal: Mutex::new(None),
            publish,
            scopes: self
                .plan
                .scopes
                .iter()
                .map(|scope| {
                    scope
                        .strip_prefix("~/")
                        .map_or_else(|| self.home.join(scope), |rest| self.home.join(rest))
                })
                .collect(),
        });
        let result = self.execute(&control);
        let mut task = control.current();
        task.budget.spent.wall_time_s = started.elapsed().as_secs();
        let (state, reason, mut data) = match result {
            Ok(data) => (State::Done, None, data),
            Err(error) if self.stop.load(Ordering::Acquire) => {
                (State::Cancelled, Some(error), json!({}))
            }
            Err(error) => (State::Failed, Some(error), json!({})),
        };
        let state = if self.stop.load(Ordering::Acquire) {
            State::Cancelled
        } else {
            state
        };
        let _ = task.transition(state, reason);
        let kind = match state {
            State::Done => EventKind::TaskDone,
            State::Cancelled => EventKind::TaskCancelled,
            _ => EventKind::TaskFailed,
        };
        if let Err(error) = control
            .append(
                kind,
                json!({"stats":task.budget.spent,"reason":task.reason}),
            )
            .and_then(|()| {
                control.append(EventKind::ProviderStopped, json!({"driver":task.driver}))
            })
        {
            task.state = State::Failed;
            task.reason = Some(format!("journal final non confirmé : {error}"));
            task.history.push(State::Failed);
        }
        data["state"] = json!(task.state);
        data["reason"] = json!(task.reason);
        data["budget"] = json!(task.budget);
        if let Err(error) = (control.publish)(task, Some(data)) {
            tracing::error!(%error,"résultat de mission non persisté");
        }
    }

    fn execute(&self, control: &Arc<Control>) -> Result<Value, String> {
        control.check_live()?;
        if self.task.budget.limits.tokens == 0 || self.task.budget.limits.steps == 0 {
            return Err("budget de mission nul".into());
        }
        control.append(
            EventKind::ProviderStarted,
            json!({"driver":self.plan.choice.reference}),
        )?;
        control.append(EventKind::TaskStarted, json!({"execution":"native-tools"}))?;
        let workspace = sfs::Workspace::begin_authorized(
            &self.home,
            &self.task.id,
            &self.plan.scopes,
            OffsetDateTime::now_utc(),
            &|path| {
                control
                    .check(
                        &self.token,
                        &CheckRequest::new(Res::Fs, Act::Read, path.display().to_string())
                            .sandbox_level(0),
                        OffsetDateTime::now_utc(),
                    )
                    .is_allow()
            },
        )
        .map_err(|e| e.to_string())?;
        let mut registry = Registry::with_authority(control.clone(), control.clone());
        registry.register(Arc::new(mcp_system::tools::Read));
        registry.register(Arc::new(mcp_system::tools::Write));
        registry.register(Arc::new(mcp_system::tools::List));
        registry.register(Arc::new(mcp_system::tools::Stat));
        registry.register(Arc::new(mcp_system::tools::Search));
        // La seule sortie réseau : l'outil ne joint que le proxy, qui fait trancher capd sur
        // l'hôte réellement visé et retire le jeton avant que quoi que ce soit ne sorte.
        registry.register(Arc::new(mcp_system::tools::Fetch::via(self.egress.clone())));
        // La navigation n'existe que si l'administrateur a nommé un navigateur : la page est
        // observée par son arbre, jamais par des pixels, et l'hôte ouvert passe par capd.
        if let Some(program) = &self.browser {
            let browsing = mcp_system::tools::Browsing::via_egress(
                program.clone(),
                self.browser_root.clone(),
                self.egress.clone(),
            );
            for tool in browsing.tools() {
                registry.register(tool);
            }
        }
        let executor = RegistryExecutor::new(
            Arc::new(registry),
            ToolContext {
                token: self.token.clone(),
                task: self.task.id.clone(),
                home: self.home.display().to_string(),
                workdir: workspace.workdir().display().to_string(),
                sandbox_level: 0,
                step: 1,
            },
        );
        let model = self
            .plan
            .choice
            .reference
            .strip_prefix("local:")
            .ok_or("pilote non local")?;
        let client = AsyncLocalModel::new(
            &self.endpoint,
            model,
            executor.tools(),
            Duration::from_secs(120),
            2048,
        )
        .map_err(|e| e.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let mut driver = NativeDriver::new(
            Box::new(Model {
                client,
                runtime,
                control: control.clone(),
                name: self.plan.choice.reference.clone(),
            }),
            Box::new(executor),
        );
        let request = StartRequest {
            driver: "prophet-agent".into(),
            task: self.task.id.clone(),
            intent: self.task.intent.clone(),
            workdir: workspace.workdir().display().to_string(),
            mcp_config: String::new(),
            token: String::new(),
            sandbox: SandboxRequest {
                level: 0,
                profile: "native-tools".into(),
            },
            limits: Limits {
                wall_time_s: self.task.budget.limits.wall_time_s,
                max_steps: self.task.budget.limits.steps,
            },
            resume: None,
        };
        let run = driver.start(&request).map_err(|e| e.to_string())?.run;
        let mut text = String::new();
        let mut calls = 0;
        loop {
            control.check_live()?;
            for event in driver.poll(&run).map_err(|e| e.to_string())? {
                match event {
                    DriverEvent::ToolCall { .. } => calls += 1,
                    DriverEvent::ToolResult {
                        tool,
                        ok: false,
                        error: Some(code),
                    } if matches!(
                        code.as_str(),
                        "PolicyDenied" | "ApprovalRequired" | "Internal"
                    ) =>
                    {
                        return Err(format!("{tool} interrompu : {code}"));
                    }
                    DriverEvent::Text { text: fragment, .. } => text = fragment,
                    DriverEvent::Done {
                        status: RunStatus::Ok,
                        ..
                    } => {
                        control.check_live()?;
                        let review = workspace.seal_review().map_err(|e| e.to_string())?;
                        control.check_live()?;
                        let diff = review.diff();
                        return Ok(
                            json!({"text":text,"tool_calls":calls,"diff":diff,"review":review}),
                        );
                    }
                    DriverEvent::Done { reason, .. } => {
                        return Err(reason.unwrap_or_else(|| "pilote interrompu".into()));
                    }
                    _ => {}
                }
            }
        }
    }
}
