//! De l'état du système à ce qu'on en montre.
//!
//! C'est ici que la surface cesse d'être une maquette. Le reste du crate sait dessiner une scène ;
//! ce module sait d'où elle vient.
//!
//! Deux traductions y sont décidées, et elles méritent d'être écrites plutôt que devinées.
//!
//! **Le budget se lit par sa dimension la plus entamée**, jamais par une moyenne. Un agent qui a
//! consommé 5 % de ses jetons mais 98 % de son temps est à bout : moyenner donnerait 51 % et
//! montrerait un courant vif juste avant qu'il ne s'arrête. On prend donc le maximum.
//!
//! **Une seule décision est montrée à la fois.** S'il y en a plusieurs, c'est la plus ancienne :
//! faire patienter quelqu'un est déjà désagréable, le faire patienter en changeant d'avis sur ce
//! qu'on lui demande serait pire.

use agentd::budget::Budget;
use agentd::task::{State, Task};
use capd::Approval;
use time::OffsetDateTime;

use crate::scene::{Courant, Decision, Etat, Isolation, Scene};

/// Part du budget consommée, prise sur la dimension la plus entamée.
#[must_use]
pub fn budget_consomme(budget: &Budget) -> f32 {
    let part = |consomme: f64, limite: f64| {
        if limite > 0.0 { consomme / limite } else { 0.0 }
    };
    let l = &budget.limits;
    let d = &budget.spent;
    let parts = [
        part(d.tokens as f64, l.tokens as f64),
        part(d.wall_time_s as f64, l.wall_time_s as f64),
        part(f64::from(d.steps), f64::from(l.steps)),
        part(f64::from(d.approvals), f64::from(l.approvals)),
        part(d.cost_eur, l.cost_eur),
        d.quota_pct / 100.0,
    ];
    let pire = parts.into_iter().fold(0.0_f64, f64::max);
    pire.clamp(0.0, 1.0) as f32
}

/// L'état d'une tâche, traduit en ce que le champ sait montrer.
#[must_use]
pub const fn etat(state: State) -> Etat {
    match state {
        State::Running => Etat::Court,
        State::WaitingApproval => Etat::Attend,
        // Une tâche en pause ou planifiée n'avance pas davantage qu'une tâche empêchée ; le champ
        // la fige pareillement, parce que c'est ce que la personne devant l'écran constate.
        State::Paused | State::Pending | State::Planned | State::Failed => Etat::Bloque,
        // Une tâche annulée après coup est finie, elle aussi : ce qu'elle avait fait a été
        // défait, et il n'y a plus rien à surveiller.
        State::Done | State::Cancelled | State::RolledBack => Etat::Fini,
    }
}

/// Débit d'une tâche, en étapes par minute.
///
/// Il se déduit des étapes franchies et du temps écoulé. Une tâche qui vient de naître n'a pas
/// encore de débit mesurable : on lui en prête un, faible, plutôt que zéro — un zéro la figerait,
/// et figer veut dire « empêchée » dans ce langage.
#[must_use]
pub fn debit(budget: &Budget) -> f32 {
    let secondes = budget.spent.wall_time_s;
    if secondes < 5 {
        return 6.0;
    }
    f32::from(u16::try_from(budget.spent.steps).unwrap_or(u16::MAX)) * 60.0 / secondes as f32
}

/// Construit la scène à montrer.
///
/// Les tâches terminées sont écartées : leur courant s'éteindrait de toute façon, et elles
/// prendraient la place de ce qui travaille. Ce qui est fini n'a plus besoin d'être surveillé.
#[must_use]
pub fn scene(
    taches: &[Task],
    approbations: &[Approval],
    niveau_max: u8,
    manque: Option<String>,
    heure: String,
    date: String,
    maintenant: OffsetDateTime,
) -> Scene {
    let courants = taches
        .iter()
        .filter(|t| !matches!(t.state, State::Done | State::Cancelled | State::RolledBack))
        .map(|t| Courant {
            tache: t.id.clone(),
            intitule: t.intent.clone(),
            agent: t.driver.clone().unwrap_or_else(|| t.agent.clone()),
            etat: etat(t.state),
            debit: debit(&t.budget),
            budget_consomme: budget_consomme(&t.budget),
            etapes: t.budget.spent.steps,
        })
        .collect();

    // La plus ancienne, comme annoncé en tête de module : celle qui attend depuis le plus
    // longtemps passe avant celle qui vient d'arriver.
    let decision = approbations
        .iter()
        .min_by_key(|a| a.created)
        .map(|a| Decision {
            question: a.summary.clone(),
            consequence: consequence(a),
            tache: a.task.clone(),
            depuis_secondes: (maintenant - a.created)
                .whole_seconds()
                .max(0)
                .unsigned_abs(),
            irreversible: a.irreversible,
        });

    let mut scene = Scene {
        heure,
        date,
        courants,
        decision,
        isolation: Isolation { niveau_max, manque },
    };
    scene.ordonner();
    scene
}

