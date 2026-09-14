//! File d'approbations : les actions irréversibles ou externes attendent une décision humaine.
//!
//! Une décision peut porter au-delà de la demande qui l'a provoquée : `once` ne vaut que pour
//! elle, `task` pour toute la tâche, `agent` pour cet agent pendant une durée. Toute règle ainsi
//! créée est révocable.

use std::collections::HashMap;

use prophet_types::ids::{Id, Kind};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Portée d'une décision humaine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalScope {
    /// Cette demande seulement.
    Once,
    /// Toute la tâche en cours.
    Task,
    /// Cet agent, pour la durée indiquée.
    Agent {
        /// Durée de validité en jours.
        days: u16,
    },
}

/// Décision humaine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// Autorisée.
    Allow,
    /// Refusée.
    Deny,
}

/// État d'une demande.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    /// En attente d'un humain.
    Pending,
    /// Tranchée.
    Resolved {
        /// Décision rendue.
        decision: Decision,
    },
    /// Expirée sans réponse.
    Expired,
}

/// Demande d'approbation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Approval {
    /// Identifiant.
    pub id: String,
    /// Tâche demandeuse.
    pub task: String,
    /// Agent demandeur.
    pub agent: String,
    /// Action concernée, sous la forme `<res>.<act>`.
    pub action: String,
    /// Cible.
    pub target: String,
    /// Résumé lisible, destiné à l'humain qui décide en cinq secondes.
    pub summary: String,
    /// Le motif que le modèle a donné, s'il en a donné un : pourquoi il veut cette action, en
    /// une phrase, pour l'humain qui tranche (ADR 0041). Il vient du modèle : c'est un dire,
    /// pas une preuve, et la surface le montre comme tel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// L'action est-elle irréversible ?
    pub irreversible: bool,
    /// A-t-elle un effet hors de la machine ?
    pub external: bool,
    /// Date de création.
    #[serde(with = "time::serde::rfc3339")]
    pub created: OffsetDateTime,
    /// Date d'expiration si personne ne répond.
    #[serde(with = "time::serde::rfc3339")]
    pub expires: OffsetDateTime,
    /// État courant.
    pub state: ApprovalState,
}

/// Règle permanente issue d'une décision de portée `task` ou `agent`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StandingRule {
    /// Identifiant de la règle.
    pub id: String,
    /// Décision appliquée.
    pub decision: Decision,
    /// Action couverte.
    pub action: String,
    /// Cible couverte.
    pub target: String,
    /// Tâche couverte, pour une portée `task`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// Agent couvert, pour une portée `agent`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Expiration de la règle.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "time::serde::rfc3339::option"
    )]
    pub expires: Option<OffsetDateTime>,
}

impl StandingRule {
    /// Vrai si la règle couvre la demande, à l'instant `now`.
    #[must_use]
    pub fn covers(&self, approval: &Approval, now: OffsetDateTime) -> bool {
        if self.expires.is_some_and(|e| now >= e) {
            return false;
        }
        if self.action != approval.action || self.target != approval.target {
            return false;
        }
        match (&self.task, &self.agent) {
            (Some(task), _) => task == &approval.task,
            (None, Some(agent)) => agent == &approval.agent,
            (None, None) => false,
        }
    }
}

/// Durée de vie par défaut d'une demande sans réponse.
pub const DEFAULT_TTL_HOURS: i64 = 24;
/// Durée pendant laquelle une demande tranchée reste lisible (`status`) et, pour une décision
/// « une fois », attend la demande identique qui la consommera.
pub const RESOLVED_TTL_HOURS: i64 = 1;
/// Longueur maximale du motif d'un modèle : une phrase, pas un plaidoyer.
pub const REASON_MAX_CHARS: usize = 400;

/// File d'approbations et règles permanentes.
#[derive(Debug, Default)]
pub struct Approvals {
    pending: HashMap<String, Approval>,
    rules: Vec<StandingRule>,
    /// Les demandes tranchées, gardées un temps pour que celui qui attend lise la décision.
    resolved: HashMap<String, Approval>,
    /// Les décisions « une fois » pas encore consommées : la prochaine demande identique (même
    /// tâche, même action, même cible) les prend et ne repasse pas devant l'humain.
    once: HashMap<String, Approval>,
}

/// Ce qui décrit une demande d'approbation à créer.
#[derive(Debug, Clone)]
pub struct Request {
    /// Tâche demandeuse.
    pub task: String,
    /// Agent demandeur.
    pub agent: String,
    /// Action `<res>.<act>`.
    pub action: String,
    /// Cible.
    pub target: String,
    /// Résumé lisible.
    pub summary: String,
    /// Irréversible ?
    pub irreversible: bool,
    /// Effet externe ?
    pub external: bool,
}

