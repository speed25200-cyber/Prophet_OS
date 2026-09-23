//! Mission locale exécutée sur son thread ; le service reste disponible pendant l'inférence.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use capd::CheckRequest;
use mcp_system::native::RegistryExecutor;
use mcp_system::protocol::{CallResult, ToolSpec};
use mcp_system::registry::{Authority, Journal, Registry, ToolContext};
use mcp_system::services::Services;
use prophet_types::cap::{Act, Decision, DenyReason, Res, Token};
use prophet_types::driver::{DriverEvent, Limits, RunStatus, SandboxRequest, StartRequest};
use prophet_types::ledger::{Actor, Draft, EventKind};
use prophet_types::manifest::Privacy;
use providers::jev::egress::EgressTransport;
use providers::jev::operator::{Cascade, Goal, Operator, Trace};
use providers::local::{AsyncLocalModel, Condensation};
use providers::native::{ModelClient, ModelTurn, NativeDriver, Usage};
use providers::{Driver, DriverError};
use serde_json::{Value, json};
use time::OffsetDateTime;

use crate::{State, Task, TaskPlan};

/// Publie l'état et le résultat sous le verrou de persistance du service.
pub type Publish = Arc<dyn Fn(Task, Option<Value>) -> Result<(), String> + Send + Sync>;

/// Une sous-mission telle que l'agent la demande : un objectif, un contexte du catalogue, et
/// au choix un autre modèle (ADR 0029).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Delegation {
    /// Objectif confié.
    pub intent: String,
    /// Contexte (profil) du catalogue qui encadre la sous-mission.
    pub profile: String,
    /// Modèle local demandé ; celui du parent sinon.
    #[serde(default)]
    pub model: Option<String>,
    /// Rôle demandé (`reflect`, `execute`, `code`) : le service choisit le modèle que le
    /// contexte visé admet pour ce rôle, parmi ceux que le moteur sert (ADR 0034). Un modèle
    /// nommé explicitement l'emporte.
    #[serde(default)]
    pub role: Option<String>,
}

/// Ce que le service fait d'une délégation : créer la sous-mission sous un jeton délégué par
/// capd, la lancer, l'attendre, et rendre son résultat au parent. Fourni par le daemon ; sans
/// lui, l'outil `task.delegate` n'existe pas.
pub type Delegate = Arc<
    dyn Fn(&str, &Token, Delegation) -> Result<Value, (mcp_system::protocol::ErrorCode, String)>
        + Send
        + Sync,
>;

/// Le décideur rapide, configuré par l'administrateur du service et jamais par un modèle :
/// le nom du secret dans le coffre (jamais sa valeur) et le modèle Jev demandé.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct JevSetup {
    /// Nom du secret que le proxy fait résoudre par le coffre.
    pub secret: String,
    /// Modèle Jev (`jev-latest` par défaut).
    pub model: String,
}

