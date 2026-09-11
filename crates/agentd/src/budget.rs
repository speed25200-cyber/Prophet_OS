//! Budgets d'une tâche.
//!
//! Une tâche agentique sans borne est un risque : elle peut boucler, coûter, ou insister là où un
//! humain se serait arrêté. Les budgets sont donc multidimensionnels et vérifiés à chaque étape.
//!
//! Les abonnements ne se comptent pas en argent mais en **fenêtres d'usage** : c'est une dimension
//! à part, avec sa propre politique d'épuisement.

use serde::{Deserialize, Serialize};

/// Dimension épuisée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    /// Tokens de modèle.
    Tokens,
    /// Temps mur.
    WallTime,
    /// Nombre d'étapes.
    Steps,
    /// Nombre d'approbations demandées à l'humain.
    Approvals,
    /// Coût monétaire, pour les fournisseurs facturés.
    Cost,
    /// Fenêtre de quota d'un abonnement.
    Quota,
}

impl Dimension {
    /// Explication destinée à l'humain.
    #[must_use]
    pub const fn explain(self) -> &'static str {
        match self {
            Self::Tokens => "le plafond de tokens de la tâche est atteint",
            Self::WallTime => "la tâche a dépassé sa durée maximale",
            Self::Steps => "la tâche a atteint son nombre maximal d'étapes",
            Self::Approvals => "la tâche a déjà demandé le nombre maximal d'approbations",
            Self::Cost => "le plafond de coût de la tâche est atteint",
            Self::Quota => "la fenêtre d'usage de l'abonnement est épuisée",
        }
    }
}

/// Plafonds d'une tâche.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    /// Tokens cumulés.
    pub tokens: u64,
    /// Durée en secondes.
    pub wall_time_s: u64,
    /// Étapes.
    pub steps: u32,
    /// Approbations.
    pub approvals: u32,
    /// Coût en euros.
    pub cost_eur: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            tokens: 200_000,
            wall_time_s: 1_200,
            steps: 200,
            approvals: 5,
            cost_eur: 0.0,
        }
    }
}

/// Consommation observée.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Spent {
    /// Tokens cumulés.
    pub tokens: u64,
    /// Secondes écoulées.
    pub wall_time_s: u64,
    /// Étapes effectuées.
    pub steps: u32,
    /// Approbations demandées.
    pub approvals: u32,
    /// Coût cumulé.
    pub cost_eur: f64,
    /// Part de la fenêtre de quota consommée, de 0 à 100.
    pub quota_pct: f64,
}

/// Budget d'une tâche : plafonds, consommation, et arithmétique de partage.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Budget {
    /// Plafonds.
    pub limits: Limits,
    /// Consommation.
    pub spent: Spent,
}

impl Budget {
    /// Budget neuf.
    #[must_use]
    pub const fn new(limits: Limits) -> Self {
        Self {
            limits,
            spent: Spent {
                tokens: 0,
                wall_time_s: 0,
                steps: 0,
                approvals: 0,
                cost_eur: 0.0,
                quota_pct: 0.0,
            },
        }
    }

    /// Première dimension épuisée, s'il y en a une.
    #[must_use]
    pub fn exhausted(&self) -> Option<Dimension> {
        if self.spent.tokens >= self.limits.tokens {
            return Some(Dimension::Tokens);
        }
        if self.spent.wall_time_s >= self.limits.wall_time_s {
            return Some(Dimension::WallTime);
        }
        if self.spent.steps >= self.limits.steps {
            return Some(Dimension::Steps);
        }
        if self.spent.approvals >= self.limits.approvals {
            return Some(Dimension::Approvals);
        }
        if self.limits.cost_eur > 0.0 && self.spent.cost_eur >= self.limits.cost_eur {
            return Some(Dimension::Cost);
        }
        if self.spent.quota_pct >= 100.0 {
            return Some(Dimension::Quota);
        }
        None
    }

    /// Vrai si la tâche peut faire une étape de plus.
    #[must_use]
    pub fn can_continue(&self) -> bool {
        self.exhausted().is_none()
    }

    /// Dimensions au-delà de 80 %, à signaler avant qu'il ne soit trop tard.
    #[must_use]
    pub fn warnings(&self) -> Vec<Dimension> {
        let mut out = Vec::new();
        let ratio = |spent: f64, limit: f64| limit > 0.0 && spent / limit >= 0.8;
        if ratio(self.spent.tokens as f64, self.limits.tokens as f64) {
            out.push(Dimension::Tokens);
        }
        if ratio(
            self.spent.wall_time_s as f64,
            self.limits.wall_time_s as f64,
        ) {
            out.push(Dimension::WallTime);
        }
        if ratio(f64::from(self.spent.steps), f64::from(self.limits.steps)) {
            out.push(Dimension::Steps);
        }
        if self.spent.quota_pct >= 80.0 {
            out.push(Dimension::Quota);
        }
        out.retain(|d| self.exhausted() != Some(*d));
        out
    }

