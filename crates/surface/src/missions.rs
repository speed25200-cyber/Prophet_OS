//! Lecture et commandes des missions, sans attente réseau dans la boucle de rendu.
use std::collections::HashSet;
use std::path::{Path, PathBuf};
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
    /// Les événements lus depuis la relecture précédente, et s'il en reste à lire.
    Trail(u64, Result<(Vec<Value>, bool), String>),
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
    /// Événements du journal réunis sur cette ligne : des sorties réseau successives vers le
    /// même hôte, avec la même issue, n'en font qu'une. Un sinon.
    pub fois: u32,
}

/// Des sorties réseau successives vers le même hôte, par la même méthode et avec la même
/// issue, réunies sur la dernière ligne du parcours : un client officiel ouvre des dizaines de
/// connexions vers son éditeur par mission, et elles ne doivent pas recouvrir ses gestes.
struct Cumul {
    hote: String,
    methode: String,
    sortis: u64,
    recus: u64,
}

/// Reconstruit le parcours à partir des événements du journal, dans l'ordre.
#[must_use]
pub fn trail_from(events: &[Value]) -> Vec<TrailEntry> {
    let mut parcours = Parcours::default();
    parcours.ajouter(events);
    parcours.gestes
}

/// Le parcours d'une mission, construit événement par événement : chaque relecture du journal
/// y ajoute ce qui s'est inscrit depuis la précédente, sans relire le début de la mission.
#[derive(Default)]
pub(crate) struct Parcours {
    gestes: Vec<TrailEntry>,
    /// La sortie relayée que porte la dernière ligne, pour y ajouter les suivantes.
    cumul: Option<Cumul>,
    /// Le dernier événement lu.
    dernier: Option<u64>,
}

impl Parcours {
    pub(crate) fn gestes(&self) -> &[TrailEntry] {
        &self.gestes
    }

    /// Le premier numéro du journal qui reste à lire.
    pub(crate) fn suivant(&self) -> u64 {
        self.dernier.map_or(0, |d| d + 1)
    }

    /// Ajoute des événements lus dans l'ordre du journal ; un événement déjà lu est passé.
    pub(crate) fn ajouter(&mut self, events: &[Value]) {
        for event in events {
            if let Some(seq) = event["seq"].as_u64() {
                if self.dernier.is_some_and(|d| seq <= d) {
                    continue;
                }
                self.dernier = Some(seq);
            }
            self.ajouter_un(event);
        }
    }

