//! Lecture et commandes des missions, sans attente réseau dans la boucle de rendu.
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use agentd::{Inspection, State};
use serde_json::{Value, json};

/// Commande explicite de l'humain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Exécuter le plan examiné.
    Start,
    /// Demander l'arrêt ; seule l'observation finale le confirme.
    Cancel,
}

enum Reply {
    Inspect(u64, Result<Box<Inspection>, String>),
    Action(String, Action, Result<Value, String>),
}

/// Accusé de réception ou erreur, lié à l'identité de la mission commandée.
#[derive(Debug, Clone)]
pub struct Notice {
    /// Mission concernée.
    pub task: String,
    /// Message visible, sans déduire un état final d'un simple acquittement.
    pub text: String,
    /// La commande n'a pas de confirmation valide.
    pub error: bool,
}

/// Contrôleur de l'inspecteur. Une lecture et une commande au maximum peuvent être en vol.
pub struct Missions {
    socket: Option<PathBuf>,
    selected: Option<String>,
    revision: u64,
    next_read: Instant,
    reading: bool,
    action: bool,
    refresh_required: bool,
    snapshot: Option<Inspection>,
    error: Option<String>,
    notice: Option<Notice>,
    tx: Sender<Reply>,
    rx: Receiver<Reply>,
}

impl Default for Missions {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            socket: None,
            selected: None,
            revision: 0,
            next_read: Instant::now(),
            reading: false,
            action: false,
            refresh_required: false,
            snapshot: None,
            error: None,
            notice: None,
            tx,
            rx,
        }
    }
}

impl Missions {
    /// Active la liaison avec un socket fourni par l'application, jamais par le modèle.
    #[must_use]
    pub fn connect(socket: PathBuf) -> Self {
        Self {
            socket: Some(socket),
            ..Self::default()
        }
    }

    /// Observe une sélection. Les réponses d'une ancienne sélection ne remplacent pas la vue.
    pub fn select(&mut self, id: Option<&str>) {
        if self.selected.as_deref() != id {
            self.revision = self.revision.wrapping_add(1);
            self.selected = id.map(str::to_owned);
            self.snapshot = None;
            self.error = None;
            self.next_read = Instant::now();
        }
    }

    /// Dépose les réponses disponibles et programme une lecture, sans bloquer l'appelant.
    pub fn update(&mut self) {
        while let Ok(reply) = self.rx.try_recv() {
            match reply {
                Reply::Inspect(revision, result) => {
                    self.reading = false;
                    if revision == self.revision {
                        match result {
                            Ok(info) => {
                                if info.task.state.is_terminal()
                                    && self
                                        .notice
                                        .as_ref()
                                        .is_some_and(|n| !n.error && n.task == info.task.id)
                                {
                                    self.notice = None;
                                }
                                self.snapshot = Some(*info);
                                self.error = None;
                                self.refresh_required = false;
                            }
                            Err(error) => {
                                self.snapshot = None;
                                self.error = Some(error);
                            }
                        }
                    }
                }
                Reply::Action(id, action, result) => {
                    self.action = false;
                    self.refresh_required = true;
                    // Invalide également toute lecture commencée avant cette commande.
                    self.revision = self.revision.wrapping_add(1);
                    self.next_read = Instant::now();
                    let valid = result.and_then(|v| match action {
                        Action::Start if v["started"].as_str() == Some(&id) => {
                            Ok("Lancement accepté par le service.")
                        }
                        Action::Cancel if v["cancel_requested"].as_str() == Some(&id) => {
                            Ok("Arrêt demandé. Attente de la confirmation du travailleur.")
                        }
                        Action::Cancel if v["cancelled"].as_str() == Some(&id) => {
                            Ok("Le service a confirmé l'annulation du plan.")
                        }
                        _ => Err(
                            "Accusé de réception incohérent ; vérifiez l'état de la mission."
                                .into(),
                        ),
                    });
                    self.notice = Some(match valid {
                        Ok(message) => Notice {
                            task: id,
                            text: message.into(),
                            error: false,
                        },
                        Err(error) => Notice {
                            task: id,
                            text: format!(
                                "Commande non confirmée : {error} Aucun nouvel envoi automatique."
                            ),
                            error: true,
                        },
                    });
                }
            }
        }
        if !self.reading
            && Instant::now() >= self.next_read
            && let (Some(socket), Some(id)) = (self.socket.clone(), self.selected.clone())
        {
            self.reading = true;
            self.next_read = Instant::now() + Duration::from_secs(1);
            let tx = self.tx.clone();
            let revision = self.revision;
            std::thread::spawn(move || {
                let result = rpc(socket, "task.inspect", json!({"id":id})).and_then(|v| {
                    let info: Inspection = serde_json::from_value(v)
                        .map_err(|_| "Détail de mission illisible.".to_owned())?;
                    if info.task.id != id
                        || info.plan.as_ref().is_some_and(|p| p.task != id)
                        || (info.can_start && info.task.state != State::Planned)
                    {
                        return Err(
                            "Le détail reçu ne correspond pas à la mission demandée.".into()
                        );
                    }
                    Ok(Box::new(info))
                });
                let _ = tx.send(Reply::Inspect(revision, result));
            });
        }
    }

