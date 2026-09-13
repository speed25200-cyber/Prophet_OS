//! Lecture et commandes des missions, sans attente réseau dans la boucle de rendu.
use std::collections::HashSet;
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
    /// Publier dans les documents l'index exact examiné.
    Apply,
    /// Annuler une publication dont les documents n'ont pas changé depuis.
    Undo,
}

enum Reply {
    Inspect(u64, Result<Box<Inspection>, String>),
    Action(String, Action, Result<Value, String>),
    Trail(u64, Result<Vec<TrailEntry>, String>),
}

/// L'issue d'un appel d'outil, telle que le journal la raconte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Appel journalisé, résultat pas encore lu.
    Pending,
    /// Résultat confirmé.
    Ok,
    /// Résultat en erreur, avec son code.
    Error(String),
    /// Refus de politique, avec son motif.
    Denied(String),
}

/// Une étape du parcours réel d'une mission : un outil, sa cible contrôlée, son issue.
///
/// Rien de ce que l'agent a lu ou écrit n'y figure : le journal ne porte que la cible que
/// capd a contrôlée et une empreinte des arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrailEntry {
    /// Numéro de séquence du journal.
    pub seq: u64,
    /// Étape de la boucle agentique, si connue.
    pub step: Option<u32>,
    /// Nom de l'outil, ou de l'acte (`publication`, `annulation`).
    pub tool: String,
    /// Hôte, chemin absolu ou fenêtre visés.
    pub target: Option<String>,
    /// Ce qu'il en est advenu.
    pub outcome: Outcome,
}

/// Reconstruit le parcours à partir des événements du journal, dans l'ordre.
#[must_use]
pub fn trail_from(events: &[Value]) -> Vec<TrailEntry> {
    let mut trail: Vec<TrailEntry> = Vec::new();
    for event in events {
        let seq = event["seq"].as_u64().unwrap_or(0);
        let step = event["step"].as_u64().and_then(|s| u32::try_from(s).ok());
        let payload = &event["payload"];
        match event["kind"].as_str() {
            Some("tool.call") => trail.push(TrailEntry {
                seq,
                step,
                tool: payload["tool"].as_str().unwrap_or("outil").to_owned(),
                target: payload["target"].as_str().map(str::to_owned),
                outcome: Outcome::Pending,
            }),
            Some("tool.result") => {
                let tool = payload["tool"].as_str().unwrap_or_default();
                if let Some(entry) = trail
                    .iter_mut()
                    .rev()
                    .find(|e| e.tool == tool && e.outcome == Outcome::Pending)
                {
                    entry.outcome = if payload["ok"].as_bool().unwrap_or(false) {
                        Outcome::Ok
                    } else {
                        Outcome::Error(
                            payload["error_code"]
                                .as_str()
                                .unwrap_or("Erreur")
                                .to_owned(),
                        )
                    };
                }
            }
            Some("policy.deny") => trail.push(TrailEntry {
                seq,
                step,
                tool: "refus".into(),
                target: payload["path"]
                    .as_str()
                    .or_else(|| payload["target"].as_str())
                    .map(str::to_owned),
                outcome: Outcome::Denied(
                    payload["reason"].as_str().unwrap_or("politique").to_owned(),
                ),
            }),
            Some("fs.commit") => trail.push(TrailEntry {
                seq,
                step,
                tool: "publication".into(),
                target: None,
                outcome: Outcome::Ok,
            }),
            Some("fs.undo") => trail.push(TrailEntry {
                seq,
                step,
                tool: "annulation".into(),
                target: None,
                outcome: Outcome::Ok,
            }),
            _ => {}
        }
    }
    trail
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
    pub(crate) files: crate::file_review::Review,
    socket: Option<PathBuf>,
    ledger: Option<PathBuf>,
    trail: Vec<TrailEntry>,
    trail_reading: bool,
    trail_next: Instant,
    selected: Option<String>,
    revision: u64,
    next_read: Instant,
    reading: bool,
    action: bool,
    refresh_required: bool,
    snapshot: Option<Inspection>,
    error: Option<String>,
    notice: Option<Notice>,
    /// Lire à voix haute le résultat d'une mission qu'on regarde finir (ADR 0036).
    announce: bool,
    /// Missions déjà lues : une fin ne se dit qu'une fois.
    announced: HashSet<String>,
    /// La phrase à dire, prise par l'application à la prochaine image.
    announcement: Option<String>,
    tx: Sender<Reply>,
    rx: Receiver<Reply>,
}