impl JevSetup {
    /// Lit la configuration du service : `PROPHET_JEV_SECRET` active Jev, `PROPHET_JEV_MODEL`
    /// choisit le modèle. Sans secret, Jev n'existe pas pour ce service.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let secret = std::env::var("PROPHET_JEV_SECRET")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        Some(Self {
            secret,
            model: std::env::var("PROPHET_JEV_MODEL")
                .ok()
                .filter(|m| !m.trim().is_empty())
                .unwrap_or_else(|| providers::jev::DEFAULT_MODEL.to_owned()),
        })
    }

    /// Vrai si ce jeton autorise une sortie vers l'hôte de Jev. Sans ce grant, la première
    /// décision serait refusée par le proxy ; autant ne pas la demander.
    #[must_use]
    pub fn permitted_by(token: &Token) -> bool {
        token.grants.iter().any(|g| {
            g.res == Res::Net
                && g.act == Act::Egress
                && prophet_types::pattern::matches(
                    prophet_types::pattern::Family::Domain,
                    &g.pattern,
                    providers::jev::HOST,
                    "",
                )
        })
    }
}

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
    /// Socket de l'adaptateur d'accessibilité de la session humaine, si le service en connaît
    /// un ; sinon aucun outil `ui.*`.
    pub sup_socket: Option<PathBuf>,
    /// Comment déléguer une sous-mission ; sans lui, aucun outil `task.delegate`.
    pub delegate: Option<Delegate>,
    /// Socket de sandboxd, par lequel `proc.exec` exécute une commande confinée.
    pub sandboxd: PathBuf,
    /// Consigne de système du relais de modèles, si la mission y participe (ADR 0034) ; le
    /// service la compose à partir du rôle de la mission et des contextes qu'elle peut confier.
    pub briefing: Option<String>,
    /// Décideur rapide du service, s'il est configuré.
    pub jev: Option<JevSetup>,
    /// Confidentialité du manifeste : une mission `local-only` n'envoie rien à Jev.
    pub privacy: Privacy,
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
    fn charge(&self, usage: Usage, model: &str) -> Result<(), String> {
        let mut task = self
            .task
            .lock()
            .map_err(|_| "état de mission indisponible")?;
        task.budget.spent.steps = task.budget.spent.steps.saturating_add(1);
        task.charge_model(model, usage.tokens_in, usage.tokens_out);
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
    /// Impute à cette mission ce qu'une sous-mission a consommé, budget global et compte par
    /// modèle, d'après le résultat qu'elle a rendu ; sans compteurs, rien n'est imputé.
    fn absorb(&self, result: &Value) {
        let spent: Option<crate::Spent> =
            serde_json::from_value(result["budget"]["spent"].clone()).ok();
        let usage: Option<crate::UsageByModel> =
            serde_json::from_value(result["usage"].clone()).ok();
        if spent.is_none() && usage.is_none() {
            return;
        }
        let Ok(mut task) = self.task.lock() else {
            return;
        };
        if let Some(spent) = spent {
            let child = crate::Budget {
                limits: task.budget.limits,
                spent,
            };
            task.budget.absorb(&child);
        }
        if let Some(usage) = usage {
            task.absorb_usage(&usage);
        }
        let updated = task.clone();
        drop(task);
        if let Err(error) = (self.publish)(updated, None) {
            tracing::warn!(%error, "consommation de la sous-mission non persistée");
        }
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
    fn request_approval(
        &self,
        token: &Token,
        request: &CheckRequest,
        summary: &str,
        now: OffsetDateTime,
    ) -> Option<capd::Approval> {
        if self.check_live().is_err() {
            return None;
        }
        self.services.request_approval(token, request, summary, now)
    }

    fn approval_status(&self, id: &str) -> Option<capd::Approval> {
        self.services.approval_status(id)
    }

    fn explain_approval(&self, id: &str, reason: &str) -> Option<capd::Approval> {
        self.services.explain_approval(id, reason)
    }

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
        let result=self.runtime.block_on(async {
            tokio::select! {
                result=self.client.next_turn(history)=>result,
                error=async {loop {if let Err(error)=self.control.check_live(){break error;} tokio::time::sleep(Duration::from_millis(20)).await;}}=>Err(DriverError::Io(error)),
            }
        })?;
        match result.turn {
            Ok(turn) => Ok((turn, result.usage)),
            Err(error) => {
                // Une génération incomplète a consommé des tokens : ils sont imputés ici,
                // puisque le compteur commun ne voit pas les tours invalides.
                self.control
                    .charge(result.usage, &self.name)
                    .map_err(DriverError::BudgetExceeded)?;
                Err(error)
            }
        }
    }
}

