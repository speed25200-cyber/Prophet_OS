//! Cycle de vie d'une tâche.
//!
//! Une tâche est un objet de première classe de Prophet OS, au même titre qu'un processus pour un
//! système classique : elle a une identité, des droits, un budget, un espace de travail, un
//! journal, et un état dont les transitions sont explicites.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::budget::{Budget, Dimension};

/// État d'une tâche.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Créée, pas encore planifiée.
    Pending,
    /// Planifiée : pilote, sandbox et capacités choisis.
    Planned,
    /// En cours.
    Running,
    /// Suspendue en attente d'une décision humaine.
    WaitingApproval,
    /// Gelée à la demande.
    Paused,
    /// Terminée avec succès.
    Done,
    /// Échouée.
    Failed,
    /// Annulée.
    Cancelled,
    /// Annulée après validation.
    RolledBack,
}

impl State {
    /// Vrai si l'état est définitif.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Done | Self::Failed | Self::Cancelled | Self::RolledBack
        )
    }

    /// Transitions autorisées depuis cet état.
    #[must_use]
    pub fn allowed_next(self) -> &'static [Self] {
        match self {
            Self::Pending => &[Self::Planned, Self::Cancelled, Self::Failed],
            Self::Planned => &[Self::Running, Self::Cancelled, Self::Failed],
            Self::Running => &[
                Self::WaitingApproval,
                Self::Paused,
                Self::Done,
                Self::Failed,
                Self::Cancelled,
            ],
            Self::WaitingApproval => &[Self::Running, Self::Cancelled, Self::Failed],
            Self::Paused => &[Self::Running, Self::Cancelled],
            // Une tâche validée reste annulable : c'est la promesse de réversibilité.
            Self::Done => &[Self::RolledBack],
            Self::Failed | Self::Cancelled | Self::RolledBack => &[],
        }
    }

    /// Vrai si la transition est permise.
    #[must_use]
    pub fn can_move_to(self, next: Self) -> bool {
        self.allowed_next().contains(&next)
    }
}

/// Erreur de cycle de vie.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TaskError {
    /// Transition interdite.
    #[error("transition interdite : {from:?} vers {to:?}")]
    BadTransition {
        /// État de départ.
        from: State,
        /// État visé.
        to: State,
    },
    /// Budget épuisé.
    #[error("budget épuisé : {}", .0.explain())]
    BudgetExhausted(Dimension),
    /// Profondeur de hiérarchie dépassée.
    #[error("profondeur maximale de sous-tâches atteinte ({0})")]
    DepthExceeded(u32),
}

/// Une tâche.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    /// Identifiant.
    pub id: String,
    /// Intention exprimée.
    pub intent: String,
    /// Agent chargé.
    pub agent: String,
    /// Utilisateur pour le compte duquel elle s'exécute.
    pub user: String,
    /// Tâche parente, le cas échéant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Profondeur dans la hiérarchie.
    pub depth: u32,
    /// État courant.
    pub state: State,
    /// Budget.
    pub budget: Budget,
    /// Pilote retenu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
    /// Niveau de sandbox appliqué.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_level: Option<u8>,
    /// Date de création.
    #[serde(with = "time::serde::rfc3339")]
    pub created: OffsetDateTime,
    /// Raison de l'état terminal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Historique des états traversés.
    pub history: Vec<State>,
}

/// Profondeur maximale de sous-tâches.
pub const MAX_DEPTH: u32 = 3;

