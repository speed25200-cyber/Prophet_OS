//! Moteur de politique, adossé à Cedar.
//!
//! Les politiques expriment ce que l'utilisateur et l'administrateur veulent, indépendamment de ce
//! qu'un jeton autorise. Certaines sont des **interdits absolus** : un `forbid` Cedar l'emporte
//! toujours sur un `permit`, donc aucun jeton, aussi large soit-il, ne peut les lever.

use std::collections::HashSet;
use std::str::FromStr as _;

use cedar_policy::{
    Authorizer, Context, Decision as CedarDecision, Entities, Entity, EntityId, EntityTypeName,
    EntityUid, PolicySet, Request, RestrictedExpression,
};
use prophet_types::cap::{Act, Res};

/// Politiques livrées par défaut. Elles implémentent les classes d'actions de `docs/PLAN.md` 5.2.
pub const DEFAULT_POLICIES: &str = r#"
// --- Interdits absolus : aucun jeton ne peut les lever. ---

// Les chemins sensibles (clés, identifiants, configuration système) ne sont jamais accessibles
// à un agent, même si son jeton les couvre.
forbid(principal, action, resource)
when { resource has sensitive && resource.sensitive == true };

// L'exécution de code arbitraire n'est permise qu'en microVM (niveau 2).
forbid(principal, action == Prophet::Action::"proc.exec", resource)
unless { context has sandbox_level && context.sandbox_level >= 2 };

// --- Classes d'actions automatiques. ---

// Lecture locale et observation : automatique, journalisée.
permit(principal, action in [
    Prophet::Action::"fs.read",
    Prophet::Action::"fs.list",
    Prophet::Action::"ui.read",
    Prophet::Action::"ledger.read",
    Prophet::Action::"memory.read",
    Prophet::Action::"model.use"
], resource);

// Écriture locale : automatique, car réversible par le sous-volume de tâche.
permit(principal, action in [
    Prophet::Action::"fs.write",
    Prophet::Action::"memory.write"
], resource);

// Sortie réseau, appel d'outil, action d'interface : autorisées par la politique ; la couche
// d'approbation traite séparément les cas irréversibles ou externes.
permit(principal, action in [
    Prophet::Action::"net.egress",
    Prophet::Action::"tool.call",
    Prophet::Action::"ui.act"
], resource);

// Exécution : permise ici, mais le `forbid` ci-dessus la restreint au niveau 2.
permit(principal, action == Prophet::Action::"proc.exec", resource);

// Sous-tâches et délégation.
permit(principal, action in [
    Prophet::Action::"task.spawn",
    Prophet::Action::"cap.delegate"
], resource);

// Lecture du journal de toutes les tâches : réservée, refusée par défaut (aucun permit).
// Capture d'écran : réservée, refusée par défaut (aucun permit).
"#;

/// Erreur du moteur de politique.
#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    /// Politique Cedar invalide.
    #[error("politique invalide : {0}")]
    Parse(String),
    /// Entité Cedar invalide.
    #[error("entité invalide : {0}")]
    Entity(String),
    /// Requête Cedar invalide.
    #[error("requête invalide : {0}")]
    Request(String),
}

/// Classe d'action, qui détermine le traitement par défaut (automatique ou approbation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionClass {
    /// Lecture locale : automatique.
    LocalRead,
    /// Écriture locale réversible : automatique, snapshotée.
    LocalWriteReversible,
    /// Écriture locale sensible : approbation.
    LocalWriteSensitive,
    /// Lecture réseau : automatique via le proxy.
    NetworkRead,
    /// Action irréversible ou externe : approbation obligatoire.
    IrreversibleExternal,
    /// Exécution de code : microVM imposée.
    CodeExecution,
    /// Élévation de droits : approbation et délai.
    Elevation,
}

impl ActionClass {
    /// Vrai si la classe exige une décision humaine par défaut.
    #[must_use]
    pub const fn requires_approval(self) -> bool {
        matches!(
            self,
            Self::LocalWriteSensitive | Self::IrreversibleExternal | Self::Elevation
        )
    }
}

/// Ce que `capd` sait de la cible au moment du contrôle. Ces faits alimentent Cedar.
#[derive(Debug, Clone, Default)]
pub struct ResourceFacts {
    /// Cible concrète (chemin absolu, hôte, nom d'outil).
    pub target: String,
    /// Vrai si la cible est un chemin sensible (clés, identifiants, configuration système).
    pub sensitive: bool,
    /// Vrai si l'action ne peut pas être annulée.
    pub irreversible: bool,
    /// Vrai si l'action a un effet hors de la machine.
    pub external: bool,
}