/// Le compteur commun : quel que soit le décideur — modèle génératif ou Jev —, chaque tour
/// valide est compté en étapes et en tokens avant que son action ne soit exécutée, et le
/// plafond d'étapes est vérifié avant de demander quoi que ce soit. Le compte par modèle
/// (ADR 0034) impute le tour à celui qui l'a décidé : Jev s'il a répondu sans rendre la main,
/// le modèle génératif sinon.
struct Metered {
    inner: Box<dyn ModelClient>,
    control: Arc<Control>,
    /// Nom du modèle génératif, tel que le plan l'a choisi.
    name: String,
    /// Trace et nom du décideur rapide, s'il tient la première main.
    decider: Option<(Arc<Mutex<Trace>>, String)>,
}
impl Metered {
    /// Décisions demandées à Jev et mains rendues, pour savoir qui a produit le tour suivant.
    fn marks(&self) -> Option<(u32, usize)> {
        let (trace, _) = self.decider.as_ref()?;
        let trace = trace.lock().ok()?;
        Some((trace.decisions, trace.handovers.len()))
    }
}
impl ModelClient for Metered {
    fn model_name(&self) -> String {
        self.inner.model_name()
    }
    fn next_turn(&mut self, history: &[Value]) -> Result<(ModelTurn, Usage), DriverError> {
        self.control.check_live().map_err(DriverError::Io)?;
        let task = self.control.current();
        if task.budget.spent.steps >= task.budget.limits.steps {
            return Err(DriverError::BudgetExceeded(
                "plafond d'étapes atteint".into(),
            ));
        }
        let before = self.marks();
        let (turn, usage) = self.inner.next_turn(history)?;
        let by_decider = match (before, self.marks()) {
            (Some((asked, handed)), Some((asked_now, handed_now))) => {
                asked_now > asked && handed_now == handed
            }
            _ => false,
        };
        let model = match &self.decider {
            Some((_, name)) if by_decider => name.as_str(),
            _ => self.name.as_str(),
        };
        self.control
            .charge(usage, model)
            .map_err(DriverError::BudgetExceeded)?;
        Ok((turn, usage))
    }
}

/// `task.status` pour une mission : ce que `mcp-system` rend (tâche, étape, isolation, droits),
/// et le budget — plafonds, consommé, restant. Un agent qui sait ce qu'il lui reste choisit ses
/// étapes au lieu de heurter le plafond ; il ne peut rien y changer.
struct Etat {
    control: Arc<Control>,
}
impl mcp_system::registry::Tool for Etat {
    fn spec(&self) -> ToolSpec {
        let mut spec = mcp_system::tools::TaskStatus.spec();
        spec.description = "Donne l'état de la mission : identifiant, étape, niveau d'isolation, \
            capacités accordées et budget (plafonds, consommé et restant en étapes, tokens et \
            secondes)."
            .into();
        spec
    }

    fn target(&self, _args: &Value, _context: &ToolContext) -> Option<String> {
        None
    }

    fn call(&self, args: &Value, context: &ToolContext) -> CallResult {
        let mut etat = mcp_system::tools::TaskStatus
            .call(args, context)
            .structured
            .unwrap_or_else(|| json!({}));
        let task = self.control.current();
        let limits = task.budget.limits;
        let spent = task.budget.spent;
        let elapsed = self.control.started.elapsed().as_secs();
        etat["budget"] = json!({
            "limits": {
                "steps": limits.steps,
                "tokens": limits.tokens,
                "wall_time_s": limits.wall_time_s,
            },
            "spent": {
                "steps": spent.steps,
                "tokens": spent.tokens,
                "wall_time_s": elapsed,
            },
            "remaining": {
                "steps": limits.steps.saturating_sub(spent.steps),
                "tokens": limits.tokens.saturating_sub(spent.tokens),
                "wall_time_s": limits.wall_time_s.saturating_sub(elapsed),
            },
        });
        CallResult::structured(etat)
    }
}

impl Mission {
    /// Exécute et publie un résultat. À appeler sur un thread dédié, hors de Tokio.
    pub fn run(self, publish: Publish) {
        let control = self.control(publish);
        let result = self.execute(&control);
        self.conclude(&control, result);
    }