    /// Détail de la mission actuellement sélectionnée.
    #[must_use]
    pub fn snapshot(&self) -> Option<&Inspection> {
        self.snapshot.as_ref()
    }
    /// Réconcilie les deux lectures : la liste et l'inspecteur doivent montrer le même état.
    pub(crate) fn align_scene(&mut self, scene: &mut crate::scene::Scene) {
        let Some(info) = &self.snapshot else {
            return;
        };
        let Some(courant) = scene.courants.iter_mut().find(|c| c.tache == info.task.id) else {
            return;
        };
        let Some(state) = courant.task_state else {
            return;
        };
        if state == info.task.state && courant.task_revision == info.task.history.len() {
            return;
        }
        if courant.task_revision < info.task.history.len() && info.task.history.contains(&state) {
            // L'inspection atteste la transition que la liste n'a pas encore reçue.
            courant.task_state = Some(info.task.state);
            courant.task_revision = info.task.history.len();
            courant.etat = crate::depuis::etat(info.task.state);
            courant.etapes = info.task.budget.spent.steps;
            courant.debit = crate::depuis::debit(&info.task.budget);
            courant.budget_consomme = crate::depuis::budget_consomme(&info.task.budget);
            scene.ordonner();
        } else {
            // La liste a changé depuis l'inspection : retirer ses anciennes commandes.
            self.snapshot = None;
            self.revision = self.revision.wrapping_add(1);
            self.next_read = Instant::now();
        }
    }
    /// Erreur de lecture à afficher.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    /// Une connexion de service est configurée.
    #[must_use]
    pub fn connected(&self) -> bool {
        self.socket.is_some()
    }
    /// Message correspondant à la sélection courante.
    #[must_use]
    pub fn notice(&self) -> Option<&Notice> {
        self.notice
            .as_ref()
            .filter(|n| Some(n.task.as_str()) == self.selected.as_deref())
    }
    /// Une commande ou sa relecture de confirmation est encore en cours.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.action || self.refresh_required
    }
    /// Force une nouvelle lecture ; ne rejoue aucune commande.
    pub fn refresh(&mut self) {
        self.next_read = Instant::now();
    }

    /// Envoie une commande une seule fois, à la mission dont le détail a été reçu.
    ///
    /// # Errors
    /// Connexion absente, détail non disponible, action non autorisée ou déjà en cours.
    pub fn command(&mut self, action: Action) -> Result<(), String> {
        if self.busy() {
            return Err("Une commande attend encore sa confirmation.".into());
        }
        let info = self
            .snapshot
            .as_ref()
            .ok_or("Le détail de la mission doit être chargé.")?;
        if !(match action {
            Action::Start => info.can_start,
            Action::Cancel => info.can_cancel,
        }) {
            return Err("Cette commande n'est pas disponible dans l'état reçu.".into());
        }
        let socket = self.socket.clone().ok_or("Connexion au service absente.")?;
        let id = info.task.id.clone();
        let tx = self.tx.clone();
        self.action = true;
        self.notice = None;
        std::thread::spawn(move || {
            let method = match action {
                Action::Start => "task.start",
                Action::Cancel => "task.cancel",
            };
            let result = rpc(socket, method, json!({"id":id}));
            let _ = tx.send(Reply::Action(id, action, result));
        });
        Ok(())
    }
}