/// Chemins sensibles par défaut, relatifs au home de l'utilisateur ou absolus.
/// Un agent n'y accède jamais, quel que soit son jeton.
pub const SENSITIVE_PATHS: &[&str] = &[
    "~/.ssh",
    "~/.gnupg",
    "~/.aws",
    "~/.config/gh",
    "~/.netrc",
    "~/.git-credentials",
    "~/.prophet/vault",
    "/etc/shadow",
    "/etc/sudoers",
    "/etc/prophet/policies",
    "/var/lib/prophet/capd",
    "/var/lib/prophet/vault",
    "/var/lib/prophet/providers",
];

/// Vrai si le chemin est sous un répertoire sensible.
#[must_use]
pub fn is_sensitive_path(path: &str, home: &str) -> bool {
    SENSITIVE_PATHS.iter().any(|pattern| {
        let expanded = prophet_types::pattern::expand_home(pattern, home);
        path == expanded || path.starts_with(&format!("{expanded}/"))
    })
}

/// Moteur de politique.
#[derive(Debug)]
pub struct PolicyEngine {
    policies: PolicySet,
    authorizer: Authorizer,
}

impl PolicyEngine {
    /// Compile un jeu de politiques Cedar.
    ///
    /// # Erreurs
    /// Si le texte n'est pas une politique Cedar valide.
    pub fn new(policies: &str) -> Result<Self, PolicyError> {
        let policies =
            PolicySet::from_str(policies).map_err(|e| PolicyError::Parse(e.to_string()))?;
        Ok(Self {
            policies,
            authorizer: Authorizer::new(),
        })
    }

    /// Moteur chargé avec les politiques par défaut.
    ///
    /// # Erreurs
    /// Ne devrait jamais échouer ; une erreur signale une régression dans [`DEFAULT_POLICIES`].
    pub fn with_defaults() -> Result<Self, PolicyError> {
        Self::new(DEFAULT_POLICIES)
    }

    /// Évalue une demande. `true` signifie que la politique autorise ; le jeton reste à vérifier.
    ///
    /// # Erreurs
    /// Si les entités ou la requête Cedar ne peuvent pas être construites.
    pub fn allows(
        &self,
        task: &str,
        res: Res,
        act: Act,
        facts: &ResourceFacts,
        sandbox_level: u8,
    ) -> Result<bool, PolicyError> {
        let principal = uid("Prophet::Task", task)?;
        let action = uid("Prophet::Action", &action_name(res, act))?;
        let resource = uid("Prophet::Resource", &facts.target)?;

        let resource_entity = Entity::new(
            resource.clone(),
            [
                (
                    "sensitive".to_owned(),
                    RestrictedExpression::new_bool(facts.sensitive),
                ),
                (
                    "irreversible".to_owned(),
                    RestrictedExpression::new_bool(facts.irreversible),
                ),
                (
                    "external".to_owned(),
                    RestrictedExpression::new_bool(facts.external),
                ),
            ]
            .into_iter()
            .collect(),
            HashSet::new(),
        )
        .map_err(|e| PolicyError::Entity(e.to_string()))?;

        let principal_entity = Entity::new(
            principal.clone(),
            std::collections::HashMap::new(),
            HashSet::new(),
        )
        .map_err(|e| PolicyError::Entity(e.to_string()))?;

        let entities = Entities::from_entities([principal_entity, resource_entity], None)
            .map_err(|e| PolicyError::Entity(e.to_string()))?;

        let context = Context::from_pairs([(
            "sandbox_level".to_owned(),
            RestrictedExpression::new_long(i64::from(sandbox_level)),
        )])
        .map_err(|e| PolicyError::Request(e.to_string()))?;

        let request = Request::new(principal, action, resource, context, None)
            .map_err(|e| PolicyError::Request(e.to_string()))?;

        let response = self
            .authorizer
            .is_authorized(&request, &self.policies, &entities);
        Ok(response.decision() == CedarDecision::Allow)
    }
}

fn uid(type_name: &str, id: &str) -> Result<EntityUid, PolicyError> {
    let type_name =
        EntityTypeName::from_str(type_name).map_err(|e| PolicyError::Entity(e.to_string()))?;
    Ok(EntityUid::from_type_name_and_id(
        type_name,
        EntityId::new(id),
    ))
}