    fn ajouter_un(&mut self, event: &Value) {
        let Self {
            gestes: trail,
            cumul,
            ..
        } = self;
        let avant = trail.len();
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
                fois: 1,
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
            Some("policy.deny") => {
                let reason = payload["reason"].as_str().unwrap_or("politique").to_owned();
                // Le refus d'un appel à la même étape est l'issue de cet appel, pas un geste de
                // plus : il s'y attache, avec le motif de capd, qu'il arrive avant ou après le
                // résultat d'outil.
                if let Some(entry) = trail.last_mut().filter(|e| {
                    step.is_some()
                        && e.step == step
                        && !matches!(e.tool.as_str(), "refus" | "rappel")
                        && matches!(&e.outcome, Outcome::Pending | Outcome::Error(_))
                }) {
                    entry.outcome = Outcome::Denied(reason);
                } else {
                    trail.push(TrailEntry {
                        seq,
                        step,
                        tool: "refus".into(),
                        target: payload["path"]
                            .as_str()
                            .or_else(|| payload["target"].as_str())
                            .map(str::to_owned),
                        outcome: Outcome::Denied(reason),
                        fois: 1,
                    });
                }
            }
            Some("fs.commit") => trail.push(TrailEntry {
                seq,
                step,
                tool: "publication".into(),
                target: None,
                outcome: Outcome::Ok,
                fois: 1,
            }),
            Some("fs.undo") => trail.push(TrailEntry {
                seq,
                step,
                tool: "annulation".into(),
                target: None,
                outcome: Outcome::Ok,
                fois: 1,
            }),
            // Ce qui est sorti par egress, sous le jeton de la mission (ADR 0056) : l'hôte, la
            // méthode, les octets dans chaque sens, le statut. Jamais le contenu.
            Some("net.request") => {
                let hote = payload["host"]
                    .as_str()
                    .unwrap_or("hôte inconnu")
                    .to_owned();
                let methode = payload["method"].as_str().unwrap_or("?").to_owned();
                let sortis = payload["bytes_out"].as_u64().unwrap_or(0);
                let recus = payload["bytes_in"].as_u64().unwrap_or(0);
                let outcome = match payload["status"].as_u64() {
                    Some(statut) if !(200..400).contains(&statut) => {
                        Outcome::Error(format!("HTTP {statut}"))
                    }
                    _ => Outcome::Ok,
                };
                let suite = trail.last_mut().zip(cumul.as_mut()).filter(|(e, c)| {
                    e.tool == "sortie"
                        && e.outcome == outcome
                        && c.hote == hote
                        && c.methode == methode
                });
                if let Some((entry, c)) = suite {
                    c.sortis += sortis;
                    c.recus += recus;
                    entry.fois += 1;
                    entry.target = Some(sortie_lisible(
                        &c.hote, &c.methode, entry.fois, c.sortis, c.recus,
                    ));
                } else {
                    trail.push(TrailEntry {
                        seq,
                        step,
                        tool: "sortie".into(),
                        target: Some(sortie_lisible(&hote, &methode, 1, sortis, recus)),
                        outcome,
                        fois: 1,
                    });
                    *cumul = Some(Cumul {
                        hote,
                        methode,
                        sortis,
                        recus,
                    });
                }
            }
            Some(genre @ ("net.deny" | "net.exfil_suspected")) => {
                let hote = payload["host"].as_str().map(str::to_owned);
                let outcome = Outcome::Denied(if genre == "net.exfil_suspected" {
                    format!(
                        "exfiltration suspectée : {}",
                        payload["reason"].as_str().unwrap_or("signaux relevés")
                    )
                } else {
                    payload["reason"].as_str().unwrap_or("refusé").to_owned()
                });
                // Un refus répété vers le même hôte, pour le même motif, se compte sur sa ligne.
                if let Some(entry) = trail.last_mut().filter(|e| {
                    e.tool == "sortie" && e.outcome == outcome && e.target == hote && hote.is_some()
                }) {
                    entry.fois += 1;
                } else {
                    trail.push(TrailEntry {
                        seq,
                        step,
                        tool: "sortie".into(),
                        target: hote,
                        outcome,
                        fois: 1,
                    });
                }
            }
            // Le service a rappelé au modèle un fichier que l'objectif demande (ADR 0049).
            Some("task.reminded") => trail.push(TrailEntry {
                seq,
                step,
                tool: "rappel".into(),
                target: payload["missing"].as_array().map(|manquants| {
                    manquants
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                }),
                outcome: Outcome::Ok,
                fois: 1,
            }),
            _ => {}
        }
        // Une autre ligne s'est posée : la sortie cumulée n'est plus la dernière.
        if trail.len() != avant && !matches!(event["kind"].as_str(), Some("net.request")) {
            *cumul = None;
        }
    }
}

/// Taille d'une page de lecture du journal.
const PAGE_DU_JOURNAL: usize = 400;

/// Pages lues au plus par relecture : une longue mission se rattrape en quelques relectures
/// rapprochées, sans qu'une seule retienne longtemps le fil de lecture.
const PAGES_PAR_RELECTURE: usize = 10;

/// Lit les événements d'une mission à partir du numéro `depuis`, page par page, dans l'ordre
/// du journal : le journal rend les plus anciens d'abord, et une seule page de quatre cents
/// laissait la fin d'une longue mission hors de vue. Dit aussi s'il en reste à lire.
fn lire_la_suite(ledger: &Path, id: &str, mut depuis: u64) -> Result<(Vec<Value>, bool), String> {
    let mut lus = Vec::new();
    for _ in 0..PAGES_PAR_RELECTURE {
        let page = rpc(
            ledger.to_path_buf(),
            "ledger.query",
            json!({"task": id, "since_seq": depuis, "limit": PAGE_DU_JOURNAL}),
        )?;
        let page = page
            .as_array()
            .ok_or_else(|| "Journal illisible.".to_owned())?;
        let pleine = page.len() == PAGE_DU_JOURNAL;
        match page.last().and_then(|e| e["seq"].as_u64()) {
            Some(seq) => depuis = seq + 1,
            // Une page sans numéro ne permet pas d'avancer : on s'arrête là.
            None => return Ok((lus, false)),
        }
        lus.extend(page.iter().cloned());
        if !pleine {
            return Ok((lus, false));
        }
    }
    Ok((lus, true))
}