    /// Réserve une part du budget pour une sous-tâche.
    ///
    /// Le budget d'un enfant est **prélevé** sur celui du parent, jamais ajouté : une hiérarchie
    /// de tâches ne peut pas dépenser plus que sa racine.
    #[must_use]
    pub fn reserve(&self, fraction: f64) -> Self {
        let fraction = fraction.clamp(0.0, 1.0);
        let remaining_tokens = self.limits.tokens.saturating_sub(self.spent.tokens);
        let remaining_time = self
            .limits
            .wall_time_s
            .saturating_sub(self.spent.wall_time_s);
        let remaining_steps = self.limits.steps.saturating_sub(self.spent.steps);
        let remaining_approvals = self.limits.approvals.saturating_sub(self.spent.approvals);
        Self::new(Limits {
            tokens: (remaining_tokens as f64 * fraction) as u64,
            wall_time_s: (remaining_time as f64 * fraction) as u64,
            steps: (f64::from(remaining_steps) * fraction) as u32,
            approvals: (f64::from(remaining_approvals) * fraction) as u32,
            cost_eur: (self.limits.cost_eur - self.spent.cost_eur).max(0.0) * fraction,
        })
    }

    /// Impute à ce budget la consommation d'une sous-tâche.
    pub fn absorb(&mut self, child: &Self) {
        self.spent.tokens += child.spent.tokens;
        self.spent.steps += child.spent.steps;
        self.spent.approvals += child.spent.approvals;
        self.spent.cost_eur += child.spent.cost_eur;
        self.spent.quota_pct = self.spent.quota_pct.max(child.spent.quota_pct);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget() -> Budget {
        Budget::new(Limits {
            tokens: 1000,
            wall_time_s: 100,
            steps: 10,
            approvals: 3,
            cost_eur: 2.0,
        })
    }

    #[test]
    fn budget_neuf_permet_de_continuer() {
        assert!(budget().can_continue());
        assert_eq!(budget().exhausted(), None);
    }

    #[test]
    fn chaque_dimension_peut_arreter_la_tache() {
        let cas = [
            (
                Spent {
                    tokens: 1000,
                    ..Spent::default()
                },
                Dimension::Tokens,
            ),
            (
                Spent {
                    wall_time_s: 100,
                    ..Spent::default()
                },
                Dimension::WallTime,
            ),
            (
                Spent {
                    steps: 10,
                    ..Spent::default()
                },
                Dimension::Steps,
            ),
            (
                Spent {
                    approvals: 3,
                    ..Spent::default()
                },
                Dimension::Approvals,
            ),
            (
                Spent {
                    cost_eur: 2.0,
                    ..Spent::default()
                },
                Dimension::Cost,
            ),
            (
                Spent {
                    quota_pct: 100.0,
                    ..Spent::default()
                },
                Dimension::Quota,
            ),
        ];
        for (spent, attendu) in cas {
            let mut b = budget();
            b.spent = spent;
            assert_eq!(b.exhausted(), Some(attendu), "{spent:?}");
            assert!(!b.can_continue());
            assert!(!attendu.explain().is_empty());
        }
    }

    #[test]
    fn avertissement_avant_epuisement() {
        let mut b = budget();
        b.spent.tokens = 850;
        assert_eq!(b.warnings(), vec![Dimension::Tokens]);
        assert!(b.can_continue(), "un avertissement n'arrête pas la tâche");

        b.spent.tokens = 1000;
        assert!(
            !b.warnings().contains(&Dimension::Tokens),
            "une dimension épuisée n'est plus un simple avertissement"
        );
    }

    #[test]
    fn quota_averti_a_quatre_vingts_pour_cent() {
        let mut b = budget();
        b.spent.quota_pct = 85.0;
        assert!(b.warnings().contains(&Dimension::Quota));
        assert!(b.can_continue());
    }

    #[test]
    fn une_sous_tache_preleve_sur_le_parent() {
        let mut parent = budget();
        parent.spent.tokens = 200;
        let enfant = parent.reserve(0.5);
        assert_eq!(enfant.limits.tokens, 400, "moitié des 800 restants");
        assert_eq!(enfant.limits.steps, 5);
        assert!(
            enfant.limits.tokens < parent.limits.tokens,
            "un enfant ne peut jamais recevoir plus que le parent"
        );
    }

    #[test]
    fn une_fraction_hors_bornes_est_ramenee_dans_les_bornes() {
        let parent = budget();
        assert_eq!(parent.reserve(5.0).limits.tokens, parent.limits.tokens);
        assert_eq!(parent.reserve(-1.0).limits.tokens, 0);
    }

    #[test]
    fn la_consommation_de_l_enfant_remonte_au_parent() {
        let mut parent = budget();
        let mut enfant = parent.reserve(0.5);
        enfant.spent.tokens = 300;
        enfant.spent.steps = 2;
        enfant.spent.quota_pct = 40.0;
        parent.absorb(&enfant);
        assert_eq!(parent.spent.tokens, 300);
        assert_eq!(parent.spent.steps, 2);
        assert_eq!(parent.spent.quota_pct, 40.0);
    }

    #[test]
    fn une_hierarchie_ne_depasse_pas_la_racine() {
        let mut racine = budget();
        let mut total = 0;
        for _ in 0..5 {
            let enfant = racine.reserve(0.5);
            total += enfant.limits.tokens;
            let mut consomme = enfant;
            consomme.spent.tokens = enfant.limits.tokens;
            racine.absorb(&consomme);
        }
        assert!(
            total <= budget().limits.tokens,
            "la somme des sous-budgets ({total}) dépasse la racine"
        );
    }

    #[test]
    fn cout_nul_signifie_pas_de_plafond_monetaire() {
        let mut b = Budget::new(Limits {
            cost_eur: 0.0,
            ..budget().limits
        });
        b.spent.cost_eur = 100.0;
        assert_ne!(b.exhausted(), Some(Dimension::Cost));
    }
}
