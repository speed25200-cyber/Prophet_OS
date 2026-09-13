//! Le runtime : ce qui tient ensemble capacités, espace de travail, pilote et journal.
//!
//! Chaque tâche suit la même séquence, dans cet ordre, parce que chaque étape dépend de la
//! précédente :
//! 1. **Planification** : choix du pilote, du niveau de sandbox, des capacités demandées.
//! 2. **Jeton** : `capd` émet un jeton borné par le manifeste.
//! 3. **Espace de travail** : `sfs` ouvre une branche, pour que tout reste annulable.
//! 4. **Exécution** : le pilote produit des événements, chacun journalisé et compté au budget.
//! 5. **Clôture** : validation ou abandon, avec un diff que l'humain peut lire.

use std::collections::BTreeMap;
use std::path::PathBuf;

use capd::Broker;
use prophet_types::cap::{Act, Grant, Res, Token};
use prophet_types::driver::{
    DriverEvent, Limits as DriverLimits, RunStatus, SandboxRequest, StartRequest,
};
use prophet_types::ledger::{Actor, Draft, EventKind};
use prophet_types::manifest::Manifest;
use providers::selection::{Availability, Choice, QuotaPolicy, choose};
use providers::{Driver, DriverError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;

use crate::budget::{Budget, Dimension, Limits};
use crate::task::{State, Task, TaskError};

#[cfg(test)]
mod review_ownership {
    use super::*;

    #[test]
    fn le_pair_observe_est_persistant_et_ne_se_deduit_pas_du_nom_declare() {
        let mut runtime = Runtime::sans_broker("/home/test");
        let mut task = Task::new(
            "one",
            "Objectif",
            "local:test",
            "uid:2000",
            Budget::new(Default::default()),
            OffsetDateTime::now_utc(),
        );
        task.state = State::Done;
        runtime.tasks.insert(task.id.clone(), task);
        runtime
            .results
            .insert("one".into(), json!({"review":{"entries":[]}}));
        assert!(runtime.review_context("one", 1000).is_err());
        runtime.bind_owner("one", 1000).unwrap();
        assert!(runtime.bind_owner("one", 2000).is_err());
        assert!(runtime.review_context("one", 2000).is_err());
        assert!(runtime.review_context("one", 0).is_err());
        let encoded = serde_json::to_vec(&runtime.etat()).unwrap();
        let mut restored = Runtime::sans_broker("/home/test");
        restored.reprendre(serde_json::from_slice(&encoded).unwrap());
        assert!(restored.review_context("one", 1000).is_ok());
        assert!(restored.review_context("one", 2000).is_err());
        let mut legacy: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        legacy.as_object_mut().unwrap().remove("owners");
        let mut legacy_runtime = Runtime::sans_broker("/home/test");
        legacy_runtime.reprendre(serde_json::from_value(legacy).unwrap());
        assert!(legacy_runtime.review_context("one", 1000).is_err());
    }
}

/// Erreur du runtime.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// Erreur de cycle de vie.
    #[error(transparent)]
    Task(#[from] TaskError),
    /// Erreur de capacités.
    #[error("capacités : {0}")]
    Capability(String),
    /// Aucun pilote disponible.
    #[error("aucun pilote disponible : {0}")]
    NoDriver(String),
    /// Erreur de pilote.
    #[error(transparent)]
    Driver(#[from] DriverError),
    /// Erreur d'espace de travail.
    #[error("espace de travail : {0}")]
    Workspace(String),
    /// Tâche inconnue.
    #[error("tâche inconnue : {0}")]
    Unknown(String),
}

/// Ce qu'il faut fournir pour planifier une tâche.
///
/// Regrouper ces éléments plutôt que de les passer un à un n'est pas cosmétique : cela rend
/// impossible d'en intervertir deux, et cela donne un nom à ce que le shell doit rassembler avant
/// de proposer un plan à l'humain.
#[derive(Debug, Clone)]
pub struct PlanRequest<'a> {
    /// Identifiant de la tâche.
    pub id: &'a str,
    /// Intention exprimée par l'humain.
    pub intent: &'a str,
    /// Manifeste de l'agent chargé.
    pub manifest: &'a Manifest,
    /// Utilisateur pour le compte duquel la tâche s'exécute.
    pub user: &'a str,
    /// Capacités demandées, qui seront intersectées avec le plafond du manifeste.
    pub requested: &'a [Grant],
    /// Périmètre du système de fichiers.
    pub scopes: &'a [&'a str],
    /// Ce qui est disponible au moment du choix du pilote.
    pub availability: &'a Availability,
}

/// Décisions prises à la planification, avant toute exécution.
///
/// Le plan est montré à l'humain avant de démarrer : il doit pouvoir dire non en connaissance de
/// cause, ce qui suppose que tout y soit, y compris ce que la tâche pourra faire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskPlan {
    /// Tâche concernée.
    pub task: String,
    /// Intention.
    pub intent: String,
    /// Pilote retenu et pourquoi.
    pub choice: Choice,
    /// Niveau de sandbox.
    pub sandbox_level: u8,
    /// Capacités demandées, en clair.
    pub grants: Vec<String>,
    /// Budget.
    pub limits: Limits,
    /// Périmètre du système de fichiers.
    pub scopes: Vec<String>,
}