/// Une sortie réseau en une ligne : « api.exemple.fr — CONNECT, 12,4 Ko envoyés, 48 Ko reçus »,
/// ou « api.exemple.fr — 14 × CONNECT, … » pour des sorties réunies.
fn sortie_lisible(hote: &str, methode: &str, fois: u32, sortis: u64, recus: u64) -> String {
    let combien = if fois > 1 {
        format!("{fois} × ")
    } else {
        String::new()
    };
    format!(
        "{hote} — {combien}{methode}, {} envoyés, {} reçus",
        octets(sortis),
        octets(recus)
    )
}

/// Un volume dit comme on le lit : « 512 o », « 12,4 Ko », « 3,1 Mo ».
fn octets(n: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let decimal = |valeur: f64| format!("{valeur:.1}").replace('.', ",");
    match n {
        0..1_000 => format!("{n} o"),
        1_000..1_000_000 => format!("{} Ko", decimal(n as f64 / 1e3)),
        _ => format!("{} Mo", decimal(n as f64 / 1e6)),
    }
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
    parcours: Parcours,
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
    /// La décision déjà dite (mission et question) : une demande ne se dit qu'une fois.
    decision_dite: Option<String>,
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
            parcours: Parcours::default(),
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
            decision_dite: None,
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
        self.parcours.gestes()
    }

    /// Observe une sélection. Les réponses d'une ancienne sélection ne remplacent pas la vue.
    pub fn select(&mut self, id: Option<&str>) {
        self.files.select(id);
        if self.selected.as_deref() != id {
            self.revision = self.revision.wrapping_add(1);
            self.selected = id.map(str::to_owned);
            self.snapshot = None;
            self.parcours = Parcours::default();
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
                        && let Ok((evenements, reste)) = result
                    {
                        self.parcours.ajouter(&evenements);
                        // Une longue mission se rattrape sans attendre la relecture suivante.
                        if reste {
                            self.trail_next = Instant::now();
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
            let depuis = self.parcours.suivant();
            std::thread::spawn(move || {
                let result = lire_la_suite(&ledger, &id, depuis);
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

    /// Une décision qui attend l'humain se dit, une fois, si la lecture est active (ADR 0036,
    /// 0041) : la question, sa conséquence, le motif du modèle s'il en a donné un, et ce
    /// qu'on peut répondre à la voix. Une phrase déjà en attente passe d'abord ; la décision
    /// sera dite à l'image suivante. Sans décision, la suivante pourra se dire.
    pub fn dire_la_decision(&mut self, decision: Option<&crate::scene::Decision>) {
        let Some(d) = decision else {
            self.decision_dite = None;
            return;
        };
        let cle = format!("{}\0{}", d.tache, d.question);
        if !self.announce
            || self.decision_dite.as_deref() == Some(cle.as_str())
            || self.announcement.is_some()
        {
            return;
        }
        self.announcement = Some(phrase_de_decision(d));
        self.decision_dite = Some(cle);
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
    rpc_dans(socket, method, params, Duration::from_secs(5))
}

/// Un appel au service, avec son propre délai.
pub(crate) fn rpc_dans(
    socket: PathBuf,
    method: &str,
    params: Value,
    delai: Duration,
) -> Result<Value, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        tokio::time::timeout(delai, async {
            let client = prophet_ipc::Client::connect(socket)
                .await
                .map_err(|e| e.to_string())?;
            client.call(method, params).await.map_err(|e| e.message)
        })
        .await
        .map_err(|_| "Le service ne répond pas dans le délai.".to_owned())?
    })
}

/// Ce que l'OS dit d'une décision qui attend : la question, sa conséquence, le motif du
/// modèle s'il en a donné un, et ce que la voix peut répondre.
#[must_use]
pub fn phrase_de_decision(d: &crate::scene::Decision) -> String {
    let mut phrase = format!(
        "Décision demandée par la mission {}. {} {}",
        d.tache,
        d.question.trim_end_matches(['.', '?', '!', ' ']),
        d.consequence
    );
    if let Some(motif) = d.motif.as_deref().filter(|m| !m.trim().is_empty()) {
        phrase.push_str(&format!(" Le modèle dit : {}", motif.trim()));
        if !motif.trim().ends_with(['.', '!', '?']) {
            phrase.push('.');
        }
    }
    phrase.push_str(" Dites « accorde » ou « refuse ».");
    phrase
}

#[cfg(test)]
mod tests {
    #[test]
    fn une_decision_qui_attend_se_dit_une_fois_avec_le_motif_du_modele() {
        let mut missions = Missions::default();
        missions.set_announce(true);
        let decision = crate::scene::Decision {
            question: "Envoyer le paiement ?".to_owned(),
            consequence: "L'argent part et ne revient pas.".to_owned(),
            motif: Some("le tarif expire ce soir".to_owned()),
            tache: "t1".to_owned(),
            depuis_secondes: 3,
            irreversible: true,
        };
        missions.dire_la_decision(Some(&decision));
        assert_eq!(
            missions.take_announcement().as_deref(),
            Some(
                "Décision demandée par la mission t1. Envoyer le paiement L'argent part et ne revient pas. Le modèle dit : le tarif expire ce soir. Dites « accorde » ou « refuse »."
            )
        );
        // La même décision, image après image, ne se redit pas ; l'attente qui s'allonge non plus.
        let plus_tard = crate::scene::Decision {
            depuis_secondes: 40,
            ..decision.clone()
        };
        missions.dire_la_decision(Some(&plus_tard));
        assert!(missions.take_announcement().is_none());
        // Une autre question se dit ; sans motif, la phrase n'en invente pas.
        let autre = crate::scene::Decision {
            question: "Supprimer le dossier ?".to_owned(),
            motif: None,
            ..decision.clone()
        };
        missions.dire_la_decision(Some(&autre));
        let dite = missions.take_announcement().unwrap();
        assert!(
            dite.starts_with("Décision demandée par la mission t1. Supprimer le dossier"),
            "{dite}"
        );
        assert!(!dite.contains("Le modèle dit"), "{dite}");
        // Plus de décision, puis la même revient : elle se redit.
        missions.dire_la_decision(None);
        missions.dire_la_decision(Some(&decision));
        assert!(missions.take_announcement().is_some());
        // Lecture coupée : rien n'est dit.
        missions.set_announce(false);
        missions.dire_la_decision(None);
        missions.dire_la_decision(Some(&decision));
        assert!(missions.take_announcement().is_none());
    }

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
            json!({"seq":7,"step":3,"kind":"task.reminded","payload":{"missing":["~/docs/out/total.txt"],"nth":1}}),
            // Un refus de capd à l'étape d'un appel devient l'issue de cet appel, qu'il arrive
            // avant ou après le résultat d'outil.
            json!({"seq":8,"step":4,"kind":"tool.call","payload":{"tool":"fs.write","target":"ailleurs/x.txt"}}),
            json!({"seq":9,"step":4,"kind":"policy.deny","payload":{"reason":"NoGrant"}}),
            json!({"seq":10,"step":4,"kind":"tool.result","payload":{"tool":"fs.write","ok":false,"error_code":"PolicyDenied"}}),
            json!({"seq":11,"step":5,"kind":"tool.call","payload":{"tool":"fs.write","target":"ailleurs/y.txt"}}),
            json!({"seq":12,"step":5,"kind":"tool.result","payload":{"tool":"fs.write","ok":false,"error_code":"PolicyDenied"}}),
            json!({"seq":13,"step":5,"kind":"policy.deny","payload":{"reason":"NoGrant"}}),
        ];
        let trail = trail_from(&events);
        assert_eq!(trail.len(), 7);
        assert_eq!(trail[5].tool, "fs.write");
        assert_eq!(trail[5].outcome, Outcome::Denied("NoGrant".into()));
        assert_eq!(trail[6].outcome, Outcome::Denied("NoGrant".into()));
        assert_eq!(trail[4].tool, "rappel");
        assert_eq!(trail[4].target.as_deref(), Some("~/docs/out/total.txt"));
        assert_eq!(trail[0].tool, "web.open");
        assert_eq!(trail[0].target.as_deref(), Some("exemple.fr"));
        assert_eq!(trail[0].outcome, Outcome::Ok);
        assert_eq!(trail[1].outcome, Outcome::Error("PolicyDenied".into()));
        assert_eq!(trail[2].outcome, Outcome::Denied("RevokedParent".into()));
        assert_eq!(trail[3].tool, "publication");
    }

    #[test]
    fn les_sorties_reseau_prennent_place_dans_le_parcours() {
        let events = vec![
            json!({"seq":1,"kind":"net.request","actor":"egress","payload":{"host":"api.anthropic.com","port":443,"method":"CONNECT","bytes_out":12_400,"bytes_in":48_000,"status":200}}),
            json!({"seq":2,"kind":"net.request","actor":"egress","payload":{"host":"exemple.fr","port":80,"method":"GET","bytes_out":90,"bytes_in":512,"status":404}}),
            json!({"seq":3,"kind":"net.deny","actor":"egress","payload":{"host":"ailleurs.fr","reason":"no_grant"}}),
            json!({"seq":4,"kind":"net.exfil_suspected","actor":"egress","payload":{"host":"fuite.fr","reason":"volume inhabituel"}}),
        ];
        let trail = trail_from(&events);
        assert_eq!(trail.len(), 4);
        assert!(trail.iter().all(|e| e.tool == "sortie"));
        assert_eq!(
            trail[0].target.as_deref(),
            Some("api.anthropic.com — CONNECT, 12,4 Ko envoyés, 48,0 Ko reçus")
        );
        assert_eq!(trail[0].outcome, Outcome::Ok);
        assert_eq!(trail[1].outcome, Outcome::Error("HTTP 404".into()));
        assert_eq!(trail[2].target.as_deref(), Some("ailleurs.fr"));
        assert_eq!(trail[2].outcome, Outcome::Denied("no_grant".into()));
        assert_eq!(
            trail[3].outcome,
            Outcome::Denied("exfiltration suspectée : volume inhabituel".into())
        );
        assert_eq!(octets(0), "0 o");
        assert_eq!(octets(3_100_000), "3,1 Mo");
        assert!(trail.iter().all(|e| e.fois == 1));
    }

    #[test]
    fn des_sorties_successives_vers_le_meme_hote_se_reunissent_sur_une_ligne() {
        let requete = |seq: u64, hote: &str, statut: u64| json!({"seq":seq,"kind":"net.request","actor":"egress","payload":{"host":hote,"port":443,"method":"CONNECT","bytes_out":1_000,"bytes_in":4_000,"status":statut}});
        let refus = |seq: u64| json!({"seq":seq,"kind":"net.deny","actor":"egress","payload":{"host":"collecte.exemple","reason":"no_grant"}});
        let events = vec![
            requete(1, "api.anthropic.com", 200),
            requete(2, "api.anthropic.com", 200),
            requete(3, "api.anthropic.com", 200),
            // Une autre issue ouvre une autre ligne.
            requete(4, "api.anthropic.com", 503),
            refus(5),
            refus(6),
            // Un geste de l'agent coupe la suite : la chronologie tient.
            json!({"seq":7,"kind":"tool.call","step":1,"payload":{"tool":"fs.write","target":"/maison/note.txt"}}),
            requete(8, "api.anthropic.com", 200),
            requete(9, "claude.ai", 200),
        ];
        let trail = trail_from(&events);
        let lignes: Vec<_> = trail
            .iter()
            .map(|e| (e.target.clone().unwrap_or_default(), e.fois))
            .collect();
        assert_eq!(
            lignes,
            vec![
                (
                    "api.anthropic.com — 3 × CONNECT, 3,0 Ko envoyés, 12,0 Ko reçus".to_owned(),
                    3
                ),
                (
                    "api.anthropic.com — CONNECT, 1,0 Ko envoyés, 4,0 Ko reçus".to_owned(),
                    1
                ),
                ("collecte.exemple".to_owned(), 2),
                ("/maison/note.txt".to_owned(), 1),
                (
                    "api.anthropic.com — CONNECT, 1,0 Ko envoyés, 4,0 Ko reçus".to_owned(),
                    1
                ),
                (
                    "claude.ai — CONNECT, 1,0 Ko envoyés, 4,0 Ko reçus".to_owned(),
                    1
                ),
            ]
        );
        assert_eq!(trail[1].outcome, Outcome::Error("HTTP 503".into()));
        assert_eq!(trail[2].outcome, Outcome::Denied("no_grant".into()));
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