impl Default for Missions {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            files: crate::file_review::Review::default(),
            socket: None,
            ledger: None,
            trail: Vec::new(),
            trail_reading: false,
            trail_next: Instant::now(),
            selected: None,
            revision: 0,
            next_read: Instant::now(),
            reading: false,
            action: false,
            refresh_required: false,
            snapshot: None,
            error: None,
            notice: None,
            announce: crate::preparation::speech_ready(),
            announced: HashSet::new(),
            announcement: None,
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

    /// Raccorde la lecture du journal, fournie par l'application, jamais par le modèle.
    pub fn brancher_journal(&mut self, socket: PathBuf) {
        self.ledger = Some(socket);
        self.trail_next = Instant::now();
    }

    /// Le parcours réel de la mission sélectionnée, tel que le journal le raconte.
    #[must_use]
    pub fn trail(&self) -> &[TrailEntry] {
        &self.trail
    }

    /// Observe une sélection. Les réponses d'une ancienne sélection ne remplacent pas la vue.
    pub fn select(&mut self, id: Option<&str>) {
        self.files.select(id);
        if self.selected.as_deref() != id {
            self.revision = self.revision.wrapping_add(1);
            self.selected = id.map(str::to_owned);
            self.snapshot = None;
            self.trail.clear();
            self.error = None;
            self.next_read = Instant::now();
            self.trail_next = Instant::now();
        }
    }