/// Nom d'action Cedar correspondant à un couple ressource/action.
#[must_use]
pub fn action_name(res: Res, act: Act) -> String {
    let res = match res {
        Res::Fs => "fs",
        Res::Net => "net",
        Res::Tool => "tool",
        Res::Proc => "proc",
        Res::Ui => "ui",
        Res::Ledger => "ledger",
        Res::Memory => "memory",
        Res::Model => "model",
        Res::Task => "task",
        Res::Cap => "cap",
    };
    let act = match act {
        Act::Read => "read",
        Act::Write => "write",
        Act::List => "list",
        Act::Egress => "egress",
        Act::Call => "call",
        Act::Exec => "exec",
        Act::Act => "act",
        Act::Vision => "vision",
        Act::ReadAll => "read_all",
        Act::Use => "use",
        Act::Spawn => "spawn",
        Act::Delegate => "delegate",
    };
    format!("{res}.{act}")
}

/// Classe d'action d'une demande, d'après la ressource, l'action et les faits.
#[must_use]
pub fn classify(res: Res, act: Act, facts: &ResourceFacts) -> ActionClass {
    match (res, act) {
        _ if facts.sensitive && act == Act::Write => ActionClass::LocalWriteSensitive,
        (Res::Proc, Act::Exec) => ActionClass::CodeExecution,
        (Res::Cap, Act::Delegate) | (Res::Ledger, Act::ReadAll) => ActionClass::Elevation,
        _ if facts.irreversible || facts.external => ActionClass::IrreversibleExternal,
        (Res::Fs, Act::Write) | (Res::Memory, Act::Write) => ActionClass::LocalWriteReversible,
        (Res::Net, Act::Egress) => ActionClass::NetworkRead,
        _ => ActionClass::LocalRead,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> PolicyEngine {
        PolicyEngine::with_defaults().unwrap()
    }

    fn facts(target: &str) -> ResourceFacts {
        ResourceFacts {
            target: target.to_owned(),
            ..ResourceFacts::default()
        }
    }

    #[test]
    fn politiques_par_defaut_compilent() {
        let _ = engine();
    }

    #[test]
    fn lecture_locale_autorisee() {
        assert!(
            engine()
                .allows("task:01", Res::Fs, Act::Read, &facts("/home/u/a.txt"), 1)
                .unwrap()
        );
    }

    #[test]
    fn chemin_sensible_refuse_meme_si_le_jeton_le_couvre() {
        let mut f = facts("/home/u/.ssh/id_ed25519");
        f.sensitive = true;
        assert!(
            !engine()
                .allows("task:01", Res::Fs, Act::Read, &f, 1)
                .unwrap(),
            "le forbid absolu doit l'emporter"
        );
    }

    #[test]
    fn execution_refusee_hors_microvm() {
        let f = facts("/usr/bin/python3");
        let e = engine();
        assert!(!e.allows("task:01", Res::Proc, Act::Exec, &f, 0).unwrap());
        assert!(!e.allows("task:01", Res::Proc, Act::Exec, &f, 1).unwrap());
        assert!(e.allows("task:01", Res::Proc, Act::Exec, &f, 2).unwrap());
    }

    #[test]
    fn actions_reservees_refusees_par_defaut() {
        let e = engine();
        assert!(
            !e.allows("task:01", Res::Ui, Act::Vision, &facts("*"), 1)
                .unwrap()
        );
        assert!(
            !e.allows("task:01", Res::Ledger, Act::ReadAll, &facts("*"), 1)
                .unwrap()
        );
    }

    #[test]
    fn detection_des_chemins_sensibles() {
        assert!(is_sensitive_path("/home/u/.ssh/id_ed25519", "/home/u"));
        assert!(is_sensitive_path("/home/u/.ssh", "/home/u"));
        assert!(is_sensitive_path("/etc/shadow", "/home/u"));
        assert!(!is_sensitive_path("/home/u/.sshfoo", "/home/u"));
        assert!(!is_sensitive_path("/home/u/ventes/q3.csv", "/home/u"));
    }

    #[test]
    fn classification_des_actions() {
        let neutre = facts("x");
        assert_eq!(
            classify(Res::Fs, Act::Read, &neutre),
            ActionClass::LocalRead
        );
        assert_eq!(
            classify(Res::Fs, Act::Write, &neutre),
            ActionClass::LocalWriteReversible
        );
        assert_eq!(
            classify(Res::Proc, Act::Exec, &neutre),
            ActionClass::CodeExecution
        );
        let externe = ResourceFacts {
            external: true,
            ..neutre.clone()
        };
        assert_eq!(
            classify(Res::Tool, Act::Call, &externe),
            ActionClass::IrreversibleExternal
        );
        assert!(classify(Res::Tool, Act::Call, &externe).requires_approval());
        assert!(!classify(Res::Fs, Act::Read, &neutre).requires_approval());
    }

    #[test]
    fn politique_invalide_rejetee() {
        assert!(PolicyEngine::new("ceci n'est pas du cedar").is_err());
    }
}