impl Task {
    /// Crée une tâche racine.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        intent: impl Into<String>,
        agent: impl Into<String>,
        user: impl Into<String>,
        budget: Budget,
        created: OffsetDateTime,
    ) -> Self {
        Self {
            id: id.into(),
            intent: intent.into(),
            agent: agent.into(),
            user: user.into(),
            parent: None,
            depth: 0,
            state: State::Pending,
            budget,
            driver: None,
            sandbox_level: None,
            created,
            reason: None,
            history: vec![State::Pending],
        }
    }

    /// Crée une sous-tâche, avec une part du budget du parent.
    ///
    /// # Errors
    /// [`TaskError::DepthExceeded`] au-delà de [`MAX_DEPTH`].
    pub fn spawn_child(
        &self,
        id: impl Into<String>,
        intent: impl Into<String>,
        fraction: f64,
        created: OffsetDateTime,
    ) -> Result<Self, TaskError> {
        if self.depth + 1 > MAX_DEPTH {
            return Err(TaskError::DepthExceeded(MAX_DEPTH));
        }
        let mut child = Self::new(
            id,
            intent,
            self.agent.clone(),
            self.user.clone(),
            self.budget.reserve(fraction),
            created,
        );
        child.parent = Some(self.id.clone());
        child.depth = self.depth + 1;
        Ok(child)
    }

    /// Change d'état.
    ///
    /// # Errors
    /// [`TaskError::BadTransition`] si la transition n'est pas permise.
    pub fn transition(&mut self, next: State, reason: Option<String>) -> Result<(), TaskError> {
        if !self.state.can_move_to(next) {
            return Err(TaskError::BadTransition {
                from: self.state,
                to: next,
            });
        }
        self.state = next;
        self.history.push(next);
        if next.is_terminal() {
            self.reason = reason;
        }
        Ok(())
    }

    /// Enregistre une étape et vérifie que le budget le permet.
    ///
    /// # Errors
    /// [`TaskError::BudgetExhausted`] si une dimension est épuisée.
    pub fn record_step(
        &mut self,
        tokens: u64,
        elapsed_s: u64,
        cost_eur: f64,
    ) -> Result<(), TaskError> {
        self.budget.spent.steps += 1;
        self.budget.spent.tokens += tokens;
        self.budget.spent.wall_time_s = elapsed_s;
        self.budget.spent.cost_eur += cost_eur;
        self.budget
            .exhausted()
            .map_or(Ok(()), |d| Err(TaskError::BudgetExhausted(d)))
    }

    /// Enregistre une demande d'approbation.
    ///
    /// # Errors
    /// Si le nombre d'approbations autorisées est atteint.
    pub fn record_approval(&mut self) -> Result<(), TaskError> {
        self.budget.spent.approvals += 1;
        if self.budget.spent.approvals >= self.budget.limits.approvals {
            return Err(TaskError::BudgetExhausted(Dimension::Approvals));
        }
        Ok(())
    }

    /// Résumé d'une ligne, pour la liste des tâches.
    #[must_use]
    pub fn summary(&self) -> String {
        let indent = "  ".repeat(self.depth as usize);
        format!(
            "{indent}{} [{:?}] {} · {} étapes, {} tokens{}",
            self.id,
            self.state,
            truncate(&self.intent, 50),
            self.budget.spent.steps,
            self.budget.spent.tokens,
            self.driver
                .as_ref()
                .map(|d| format!(" · {d}"))
                .unwrap_or_default()
        )
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let cut: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::Limits;

    fn now() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_789_000_000).unwrap()
    }

    fn tache() -> Task {
        Task::new(
            "task:01",
            "prépare le rapport de ventes",
            "org.test.agent",
            "u",
            Budget::new(Limits::default()),
            now(),
        )
    }

    #[test]
    fn parcours_nominal() {
        let mut t = tache();
        t.transition(State::Planned, None).unwrap();
        t.transition(State::Running, None).unwrap();
        t.transition(State::Done, None).unwrap();
        assert!(t.state.is_terminal());
        assert_eq!(
            t.history,
            vec![State::Pending, State::Planned, State::Running, State::Done]
        );
    }

    #[test]
    fn une_tache_validee_reste_annulable() {
        let mut t = tache();
        t.transition(State::Planned, None).unwrap();
        t.transition(State::Running, None).unwrap();
        t.transition(State::Done, None).unwrap();
        t.transition(State::RolledBack, Some("demande de l'utilisateur".into()))
            .unwrap();
        assert_eq!(t.state, State::RolledBack);
    }

    #[test]
    fn un_etat_definitif_ne_redemarre_pas() {
        for etat in [State::Failed, State::Cancelled, State::RolledBack] {
            let mut t = tache();
            t.state = etat;
            assert_eq!(
                t.transition(State::Running, None),
                Err(TaskError::BadTransition {
                    from: etat,
                    to: State::Running
                })
            );
        }
    }

    #[test]
    fn toutes_les_transitions_sont_explicites() {
        // Aucune transition ne doit être possible « par hasard » : on vérifie la table entière.
        let etats = [
            State::Pending,
            State::Planned,
            State::Running,
            State::WaitingApproval,
            State::Paused,
            State::Done,
            State::Failed,
            State::Cancelled,
            State::RolledBack,
        ];
        for from in etats {
            for to in etats {
                let mut t = tache();
                t.state = from;
                let permis = from.can_move_to(to);
                let resultat = t.transition(to, None);
                assert_eq!(
                    resultat.is_ok(),
                    permis,
                    "transition {from:?} vers {to:?} incohérente"
                );
            }
        }
    }

    #[test]
    fn un_etat_terminal_n_a_aucune_suite_sauf_l_annulation_d_une_reussite() {
        assert!(State::Done.allowed_next().contains(&State::RolledBack));
        assert!(State::Failed.allowed_next().is_empty());
        assert!(State::Cancelled.allowed_next().is_empty());
        assert!(State::RolledBack.allowed_next().is_empty());
    }

    #[test]
    fn attente_d_approbation_et_reprise() {
        let mut t = tache();
        t.transition(State::Planned, None).unwrap();
        t.transition(State::Running, None).unwrap();
        t.transition(State::WaitingApproval, None).unwrap();
        t.transition(State::Running, None).unwrap();
        assert_eq!(t.state, State::Running);
    }

    #[test]
    fn le_budget_arrete_la_tache() {
        let mut t = Task::new(
            "task:01",
            "boucle",
            "a",
            "u",
            Budget::new(Limits {
                steps: 3,
                ..Limits::default()
            }),
            now(),
        );
        t.record_step(10, 1, 0.0).unwrap();
        t.record_step(10, 2, 0.0).unwrap();
        let err = t.record_step(10, 3, 0.0).unwrap_err();
        assert_eq!(err, TaskError::BudgetExhausted(Dimension::Steps));
    }

    #[test]
    fn hierarchie_bornee() {
        let racine = tache();
        let mut courante = racine;
        for niveau in 1..=MAX_DEPTH {
            courante = courante
                .spawn_child(format!("task:sub{niveau}"), "sous-tâche", 0.5, now())
                .unwrap();
            assert_eq!(courante.depth, niveau);
        }
        assert_eq!(
            courante
                .spawn_child("task:trop", "x", 0.5, now())
                .unwrap_err(),
            TaskError::DepthExceeded(MAX_DEPTH)
        );
    }

    #[test]
    fn une_sous_tache_herite_de_l_agent_et_de_l_utilisateur() {
        let racine = tache();
        let enfant = racine.spawn_child("task:02", "sous", 0.5, now()).unwrap();
        assert_eq!(enfant.agent, racine.agent);
        assert_eq!(enfant.user, racine.user);
        assert_eq!(enfant.parent.as_deref(), Some("task:01"));
        assert!(enfant.budget.limits.tokens < racine.budget.limits.tokens);
    }

    #[test]
    fn resume_lisible_et_indente() {
        let racine = tache();
        let enfant = racine
            .spawn_child("task:02", "sous-tâche", 0.5, now())
            .unwrap();
        assert!(racine.summary().starts_with("task:01"));
        assert!(enfant.summary().starts_with("  task:02"));
        let long = Task::new(
            "task:03",
            "x".repeat(200),
            "a",
            "u",
            Budget::new(Limits::default()),
            now(),
        );
        assert!(long.summary().contains('…'));
    }

    #[test]
    fn serialisation_aller_retour() {
        let t = tache();
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<Task>(&json).unwrap(), t);
    }
}