impl Approvals {
    /// File vide.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Crée une demande, ou la tranche immédiatement si une règle permanente la couvre.
    ///
    /// Retourne la demande dans son état résultant.
    pub fn request(&mut self, request: Request, now: OffsetDateTime) -> Approval {
        let mut approval = Approval {
            id: Id::new(Kind::Approval).to_string(),
            task: request.task,
            agent: request.agent,
            action: request.action,
            target: request.target,
            summary: request.summary,
            reason: None,
            irreversible: request.irreversible,
            external: request.external,
            created: now,
            expires: now + time::Duration::hours(DEFAULT_TTL_HOURS),
            state: ApprovalState::Pending,
        };
        if let Some(rule) = self.rules.iter().find(|r| r.covers(&approval, now)) {
            approval.state = ApprovalState::Resolved {
                decision: rule.decision,
            };
            return approval;
        }
        // Une décision « une fois » rendue pour cette même action vaut pour cette demande, et
        // pour elle seule.
        let consommable = self
            .once
            .iter()
            .filter(|(_, a)| {
                a.task == approval.task
                    && a.action == approval.action
                    && a.target == approval.target
            })
            .min_by_key(|(_, a)| a.created)
            .map(|(id, _)| id.clone());
        if let Some(id) = consommable
            && let Some(rendue) = self.once.remove(&id)
        {
            approval.state = rendue.state;
            return approval;
        }
        self.pending.insert(approval.id.clone(), approval.clone());
        approval
    }

    /// Joint à une demande en attente le motif que le modèle donne — pourquoi il veut cette
    /// action —, borné à une phrase ; rend la demande ainsi complétée. `None` si la demande
    /// est inconnue ou déjà tranchée, ou si le motif est vide : un motif ne se rediscute pas
    /// après coup, et le vide ne dit rien à l'humain.
    pub fn explain(&mut self, id: &str, reason: &str) -> Option<Approval> {
        let motif = reason.trim();
        if motif.is_empty() {
            return None;
        }
        let approval = self.pending.get_mut(id)?;
        let borne: String = motif.chars().take(REASON_MAX_CHARS).collect();
        approval.reason = Some(borne);
        Some(approval.clone())
    }

    /// L'état d'une demande : en attente, ou tranchée récemment.
    #[must_use]
    pub fn status(&self, id: &str) -> Option<Approval> {
        self.pending
            .get(id)
            .or_else(|| self.resolved.get(id))
            .cloned()
    }

    /// Tranche une demande et crée la règle permanente correspondant à la portée.
    ///
    /// # Erreurs
    /// Retourne `None` si la demande est inconnue ou déjà tranchée.
    pub fn resolve(
        &mut self,
        id: &str,
        decision: Decision,
        scope: ApprovalScope,
        now: OffsetDateTime,
    ) -> Option<Approval> {
        let mut approval = self.pending.remove(id)?;
        approval.state = ApprovalState::Resolved { decision };
        self.resolved.insert(id.to_owned(), approval.clone());
        match scope {
            ApprovalScope::Once => {
                self.once.insert(id.to_owned(), approval.clone());
            }
            ApprovalScope::Task => self.rules.push(StandingRule {
                id: Id::new(Kind::Approval).to_string(),
                decision,
                action: approval.action.clone(),
                target: approval.target.clone(),
                task: Some(approval.task.clone()),
                agent: None,
                expires: None,
            }),
            ApprovalScope::Agent { days } => self.rules.push(StandingRule {
                id: Id::new(Kind::Approval).to_string(),
                decision,
                action: approval.action.clone(),
                target: approval.target.clone(),
                task: None,
                agent: Some(approval.agent.clone()),
                expires: Some(now + time::Duration::days(i64::from(days))),
            }),
        }
        Some(approval)
    }

    /// Demandes encore en attente, les plus anciennes d'abord.
    #[must_use]
    pub fn pending(&self) -> Vec<Approval> {
        let mut list: Vec<Approval> = self.pending.values().cloned().collect();
        list.sort_by_key(|a| a.created);
        list
    }