/// Vue atomique destinée à la supervision, sans jeton ni état interne du broker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inspection {
    /// État et budget observés.
    pub task: Task,
    /// Plan conservé, absent pour certaines anciennes tâches.
    pub plan: Option<TaskPlan>,
    /// Résultat conservé, absent avant la fin.
    pub result: Option<serde_json::Value>,
    /// Le service possède le parcours de lancement correspondant à ce plan.
    pub can_start: bool,
    /// Une annulation peut être demandée au service.
    pub can_cancel: bool,
    /// Pourquoi un plan ne peut pas être lancé ici.
    pub start_reason: Option<String>,
    /// État de publication des versions, lu dans SFS ; absent sans versions conservées.
    #[serde(default)]
    pub publication: Option<sfs::WorkspaceState>,
    /// Le créateur peut publier les versions examinées dans ses documents.
    #[serde(default)]
    pub can_apply: bool,
    /// Le créateur peut annuler une publication effectuée.
    #[serde(default)]
    pub can_undo: bool,
    /// Où l'agent navigue en ce moment : adresse, titre et taille de la page, jamais son contenu.
    #[serde(default)]
    pub browsing: Option<serde_json::Value>,
}

impl Inspection {
    /// Complète la vue avec l'état de publication lu hors du verrou du service.
    ///
    /// Les commandes ne sont offertes qu'au créateur constaté de la mission : la publication
    /// touche ses documents, et l'état SFS seul ne dit pas qui a le droit de la demander.
    /// Une publication ou une annulation interrompue se reprend par la même commande.
    #[must_use]
    pub fn with_publication(mut self, owner: bool, state: Option<sfs::WorkspaceState>) -> Self {
        use sfs::WorkspaceState as W;
        self.publication = state;
        let done = self.task.state == State::Done;
        self.can_apply = owner && done && matches!(state, Some(W::Open | W::Applying));
        self.can_undo = owner && done && matches!(state, Some(W::Committed | W::Undoing));
        self
    }
}

/// Ce que la publication a fait, pour le journal et l'état de la tâche.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Publication {
    /// Les versions examinées ont atteint les documents.
    Applied,
    /// Les documents publiés ont retrouvé leurs versions initiales.
    Undone,
}

impl TaskPlan {
    /// Rendu lisible, tel que le shell l'affiche avant de demander le feu vert.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = format!("Tâche {} : {}\n", self.task, self.intent);
        out.push_str(&format!(
            "  Pilote      : {} ({})\n",
            self.choice.reference, self.choice.reason
        ));
        out.push_str(&format!("  Isolation   : niveau {}\n", self.sandbox_level));
        out.push_str(&format!("  Périmètre   : {}\n", self.scopes.join(", ")));
        out.push_str("  Capacités   :\n");
        for grant in &self.grants {
            out.push_str(&format!("    - {grant}\n"));
        }
        out.push_str(&format!(
            "  Budget      : {} tokens, {} s, {} étapes, {} approbations\n",
            self.limits.tokens, self.limits.wall_time_s, self.limits.steps, self.limits.approvals
        ));
        out
    }
}

/// Ce qu'une exécution a produit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// Tâche.
    pub task: String,
    /// État final.
    pub state: State,
    /// Pilote employé.
    pub driver: String,
    /// Nombre d'étapes.
    pub steps: u32,
    /// Appels d'outils.
    pub tool_calls: u32,
    /// Approbations demandées.
    pub approvals: u32,
    /// Tokens consommés.
    pub tokens: u64,
    /// Texte final rendu à l'humain.
    pub final_text: Option<String>,
    /// Raison d'un arrêt anticipé.
    pub reason: Option<String>,
}

/// Ce qu'un runtime doit retenir d'un démarrage à l'autre.
///
/// Sans cela, redémarrer `prophet-agentd` — une mise à jour, un plantage, un simple
/// `systemctl restart` — effacerait toutes les tâches en cours. L'écran se viderait, `prophet task
/// ls` dirait « aucune tâche », et le travail en cours continuerait sans que rien ne le surveille.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EtatPersistant {
    /// Les tâches connues.
    pub taches: Vec<Task>,
    /// Leurs jetons, indexés par tâche.
    pub jetons: BTreeMap<String, Token>,
    /// Plans retenus, y compris les périmètres nécessaires au lancement.
    #[serde(default)]
    pub plans: BTreeMap<String, TaskPlan>,
    /// Résultats de missions relisibles après redémarrage.
    #[serde(default)]
    pub results: BTreeMap<String, serde_json::Value>,
    /// UID constaté sur le socket à la création, distinct d'une identité déclarée dans le plan.
    #[serde(default)]
    pub owners: BTreeMap<String, u32>,
    /// Manifestes des missions, pour émettre un jeton lié à l'index exact au moment de publier.
    #[serde(default)]
    pub manifests: BTreeMap<String, Manifest>,
}