    /// Dépose les réponses disponibles et programme une lecture, sans bloquer l'appelant.
    pub fn update(&mut self) {
        self.files.update(self.socket.as_ref());
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
                                // Une mission qu'on regardait courir vient de finir : l'OS le
                                // dit, une fois. Une mission déjà finie quand on la choisit
                                // n'est pas relue : l'humain l'a sous les yeux.
                                if self.announce
                                    && info.task.state.is_terminal()
                                    && self.snapshot.as_ref().is_some_and(|avant| {
                                        avant.task.id == info.task.id
                                            && !avant.task.state.is_terminal()
                                    })
                                    && self.announced.insert(info.task.id.clone())
                                {
                                    let vide = json!({"state": info.task.state});
                                    self.announcement = Some(voice::resume_du_resultat(
                                        info.result.as_ref().unwrap_or(&vide),
                                    ));
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
                Reply::Trail(revision, result) => {
                    self.trail_reading = false;
                    if revision == self.revision
                        && let Ok(trail) = result
                    {
                        self.trail = trail;
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
                        Action::Apply if v["applied"].as_str() == Some(&id) => {
                            Ok("Versions publiées dans vos documents.")
                        }
                        Action::Undo if v["undone"].as_str() == Some(&id) => {
                            Ok("Publication annulée : vos documents ont retrouvé leurs versions initiales.")
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
        if !self.trail_reading
            && Instant::now() >= self.trail_next
            && let (Some(ledger), Some(id)) = (self.ledger.clone(), self.selected.clone())
        {
            self.trail_reading = true;
            self.trail_next = Instant::now() + Duration::from_secs(2);
            let tx = self.tx.clone();
            let revision = self.revision;
            std::thread::spawn(move || {
                let result = rpc(ledger, "ledger.query", json!({"task": id, "limit": 400}))
                    .and_then(|v| {
                        v.as_array()
                            .map(|events| trail_from(events))
                            .ok_or_else(|| "Journal illisible.".to_owned())
                    });
                let _ = tx.send(Reply::Trail(revision, result));
            });
        }
    }

    /// Détail de la mission actuellement sélectionnée.
    #[must_use]
    pub fn snapshot(&self) -> Option<&Inspection> {
        self.snapshot.as_ref()
    }

    /// Les résultats des missions qu'on regarde finir sont lus à voix haute.
    #[must_use]
    pub fn announce(&self) -> bool {
        self.announce
    }

    /// Active ou coupe la lecture des résultats.
    pub fn set_announce(&mut self, on: bool) {
        self.announce = on;
        if !on {
            self.announcement = None;
        }
    }

    /// La phrase que l'OS doit dire maintenant, s'il y en a une ; prise une seule fois.
    pub fn take_announcement(&mut self) -> Option<String> {
        self.announcement.take()
    }

    /// « Résultat » : dire maintenant où en est la mission choisie, même si la lecture des
    /// fins est coupée — c'est une demande explicite.
    pub fn announce_now(&mut self) {
        self.announcement = Some(match &self.snapshot {
            None => "Aucune mission n'est choisie.".to_owned(),
            Some(info) => match &info.result {
                Some(result) if info.task.state.is_terminal() => voice::resume_du_resultat(result),
                _ => voice::resume_du_resultat(&json!({"state": info.task.state})),
            },
        });
    }

    /// Ouvre une version conservée d'un changement reçu dans le résultat de cette mission.
    ///
    /// # Errors
    /// Mission non terminée, connexion absente ou chemin absent du résultat reçu.
    pub fn open_file(&mut self, path: &str) -> Result<(), String> {
        let info = self.snapshot.as_ref().ok_or("Détail de mission absent.")?;
        if info.task.state != State::Done
            || self.socket.is_none()
            || !info
                .result
                .as_ref()
                .and_then(|r| r["diff"]["changes"].as_array())
                .is_some_and(|changes| changes.iter().any(|c| c["path"].as_str() == Some(path)))
        {
            return Err("Ce fichier n'est pas disponible dans le résultat de la mission.".into());
        }
        self.files.open(path.into());
        Ok(())
    }

    /// Version vérifiée actuellement affichée ; absente pendant une lecture ou après erreur.
    #[must_use]
    pub fn file_review(&self) -> Option<&agentd::ChangeReview> {
        self.files.document.as_ref().map(|d| &d.review)
    }

    /// Échec de la dernière lecture de fichier, sans réutiliser son ancien aperçu.
    #[must_use]
    pub fn file_error(&self) -> Option<&str> {
        self.files.error.as_deref()
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
            Action::Apply => info.can_apply,
            Action::Undo => info.can_undo,
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
                Action::Apply => "task.apply",
                Action::Undo => "task.undo",
            };
            let result = rpc(socket, method, json!({"id":id}));
            let _ = tx.send(Reply::Action(id, action, result));
        });
        Ok(())
    }
}

pub(crate) fn rpc(socket: PathBuf, method: &str, params: Value) -> Result<Value, String> {
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
            publication: None,
            can_apply: false,
            can_undo: false,
            browsing: None,
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

    /// Une mission regardée pendant qu'elle court, puis finie : sa fin est dite, une fois. Une
    /// mission déjà finie quand on la choisit n'est pas lue, et l'écoute coupée ne dit rien.
    #[test]
    fn la_fin_d_une_mission_regardee_est_dite_une_fois() {
        let mut missions = Missions::default();
        missions.set_announce(true);
        missions.select(Some("a"));
        let inspect = |state: State, result: Option<Value>| {
            let mut info = inspection("a", state);
            info.result = result;
            info
        };
        missions
            .tx
            .send(Reply::Inspect(
                missions.revision,
                Ok(inspect(State::Running, None).into()),
            ))
            .unwrap();
        missions.update();
        assert!(missions.take_announcement().is_none());
        let fini = json!({
            "state": "done",
            "text": "La note de réunion est écrite dans vos documents.",
            "diff": {"changes": [{"path": "docs/note.md", "kind": "added"}]}
        });
        missions
            .tx
            .send(Reply::Inspect(
                missions.revision,
                Ok(inspect(State::Done, Some(fini.clone())).into()),
            ))
            .unwrap();
        missions.update();
        assert_eq!(
            missions.take_announcement().as_deref(),
            Some(
                "Mission terminée. La note de réunion est écrite dans vos documents. Un changement est à examiner."
            )
        );
        // Une relecture du même état ne redit rien.
        missions
            .tx
            .send(Reply::Inspect(
                missions.revision,
                Ok(inspect(State::Done, Some(fini.clone())).into()),
            ))
            .unwrap();
        missions.update();
        assert!(missions.take_announcement().is_none());
        // Une mission choisie déjà finie n'est pas lue.
        missions.select(Some("b"));
        let mut deja = inspection("b", State::Done);
        deja.result = Some(fini.clone());
        missions
            .tx
            .send(Reply::Inspect(missions.revision, Ok(deja.into())))
            .unwrap();
        missions.update();
        assert!(missions.take_announcement().is_none());
        // « Résultat », demandé : la mission choisie est dite, même finie avant le choix.
        missions.announce_now();
        assert_eq!(
            missions.take_announcement().as_deref(),
            Some(
                "Mission terminée. La note de réunion est écrite dans vos documents. Un changement est à examiner."
            )
        );
        // Écoute coupée : une fin regardée reste muette.
        missions.set_announce(false);
        missions.select(Some("c"));
        for state in [State::Running, State::Failed] {
            let mut info = inspection("c", state);
            info.result = Some(json!({"state": "failed", "reason": "budget épuisé"}));
            missions
                .tx
                .send(Reply::Inspect(missions.revision, Ok(info.into())))
                .unwrap();
            missions.update();
        }
        assert!(missions.take_announcement().is_none());
        // Mais « résultat » répond toujours : ici, une mission en cours.
        missions.announce_now();
        assert_eq!(
            missions.take_announcement().as_deref(),
            Some("Mission échouée. budget épuisé.")
        );
        missions.select(None);
        missions.announce_now();
        assert_eq!(
            missions.take_announcement().as_deref(),
            Some("Aucune mission n'est choisie.")
        );
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
    fn le_parcours_relie_chaque_appel_a_son_issue_sans_le_contenu() {
        let events = vec![
            json!({"seq":1,"step":1,"kind":"tool.call","payload":{"tool":"web.open","target":"exemple.fr","args_digest":"blake3:x"}}),
            json!({"seq":2,"step":1,"kind":"tool.result","payload":{"tool":"web.open","ok":true}}),
            json!({"seq":3,"step":2,"kind":"tool.call","payload":{"tool":"fs.write","target":"docs/note.txt"}}),
            json!({"seq":4,"step":2,"kind":"tool.result","payload":{"tool":"fs.write","ok":false,"error_code":"PolicyDenied"}}),
            json!({"seq":5,"kind":"policy.deny","payload":{"stage":"publish","path":"docs/note.txt","reason":"RevokedParent"}}),
            json!({"seq":6,"kind":"fs.commit","payload":{"added":1}}),
        ];
        let trail = trail_from(&events);
        assert_eq!(trail.len(), 4);
        assert_eq!(trail[0].tool, "web.open");
        assert_eq!(trail[0].target.as_deref(), Some("exemple.fr"));
        assert_eq!(trail[0].outcome, Outcome::Ok);
        assert_eq!(trail[1].outcome, Outcome::Error("PolicyDenied".into()));
        assert_eq!(trail[2].outcome, Outcome::Denied("RevokedParent".into()));
        assert_eq!(trail[3].tool, "publication");
    }

    #[test]
    fn publier_et_annuler_exigent_les_droits_recus_et_un_acquittement_nominatif() {
        let mut missions = Missions::default();
        missions.select(Some("a"));
        let mut done = inspection("a", State::Done);
        assert!(missions.command(Action::Apply).is_err(), "détail absent");
        missions.snapshot = Some(done.clone());
        assert!(
            missions.command(Action::Apply).is_err(),
            "sans can_apply, aucune publication n'est envoyée"
        );
        done.can_apply = true;
        done.publication = Some(agentd::WorkspaceState::Open);
        missions.snapshot = Some(done);
        // Sans socket, la commande est refusée avant tout envoi ; l'acquittement se vérifie seul.
        assert!(missions.command(Action::Apply).is_err());
        missions
            .tx
            .send(Reply::Action(
                "a".into(),
                Action::Apply,
                Ok(json!({"applied":"b","changes":{"added":1}})),
            ))
            .unwrap();
        missions.update();
        let notice = missions.notice().unwrap();
        assert!(
            notice.error,
            "un acquittement pour une autre mission ne vaut rien"
        );
        missions
            .tx
            .send(Reply::Action(
                "a".into(),
                Action::Undo,
                Ok(json!({"undone":"a","state":"rolled_back"})),
            ))
            .unwrap();
        missions.update();
        let notice = missions.notice().unwrap();
        assert!(!notice.error, "{}", notice.text);
        assert!(notice.text.contains("annulée"), "{}", notice.text);
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
