//! La surface, branchée sur le système qui tourne.
//!
//! Jusqu'ici `prophet-surface` affichait une scène d'exemple : de beaux filaments qui ne
//! correspondaient à aucune tâche. Une interface d'observation qui montre des données inventées est
//! pire qu'une interface absente — elle a l'air de dire quelque chose.
//!
//! Ce module interroge `agentd` pour les tâches, `capd` pour les décisions en attente, et
//! `sandboxd` pour ce que la machine sait isoler. Il n'invente rien, et surtout il **ne remplace
//! pas un daemon injoignable par une démonstration** : quand rien ne répond, le champ est vide et
//! la ligne d'isolation le dit.
//!
//! ## Pourquoi un fil séparé
//!
//! [`Source`](crate::fenetre::Source) est synchrone, et il doit le rester : il est appelé à chaque
//! image, et une image qui attendrait une réponse réseau ferait saccader le champ. Un fil de fond
//! interroge donc les daemons à son rythme et dépose le résultat ; la boucle de rendu lit ce qui
//! est déposé, sans jamais attendre.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agentd::task::Task;
use capd::Approval;
use time::OffsetDateTime;

use crate::fenetre::{Reponse, Source};
use crate::scene::{Decision, Isolation, Scene};

/// À quel rythme l'état du système est relu.
///
/// Quatre fois par seconde : assez pour qu'une tâche qui démarre apparaisse tout de suite, assez
/// peu pour ne pas réveiller trois daemons soixante fois par seconde afin de redessiner la même
/// chose.
const PERIODE: Duration = Duration::from_millis(250);

/// Ce que le fil de fond dépose pour la boucle de rendu.
#[derive(Debug, Default)]
struct Partage {
    taches: Vec<Task>,
    approbations: Vec<Approval>,
    isolation: Option<Isolation>,
    /// Ce qui empêche de savoir, quand quelque chose l'empêche.
    panne: Option<String>,
}

/// Où joindre les daemons.
#[derive(Debug, Clone)]
pub struct Sockets {
    /// Le runtime d'agents, pour les tâches.
    pub agentd: PathBuf,
    /// Le broker, pour les décisions en attente.
    pub capd: PathBuf,
    /// Le gestionnaire d'isolation, pour ce que la machine sait faire.
    pub sandboxd: PathBuf,
}

impl Default for Sockets {
    fn default() -> Self {
        Self {
            agentd: chemin("PROPHET_AGENTD_SOCKET", "agentd"),
            capd: chemin("PROPHET_CAPD_SOCKET", "capd"),
            sandboxd: chemin("PROPHET_SANDBOXD_SOCKET", "sandboxd"),
        }
    }
}

fn chemin(variable: &str, daemon: &str) -> PathBuf {
    std::env::var(variable).map_or_else(|_| prophet_ipc::socket_path(daemon), PathBuf::from)
}

/// La source qui lit le système.
pub struct Reel {
    partage: Arc<Mutex<Partage>>,
    sockets: Sockets,
    /// La décision montrée en ce moment, pour savoir laquelle trancher quand on répond.
    montree: Option<String>,
}

impl std::fmt::Debug for Reel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reel")
            .field("sockets", &self.sockets)
            .finish_non_exhaustive()
    }
}

impl Reel {
    /// Démarre l'interrogation des daemons et rend la source.
    ///
    /// Le fil de fond vit aussi longtemps que le programme. Il ne rend jamais la main, et c'est
    /// voulu : la surface est ce que la machine montre en s'allumant, elle ne s'arrête pas.
    #[must_use]
    pub fn demarrer(sockets: Sockets) -> Self {
        let partage = Arc::new(Mutex::new(Partage::default()));
        let copie = Arc::clone(&partage);
        let adresses = sockets.clone();
        std::thread::spawn(move || interroger(&adresses, &copie));
        Self {
            partage,
            sockets,
            montree: None,
        }
    }
}