/// Le runtime.
pub struct Runtime {
    /// Le broker, quand le runtime émet lui-même les jetons.
    ///
    /// Il est absent dans le daemon : là, c'est `prophet-capd` qui émet, parce qu'une seule clé
    /// doit signer les jetons de tout le système. Deux brokers avec deux clés produiraient des
    /// jetons que le reste des services jugerait contrefaits — et ils auraient raison.
    broker: Option<Broker>,
    home: PathBuf,
    tasks: BTreeMap<String, Task>,
    tokens: BTreeMap<String, Token>,
    journal: Vec<Draft>,
    quota_policy: QuotaPolicy,
    plans: BTreeMap<String, TaskPlan>,
    results: BTreeMap<String, serde_json::Value>,
    owners: BTreeMap<String, u32>,
    manifests: BTreeMap<String, Manifest>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("tasks", &self.tasks.len())
            .field("home", &self.home)
            .finish_non_exhaustive()
    }
}

impl Runtime {
    /// Construit un runtime.
    #[must_use]
    pub fn new(broker: Broker, home: impl Into<PathBuf>) -> Self {
        Self {
            broker: Some(broker),
            home: home.into(),
            tasks: BTreeMap::new(),
            tokens: BTreeMap::new(),
            journal: Vec::new(),
            quota_policy: QuotaPolicy::default(),
            plans: BTreeMap::new(),
            results: BTreeMap::new(),
            owners: BTreeMap::new(),
            manifests: BTreeMap::new(),
        }
    }

    /// Runtime sans broker, dont les jetons viennent d'ailleurs.
    ///
    /// C'est la forme qu'utilise `prophet-agentd` : il demande le jeton à `capd`, puis appelle
    /// [`Runtime::plan_with_token`]. Appeler [`Runtime::plan`] sur un tel runtime échoue au lieu
    /// d'émettre un jeton que personne d'autre ne reconnaîtrait.
    #[must_use]
    pub fn sans_broker(home: impl Into<PathBuf>) -> Self {
        Self {
            broker: None,
            home: home.into(),
            tasks: BTreeMap::new(),
            tokens: BTreeMap::new(),
            journal: Vec::new(),
            quota_policy: QuotaPolicy::default(),
            plans: BTreeMap::new(),
            results: BTreeMap::new(),
            owners: BTreeMap::new(),
            manifests: BTreeMap::new(),
        }
    }

    /// Choisit la conduite à tenir quand un quota d'abonnement s'épuise.
    #[must_use]
    pub const fn quota_policy(mut self, policy: QuotaPolicy) -> Self {
        self.quota_policy = policy;
        self
    }

    /// L'état persistable du runtime : les tâches et leurs jetons.
    ///
    /// Le journal n'en fait pas partie : il est poussé vers `ledger` au fur et à mesure, et le
    /// conserver ici le ferait écrire deux fois.
    #[must_use]
    pub fn etat(&self) -> EtatPersistant {
        EtatPersistant {
            taches: self.tasks.values().cloned().collect(),
            jetons: self.tokens.clone(),
            plans: self.plans.clone(),
            results: self.results.clone(),
            owners: self.owners.clone(),
            manifests: self.manifests.clone(),
        }
    }

    /// Reprend un état écrit par [`Runtime::etat`].
    ///
    /// Ce que cette fonction ne fait pas : elle ne revalide pas les jetons. Un jeton périmé le
    /// reste, et `capd` le refusera au premier contrôle — c'est lui qui décide, pas nous. Le
    /// reconstituer ici serait se donner un droit qu'on n'a pas.
    pub fn reprendre(&mut self, etat: EtatPersistant) {
        for tache in etat.taches {
            self.tasks.insert(tache.id.clone(), tache);
        }
        self.tokens.extend(etat.jetons);
        self.plans.extend(etat.plans);
        self.results.extend(etat.results);
        self.owners.extend(etat.owners);
        self.manifests.extend(etat.manifests);
    }

    /// Événements journalisés.
    #[must_use]
    pub fn journal(&self) -> &[Draft] {
        &self.journal
    }

    /// Retire les événements accumulés et les rend.
    ///
    /// `prophet-agentd` les pousse vers `prophet-ledger` puis n'a plus à s'en soucier. Sans ce
    /// retrait, chaque envoi rejouerait tout l'historique : le journal se remplirait de doublons,
    /// et un journal qui raconte deux fois la même chose ne raconte plus rien de fiable.
    pub fn retirer_le_journal(&mut self) -> Vec<Draft> {
        std::mem::take(&mut self.journal)
    }

    /// Tâches connues.
    #[must_use]
    pub fn tasks(&self) -> Vec<&Task> {
        self.tasks.values().collect()
    }

