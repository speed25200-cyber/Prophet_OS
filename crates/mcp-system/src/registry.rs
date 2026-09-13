//! Registre des outils système et point de passage obligé de tout appel.
//!
//! Chaque appel suit la même séquence, sans exception ni raccourci :
//! politique et jeton, puis approbation si la classe d'action l'exige, puis exécution, avec un
//! événement au journal avant et après. Un outil ne peut pas court-circuiter cette séquence : il
//! ne reçoit la main qu'après.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use capd::{Broker, CheckRequest};
use prophet_types::cap::{Act, Decision, DenyReason, Res, Token};
use prophet_types::ledger::{Actor, Draft, EventKind};
use serde_json::{Value, json};
use time::OffsetDateTime;

use crate::protocol::{CallResult, ErrorCode, ToolMeta, ToolSpec};

/// Contexte d'exécution d'un appel d'outil.
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// Jeton de la tâche appelante.
    pub token: Token,
    /// Identifiant de tâche.
    pub task: String,
    /// Répertoire personnel de l'utilisateur.
    pub home: String,
    /// Répertoire de travail de la tâche.
    pub workdir: String,
    /// Niveau de sandbox courant.
    pub sandbox_level: u8,
    /// Étape de la boucle agentique.
    pub step: u32,
}

/// Un outil système.
pub trait Tool: Send + Sync {
    /// Description destinée au modèle.
    fn spec(&self) -> ToolSpec;

    /// Cible concrète de l'appel, déduite des arguments, sur laquelle porte le contrôle d'accès.
    ///
    /// `None` n'est accepté que pour une exigence `tool.call`. Une capacité de ressource exige
    /// une cible concrète non vide ; son absence provoque un refus avant l'exécution.
    fn target(&self, args: &Value, context: &ToolContext) -> Option<String>;

    /// Exécute l'appel. N'est appelé qu'après autorisation.
    fn call(&self, args: &Value, context: &ToolContext) -> CallResult;

    /// Effets de cet appel précis — `(irréversible, externe)` — quand ils dépendent des
    /// arguments : lire une page n'engage pas la même décision qu'y poster. Par défaut, ceux
    /// que la description annonce, qui restent le pire cas.
    fn effects(&self, _args: &Value, meta: &ToolMeta) -> (bool, bool) {
        (meta.irreversible, meta.external)
    }

    /// Exécute en pouvant recontrôler les ressources découvertes pendant l'appel.
    fn call_checked(
        &self,
        args: &Value,
        context: &ToolContext,
        _access: &dyn ResourceAccess,
    ) -> CallResult {
        self.call(args, context)
    }
}

/// Contrôle de ressources supplémentaires, fourni par le registre à l'outil.
pub trait ResourceAccess {
    /// Vérifie un droit sur un chemin logique, avec le jeton et les révocations courants.
    fn permits(&self, act: Act, path: &str) -> bool;
}

/// Autorité de capacités, locale aux tests ou reliée au service capd.
pub trait Authority: Send + Sync {
    /// Vérifie la demande complète ; une panne doit produire un refus.
    fn check(&self, token: &Token, request: &CheckRequest, now: OffsetDateTime) -> Decision;
}

impl Authority for Mutex<Broker> {
    fn check(&self, token: &Token, request: &CheckRequest, now: OffsetDateTime) -> Decision {
        self.lock()
            .ok()
            .and_then(|b| b.check(token, request, now).ok())
            .unwrap_or_else(|| Decision::deny(DenyReason::PolicyDenied))
    }
}

struct FileAccess<'a> {
    registry: &'a Registry,
    context: &'a ToolContext,
    tool: &'a str,
    now: OffsetDateTime,
    started: std::time::Instant,
}

impl ResourceAccess for FileAccess<'_> {
    fn permits(&self, act: Act, path: &str) -> bool {
        let now = self.now.saturating_add(
            time::Duration::try_from(self.started.elapsed()).unwrap_or(time::Duration::MAX),
        );
        // La recherche peut durer : une révocation ou une expiration doit arrêter les accès
        // suivants. Le droit d'appeler l'outil est lui aussi revérifié.
        [
            CheckRequest::new(Res::Tool, Act::Call, self.tool),
            CheckRequest::new(Res::Fs, act, path),
        ]
        .into_iter()
        .all(|r| {
            self.registry
                .authority
                .check(
                    &self.context.token,
                    &r.sandbox_level(self.context.sandbox_level),
                    now,
                )
                .is_allow()
        })
    }
}