    /// Ouvre une séance d'outils pour un client MCP de l'humain : même jeton, même registre,
    /// même travail SFS et même journal qu'une mission native, mais aucun modèle. Le client
    /// appelle les outils un à un ; la mission se conclut quand il se retire.
    ///
    /// À appeler hors de Tokio : le journal et le travail SFS sont ouverts ici. Un échec est
    /// déjà conclu (mission en échec, journal écrit) quand l'erreur est rendue.
    ///
    /// # Errors
    /// Journal injoignable, travail SFS refusé ou budget nul.
    pub fn attach(self, publish: Publish, client: &str) -> Result<Seance, String> {
        let control = self.control(publish);
        let prepared = (|| {
            control.check_live()?;
            if self.task.budget.limits.steps == 0 {
                return Err("budget de mission nul".to_owned());
            }
            control.append(
                EventKind::ProviderStarted,
                json!({"driver":"mcp-client","client":client}),
            )?;
            control.append(EventKind::TaskStarted, json!({"execution":"mcp-client"}))?;
            let workspace = self.open_workspace(&control)?;
            let registry = Arc::new(self.registry(&control));
            let context = ToolContext {
                token: self.token.clone(),
                task: self.task.id.clone(),
                home: self.home.display().to_string(),
                workdir: workspace.workdir().display().to_string(),
                sandbox_level: 0,
                step: 1,
            };
            Ok((workspace, registry, context))
        })();
        match prepared {
            Ok((workspace, registry, context)) => Ok(Seance {
                mission: self,
                control,
                registry,
                context,
                workspace,
                calls: 0,
                client: format!("client:{}", client.trim().to_lowercase()),
            }),
            Err(error) => {
                self.conclude(&control, Err(error.clone()));
                Err(error)
            }
        }
    }

    fn control(&self, publish: Publish) -> Arc<Control> {
        let started = Instant::now();
        let until = started
            .checked_add(Duration::from_secs(self.task.budget.limits.wall_time_s))
            .unwrap_or(started);
        Arc::new(Control {
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
        })
    }

    /// Conclut la mission : état final, journal, résultat publié.
    fn conclude(&self, control: &Arc<Control>, result: Result<Value, String>) {
        let mut task = control.current();
        task.budget.spent.wall_time_s = control.started.elapsed().as_secs();
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
                json!({"stats":task.budget.spent,"by_model":task.usage,"role":task.role,"reason":task.reason}),
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
        data["usage"] = json!(task.usage);
        data["role"] = json!(task.role);
        data["driver"] = json!(task.driver);
        if let Err(error) = (control.publish)(task, Some(data)) {
            tracing::error!(%error,"résultat de mission non persisté");
        }
    }

    /// Capture les périmètres dans le travail SFS, chaque lecture étant tranchée par capd.
    /// Une sous-mission part de l'espace de travail de son parent, s'il est ouvert : elle voit
    /// ce qu'il a déjà fait, et ce qu'elle fera y reviendra à sa fin (ADR 0039).
    fn open_workspace(&self, control: &Arc<Control>) -> Result<sfs::Workspace, String> {
        let parent = self.task.parent.as_deref().and_then(|parent| {
            sfs::Workspace::open(&self.home, parent)
                .ok()
                .filter(|w| w.state() == sfs::WorkspaceState::Open)
                .map(|w| w.workdir())
        });
        sfs::Workspace::begin_authorized_from(
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
            parent.as_deref(),
        )
        .map_err(|e| e.to_string())
    }

    /// Les outils d'une mission, tous sous le contrôle et le journal de `control`.
    fn registry(&self, control: &Arc<Control>) -> Registry {
        let mut registry = Registry::with_authority(control.clone(), control.clone());
        // L'agent lit où il en est : ses droits, son budget restant, ses propres changements.
        // Offerts seulement si le profil accorde `tool.call` sur leur nom, comme les autres.
        registry.register(Arc::new(Etat {
            control: control.clone(),
        }));
        registry.register(Arc::new(mcp_system::tools::TaskDiff));
        registry.register(Arc::new(mcp_system::tools::Read));
        registry.register(Arc::new(mcp_system::tools::Write));
        registry.register(Arc::new(mcp_system::tools::List));
        registry.register(Arc::new(mcp_system::tools::Stat));
        registry.register(Arc::new(mcp_system::tools::Search));
        // Tout format se lit sous le droit `fs.read` : PDF, bureautique, images, médias.
        registry.register(Arc::new(mcp_system::tools::DocRead));
        // Une commande tourne sous sandboxd, dans l'espace de travail, le home en lecture seule
        // selon le jeton (ADR 0031) ; le profil doit accorder `proc.exec` sur son nom.
        registry.register(Arc::new(mcp_system::tools::Exec::via(
            self.sandboxd.clone(),
        )));
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
        // Les applications de la session humaine, par leur arbre d'accessibilité : lire exige
        // `ui.read` sur le nom de l'application, agir `ui.act` ; l'adaptateur tourne dans la
        // session et n'admet que ce service (ADR 0027).
        if let Some(socket) = &self.sup_socket {
            for tool in mcp_system::tools::Desktop::at(socket.clone()).tools() {
                registry.register(tool);
            }
        }
        // Un agent en fait travailler un autre : sous-mission à droits inclus, autre contexte ou
        // autre modèle, résultat rendu ici. capd tranche `task.spawn` sur le contexte visé.
        // Ce que l'enfant a consommé est imputé ici, dans l'état que ce fil publie : le service
        // l'impute aussi dans le sien, mais c'est cette copie qui écrit la suivante.
        if let Some(delegate) = &self.delegate {
            let delegate = delegate.clone();
            let absorbing = control.clone();
            let counted: Delegate = Arc::new(move |task, token, request| {
                let value = delegate(task, token, request)?;
                absorbing.absorb(&value["result"]);
                Ok(value)
            });
            registry.register(Arc::new(crate::delegate::Tool::new(counted)));
        }
        registry
    }