    /// Une tâche par identifiant.
    #[must_use]
    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.get(id)
    }

    /// Résultat durable d'une mission terminée.
    #[must_use]
    pub fn result(&self, id: &str) -> Option<&serde_json::Value> {
        self.results.get(id)
    }

    /// Lie une nouvelle mission au pair réellement observé. Aucune méthode IPC ne permet de changer ce lien.
    ///
    /// # Errors
    /// Tâche absente ou déjà liée à une autre identité.
    pub fn bind_owner(&mut self, id: &str, uid: u32) -> Result<(), String> {
        if !self.tasks.contains_key(id) || self.owners.get(id).is_some_and(|owner| *owner != uid) {
            return Err("Propriétaire de mission incohérent.".into());
        }
        self.owners.insert(id.into(), uid);
        Ok(())
    }

    /// Le propriétaire d'une mission, s'il est connu.
    #[must_use]
    pub fn owner_of(&self, id: &str) -> Option<u32> {
        self.owners.get(id).copied()
    }

    /// Rattache une mission planifiée à son parent : filiation, profondeur, et un budget
    /// **prélevé** sur celui du parent, jamais ajouté (ADR 0029).
    ///
    /// # Errors
    /// Mission ou parent inconnus, profondeur maximale atteinte.
    pub fn link_child(
        &mut self,
        child: &str,
        parent: &str,
        fraction: f64,
    ) -> Result<(), RuntimeError> {
        let (budget, depth) = {
            let parent_task = self
                .tasks
                .get(parent)
                .ok_or_else(|| RuntimeError::Unknown(parent.to_owned()))?;
            if parent_task.depth + 1 > crate::task::MAX_DEPTH {
                return Err(RuntimeError::Capability(format!(
                    "profondeur maximale de sous-missions atteinte ({})",
                    crate::task::MAX_DEPTH
                )));
            }
            (parent_task.budget.reserve(fraction), parent_task.depth + 1)
        };
        let task = self
            .tasks
            .get_mut(child)
            .ok_or_else(|| RuntimeError::Unknown(child.to_owned()))?;
        task.parent = Some(parent.to_owned());
        task.depth = depth;
        task.budget = budget;
        Ok(())
    }

    /// Impute au parent ce que la sous-mission a consommé.
    pub fn absorb_child(&mut self, child: &str, parent: &str) {
        let Some(spent) = self.tasks.get(child).map(|t| t.budget) else {
            return;
        };
        if let Some(parent_task) = self.tasks.get_mut(parent) {
            parent_task.budget.absorb(&spent);
        }
    }

    /// Les sous-missions d'une mission, dans l'ordre de création.
    #[must_use]
    pub fn children_of(&self, parent: &str) -> Vec<&Task> {
        let mut enfants: Vec<&Task> = self
            .tasks
            .values()
            .filter(|t| t.parent.as_deref() == Some(parent))
            .collect();
        enfants.sort_by_key(|t| t.created);
        enfants
    }

    /// Donne au créateur l'index de ses versions capturées, pour une lecture hors du verrou du runtime.
    ///
    /// # Errors
    /// Identité absente ou différente, mission non terminée, anciennes versions indisponibles.
    pub fn review_context(
        &self,
        id: &str,
        uid: u32,
    ) -> Result<(PathBuf, sfs::ReviewIndex), String> {
        if self.owners.get(id) != Some(&uid) {
            return Err("L'examen des fichiers est réservé au créateur de cette mission.".into());
        }
        if self
            .tasks
            .get(id)
            .is_none_or(|task| task.state != State::Done)
        {
            return Err("Les versions ne sont disponibles qu'après la fin de la mission.".into());
        }
        let index = self
            .results
            .get(id)
            .and_then(|result| result.get("review"))
            .ok_or("Cette mission ne contient pas de versions vérifiables.")?;
        let index =
            serde_json::from_value(index.clone()).map_err(|_| "Index des versions illisible.")?;
        Ok((self.home.clone(), index))
    }

    /// Vue de supervision cohérente, créée sous le verrou du service.
    ///
    /// # Errors
    /// Tâche inconnue.
    pub fn inspect(
        &self,
        id: &str,
        local_configured: bool,
        has_worker: bool,
    ) -> Result<Inspection, RuntimeError> {
        let task = self
            .tasks
            .get(id)
            .ok_or_else(|| RuntimeError::Unknown(id.into()))?
            .clone();
        let plan = self.plans.get(id).cloned();
        let start_reason = if !local_configured {
            Some("Le moteur local du service n'est pas configuré.".into())
        } else if plan.is_none() {
            Some("Aucun plan conservé : cette mission doit être recréée.".into())
        } else if plan
            .as_ref()
            .is_none_or(|p| !p.choice.reference.starts_with("local:") || p.sandbox_level != 0)
        {
            Some("Ce plan nécessite un pilote isolé qui reste à raccorder.".into())
        } else {
            None
        };
        Ok(Inspection {
            can_start: task.state == State::Planned && start_reason.is_none(),
            can_cancel: !task.state.is_terminal()
                && (has_worker || matches!(task.state, State::Pending | State::Planned)),
            task,
            plan,
            result: self.results.get(id).cloned(),
            start_reason,
            publication: None,
            can_apply: false,
            can_undo: false,
            browsing: None,
        })
    }

    /// Répertoire personnel dont le service tient les captures.
    #[must_use]
    pub fn home(&self) -> &std::path::Path {
        &self.home
    }

    /// Vrai si ce pair est le créateur persisté de la mission.
    #[must_use]
    pub fn is_owner(&self, id: &str, uid: u32) -> bool {
        self.owners.get(id) == Some(&uid)
    }

    /// Donne au créateur l'index exact de ses versions et la provenance à poser, pour une
    /// publication ou une annulation hors du verrou du runtime.
    ///
    /// # Errors
    /// Identité absente ou différente, mission non terminée, versions indisponibles.
    pub fn publication_context(
        &self,
        id: &str,
        uid: u32,
    ) -> Result<(PathBuf, sfs::ReviewIndex, sfs::Provenance), String> {
        if self.owners.get(id) == Some(&uid)
            && self
                .tasks
                .get(id)
                .is_some_and(|task| task.state == State::RolledBack)
        {
            return Err("Cette publication a déjà été annulée ; la mission est close.".into());
        }
        let (home, index) = self.review_context(id, uid)?;
        let task = self.tasks.get(id).ok_or("Mission inconnue.")?;
        let model = task
            .driver
            .clone()
            .or_else(|| self.plans.get(id).map(|plan| plan.choice.reference.clone()))
            .unwrap_or_default();
        let provenance = sfs::Provenance {
            task: task.id.clone(),
            agent: task.agent.clone(),
            step: task.budget.spent.steps,
            model,
        };
        Ok((home, index, provenance))
    }

    /// Consigne une publication effectuée par le créateur, après son succès sur le disque.
    ///
    /// Une annulation rend la tâche `rolled_back` : c'est la promesse de réversibilité, et
    /// l'état de la tâche doit le dire sans obliger à relire SFS.
    ///
    /// # Errors
    /// Tâche inconnue ou transition interdite.
    pub fn record_publication(
        &mut self,
        id: &str,
        outcome: Publication,
        diff: &sfs::Diff,
        now: OffsetDateTime,
    ) -> Result<(), RuntimeError> {
        let (added, modified, deleted) = diff.counts();
        let counts = json!({"added":added,"modified":modified,"deleted":deleted});
        if !self.tasks.contains_key(id) {
            return Err(RuntimeError::Unknown(id.into()));
        }
        match outcome {
            Publication::Applied => {
                self.record(id, EventKind::FsCommit, Actor::user(), counts, now);
            }
            Publication::Undone => {
                self.tasks
                    .get_mut(id)
                    .ok_or_else(|| RuntimeError::Unknown(id.into()))?
                    .transition(
                        State::RolledBack,
                        Some("versions annulées par l'utilisateur".into()),
                    )?;
                self.record(id, EventKind::FsUndo, Actor::user(), counts, now);
                self.record(
                    id,
                    EventKind::TaskRolledBack,
                    Actor::user(),
                    json!({"reason":"versions annulées par l'utilisateur"}),
                    now,
                );
            }
        }
        Ok(())
    }

    /// Ce qu'il faut demander à capd avant de publier : le manifeste de la mission, son
    /// utilisateur, et un grant d'écriture par fichier de l'index exact.
    ///
    /// Le jeton de la mission a pu expirer pendant l'examen humain ; un jeton neuf, borné à
    /// ces chemins, est la seule façon de faire trancher capd sur ce qui va réellement changer.
    ///
    /// # Errors
    /// Mission planifiée sans manifeste conservé.
    pub fn publication_grants(
        &self,
        id: &str,
        review: &sfs::ReviewIndex,
    ) -> Result<(Manifest, String, Vec<Grant>), String> {
        let manifest = self.manifests.get(id).ok_or(
            "Cette mission a été planifiée sans manifeste conservé ; replanifiez-la avant de publier.",
        )?;
        let user = self
            .tasks
            .get(id)
            .map(|task| task.user.clone())
            .ok_or("Mission inconnue.")?;
        let grants = review
            .diff()
            .changes
            .iter()
            .map(|change| Grant::new(Res::Fs, Act::Write, format!("~/{}", change.path.display())))
            .collect();
        Ok((manifest.clone(), user, grants))
    }

    /// Consigne un refus de capd au moment de publier, sans changer l'état de la tâche.
    pub fn record_publication_denied(
        &mut self,
        id: &str,
        path: &str,
        reason: &str,
        now: OffsetDateTime,
    ) {
        self.record(
            id,
            EventKind::PolicyDeny,
            Actor::daemon("agentd"),
            json!({"stage":"publish","path":path,"reason":reason}),
            now,
        );
    }

    /// Annule une tâche qui n'a pas de travailleur lancé.
    ///
    /// # Errors
    /// Tâche inconnue, déjà lancée ou terminée.
    pub fn cancel_unstarted(&mut self, id: &str, now: OffsetDateTime) -> Result<(), RuntimeError> {
        let task = self
            .tasks
            .get_mut(id)
            .ok_or_else(|| RuntimeError::Unknown(id.into()))?;
        if !matches!(task.state, State::Pending | State::Planned) {
            return Err(TaskError::BadTransition {
                from: task.state,
                to: State::Cancelled,
            }
            .into());
        }
        task.transition(State::Cancelled, Some("annulée par l'utilisateur".into()))?;
        self.record(
            id,
            EventKind::TaskCancelled,
            Actor::user(),
            json!({"reason":"annulée par l'utilisateur"}),
            now,
        );
        Ok(())
    }

    /// Réserve le lancement local sans laisser deux travailleurs prendre la même mission.
    ///
    /// # Errors
    /// Tâche non planifiée, plan absent, client officiel ou isolation non raccordée.
    pub fn begin_local(
        &mut self,
        id: &str,
    ) -> Result<(Task, Token, TaskPlan, PathBuf), RuntimeError> {
        let task = self
            .tasks
            .get_mut(id)
            .ok_or_else(|| RuntimeError::Unknown(id.into()))?;
        let plan = self
            .plans
            .get(id)
            .ok_or_else(|| RuntimeError::Workspace("plan absent : recréer la mission".into()))?;
        if !plan.choice.reference.starts_with("local:") || plan.sandbox_level != 0 {
            return Err(RuntimeError::NoDriver("le lanceur local d'outils ne lance aucun processus non fiable ; les pilotes isolés restent à raccorder".into()));
        }
        let token = self.tokens.get(id).filter(|t| t.sub == id).ok_or_else(|| {
            RuntimeError::Capability("jeton de tâche absent ou incohérent".into())
        })?;
        task.transition(State::Running, None)?;
        Ok((task.clone(), token.clone(), plan.clone(), self.home.clone()))
    }

    /// Publie un état de travailleur et, à la fin, le résultat à conserver.
    pub fn publish_local(&mut self, task: Task, result: Option<serde_json::Value>) {
        if let Some(result) = result {
            self.results.insert(task.id.clone(), result);
        }
        self.tasks.insert(task.id.clone(), task);
    }

    fn record(
        &mut self,
        task: &str,
        kind: EventKind,
        actor: Actor,
        payload: serde_json::Value,
        now: OffsetDateTime,
    ) {
        self.journal
            .push(Draft::new(now, actor, kind, payload).task(task));
    }

    /// Crée et planifie une tâche.
    ///
    /// # Errors
    /// Aucun pilote disponible, ou capacités refusées.
    pub fn plan(
        &mut self,
        request: &PlanRequest<'_>,
        now: OffsetDateTime,
    ) -> Result<TaskPlan, RuntimeError> {
        self.planifier(request, None, now)
    }

    /// Crée et planifie une tâche dont le jeton a été émis ailleurs.
    ///
    /// C'est la voie du daemon : `capd` a déjà fait l'intersection entre ce que la tâche demande
    /// et ce que son manifeste plafonne, et le jeton qui en résulte est signé par la seule clé que
    /// le reste du système reconnaît.
    ///
    /// # Errors
    /// Aucun pilote disponible, ou capacités refusées.
    pub fn plan_with_token(
        &mut self,
        request: &PlanRequest<'_>,
        token: Token,
        now: OffsetDateTime,
    ) -> Result<TaskPlan, RuntimeError> {
        self.planifier(request, Some(token), now)
    }

    fn planifier(
        &mut self,
        request: &PlanRequest<'_>,
        jeton_fourni: Option<Token>,
        now: OffsetDateTime,
    ) -> Result<TaskPlan, RuntimeError> {
        let PlanRequest {
            id,
            intent,
            manifest,
            user,
            requested,
            scopes,
            availability,
        } = *request;
        let path = std::path::Path::new(id);
        if !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
            || self.tasks.contains_key(id)
            || id.len() > 160
            || path.components().count() != 1
            || !matches!(
                path.components().next(),
                Some(std::path::Component::Normal(_))
            )
        {
            return Err(RuntimeError::Capability(
                "identifiant de tâche invalide ou déjà utilisé".into(),
            ));
        }
        let limits = Limits {
            tokens: manifest.budget.default.tokens,
            wall_time_s: manifest.wall_time_seconds().unwrap_or(1200),
            steps: 200,
            approvals: manifest.budget.default.approvals,
            cost_eur: manifest.budget.default.cost_eur,
        };
        let mut task = Task::new(
            id,
            intent,
            &manifest.agent.id,
            user,
            Budget::new(limits),
            now,
        );
        self.record(
            id,
            EventKind::TaskCreated,
            Actor::daemon("agentd"),
            json!({
                "agent": manifest.agent.id,
                "user": user,
                "intent_digest": digest(intent),
            }),
            now,
        );

        let choice =
            choose(manifest, availability).map_err(|e| RuntimeError::NoDriver(format!("{e:?}")))?;

        // Le niveau de sandbox est le plus contraignant des trois exigences : manifeste, jeton,
        // et nature de la tâche. Jamais le plus permissif.
        let executes_code = requested
            .iter()
            .any(|g| g.res == Res::Proc && g.act == Act::Exec);
        let sandbox_level = sandboxd::Manager::required_level(
            manifest.sandbox.min_level,
            requested
                .iter()
                .filter_map(|g| g.constraints.level)
                .max()
                .unwrap_or(0),
            executes_code,
        );

        let token = match &mut self.broker {
            Some(broker) => broker
                .mint(
                    manifest,
                    id,
                    user,
                    requested,
                    i64::try_from(limits.wall_time_s).unwrap_or(1800),
                    now,
                )
                .map_err(|e| RuntimeError::Capability(e.to_string()))?,
            None => match jeton_fourni {
                Some(jeton) => jeton,
                None => {
                    return Err(RuntimeError::Capability(
                        "ce runtime n'émet pas de jetons : fournissez-en un (plan_with_token)"
                            .to_owned(),
                    ));
                }
            },
        };

        let grants: Vec<String> = token
            .grants
            .iter()
            .map(|g| format!("{:?}.{:?} sur {}", g.res, g.act, g.pattern).to_lowercase())
            .collect();

        task.driver = Some(choice.reference.clone());
        task.sandbox_level = Some(sandbox_level);
        task.transition(State::Planned, None)?;

        self.record(
            id,
            EventKind::TaskPlanned,
            Actor::daemon("agentd"),
            json!({
                "provider": choice.reference,
                "sandbox_level": sandbox_level,
                "grants_digest": digest(&grants.join("|")),
                "budget": {"tokens": limits.tokens, "steps": limits.steps},
            }),
            now,
        );

        self.tasks.insert(id.to_owned(), task);
        self.tokens.insert(id.to_owned(), token);

        let plan = TaskPlan {
            task: id.to_owned(),
            intent: intent.to_owned(),
            choice,
            sandbox_level,
            grants,
            limits,
            scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
        };
        self.plans.insert(id.to_owned(), plan.clone());
        self.manifests.insert(id.to_owned(), manifest.clone());
        Ok(plan)
    }

    /// Annule une tâche en cours.
    ///
    /// L'annulation est un geste de l'humain : elle doit aboutir même si le pilote traîne, d'où
    /// l'ordre choisi. On demande d'abord au pilote de s'arrêter proprement, puis on marque la
    /// tâche annulée sans attendre sa confirmation. Un pilote qui répond plus tard trouve une
    /// tâche déjà close, ce qui est sans conséquence.
    ///
    /// # Errors
    /// Tâche inconnue, ou tâche déjà dans un état définitif.
    pub fn cancel(
        &mut self,
        id: &str,
        run: Option<&str>,
        driver: &mut dyn Driver,
        now: OffsetDateTime,
    ) -> Result<(), RuntimeError> {
        if let Some(run) = run {
            // Une erreur du pilote ne bloque pas l'annulation : elle est seulement journalisée.
            if let Err(error) = driver.cancel(run) {
                tracing::warn!(%error, "le pilote n'a pas confirmé l'annulation");
            }
        }
        let task = self
            .tasks
            .get_mut(id)
            .ok_or_else(|| RuntimeError::Unknown(id.to_owned()))?;
        task.transition(
            State::Cancelled,
            Some("annulée par l'utilisateur".to_owned()),
        )?;
        self.record(
            id,
            EventKind::TaskCancelled,
            Actor::user(),
            json!({"reason": "annulée par l'utilisateur"}),
            now,
        );
        Ok(())
    }

    /// Jeton d'une tâche.
    #[must_use]
    pub fn token(&self, task: &str) -> Option<&Token> {
        self.tokens.get(task)
    }

    /// Exécute une tâche jusqu'à son terme, en consommant les événements du pilote.
    ///
    /// Chaque événement est journalisé et imputé au budget. Une dimension épuisée arrête la tâche
    /// proprement plutôt que de la laisser filer.
    ///
    /// # Errors
    /// Tâche inconnue, ou erreur de pilote au démarrage.
    pub fn run(
        &mut self,
        id: &str,
        driver: &mut dyn Driver,
        now: OffsetDateTime,
    ) -> Result<Report, RuntimeError> {
        let workdir = self.home.join(".prophet/tasks").join(id).join("work");
        let token_text = self
            .tokens
            .get(id)
            .and_then(|t| serde_json::to_string(t).ok())
            .unwrap_or_default();
        let (intent, limits, driver_name) = {
            let task = self
                .tasks
                .get(id)
                .ok_or_else(|| RuntimeError::Unknown(id.to_owned()))?;
            (
                task.intent.clone(),
                task.budget.limits,
                task.driver.clone().unwrap_or_else(|| "inconnu".to_owned()),
            )
        };

        let request = StartRequest {
            driver: driver_name.clone(),
            task: id.to_owned(),
            intent,
            workdir: workdir.display().to_string(),
            mcp_config: workdir
                .parent()
                .map(|p| p.join("mcp.json").display().to_string())
                .unwrap_or_default(),
            token: token_text,
            sandbox: SandboxRequest {
                level: self.tasks[id].sandbox_level.unwrap_or(1),
                profile: "base".into(),
            },
            limits: DriverLimits {
                wall_time_s: limits.wall_time_s,
                max_steps: limits.steps,
            },
            resume: None,
        };

        let response = driver.start(&request)?;
        self.record(
            id,
            EventKind::ProviderStarted,
            Actor::driver(&driver_name),
            json!({"driver": driver_name, "session_ref_digest": digest(&response.session_ref)}),
            now,
        );
        if let Some(task) = self.tasks.get_mut(id) {
            task.transition(State::Running, None)?;
        }

        let mut tool_calls = 0_u32;
        let mut final_text = None;
        let mut stop_reason = None;
        let mut status = RunStatus::Failed;

        'outer: for _ in 0..limits.steps.saturating_add(10) {
            let events = driver.poll(&response.run)?;
            if events.is_empty() {
                break;
            }
            for event in events {
                match &event {
                    DriverEvent::Step { n, .. } => {
                        if let Some(task) = self.tasks.get_mut(id)
                            && let Err(TaskError::BudgetExhausted(dimension)) =
                                task.record_step(0, u64::from(*n), 0.0)
                        {
                            stop_reason = Some(dimension);
                            break 'outer;
                        }
                    }
                    DriverEvent::Usage {
                        tokens_out,
                        quota_pct,
                        ..
                    } => {
                        if let Some(task) = self.tasks.get_mut(id) {
                            task.budget.spent.tokens += tokens_out.unwrap_or(0);
                            if let Some(pct) = quota_pct {
                                task.budget.spent.quota_pct = *pct;
                            }
                            if let Some(dimension) = task.budget.exhausted() {
                                stop_reason = Some(dimension);
                                break 'outer;
                            }
                        }
                    }
                    DriverEvent::ToolCall {
                        tool, args_digest, ..
                    } => {
                        tool_calls += 1;
                        self.record(
                            id,
                            EventKind::ToolCall,
                            Actor::driver(&driver_name),
                            json!({"tool": tool, "args_digest": args_digest}),
                            now,
                        );
                    }
                    DriverEvent::ToolResult { tool, ok, error } => {
                        self.record(
                            id,
                            EventKind::ToolResult,
                            Actor::driver(&driver_name),
                            json!({"tool": tool, "ok": ok, "error_code": error}),
                            now,
                        );
                    }
                    DriverEvent::PermissionRequest {
                        id: approval_id,
                        tool,
                        ..
                    } => {
                        self.record(
                            id,
                            EventKind::ApprovalRequested,
                            Actor::driver(&driver_name),
                            json!({"approval_id": approval_id, "action": tool}),
                            now,
                        );
                        if let Some(task) = self.tasks.get_mut(id) {
                            let _ = task.transition(State::WaitingApproval, None);
                            if task.record_approval().is_err() {
                                stop_reason = Some(Dimension::Approvals);
                                break 'outer;
                            }
                        }
                    }
                    DriverEvent::Text { text, .. } => final_text = Some(text.clone()),
                    DriverEvent::Checkpoint { .. } => {}
                    DriverEvent::Done {
                        status: s, reason, ..
                    } => {
                        status = *s;
                        if reason.is_some()
                            && let Some(task) = self.tasks.get_mut(id)
                        {
                            task.reason.clone_from(reason);
                        }
                        break 'outer;
                    }
                }
            }
        }

        let (state, reason) = match (stop_reason, status) {
            (Some(dimension), _) => (State::Failed, Some(dimension.explain().to_owned())),
            (None, RunStatus::Ok) => (State::Done, None),
            (None, RunStatus::Cancelled) => (State::Cancelled, Some("annulée".to_owned())),
            (None, RunStatus::Failed) => (
                State::Failed,
                self.tasks.get(id).and_then(|t| t.reason.clone()),
            ),
        };

        let report = {
            let task = self
                .tasks
                .get_mut(id)
                .ok_or_else(|| RuntimeError::Unknown(id.to_owned()))?;
            if task.state == State::WaitingApproval {
                let _ = task.transition(State::Running, None);
            }
            task.transition(state, reason.clone())?;
            Report {
                task: id.to_owned(),
                state,
                driver: driver_name.clone(),
                steps: task.budget.spent.steps,
                tool_calls,
                approvals: task.budget.spent.approvals,
                tokens: task.budget.spent.tokens,
                final_text,
                reason,
            }
        };

        let kind = match state {
            State::Done => EventKind::TaskDone,
            State::Cancelled => EventKind::TaskCancelled,
            _ => EventKind::TaskFailed,
        };
        self.record(
            id,
            kind,
            Actor::daemon("agentd"),
            json!({
                "stats": {
                    "steps": report.steps,
                    "tool_calls": report.tool_calls,
                    "approvals": report.approvals,
                    "tokens": report.tokens
                },
                "reason": report.reason
            }),
            now,
        );
        self.record(
            id,
            EventKind::ProviderStopped,
            Actor::driver(&driver_name),
            json!({"driver": driver_name}),
            now,
        );
        Ok(report)
    }
}

fn digest(text: &str) -> String {
    format!("blake3:{}", blake3::hash(text.as_bytes()).to_hex())
}