/// Ce qui arrivera si l'on accepte, dit en conséquence et non en mécanisme.
///
/// Une demande d'approbation porte une action et une cible, qui sont le vocabulaire du système.
/// La personne devant l'écran n'a pas à le parler : on lui dit ce que ça fait.
fn consequence(approbation: &Approval) -> String {
    if approbation.irreversible {
        format!(
            "{} sur {}. Cette action ne peut pas être annulée.",
            approbation.action, approbation.target
        )
    } else {
        format!(
            "{} sur {}. Réversible depuis l'historique de la tâche.",
            approbation.action, approbation.target
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentd::budget::{Limits, Spent};

    fn approbation(id: &str, action: &str, irreversible: bool, age_s: i64) -> Approval {
        let creee = OffsetDateTime::now_utc() - time::Duration::seconds(age_s);
        Approval {
            id: id.to_owned(),
            task: "t1".to_owned(),
            agent: "codex".to_owned(),
            action: action.to_owned(),
            target: "SNCF Connect".to_owned(),
            summary: "Payer le billet ?".to_owned(),
            irreversible,
            external: true,
            created: creee,
            expires: creee + time::Duration::minutes(10),
            state: capd::ApprovalState::Pending,
        }
    }

    fn budget(limites: Limits, depense: Spent) -> Budget {
        Budget {
            limits: limites,
            spent: depense,
        }
    }

    fn limites() -> Limits {
        Limits {
            tokens: 100_000,
            wall_time_s: 600,
            steps: 200,
            approvals: 10,
            cost_eur: 5.0,
        }
    }

    fn rien() -> Spent {
        Spent {
            tokens: 0,
            wall_time_s: 0,
            steps: 0,
            approvals: 0,
            cost_eur: 0.0,
            quota_pct: 0.0,
        }
    }

    #[test]
    fn le_budget_se_lit_par_sa_dimension_la_plus_entamee() {
        // Le cas qui justifie la règle : presque aucun jeton consommé, mais le temps est écoulé.
        // Une moyenne donnerait un courant vif juste avant l'arrêt.
        let depense = Spent {
            tokens: 5_000,
            wall_time_s: 588,
            ..rien()
        };
        let part = budget_consomme(&budget(limites(), depense));
        assert!(
            part > 0.9,
            "la dimension la plus entamée vaut 98 %, obtenu {part}"
        );
    }

    #[test]
    fn une_limite_nulle_ne_divise_pas_par_zero() {
        let sans_limite = Limits {
            tokens: 0,
            wall_time_s: 0,
            steps: 0,
            approvals: 0,
            cost_eur: 0.0,
        };
        let part = budget_consomme(&budget(sans_limite, rien()));
        assert!(part.is_finite() && part >= 0.0);
    }

    #[test]
    fn le_quota_d_abonnement_compte_comme_les_autres() {
        // Un abonnement épuisé arrête la tâche aussi sûrement qu'un budget de jetons.
        let depense = Spent {
            quota_pct: 97.0,
            ..rien()
        };
        let part = budget_consomme(&budget(limites(), depense));
        assert!(part > 0.9, "obtenu {part}");
    }

    #[test]
    fn une_tache_en_pause_se_fige_comme_une_tache_empechee() {
        // Du point de vue de qui regarde, les deux ne bougent pas. Le langage du champ n'a pas à
        // distinguer ce que l'œil ne distingue pas.
        assert_eq!(etat(State::Paused), Etat::Bloque);
        assert_eq!(etat(State::Pending), Etat::Bloque);
        assert_eq!(etat(State::Running), Etat::Court);
        assert_eq!(etat(State::WaitingApproval), Etat::Attend);
        assert_eq!(etat(State::Done), Etat::Fini);
    }

    #[test]
    fn une_tache_qui_vient_de_naitre_n_est_pas_figee() {
        // Zéro étape en zéro seconde donnerait un débit nul, donc un courant immobile, donc
        // « empêchée » — ce qui serait faux et alarmant.
        let jeune = budget(limites(), Spent { ..rien() });
        assert!(debit(&jeune) > 0.0);
    }

    #[test]
    fn le_debit_se_mesure_en_etapes_par_minute() {
        let depense = Spent {
            steps: 30,
            wall_time_s: 60,
            ..rien()
        };
        let mesure = debit(&budget(limites(), depense));
        assert!((mesure - 30.0).abs() < 0.01, "obtenu {mesure}");
    }

    #[test]
    fn une_consequence_irreversible_le_dit() {
        let approbation = approbation("a1", "Envoyer un paiement de 87,40 €", true, 0);
        let dit = consequence(&approbation);
        assert!(dit.contains("ne peut pas être annulée"), "{dit}");
        assert!(dit.contains("SNCF Connect"), "{dit}");
    }

    #[test]
    fn c_est_la_plus_ancienne_qui_est_montree() {
        // Faire patienter quelqu'un est deja desagreable ; changer d'avis sur ce qu'on lui demande
        // pendant qu'il patiente le serait davantage.
        let maintenant = OffsetDateTime::now_utc();
        let approbations = [
            approbation("recente", "Envoyer un message", false, 5),
            approbation("ancienne", "Payer", true, 300),
            approbation("moyenne", "Supprimer", true, 60),
        ];
        let scene = scene(
            &[],
            &approbations,
            1,
            None,
            "14:37".to_owned(),
            "jeudi".to_owned(),
            maintenant,
        );
        let decision = scene.decision.expect("une décision attend");
        assert!(
            decision.consequence.starts_with("Payer"),
            "obtenu : {}",
            decision.consequence
        );
        assert!(
            decision.depuis_secondes >= 299,
            "l'attente doit être dite telle qu'elle est, obtenu {}",
            decision.depuis_secondes
        );
    }

    #[test]
    fn sans_approbation_il_n_y_a_rien_a_trancher() {
        let scene = scene(
            &[],
            &[],
            2,
            None,
            "14:37".to_owned(),
            "jeudi".to_owned(),
            OffsetDateTime::now_utc(),
        );
        assert!(scene.decision.is_none());
        assert_eq!(scene.attenuation_du_champ(), 1.0);
    }
}