fn rpc(socket: PathBuf, method: &str, params: Value) -> Result<Value, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            let client = prophet_ipc::Client::connect(socket)
                .await
                .map_err(|e| e.to_string())?;
            client.call(method, params).await.map_err(|e| e.message)
        })
        .await
        .map_err(|_| "Le service ne répond pas dans le délai.".to_owned())?
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inspection(id: &str, state: State) -> Inspection {
        let mut task = agentd::Task::new(
            id,
            "Objectif de test",
            "local:test",
            "prophet",
            agentd::budget::Budget::new(Default::default()),
            time::OffsetDateTime::now_utc(),
        );
        task.state = state;
        Inspection {
            task,
            plan: None,
            result: None,
            can_start: state == State::Planned,
            can_cancel: !state.is_terminal(),
            start_reason: None,
        }
    }

    #[test]
    fn changer_de_selection_ecarte_la_reponse_de_la_mission_precedente() {
        let mut missions = Missions::default();
        missions.select(Some("a"));
        let old = missions.revision;
        missions.select(Some("b"));
        missions
            .tx
            .send(Reply::Inspect(
                old,
                Ok(inspection("a", State::Planned).into()),
            ))
            .unwrap();
        missions.update();
        assert!(missions.snapshot().is_none());
        assert!(missions.command(Action::Start).is_err());
        missions
            .tx
            .send(Reply::Inspect(
                missions.revision,
                Ok(inspection("b", State::Running).into()),
            ))
            .unwrap();
        missions.update();
        assert_eq!(missions.snapshot().unwrap().task.id, "b");
    }

    #[test]
    fn un_arret_acquitte_ne_devient_pas_une_fin_optimiste() {
        let mut missions = Missions::default();
        missions.select(Some("a"));
        missions.snapshot = Some(inspection("a", State::Running));
        missions
            .tx
            .send(Reply::Action(
                "a".into(),
                Action::Cancel,
                Ok(json!({"cancel_requested":"a"})),
            ))
            .unwrap();
        missions.update();
        assert_eq!(missions.snapshot().unwrap().task.state, State::Running);
        assert!(missions.busy());
        assert!(!missions.notice().unwrap().error);
        assert!(missions.command(Action::Cancel).is_err());
        missions
            .tx
            .send(Reply::Inspect(
                missions.revision,
                Ok(inspection("a", State::Cancelled).into()),
            ))
            .unwrap();
        missions.update();
        assert_eq!(missions.snapshot().unwrap().task.state, State::Cancelled);
        assert!(!missions.busy());
        assert!(missions.notice().is_none());
    }

    #[test]
    fn une_commande_invalide_et_sa_relecture_ancienne_ne_confirment_rien() {
        let mut missions = Missions::default();
        missions.select(Some("a"));
        let old = missions.revision;
        missions
            .tx
            .send(Reply::Action(
                "a".into(),
                Action::Start,
                Ok(json!({"started":"b"})),
            ))
            .unwrap();
        missions
            .tx
            .send(Reply::Inspect(
                old,
                Ok(inspection("a", State::Planned).into()),
            ))
            .unwrap();
        missions.update();
        assert!(missions.snapshot().is_none());
        assert!(missions.busy());
        assert!(missions.notice().unwrap().error);
        assert!(
            missions
                .notice()
                .unwrap()
                .text
                .contains("Aucun nouvel envoi automatique")
        );
        missions.select(Some("b"));
        assert!(missions.notice().is_none());
    }

    #[test]
    fn une_erreur_de_lecture_retire_le_detail_et_ses_commandes() {
        let mut missions = Missions::default();
        missions.select(Some("a"));
        missions.snapshot = Some(inspection("a", State::Planned));
        missions
            .tx
            .send(Reply::Inspect(
                missions.revision,
                Err("service absent".into()),
            ))
            .unwrap();
        missions.update();
        assert!(missions.snapshot().is_none());
        assert_eq!(missions.error(), Some("service absent"));
        assert!(missions.command(Action::Start).is_err());
    }

    #[test]
    fn liste_et_inspecteur_ne_montrent_pas_deux_etats_contradictoires() {
        let mut missions = Missions::default();
        let mut info = inspection("a", State::Cancelled);
        info.task.history = vec![
            State::Pending,
            State::Planned,
            State::Running,
            State::Cancelled,
        ];
        let mut scene = crate::depuis::scene(
            &[inspection("a", State::Running).task],
            &[],
            0,
            None,
            String::new(),
            String::new(),
            time::OffsetDateTime::now_utc(),
        );
        missions.snapshot = Some(info);
        missions.align_scene(&mut scene);
        assert_eq!(scene.courants[0].task_state, Some(State::Cancelled));
        assert_eq!(scene.actives(), 0);
        assert!(missions.snapshot().is_some());

        // Dans l'autre sens, une liste plus récente invalide l'ancien bouton de lancement.
        missions.snapshot = Some(inspection("a", State::Planned));
        missions.align_scene(&mut scene);
        assert!(missions.snapshot().is_none());
        assert!(missions.command(Action::Start).is_err());
        assert_eq!(scene.courants[0].task_state, Some(State::Cancelled));

        // Une reprise repasse par Running : la simple présence dans l'historique ne date pas l'état.
        let mut paused = inspection("a", State::Paused);
        paused.task.history = vec![
            State::Pending,
            State::Planned,
            State::Running,
            State::Paused,
        ];
        let mut resumed = paused.task.clone();
        resumed.state = State::Running;
        resumed.history.push(State::Running);
        let mut scene = crate::depuis::scene(
            &[resumed],
            &[],
            0,
            None,
            String::new(),
            String::new(),
            time::OffsetDateTime::now_utc(),
        );
        missions.snapshot = Some(paused);
        missions.align_scene(&mut scene);
        assert_eq!(scene.courants[0].task_state, Some(State::Running));
        assert!(missions.snapshot().is_none());
    }
}