impl Source for Reel {
    fn scene(&mut self) -> Scene {
        let maintenant = OffsetDateTime::now_utc();
        let (taches, approbations, isolation) = {
            // Un verrou empoisonné veut dire que le fil de fond a paniqué. On reprend la garde
            // quand même : la panne apparaîtra au tour suivant sous la forme d'un champ vide, ce
            // qui vaut mieux qu'une panique en cascade dans la boucle de rendu.
            let partage = self
                .partage
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (
                partage.taches.clone(),
                partage.approbations.clone(),
                partage.isolation.clone().unwrap_or(Isolation {
                    niveau_max: 0,
                    manque: partage
                        .panne
                        .clone()
                        .or_else(|| Some("isolation inconnue".to_owned())),
                }),
            )
        };

        let mut scene = crate::depuis::scene(
            &taches,
            &approbations,
            isolation.niveau_max,
            isolation.manque,
            heure(maintenant),
            date(maintenant),
            maintenant,
        );
        self.montree = scene
            .decision
            .as_ref()
            .map(|d| identifiant(d, &approbations));
        scene.ordonner();
        scene
    }

    fn repond(&mut self, reponse: Reponse) {
        let Some(id) = self.montree.clone() else {
            return;
        };
        let socket = self.sockets.capd.clone();
        let decision = match reponse {
            Reponse::Accepte => "allow",
            Reponse::Refuse => "deny",
        };
        // La réponse part sur un fil à part : trancher ne doit pas retenir l'image suivante, et un
        // `capd` lent ne doit pas geler l'écran de quelqu'un qui vient d'appuyer sur une touche.
        std::thread::spawn(move || {
            let Ok(execution) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            execution.block_on(async {
                match appeler(
                    &socket,
                    "approval.resolve",
                    serde_json::json!({ "id": id, "decision": decision }),
                )
                .await
                {
                    Ok(_) => tracing::info!(%id, %decision, "décision transmise à capd"),
                    // Le dire fort : une décision humaine perdue est exactement ce qu'un système
                    // d'approbation ne doit jamais faire en silence.
                    Err(erreur) => {
                        tracing::error!(%id, %decision, %erreur, "décision NON transmise");
                    }
                }
            });
        });
    }
}

/// Retrouve l'identifiant de la demande montrée.
///
/// [`Decision`] ne le porte pas : la surface n'a pas à connaître les identifiants du système pour
/// dessiner. On le retrouve donc par la demande la plus ancienne, qui est celle que
/// `depuis::scene` a choisi de montrer — la même règle des deux côtés.
fn identifiant(_montree: &Decision, approbations: &[Approval]) -> String {
    approbations
        .iter()
        .min_by_key(|a| a.created)
        .map(|a| a.id.clone())
        .unwrap_or_default()
}

/// La boucle qui interroge, pour toujours.
fn interroger(sockets: &Sockets, partage: &Arc<Mutex<Partage>>) {
    let Ok(execution) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        tracing::error!("pas d'exécution asynchrone : la surface restera vide");
        return;
    };

    execution.block_on(async {
        loop {
            let taches = appeler(&sockets.agentd, "task.list", serde_json::json!({})).await;
            let approbations =
                appeler(&sockets.capd, "approval.pending", serde_json::json!({})).await;
            let capacites = appeler(
                &sockets.sandboxd,
                "sandbox.capabilities",
                serde_json::json!({}),
            )
            .await;

            let mut panne = None;
            if let Err(erreur) = &taches {
                panne = Some(format!("agentd injoignable : {erreur}"));
            }

            if let Ok(mut etat) = partage.lock() {
                // Un daemon qui ne répond pas met sa part à zéro, et ne laisse pas la précédente.
                // Montrer d'anciennes tâches comme si elles couraient encore serait le pire des
                // deux mondes : faux, et crédible.
                etat.taches = taches.ok().and_then(deserialiser).unwrap_or_default();
                etat.approbations = approbations.ok().and_then(deserialiser).unwrap_or_default();
                etat.isolation = capacites.ok().map(|valeur| Isolation {
                    niveau_max: u8::try_from(valeur["max_level"].as_u64().unwrap_or(0))
                        .unwrap_or(0),
                    manque: valeur["report"].as_str().map(str::to_owned),
                });
                etat.panne = panne;
            }

            tokio::time::sleep(PERIODE).await;
        }
    });
}

