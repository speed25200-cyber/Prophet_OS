//! Ce qu'il y a à voir, avant qu'on sache comment le dessiner.
//!
//! La personne définit les objectifs, examine les accès et supervise les missions. La scène
//! transporte leur état vers l'interface sans dépendre du GPU. L'ancien rendu de courants utilise
//! encore quatre catégories visuelles ; l'espace de supervision affiche aussi l'état exact du
//! runtime et conserve les missions terminées pour permettre l'examen du travail.

/// Où en est une tâche.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Etat {
    /// Elle avance.
    Court,
    /// Elle attend une décision humaine.
    Attend,
    /// Elle est empêchée, et personne n'a encore agi.
    Bloque,
    /// Elle est terminée ; son courant s'efface.
    Fini,
}

/// Une tâche, vue comme un courant dans le champ.
#[derive(Debug, Clone)]
pub struct Courant {
    /// Identifiant de la tâche.
    pub tache: String,
    /// Ce qu'elle fait, en une phrase d'humain.
    pub intitule: String,
    /// Le pilote qui la mène.
    pub agent: String,
    /// Son état.
    pub etat: Etat,
    /// Étapes par minute, telles que mesurées.
    pub debit: f32,
    /// Part du budget déjà consommée, de 0 à 1.
    pub budget_consomme: f32,
    /// Nombre d'étapes franchies.
    pub etapes: u32,
    /// État exact reçu du runtime, absent dans les anciennes scènes de démonstration.
    pub task_state: Option<agentd::State>,
    /// Longueur de l'historique reçu ; distingue notamment une reprise d'une ancienne exécution.
    pub task_revision: usize,
}

/// La décision qu'on attend d'un humain.
///
/// Une seule à la fois. Empiler des questions reviendrait à demander à quelqu'un de faire la file
/// devant sa propre machine.
#[derive(Debug, Clone)]
pub struct Decision {
    /// La question, formulée en conséquence et non en mécanisme.
    pub question: String,
    /// Ce qui arrivera si l'on accepte. Dit avant, jamais après.
    pub consequence: String,
    /// La tâche qui demande.
    pub tache: String,
    /// Depuis combien de secondes elle attend.
    pub depuis_secondes: u64,
    /// Vrai si l'action est irréversible.
    pub irreversible: bool,
}

/// L'isolation réellement disponible, telle que `sandboxd` la sonde.
#[derive(Debug, Clone)]
pub struct Isolation {
    /// Niveau maximal atteignable.
    pub niveau_max: u8,
    /// Ce qui manque pour aller plus haut, si quelque chose manque.
    pub manque: Option<String>,
}

/// Ce qu'il y a à voir, à un instant.
#[derive(Debug, Clone)]
pub struct Scene {
    /// L'heure, telle qu'on l'affiche.
    pub heure: String,
    /// La date, telle qu'on l'affiche.
    pub date: String,
    /// Les courants, dans l'ordre où ils seront dessinés.
    pub courants: Vec<Courant>,
    /// La décision en attente, s'il y en a une.
    pub decision: Option<Decision>,
    /// L'isolation de cette machine.
    pub isolation: Isolation,
}

/// Vitesse maximale d'un courant, en unités de champ par seconde.
const VITESSE_MAX: f32 = 0.42;

/// Débit au-delà duquel un courant est à pleine vitesse, en étapes par minute.
///
/// Au-delà, l'œil ne distingue plus : accélérer davantage n'ajouterait pas d'information et
/// rendrait l'écran fatigant.
const DEBIT_SATURANT: f32 = 40.0;

impl Courant {
    /// Vitesse du courant, telle que le champ la rendra.
    ///
    /// Un courant bloqué ou terminé ne bouge pas. C'est la propriété qui compte : **on doit voir
    /// qu'une tâche est arrêtée sans avoir à lire qu'elle l'est.**
    #[must_use]
    pub fn vitesse(&self) -> f32 {
        match self.etat {
            Etat::Bloque | Etat::Fini => 0.0,
            // Une tâche qui attend une décision respire encore, faiblement : elle n'est pas morte,
            // elle attend quelqu'un.
            Etat::Attend => VITESSE_MAX * 0.08,
            Etat::Court => VITESSE_MAX * (self.debit / DEBIT_SATURANT).clamp(0.05, 1.0),
        }
    }

    /// Clarté du courant, de 0 à 1.
    ///
    /// Elle décroît avec le budget consommé : un agent qui approche de sa limite pâlit, ce qui se
    /// remarque avant qu'un chiffre ne soit lu.
    #[must_use]
    pub fn clarte(&self) -> f32 {
        let reste = (1.0 - self.budget_consomme).clamp(0.0, 1.0);
        match self.etat {
            Etat::Fini => 0.12,
            Etat::Bloque => 0.30,
            _ => 0.35 + 0.65 * reste,
        }
    }

    /// Vrai si ce courant réclame le regard.
    #[must_use]
    pub const fn reclame(&self) -> bool {
        matches!(self.etat, Etat::Bloque | Etat::Attend)
    }
}