/// Journalisation des appels.
pub trait Journal: Send + Sync {
    /// Enregistre un événement ; une erreur interdit de poursuivre les outils de la tâche.
    fn record(&self, draft: Draft) -> Result<(), String>;
}

/// Journal en mémoire, utile aux tests et au mode dégradé.
#[derive(Debug, Default)]
pub struct MemoryJournal {
    events: Mutex<Vec<Draft>>,
}

impl MemoryJournal {
    /// Journal vide.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Événements enregistrés.
    #[must_use]
    pub fn events(&self) -> Vec<Draft> {
        self.events.lock().map(|e| e.clone()).unwrap_or_default()
    }

    /// Types d'événements enregistrés, dans l'ordre.
    #[must_use]
    pub fn kinds(&self) -> Vec<EventKind> {
        self.events().iter().map(|e| e.kind).collect()
    }
}

impl Journal for MemoryJournal {
    fn record(&self, draft: Draft) -> Result<(), String> {
        self.events
            .lock()
            .map_err(|_| "journal verrouillé".to_owned())?
            .push(draft);
        Ok(())
    }
}

/// Registre d'outils.
pub struct Registry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
    authority: Arc<dyn Authority>,
    journal: Arc<dyn Journal>,
    journal_failed: AtomicBool,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry")
            .field("tools", &self.tools.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl Registry {
    /// Registre vide.
    #[must_use]
    pub fn new(broker: Arc<Mutex<Broker>>, journal: Arc<dyn Journal>) -> Self {
        Self::with_authority(broker, journal)
    }

    /// Registre dont les contrôles peuvent être réalisés par le vrai service capd.
    #[must_use]
    pub fn with_authority(authority: Arc<dyn Authority>, journal: Arc<dyn Journal>) -> Self {
        Self {
            tools: BTreeMap::new(),
            authority,
            journal,
            journal_failed: AtomicBool::new(false),
        }
    }

    /// Enregistre un outil.
    ///
    /// # Panics
    /// Si deux outils portent le même nom : c'est une erreur de programmation, pas une condition
    /// d'exécution.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.spec().name;
        assert!(
            self.tools.insert(name.clone(), tool).is_none(),
            "outil enregistré deux fois : {name}"
        );
    }

    /// Outils visibles pour une tâche : ceux que son jeton lui permet d'appeler.
    ///
    /// Un modèle ne voit donc jamais un outil qu'il ne peut pas utiliser, ce qui économise du
    /// contexte et évite des tentatives vouées à l'échec.
    #[must_use]
    pub fn visible_for(&self, token: &Token) -> Vec<ToolSpec> {
        self.tools
            .values()
            .map(|t| t.spec())
            .filter(|spec| {
                token.grants.iter().any(|grant| {
                    grant.res == Res::Tool
                        && grant.act == Act::Call
                        && prophet_types::pattern::matches(
                            prophet_types::pattern::Family::Name,
                            &grant.pattern,
                            &spec.name,
                            "",
                        )
                })
            })
            .collect()
    }

    /// Toutes les descriptions, sans filtrage.
    #[must_use]
    pub fn all(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|t| t.spec()).collect()
    }

    /// Appelle un outil, après contrôle complet.
    pub fn call(
        &self,
        name: &str,
        args: &Value,
        context: &ToolContext,
        now: OffsetDateTime,
    ) -> CallResult {
        if self.journal_failed.load(Ordering::Acquire) {
            return journal_error();
        }
        match self.call_recorded(name, args, context, now) {
            Ok(result) => result,
            Err(_) => {
                self.journal_failed.store(true, Ordering::Release);
                journal_error()
            }
        }
    }

    fn call_recorded(
        &self,
        name: &str,
        args: &Value,
        context: &ToolContext,
        now: OffsetDateTime,
    ) -> Result<CallResult, String> {
        let Some(tool) = self.tools.get(name) else {
            return Ok(CallResult::error(
                ErrorCode::NotFound,
                format!("outil inconnu : {name}"),
            ));
        };
        let spec = tool.spec();
        let Some(meta) = spec.meta.clone() else {
            return Ok(CallResult::error(
                ErrorCode::Internal,
                format!("l'outil {name} ne déclare pas ses exigences"),
            ));
        };

        let args_digest = digest(args);
        // La cible contrôlée — un hôte, un chemin, une fenêtre — entre au journal ; jamais le
        // contenu des arguments. C'est ce qui permet à l'humain de lire où l'agent est allé.
        let target = tool.target(args, context);
        self.journal.record(
            Draft::new(
                now,
                Actor::mcp(name),
                EventKind::ToolCall,
                json!({
                    "tool": name,
                    "target": target,
                    "args_digest": args_digest,
                    "args_size": args.to_string().len(),
                    "requires": meta.requires,
                }),
            )
            .task(&context.task)
            .step(context.step),
        )?;

        let decision = self.authorize(name, &meta, args, context, now);
        if let Decision::Deny { reason, rule } = &decision {
            self.journal.record(
                Draft::new(
                    now,
                    Actor::daemon("capd"),
                    EventKind::PolicyDeny,
                    json!({
                        "res": format!("{:?}", meta.requires),
                        "act": "call",
                        "tool": name,
                        "reason": format!("{reason:?}"),
                        "rule": rule,
                    }),
                )
                .task(&context.task)
                .step(context.step),
            )?;
            let code = if *reason == DenyReason::ApprovalRequired {
                ErrorCode::ApprovalRequired
            } else {
                ErrorCode::PolicyDenied
            };
            let result = CallResult::error(
                code,
                format!(
                    "{name} refusé : {reason:?}{}",
                    rule.as_ref().map(|r| format!(" ({r})")).unwrap_or_default()
                ),
            );
            self.record_result(name, &result, context, now)?;
            return Ok(result);
        }

        let access = FileAccess {
            registry: self,
            context,
            tool: name,
            now,
            started: std::time::Instant::now(),
        };
        let result = tool.call_checked(args, context, &access);
        self.record_result(name, &result, context, now)?;
        Ok(result)
    }

    fn authorize(
        &self,
        name: &str,
        meta: &ToolMeta,
        args: &Value,
        context: &ToolContext,
        now: OffsetDateTime,
    ) -> Decision {
        // Une description incomplète ou un contexte incohérent ne doivent jamais transformer
        // une autorisation impossible à vérifier en permission implicite.
        let Some((res, act)) = parse_requires(&meta.requires) else {
            return Decision::deny(DenyReason::PolicyDenied);
        };
        if context.task != context.token.sub
            || context.sandbox_level > 2
            || meta
                .sandbox_level_min
                .is_some_and(|minimum| context.sandbox_level < minimum)
            || (res == Res::Tool && act != Act::Call)
        {
            return Decision::deny(DenyReason::PolicyDenied);
        }

        let Some(tool) = self.tools.get(name) else {
            return Decision::deny(DenyReason::PolicyDenied);
        };
        let (irreversible, external) = tool.effects(args, meta);

        // Premier contrôle : le droit d'appeler cet outil.
        let mut call_request =
            CheckRequest::new(Res::Tool, Act::Call, name).sandbox_level(context.sandbox_level);
        if irreversible {
            call_request = call_request.irreversible();
        }
        if external {
            call_request = call_request.external();
        }
        let decision = self.authority.check(&context.token, &call_request, now);
        if !decision.is_allow() {
            return decision;
        }

        // Second contrôle : la ressource que l'outil va toucher. Le droit d'appeler `fs.read` ne
        // dit rien sur le fichier visé ; c'est ici que le périmètre est vérifié.
        if res == Res::Tool {
            return decision;
        }
        let Some(target) = tool.target(args, context) else {
            return Decision::deny(DenyReason::PolicyDenied);
        };
        if target.trim().is_empty() {
            return Decision::deny(DenyReason::PolicyDenied);
        }
        let mut request = CheckRequest::new(res, act, target).sandbox_level(context.sandbox_level);
        if irreversible {
            request = request.irreversible();
        }
        if external {
            request = request.external();
        }
        self.authority.check(&context.token, &request, now)
    }

    fn record_result(
        &self,
        name: &str,
        result: &CallResult,
        context: &ToolContext,
        now: OffsetDateTime,
    ) -> Result<(), String> {
        let code = result
            .structured
            .as_ref()
            .and_then(|v| v.get("code"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        self.journal.record(
            Draft::new(
                now,
                Actor::mcp(name),
                EventKind::ToolResult,
                json!({
                    "tool": name,
                    "ok": !result.is_error,
                    "error_code": code,
                    "result_digest": digest(&json!(result.structured)),
                }),
            )
            .task(&context.task)
            .step(context.step),
        )
    }
}

fn journal_error() -> CallResult {
    CallResult::error(
        ErrorCode::Internal,
        "journal non confirmé : l'action peut avoir eu lieu ; tâche suspendue, ne pas réessayer automatiquement",
    )
}

/// Empreinte d'une valeur, pour le journal. Le contenu n'y figure jamais.
#[must_use]
pub fn digest(value: &Value) -> String {
    format!(
        "blake3:{}",
        blake3::hash(value.to_string().as_bytes()).to_hex()
    )
}

/// Traduit `<res>.<act>` en couple typé.
#[must_use]
pub fn parse_requires(requires: &str) -> Option<(Res, Act)> {
    prophet_types::manifest::parse_capability_key(requires).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use prophet_types::cap::{Grant, TokenBuilder};
    use serde_json::json;

    struct Faux;

    impl Tool for Faux {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "test.faux".into(),
                description: "Outil de test.".into(),
                input_schema: json!({"type": "object"}),
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
            CallResult::text("exécuté")
        }
    }

    struct SansMeta;

    impl Tool for SansMeta {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "test.sans-meta".into(),
                description: "Outil sans exigences déclarées.".into(),
                input_schema: json!({"type": "object"}),
                meta: None,
            }
        }
        fn target(&self, _args: &Value, _context: &ToolContext) -> Option<String> {
            None
        }
        fn call(&self, _args: &Value, _context: &ToolContext) -> CallResult {
            CallResult::text("ne devrait jamais s'exécuter")
        }
    }

    const MANIFESTE: &str = r#"