fn deserialiser<T: serde::de::DeserializeOwned>(valeur: serde_json::Value) -> Option<T> {
    match serde_json::from_value(valeur) {
        Ok(v) => Some(v),
        Err(erreur) => {
            // Le signaler : une forme inattendue veut dire que la surface et le daemon ne parlent
            // plus la même langue, et un écran vide sans explication ferait chercher ailleurs.
            tracing::error!(%erreur, "réponse d'un daemon illisible");
            None
        }
    }
}

async fn appeler(
    socket: &std::path::Path,
    methode: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let client = prophet_ipc::Client::connect(socket)
        .await
        .map_err(|e| e.to_string())?;
    client
        .call(methode, params)
        .await
        .map_err(|e| e.message.clone())
}

fn heure(instant: OffsetDateTime) -> String {
    format!("{:02}:{:02}", instant.hour(), instant.minute())
}

fn date(instant: OffsetDateTime) -> String {
    let jours = [
        "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
    ];
    let mois = [
        "janvier",
        "février",
        "mars",
        "avril",
        "mai",
        "juin",
        "juillet",
        "août",
        "septembre",
        "octobre",
        "novembre",
        "décembre",
    ];
    let jour = jours[instant.weekday().number_days_from_monday() as usize];
    let m = mois[(instant.month() as u8 - 1) as usize];
    format!("{jour} {} {m}", instant.day())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_date_se_dit_en_francais() {
        // Le 12 septembre 2026 est un samedi.
        let instant = OffsetDateTime::from_unix_timestamp(1_789_000_000).expect("date valide");
        let rendue = date(instant);
        assert!(
            rendue.contains("septembre") || rendue.contains("juillet"),
            "mois en français attendu, obtenu : {rendue}"
        );
        assert!(
            !rendue.is_empty() && rendue.chars().next().is_some_and(char::is_lowercase),
            "obtenu : {rendue}"
        );
    }

    #[test]
    fn l_heure_est_sur_deux_chiffres() {
        let minuit = OffsetDateTime::from_unix_timestamp(0).expect("date valide");
        assert_eq!(heure(minuit), "00:00");
    }

    #[test]
    fn sans_aucun_daemon_le_champ_est_vide_et_le_dit() {
        // La propriété qui compte : rien ne répond, donc rien n'est montré — et surtout pas une
        // démonstration qui ressemblerait à un système en marche.
        let mut source = Reel::demarrer(Sockets {
            agentd: PathBuf::from("/nulle/part/agentd.sock"),
            capd: PathBuf::from("/nulle/part/capd.sock"),
            sandboxd: PathBuf::from("/nulle/part/sandboxd.sock"),
        });
        std::thread::sleep(Duration::from_millis(400));
        let scene = source.scene();
        assert!(
            scene.courants.is_empty(),
            "aucun courant ne doit être inventé : {:?}",
            scene.courants
        );
        assert!(scene.decision.is_none());
        assert_eq!(scene.isolation.niveau_max, 0);
        assert!(
            scene
                .isolation
                .manque
                .as_deref()
                .is_some_and(|m| m.contains("agentd") || m.contains("inconnue")),
            "la ligne d'isolation doit dire ce qui manque, obtenu : {:?}",
            scene.isolation.manque
        );
    }

    #[test]
    fn repondre_sans_decision_montree_ne_fait_rien() {
        let mut source = Reel::demarrer(Sockets {
            agentd: PathBuf::from("/nulle/part/agentd.sock"),
            capd: PathBuf::from("/nulle/part/capd.sock"),
            sandboxd: PathBuf::from("/nulle/part/sandboxd.sock"),
        });
        // Ne doit ni paniquer, ni envoyer quoi que ce soit.
        source.repond(Reponse::Accepte);
    }
}
