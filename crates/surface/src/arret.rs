//! L'arrêt d'urgence : toutes les missions en main s'arrêtent d'un geste de l'humain
//! (FRONTIER, interface ; `task.halt` d'agentd).
//!
//! Le geste ouvre une confirmation, et l'arrêt ne part qu'après elle : un clic égaré ne coupe
//! pas dix missions. Seule la réponse du service dit ce qui s'est arrêté ; la surface ne le
//! suppose pas.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{Value, json};

/// Le délai laissé au service : il tue chaque client officiel lancé, trois secondes au plus
/// chacun, avant de répondre.
const DELAI: Duration = Duration::from_secs(30);

/// Ce que l'humain lit une fois l'arrêt répondu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// La phrase.
    pub texte: String,
    /// Vrai si tout ne s'est pas arrêté.
    pub erreur: bool,
}

/// L'état de l'arrêt d'urgence dans la surface.
#[derive(Default)]
pub struct Arret {
    socket: Option<PathBuf>,
    confirmation: bool,
    envoi: Option<Receiver<Result<Value, String>>>,
    issue: Option<Issue>,
}

impl Arret {
    /// Raccorde l'arrêt au service des missions. Sans raccord (scène d'exemple), la
    /// confirmation dit qu'il n'y a rien de réel à arrêter.
    #[must_use]
    pub fn connect(socket: PathBuf) -> Self {
        Self {
            socket: Some(socket),
            ..Self::default()
        }
    }

    /// Ouvre la confirmation, sauf si un arrêt attend déjà sa réponse.
    pub fn demander(&mut self) {
        if self.envoi.is_none() {
            self.confirmation = true;
        }
    }

    /// Referme la confirmation sans rien arrêter.
    pub fn renoncer(&mut self) {
        self.confirmation = false;
    }

    /// Vrai tant que la confirmation est ouverte.
    #[must_use]
    pub fn confirmation(&self) -> bool {
        self.confirmation
    }

    /// L'humain a confirmé : la demande part, hors du fil des images.
    pub fn confirmer(&mut self) {
        self.confirmation = false;
        let Some(socket) = self.socket.clone() else {
            self.issue = Some(Issue {
                texte: "Scène d'exemple : aucune mission réelle à arrêter.".into(),
                erreur: false,
            });
            return;
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(crate::missions::rpc_dans(
                socket,
                "task.halt",
                json!({}),
                DELAI,
            ));
        });
        self.envoi = Some(rx);
        self.issue = None;
    }

    /// Relève la réponse du service, si elle est arrivée.
    pub fn update(&mut self) {
        if let Some(rx) = &self.envoi
            && let Ok(reponse) = rx.try_recv()
        {
            self.envoi = None;
            self.issue = Some(match reponse {
                Ok(v) => issue(&v),
                Err(e) => Issue {
                    texte: format!(
                        "Arrêt non confirmé : {e}. Réessayez, ou tapez `prophet task halt`."
                    ),
                    erreur: true,
                },
            });
        }
    }

    /// Vrai tant qu'un arrêt attend sa réponse.
    #[must_use]
    pub fn en_cours(&self) -> bool {
        self.envoi.is_some()
    }

    /// La dernière issue, jusqu'à ce que l'humain l'écarte.
    #[must_use]
    pub fn issue(&self) -> Option<&Issue> {
        self.issue.as_ref()
    }

    /// Écarte l'issue lue.
    pub fn ecarter(&mut self) {
        self.issue = None;
    }
}

/// La réponse de `task.halt`, en une phrase.
#[must_use]
pub fn issue(reponse: &Value) -> Issue {
    let compte = |cle: &str| reponse[cle].as_array().map_or(0, Vec::len);
    let (arret, annulees, erreurs) = (
        compte("cancel_requested"),
        compte("cancelled"),
        compte("errors"),
    );
    let pluriel = |n: usize, un: &str, plusieurs: &str| {
        if n == 1 {
            format!("1 {un}")
        } else {
            format!("{n} {plusieurs}")
        }
    };
    let mut parties = Vec::new();
    if arret > 0 {
        parties.push(format!(
            "{} ({})",
            pluriel(arret, "mission s'arrête", "missions s'arrêtent"),
            "l'état final confirmera chaque arrêt"
        ));
    }
    if annulees > 0 {
        parties.push(pluriel(annulees, "plan annulé", "plans annulés"));
    }
    if erreurs > 0 {
        parties.push(format!(
            "{} : arrêtez-les une à une",
            pluriel(erreurs, "mission a résisté", "missions ont résisté")
        ));
    }
    Issue {
        texte: if parties.is_empty() {
            "Arrêt d'urgence : aucune mission en main, rien à arrêter.".into()
        } else {
            format!("Arrêt d'urgence : {}.", parties.join(" ; "))
        },
        erreur: erreurs > 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_reponse_du_service_se_dit_en_une_phrase() {
        let i = issue(&json!({"cancel_requested":["a","b"],"cancelled":["p"],"errors":[]}));
        assert_eq!(
            i.texte,
            "Arrêt d'urgence : 2 missions s'arrêtent (l'état final confirmera chaque arrêt) ; 1 plan annulé."
        );
        assert!(!i.erreur);
        let i = issue(&json!({"cancel_requested":[],"cancelled":[],"errors":[{"id":"c"}]}));
        assert!(i.erreur);
        assert!(i.texte.contains("1 mission a résisté"), "{}", i.texte);
        let i = issue(&json!({"cancel_requested":[],"cancelled":[],"errors":[]}));
        assert!(i.texte.contains("rien à arrêter"), "{}", i.texte);
    }

    #[test]
    fn rien_ne_part_sans_confirmation_et_une_scene_d_exemple_n_arrete_rien() {
        let mut arret = Arret::default();
        arret.demander();
        assert!(arret.confirmation());
        arret.renoncer();
        assert!(!arret.confirmation() && !arret.en_cours() && arret.issue().is_none());
        arret.demander();
        arret.confirmer();
        assert!(!arret.en_cours(), "sans raccord, rien n'est envoyé");
        assert!(arret.issue().unwrap().texte.contains("Scène d'exemple"));
    }
}