    /// Retire les demandes expirées et les renvoie ; oublie aussi les décisions anciennes.
    pub fn expire(&mut self, now: OffsetDateTime) -> Vec<Approval> {
        let horizon = now - time::Duration::hours(RESOLVED_TTL_HOURS);
        self.resolved.retain(|_, a| a.created > horizon);
        self.once.retain(|_, a| a.created > horizon);
        let expired: Vec<String> = self
            .pending
            .iter()
            .filter(|(_, a)| now >= a.expires)
            .map(|(id, _)| id.clone())
            .collect();
        expired
            .into_iter()
            .filter_map(|id| {
                let mut approval = self.pending.remove(&id)?;
                approval.state = ApprovalState::Expired;
                Some(approval)
            })
            .collect()
    }

    /// Règles permanentes actives.
    #[must_use]
    pub fn rules(&self) -> &[StandingRule] {
        &self.rules
    }

    /// Révoque une règle permanente.
    pub fn revoke_rule(&mut self, id: &str) -> bool {
        let before = self.rules.len();
        self.rules.retain(|r| r.id != id);
        self.rules.len() != before
    }

    /// Retire toutes les règles liées à une tâche terminée.
    pub fn clear_task_rules(&mut self, task: &str) {
        self.rules.retain(|r| r.task.as_deref() != Some(task));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_789_000_000).unwrap()
    }

    fn requete() -> Request {
        Request {
            task: "task:01".into(),
            agent: "org.test.agent".into(),
            action: "tool.call".into(),
            target: "mail.send".into(),
            summary: "Envoyer le rapport à Marie".into(),
            irreversible: true,
            external: true,
        }
    }

    #[test]
    fn demande_puis_decision_ponctuelle() {
        let mut file = Approvals::new();
        let demande = file.request(requete(), now());
        assert_eq!(demande.state, ApprovalState::Pending);
        assert_eq!(file.pending().len(), 1);

        let tranchee = file
            .resolve(&demande.id, Decision::Allow, ApprovalScope::Once, now())
            .unwrap();
        assert_eq!(
            tranchee.state,
            ApprovalState::Resolved {
                decision: Decision::Allow
            }
        );
        assert!(file.pending().is_empty());
        assert!(
            file.rules().is_empty(),
            "une décision ponctuelle ne crée pas de règle"
        );
    }

    #[test]
    fn le_modele_joint_son_motif_a_la_demande_en_attente() {
        let mut file = Approvals::new();
        let demande = file.request(requete(), now());
        assert_eq!(demande.reason, None, "sans motif à la création");

        // Le motif se joint, borné, et se lit avec la demande — en attente comme tranchée.
        let long = "x".repeat(REASON_MAX_CHARS + 50);
        assert!(file.explain(&demande.id, &long).is_some());
        let expliquee = file
            .explain(&demande.id, "  Le rapport doit partir avant midi.  ")
            .expect("la demande attend");
        assert_eq!(
            expliquee.reason.as_deref(),
            Some("Le rapport doit partir avant midi.")
        );
        assert_eq!(
            file.status(&demande.id).unwrap().reason,
            expliquee.reason,
            "le motif se lit par l'état"
        );
        assert!(
            file.explain(&demande.id, "   ").is_none(),
            "le vide ne dit rien"
        );
        assert!(file.explain("inconnue", "motif").is_none());

        let tranchee = file
            .resolve(&demande.id, Decision::Allow, ApprovalScope::Once, now())
            .unwrap();
        assert_eq!(
            tranchee.reason, expliquee.reason,
            "le motif suit la décision"
        );
        assert!(
            file.explain(&demande.id, "trop tard").is_none(),
            "une demande tranchée ne se rediscute pas"
        );
        // Une autre action (la même serait tranchée par la décision « une fois » qui attend) :
        // un motif trop long est coupé, pas refusé.
        let autre = file.request(
            Request {
                target: "mail.send.autre".into(),
                ..requete()
            },
            now(),
        );
        assert_eq!(
            file.explain(&autre.id, &long)
                .unwrap()
                .reason
                .unwrap()
                .chars()
                .count(),
            REASON_MAX_CHARS,
            "un motif trop long est coupé, pas refusé"
        );
    }

    #[test]
    fn portee_tache_couvre_les_demandes_suivantes_de_la_meme_tache() {
        let mut file = Approvals::new();
        let premiere = file.request(requete(), now());
        file.resolve(&premiere.id, Decision::Allow, ApprovalScope::Task, now())
            .unwrap();

        let seconde = file.request(requete(), now());
        assert_eq!(
            seconde.state,
            ApprovalState::Resolved {
                decision: Decision::Allow
            },
            "la règle de tâche doit trancher sans redemander"
        );
        assert!(file.pending().is_empty());

        let autre_tache = Request {
            task: "task:02".into(),
            ..requete()
        };
        assert_eq!(
            file.request(autre_tache, now()).state,
            ApprovalState::Pending
        );
    }

    #[test]
    fn portee_agent_expire() {
        let mut file = Approvals::new();
        let premiere = file.request(requete(), now());
        file.resolve(
            &premiere.id,
            Decision::Allow,
            ApprovalScope::Agent { days: 30 },
            now(),
        )
        .unwrap();

        let plus_tard = now() + time::Duration::days(10);
        let autre_tache = Request {
            task: "task:99".into(),
            ..requete()
        };
        assert!(matches!(
            file.request(autre_tache.clone(), plus_tard).state,
            ApprovalState::Resolved { .. }
        ));

        let bien_plus_tard = now() + time::Duration::days(31);
        assert_eq!(
            file.request(autre_tache, bien_plus_tard).state,
            ApprovalState::Pending,
            "au-delà de 30 jours, la règle ne s'applique plus"
        );
    }

    #[test]
    fn refus_permanent_bloque_sans_redemander() {
        let mut file = Approvals::new();
        let premiere = file.request(requete(), now());
        file.resolve(&premiere.id, Decision::Deny, ApprovalScope::Task, now())
            .unwrap();
        assert_eq!(
            file.request(requete(), now()).state,
            ApprovalState::Resolved {
                decision: Decision::Deny
            }
        );
    }

    #[test]
    fn expiration_apres_vingt_quatre_heures() {
        let mut file = Approvals::new();
        file.request(requete(), now());
        assert!(file.expire(now() + time::Duration::hours(23)).is_empty());
        let expirees = file.expire(now() + time::Duration::hours(25));
        assert_eq!(expirees.len(), 1);
        assert_eq!(expirees[0].state, ApprovalState::Expired);
        assert!(file.pending().is_empty());
    }

    #[test]
    fn revocation_de_regle() {
        let mut file = Approvals::new();
        let premiere = file.request(requete(), now());
        file.resolve(&premiere.id, Decision::Allow, ApprovalScope::Task, now())
            .unwrap();
        let id = file.rules()[0].id.clone();
        assert!(file.revoke_rule(&id));
        assert!(!file.revoke_rule(&id));
        assert_eq!(file.request(requete(), now()).state, ApprovalState::Pending);
    }

    #[test]
    fn regles_de_tache_nettoyees_a_la_fin() {
        let mut file = Approvals::new();
        let premiere = file.request(requete(), now());
        file.resolve(&premiere.id, Decision::Allow, ApprovalScope::Task, now())
            .unwrap();
        file.clear_task_rules("task:01");
        assert!(file.rules().is_empty());
    }

    #[test]
    fn decision_sur_demande_inconnue() {
        let mut file = Approvals::new();
        assert!(
            file.resolve(
                "apr:inexistant",
                Decision::Allow,
                ApprovalScope::Once,
                now()
            )
            .is_none()
        );
    }

    #[test]
    fn une_decision_une_fois_vaut_pour_la_prochaine_demande_identique_et_se_lit_par_son_etat() {
        let mut file = Approvals::new();
        let premiere = file.request(requete(), now());
        assert_eq!(premiere.state, ApprovalState::Pending);
        assert_eq!(
            file.status(&premiere.id).unwrap().state,
            ApprovalState::Pending
        );
        assert!(file.status("inconnue").is_none());
        file.resolve(&premiere.id, Decision::Allow, ApprovalScope::Once, now())
            .unwrap();
        // Celui qui attendait lit la décision.
        assert_eq!(
            file.status(&premiere.id).unwrap().state,
            ApprovalState::Resolved {
                decision: Decision::Allow
            }
        );
        // La demande identique qui suit est tranchée par cette décision, sans repasser devant
        // l'humain ; la suivante repasse.
        let seconde = file.request(requete(), now());
        assert_eq!(
            seconde.state,
            ApprovalState::Resolved {
                decision: Decision::Allow
            }
        );
        assert!(file.pending().is_empty());
        let troisieme = file.request(requete(), now());
        assert_eq!(troisieme.state, ApprovalState::Pending);
        // Une autre cible n'en profite pas.
        let mut autre = requete();
        autre.target = "mail.send.autre".into();
        file.resolve(&troisieme.id, Decision::Deny, ApprovalScope::Once, now())
            .unwrap();
        assert_eq!(file.request(autre, now()).state, ApprovalState::Pending);
        // Les décisions anciennes s'oublient.
        let _ = file.expire(now() + time::Duration::hours(2));
        assert!(file.status(&premiere.id).is_none());
    }
}