    fn execute(&self, control: &Arc<Control>) -> Result<Value, String> {
        control.check_live()?;
        if self.task.budget.limits.tokens == 0 || self.task.budget.limits.steps == 0 {
            return Err("budget de mission nul".into());
        }
        // Jev n'a la main que si tout est réuni : configuré par le service, un navigateur pour
        // observer des arbres, une confidentialité qui admet un service distant, et un jeton qui
        // autorise la sortie vers son hôte. Il n'obtient aucun droit de plus que le modèle.
        let decider = self.jev.as_ref().filter(|_| {
            self.browser.is_some()
                && self.privacy != Privacy::LocalOnly
                && JevSetup::permitted_by(&self.token)
        });
        control.append(
            EventKind::ProviderStarted,
            json!({
                "driver": self.plan.choice.reference,
                "decider": decider.map(|d| format!("jev:{}", d.model)),
            }),
        )?;
        control.append(EventKind::TaskStarted, json!({"execution":"native-tools"}))?;
        let workspace = self.open_workspace(control)?;
        let registry = self.registry(control);
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
        // La consigne du relais, s'il y en a une, précède l'intention ; les anciens résultats
        // d'outils sont condensés avant chaque envoi : le modèle relit un résumé, pas des
        // kilo-octets déjà vus, et peut relancer l'outil s'il lui faut le détail (ADR 0034).
        let client = AsyncLocalModel::new(
            &self.endpoint,
            model,
            executor.tools(),
            Duration::from_secs(120),
            2048,
        )
        .map_err(|e| e.to_string())?
        .with_system(self.briefing.clone())
        .with_condensation(Condensation::default());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let generative: Box<dyn ModelClient> = Box::new(Model {
            client,
            runtime,
            control: control.clone(),
            name: self.plan.choice.reference.clone(),
        });
        let (model, trace): (Box<dyn ModelClient>, Option<Arc<Mutex<Trace>>>) = match decider {
            Some(setup) => {
                let transport = EgressTransport::new(&self.egress, &self.token, &setup.secret)
                    .map_err(|e| e.to_string())?;
                let operator = Operator::new(
                    Arc::new(transport),
                    Goal::from_intent(&self.task.intent, None),
                    &setup.model,
                );
                let cascade = Cascade::new(operator, generative);
                let trace = cascade.trace();
                (Box::new(cascade), Some(trace))
            }
            None => (generative, None),
        };
        let metered: Box<dyn ModelClient> = Box::new(Metered {
            inner: model,
            control: control.clone(),
            name: self.plan.choice.reference.clone(),
            decider: trace
                .clone()
                .zip(decider.map(|d| format!("jev:{}", d.model))),
        });
        // Ce que l'objectif nomme et que la portée couvre, absent au départ, est un livrable :
        // une conclusion qui ne l'a pas produit est rappelée au modèle (ADR 0049).
        let attendus = crate::livrables::attendus(
            &crate::livrables::nommes(&self.task.intent, &self.home),
            |reel| workspace.to_work_path(reel),
        );
        let rappeles = Arc::new(Mutex::new(Vec::<String>::new()));
        let consigne = {
            let control = control.clone();
            let rappeles = rappeles.clone();
            Box::new(move |manquants: &[String], rang: u8| {
                control.append(
                    EventKind::TaskReminded,
                    json!({"missing": manquants, "nth": rang}),
                )?;
                if let Ok(mut rappeles) = rappeles.lock() {
                    for manquant in manquants {
                        if !rappeles.contains(manquant) {
                            rappeles.push(manquant.clone());
                        }
                    }
                }
                Ok(())
            })
        };
        let mut driver = NativeDriver::new(
            Box::new(crate::livrables::Rappel::new(metered, attendus, consigne)),
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
                    } if matches!(code.as_str(), "PolicyDenied" | "Internal") => {
                        // Une décision humaine demandée (ApprovalRequired) n'interrompt pas :
                        // le modèle l'attend et réessaie (ADR 0041).
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
                        let jev = trace
                            .as_ref()
                            .and_then(|t| t.lock().ok().map(|t| t.clone()));
                        let reminded = rappeles.lock().map(|r| r.clone()).unwrap_or_default();
                        return Ok(json!({
                            "text": text,
                            "tool_calls": calls,
                            "diff": diff,
                            "review": review,
                            "jev": jev,
                            "reminded": reminded,
                        }));
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

/// Les outils d'une mission tenus par le service pour un client MCP de l'humain.
///
/// Rien n'est délégué au client : le jeton reste dans le service, chaque appel passe par le
/// registre (droits, journal, approbations) et le travail SFS reçoit les écritures, que le
/// créateur examine puis publie comme pour une mission native.
pub struct Seance {
    mission: Mission,
    control: Arc<Control>,
    registry: Arc<Registry>,
    context: ToolContext,
    workspace: sfs::Workspace,
    calls: u32,
    /// Nom sous lequel les tours du client sont comptés (`client:<nom>`), sans tokens : le
    /// client officiel de l'humain ne rend pas ses compteurs au service.
    client: String,
}

impl Seance {
    /// Mission servie.
    #[must_use]
    pub fn task(&self) -> &str {
        &self.mission.task.id
    }

    /// Outils que le jeton de la mission rend visibles.
    #[must_use]
    pub fn tools(&self) -> Vec<mcp_system::protocol::ToolSpec> {
        self.registry.visible_for(&self.context.token)
    }

    /// Un appel d'outil, compté comme une étape de la mission.
    ///
    /// # Errors
    /// Mission annulée, durée ou plafond d'étapes atteint ; l'erreur d'un outil est rendue
    /// dans le résultat, pas ici.
    pub fn call(&mut self, name: &str, args: &Value) -> Result<CallResult, String> {
        self.control.check_live()?;
        let task = self.control.current();
        if task.budget.spent.steps >= task.budget.limits.steps {
            return Err("plafond d'étapes atteint".into());
        }
        self.control.charge(
            Usage {
                tokens_in: 0,
                tokens_out: 0,
            },
            &self.client,
        )?;
        self.calls = self.calls.saturating_add(1);
        self.context.step = self.context.step.saturating_add(1);
        Ok(self
            .registry
            .call(name, args, &self.context, OffsetDateTime::now_utc()))
    }

    /// Le client se retire : les versions sont scellées et la mission conclue.
    pub fn finish(self, text: Option<String>) {
        let result = self.control.check_live().and_then(|()| {
            let review = self.workspace.seal_review().map_err(|e| e.to_string())?;
            let diff = review.diff();
            Ok(json!({
                "text": text.unwrap_or_default(),
                "tool_calls": self.calls,
                "diff": diff,
                "review": review,
                "execution": "mcp-client",
            }))
        });
        self.mission.conclude(&self.control, result);
    }

    /// La séance est interrompue par le service : la mission se conclut sur cette raison.
    pub fn abort(self, reason: &str) {
        self.mission.conclude(&self.control, Err(reason.to_owned()));
    }
}