impl Scene {
    /// Ordonne les courants : ce qui réclame d'abord, puis le plus vif, puis le reste.
    ///
    /// L'ordre n'est pas cosmétique. Le premier courant occupe la place la plus lisible ; s'il
    /// était choisi au hasard, il faudrait chercher, et chercher est déjà une interaction.
    pub fn ordonner(&mut self) {
        self.courants.sort_by(|a, b| {
            b.reclame()
                .cmp(&a.reclame())
                .then_with(|| {
                    b.vitesse()
                        .partial_cmp(&a.vitesse())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| a.tache.cmp(&b.tache))
        });
    }

    /// Atténuation appliquée au champ quand une décision attend.
    ///
    /// Une question posée à un humain ne doit pas concourir avec un fond animé. Le champ recule,
    /// sans disparaître : ce qui tourne continue de tourner, et doit rester visible.
    #[must_use]
    pub fn attenuation_du_champ(&self) -> f32 {
        if self.decision.is_some() { 0.22 } else { 1.0 }
    }

    /// Nombre de tâches qui avancent réellement.
    #[must_use]
    pub fn actives(&self) -> usize {
        self.courants
            .iter()
            .filter(|c| c.etat == Etat::Court)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn courant(tache: &str, etat: Etat, debit: f32, budget: f32) -> Courant {
        Courant {
            tache: tache.to_owned(),
            intitule: "quelque chose".to_owned(),
            agent: "claude-code".to_owned(),
            etat,
            debit,
            budget_consomme: budget,
            etapes: 12,
            task_state: None,
            task_revision: 0,
        }
    }

    #[test]
    fn un_courant_bloque_ne_bouge_pas() {
        // La propriété centrale : l'arrêt se voit, il ne se lit pas.
        let bloque = courant("t1", Etat::Bloque, 30.0, 0.2);
        assert_eq!(bloque.vitesse(), 0.0);
    }

    #[test]
    fn un_courant_qui_attend_respire_encore() {
        // Attendre n'est pas être mort : la distinction doit se voir aussi.
        let attend = courant("t1", Etat::Attend, 30.0, 0.2);
        assert!(attend.vitesse() > 0.0);
        assert!(attend.vitesse() < courant("t2", Etat::Court, 30.0, 0.2).vitesse());
    }

    #[test]
    fn la_vitesse_suit_le_debit_puis_sature() {
        let lent = courant("t1", Etat::Court, 4.0, 0.0);
        let vif = courant("t2", Etat::Court, 30.0, 0.0);
        let effrene = courant("t3", Etat::Court, 400.0, 0.0);
        assert!(lent.vitesse() < vif.vitesse());
        assert!(vif.vitesse() <= effrene.vitesse());
        assert!(
            (effrene.vitesse() - VITESSE_MAX).abs() < 1e-6,
            "au-delà du seuil, l'œil ne distingue plus : la vitesse doit saturer"
        );
    }

    #[test]
    fn un_budget_epuise_palit() {
        let frais = courant("t1", Etat::Court, 10.0, 0.0);
        let epuise = courant("t2", Etat::Court, 10.0, 0.95);
        assert!(
            epuise.clarte() < frais.clarte(),
            "un agent proche de sa limite doit se remarquer sans qu'on lise un chiffre"
        );
    }

    #[test]
    fn ce_qui_reclame_passe_devant() {
        let mut scene = Scene {
            heure: "14:37".to_owned(),
            date: "jeudi".to_owned(),
            courants: vec![
                courant("rapide", Etat::Court, 38.0, 0.1),
                courant("bloque", Etat::Bloque, 0.0, 0.4),
                courant("lent", Etat::Court, 2.0, 0.1),
            ],
            decision: None,
            isolation: Isolation {
                niveau_max: 1,
                manque: None,
            },
        };
        scene.ordonner();
        assert_eq!(
            scene.courants[0].tache, "bloque",
            "ce qui est empêché doit occuper la place la plus lisible"
        );
        assert_eq!(scene.courants[1].tache, "rapide");
    }

    #[test]
    fn une_decision_fait_reculer_le_champ_sans_l_eteindre() {
        let mut scene = Scene {
            heure: "14:37".to_owned(),
            date: "jeudi".to_owned(),
            courants: vec![courant("t1", Etat::Court, 20.0, 0.1)],
            decision: None,
            isolation: Isolation {
                niveau_max: 2,
                manque: None,
            },
        };
        assert_eq!(scene.attenuation_du_champ(), 1.0);

        scene.decision = Some(Decision {
            question: "Envoyer le message à quatre destinataires ?".to_owned(),
            consequence: "Le message part et ne peut pas être rappelé.".to_owned(),
            tache: "t1".to_owned(),
            depuis_secondes: 12,
            irreversible: true,
        });
        let attenue = scene.attenuation_du_champ();
        assert!(
            attenue < 1.0,
            "une question ne doit pas concourir avec un fond animé"
        );
        assert!(
            attenue > 0.0,
            "ce qui tourne continue de tourner et doit rester visible"
        );
    }

    #[test]
    fn l_ordre_est_stable_a_activite_egale() {
        // Deux courants identiques ne doivent pas changer de place d'une image à l'autre : un
        // classement instable ferait clignoter l'écran sans que rien ne se passe.
        let faire = || Scene {
            heure: "14:37".to_owned(),
            date: "jeudi".to_owned(),
            courants: vec![
                courant("bbb", Etat::Court, 10.0, 0.1),
                courant("aaa", Etat::Court, 10.0, 0.1),
            ],
            decision: None,
            isolation: Isolation {
                niveau_max: 0,
                manque: Some("gVisor".to_owned()),
            },
        };
        let mut une = faire();
        une.ordonner();
        let mut deux = faire();
        deux.ordonner();
        assert_eq!(une.courants[0].tache, deux.courants[0].tache);
        assert_eq!(une.courants[0].tache, "aaa");
    }
}