[agent]
id = "org.test.agent"
version = "1.0.0"
name = "Test"
publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
[model]
preferred = ["local:test"]
[capabilities.max]
"tool.call" = ["test.*"]
"#;

    fn now() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_789_000_000).unwrap()
    }

    fn contexte(broker: &mut Broker, patterns: &[&str]) -> ToolContext {
        let manifest = prophet_types::manifest::Manifest::from_toml(MANIFESTE).unwrap();
        let grants: Vec<Grant> = patterns
            .iter()
            .map(|p| Grant::new(Res::Tool, Act::Call, *p))
            .collect();
        let token = broker
            .mint(&manifest, "task:01", "u", &grants, 1800, now())
            .unwrap();
        let _ = TokenBuilder::new("a", "b", "c", "d");
        ToolContext {
            token,
            task: "task:01".into(),
            home: "/home/u".into(),
            workdir: "/home/u/.prophet/tasks/task:01/work".into(),
            sandbox_level: 1,
            step: 3,
        }
    }

    fn registre() -> (Registry, Arc<MemoryJournal>, Arc<Mutex<Broker>>) {
        let broker = Broker::new(
            ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng),
            "capd@test",
            "/home/u",
        )
        .unwrap();
        let broker = Arc::new(Mutex::new(broker));
        let journal = Arc::new(MemoryJournal::new());
        let mut registry = Registry::new(Arc::clone(&broker), journal.clone());
        registry.register(Arc::new(Faux));
        registry.register(Arc::new(SansMeta));
        (registry, journal, broker)
    }

    #[test]
    fn appel_autorise_et_journalise() {
        let (registry, journal, broker) = registre();
        let context = {
            let mut b = broker.lock().unwrap();
            contexte(&mut b, &["test.faux"])
        };
        let result = registry.call("test.faux", &json!({}), &context, now());
        assert!(!result.is_error);
        assert_eq!(
            journal.kinds(),
            vec![EventKind::ToolCall, EventKind::ToolResult],
            "chaque appel laisse une trace avant et après"
        );
    }

    #[test]
    fn appel_non_couvert_par_le_jeton_refuse() {
        let (registry, journal, broker) = registre();
        let context = {
            let mut b = broker.lock().unwrap();
            contexte(&mut b, &["test.autre"])
        };
        let result = registry.call("test.faux", &json!({}), &context, now());
        assert!(result.is_error);
        assert_eq!(result.structured.unwrap()["code"], json!("PolicyDenied"));
        assert!(journal.kinds().contains(&EventKind::PolicyDeny));
    }

    #[test]
    fn outil_inconnu() {
        let (registry, _, broker) = registre();
        let context = {
            let mut b = broker.lock().unwrap();
            contexte(&mut b, &["test.faux"])
        };
        let result = registry.call("test.inexistant", &json!({}), &context, now());
        assert_eq!(result.structured.unwrap()["code"], json!("NotFound"));
    }

    #[test]
    fn outil_sans_exigences_declarees_refuse() {
        let (registry, _, broker) = registre();
        let context = {
            let mut b = broker.lock().unwrap();
            contexte(&mut b, &["test.*"])
        };
        let result = registry.call("test.sans-meta", &json!({}), &context, now());
        assert!(
            result.is_error,
            "un outil qui ne déclare pas ses exigences ne doit jamais s'exécuter"
        );
    }

    #[test]
    fn la_liste_visible_depend_du_jeton() {
        let (registry, _, broker) = registre();
        let context = {
            let mut b = broker.lock().unwrap();
            contexte(&mut b, &["test.faux"])
        };
        let visibles = registry.visible_for(&context.token);
        assert_eq!(visibles.len(), 1);
        assert_eq!(visibles[0].name, "test.faux");
        assert_eq!(registry.all().len(), 2);
    }

    #[test]
    fn le_journal_ne_contient_pas_les_arguments() {
        let (registry, journal, broker) = registre();
        let context = {
            let mut b = broker.lock().unwrap();
            contexte(&mut b, &["test.faux"])
        };
        let _ = registry.call(
            "test.faux",
            &json!({"mot_de_passe_en_clair": "tres-secret"}),
            &context,
            now(),
        );
        let rendu = serde_json::to_string(
            &journal
                .events()
                .iter()
                .map(|e| e.payload.clone())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(!rendu.contains("tres-secret"), "{rendu}");
    }

    #[test]
    #[should_panic(expected = "enregistré deux fois")]
    fn doublon_refuse() {
        let (mut registry, _, _) = registre();
        registry.register(Arc::new(Faux));
    }

    struct Exigences {
        requires: &'static str,
        level: Option<u8>,
        target: Option<&'static str>,
    }

    impl Tool for Exigences {
        fn spec(&self) -> ToolSpec {
            let mut spec = Faux.spec();
            spec.name = "test.exigences".into();
            let meta = spec.meta.as_mut().unwrap();
            meta.requires = self.requires.into();
            meta.sandbox_level_min = self.level;
            spec
        }

        fn target(&self, _: &Value, _: &ToolContext) -> Option<String> {
            self.target.map(str::to_owned)
        }

        fn call(&self, _: &Value, _: &ToolContext) -> CallResult {
            panic!("le registre doit refuser avant de passer la main à l'outil")
        }
    }

    #[test]
    fn les_exigences_invalides_ou_inapplicables_ne_sont_pas_ignorees() {
        for tool in [
            Exigences {
                requires: "inconnue.action",
                level: None,
                target: None,
            },
            Exigences {
                requires: "tool.read",
                level: None,
                target: None,
            },
            Exigences {
                requires: "fs.read",
                level: None,
                target: None,
            },
            Exigences {
                requires: "fs.read",
                level: None,
                target: Some(""),
            },
            Exigences {
                requires: "tool.call",
                level: Some(2),
                target: None,
            },
        ] {
            let (mut registry, _, broker) = registre();
            let context = contexte(&mut broker.lock().unwrap(), &["test.*"]);
            registry.register(Arc::new(tool));
            let result = registry.call("test.exigences", &json!({}), &context, now());
            assert!(result.is_error);
        }
    }

    #[test]
    fn un_jeton_ne_peut_pas_agir_pour_une_autre_tache() {
        let (mut registry, _, broker) = registre();
        let mut context = contexte(&mut broker.lock().unwrap(), &["test.*"]);
        context.task = "task:autre".into();
        registry.register(Arc::new(Exigences {
            requires: "tool.call",
            level: None,
            target: None,
        }));
        let result = registry.call("test.exigences", &json!({}), &context, now());
        assert!(result.is_error);
        assert_eq!(result.structured.unwrap()["code"], "PolicyDenied");
    }
}
