//! Brouillon de mission, catalogue du service et confirmation asynchrone du plan.
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use agentd::preparation::{Options, ProfileView, Request};
use agentd::{Inspection, TaskPlan};
use serde_json::json;

enum Reply {
    Options(Result<Options, String>),
    Plan(Result<Box<TaskPlan>, String>),
    /// Une dictée : le texte transcrit en local, ou ce qui a manqué (ADR 0036).
    Dictation(Result<String, String>),
    /// Le fil d'écoute permanente s'est arrêté (`false`), sur demande ou sur erreur.
    Listening(bool),
    /// Un ordre bref dit après le mot d'activation : préparer, lancer, entendre le résultat.
    Order(voice::Ordre),
}

/// Préparation séparée du dialogue : aucun modèle ne décide du profil ou des droits.
pub struct Preparation {
    socket: Option<PathBuf>,
    /// Objectif conservé pendant la navigation.
    pub intent: String,
    /// Profil choisi dans le catalogue reçu.
    pub profile: String,
    /// Identifiant du modèle choisi.
    pub model: String,
    options: Option<Options>,
    loading: bool,
    pending: bool,
    attempt: Option<Request>,
    prepared: Option<TaskPlan>,
    error: Option<String>,
    /// Une dictée est en cours : le micro écoute puis whisper transcrit, hors du fil graphique.
    dictating: bool,
    /// Écoute permanente en cours : le drapeau qui l'arrête, tenu par son fil.
    listening: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Mot d'activation de l'écoute permanente.
    wake: String,
    /// Ordres dits pendant l'écoute, pris par la supervision dans l'ordre.
    orders: std::collections::VecDeque<voice::Ordre>,
    tx: Sender<Reply>,
    rx: Receiver<Reply>,
}

impl Default for Preparation {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            socket: None,
            intent: String::new(),
            profile: String::new(),
            model: String::new(),
            options: None,
            loading: false,
            pending: false,
            attempt: None,
            prepared: None,
            error: None,
            dictating: false,
            listening: None,
            wake: "prophète".into(),
            orders: std::collections::VecDeque::new(),
            tx,
            rx,
        }
    }
}

/// La parole est configurée sur cette machine (modèle et whisper.cpp présents).
#[must_use]
pub fn voice_ready() -> bool {
    voice::Tools::from_env().is_ok()
}

/// L'OS peut parler sur cette machine (Piper et une voix, en plus de la parole).
#[must_use]
pub fn speech_ready() -> bool {
    voice::Tools::from_env().is_ok_and(|tools| tools.can_speak())
}

impl Preparation {
    /// Branche le contrôleur au même service que les commandes de mission.
    #[must_use]
    pub fn connect(socket: PathBuf) -> Self {
        Self {
            socket: Some(socket),
            ..Default::default()
        }
    }

    /// Recharge les profils et modèles auprès du service, sans bloquer le dessin.
    pub fn discover(&mut self, ctx: &egui::Context) {
        if self.loading || self.pending || self.attempt.is_some() {
            return;
        }
        let Some(socket) = self.socket.clone() else {
            self.error =
                Some("La préparation de missions n'est pas connectée à un service.".into());
            return;
        };
        self.loading = true;
        self.error = None;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = crate::missions::rpc(socket, "task.options", json!({})).and_then(|v| {
                serde_json::from_value(v).map_err(|_| "Catalogue de mission illisible.".into())
            });
            let _ = tx.send(Reply::Options(result));
            ctx.request_repaint();
        });
    }

    /// Draine les réponses ; une erreur ne provoque jamais une seconde création.
    pub fn update(&mut self) {
        while let Ok(reply) = self.rx.try_recv() {
            match reply {
                Reply::Options(result) => {
                    self.loading = false;
                    match result {
                        Ok(options) => {
                            self.options = Some(options);
                            self.error = None;
                            self.reconcile();
                        }
                        Err(error) => {
                            self.options = None;
                            self.error = Some(error);
                        }
                    }
                }
                Reply::Plan(result) => {
                    self.pending = false;
                    match result.and_then(|plan| {
                        let request = self
                            .attempt
                            .as_ref()
                            .ok_or("Préparation sans référence.".to_owned())?;
                        if plan.task != request.id
                            || plan.intent != request.intent
                            || plan.choice.reference != format!("local:{}", request.model)
                        {
                            return Err("Le plan reçu ne correspond pas à votre demande.".into());
                        }
                        Ok(*plan)
                    }) {
                        Ok(plan) => {
                            self.prepared = Some(plan);
                            self.error = None;
                        }
                        Err(error) => {
                            self.error = Some(format!("Préparation non confirmée : {error}"))
                        }
                    }
                }
                Reply::Dictation(result) => {
                    self.dictating = false;
                    match result {
                        Ok(text) if text.trim().is_empty() => {
                            self.error = Some("Rien n'a été compris ; réessayez.".into());
                        }
                        Ok(text) => {
                            if !self.intent.is_empty()
                                && !self.intent.ends_with(char::is_whitespace)
                            {
                                self.intent.push(' ');
                            }
                            self.intent.push_str(text.trim());
                            self.error = None;
                        }
                        Err(error) => self.error = Some(format!("Dictée impossible : {error}")),
                    }
                }
                Reply::Listening(active) => {
                    if !active {
                        self.listening = None;
                    }
                }
                Reply::Order(ordre) => self.orders.push_back(ordre),
            }
        }
    }

    /// Écoute le micro `seconds` secondes puis transcrit en local ; le texte rejoint l'objectif
    /// à la réception, l'humain le relit avant tout envoi (ADR 0036).
    pub fn dictate(&mut self, ctx: &egui::Context, seconds: u32) {
        if self.dictating || self.pending || self.attempt.is_some() {
            return;
        }
        self.dictating = true;
        self.error = None;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let tools = voice::Tools::from_env().map_err(|e| e.to_string())?;
                let wav =
                    std::env::temp_dir().join(format!("prophet-dictee-{}.wav", std::process::id()));
                tools.record(seconds, &wav).map_err(|e| e.to_string())?;
                let transcript = tools.transcribe(&wav, None).map_err(|e| e.to_string());
                let _ = std::fs::remove_file(&wav);
                transcript.map(|t| t.text)
            })();
            let _ = tx.send(Reply::Dictation(result));
            ctx.request_repaint();
        });
    }

    /// Dictée en cours.
    #[must_use]
    pub const fn dictating(&self) -> bool {
        self.dictating
    }

    /// Bascule l'écoute permanente : un fil écoute le micro par tranches et ne retient qu'une
    /// phrase qui commence par le mot d'activation ; le reste rejoint l'objectif comme une
    /// dictée (ADR 0036). Un second appel arrête l'écoute à la fin de la tranche en cours.
    pub fn listen_toggle(&mut self, ctx: &egui::Context) {
        if let Some(stop) = self.listening.take() {
            stop.store(true, std::sync::atomic::Ordering::Release);
            return;
        }
        match voice::Tools::from_env() {
            Ok(tools) => self.listen_with(ctx, tools, 6),
            Err(e) => self.error = Some(format!("Écoute impossible : {e}")),
        }
    }

    /// Comme [`Self::listen_toggle`], avec des outils et une durée de tranche donnés (essais).
    pub fn listen_with(&mut self, ctx: &egui::Context, tools: voice::Tools, seconds: u32) {
        if self.listening.is_some() {
            return;
        }
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.listening = Some(stop.clone());
        self.error = None;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::Acquire) {
                let wav = std::env::temp_dir().join(format!(
                    "prophet-ecoute-{}-{}.wav",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_millis())
                ));
                let heard = tools
                    .record(seconds, &wav)
                    .and_then(|()| tools.transcribe(&wav, None));
                let _ = std::fs::remove_file(&wav);
                match heard {
                    Ok(transcript) => {
                        if let Some(intent) = voice::after_wake_word(&transcript.text, &wake) {
                            // Un ordre bref (préparer, lancer, résultat) va à la supervision ;
                            // tout le reste rejoint l'objectif que l'humain relit.
                            let _ = match voice::ordre_vocal(&intent) {
                                voice::Ordre::Intention => tx.send(Reply::Dictation(Ok(intent))),
                                ordre => tx.send(Reply::Order(ordre)),
                            };
                            ctx.request_repaint();
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Reply::Dictation(Err(e.to_string())));
                        break;
                    }
                }
            }
            let _ = tx.send(Reply::Listening(false));
            ctx.request_repaint();
        });
    }

    /// Écoute permanente en cours.
    #[must_use]
    pub fn listening(&self) -> bool {
        self.listening.is_some()
    }

    /// Mot d'activation de l'écoute permanente.
    #[must_use]
    pub fn wake(&self) -> &str {
        &self.wake
    }

    /// Le prochain ordre dit pendant l'écoute, s'il y en a un ; chacun n'est rendu qu'une fois.
    pub fn take_order(&mut self) -> Option<voice::Ordre> {
        self.orders.pop_front()
    }

    /// Conserve un choix présent ; ne reprend pas un modèle d'un autre profil par accident.
    pub fn reconcile(&mut self) {
        if self.attempt.is_some() {
            return;
        }
        let Some(options) = &self.options else {
            return;
        };
        if !options.profiles.iter().any(|p| p.id == self.profile) {
            self.profile = options
                .profiles
                .first()
                .map(|p| p.id.clone())
                .unwrap_or_default();
        }
        if let Some(profile) = options.profiles.iter().find(|p| p.id == self.profile)
            && !profile.models.contains(&self.model)
        {
            self.model = profile.models.first().cloned().unwrap_or_default();
        }
    }

    /// Profil actuel reçu du service.
    #[must_use]
    pub fn selected(&self) -> Option<&ProfileView> {
        self.options
            .as_ref()?
            .profiles
            .iter()
            .find(|p| p.id == self.profile)
    }
    /// Catalogue reçu et diagnostic du moteur.
    #[must_use]
    pub fn options(&self) -> Option<&Options> {
        self.options.as_ref()
    }
    /// Dernière erreur visible.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    /// Signale un refus local de formulaire.
    pub fn report_error(&mut self, error: String) {
        self.error = Some(error);
    }
    /// Découverte en cours.
    #[must_use]
    pub const fn loading(&self) -> bool {
        self.loading
    }
    /// Création ou récupération en cours.
    #[must_use]
    pub const fn pending(&self) -> bool {
        self.pending
    }
    /// Référence envoyée, conservée même après un délai dépassé.
    #[must_use]
    pub fn attempted_id(&self) -> Option<&str> {
        self.attempt.as_ref().map(|r| r.id.as_str())
    }
    /// Le plan confirmé est transmis une seule fois à l'inspecteur.
    pub fn take_prepared(&mut self) -> Option<TaskPlan> {
        self.prepared.take()
    }

    /// Crée un brouillon distinct par action explicite, sans abandonner une requête en vol.
    pub fn reset(&mut self) {
        if self.pending {
            return;
        }
        self.intent.clear();
        self.attempt = None;
        self.prepared = None;
        self.error = None;
    }

    /// Prépare sans lancer. L'identifiant ULID sert à retrouver une réponse incertaine.
    ///
    /// # Errors
    /// Brouillon invalide, service absent ou tentative déjà envoyée.
    pub fn submit(&mut self, ctx: &egui::Context) -> Result<(), String> {
        if self.pending || self.loading || self.attempt.is_some() {
            return Err("Ce brouillon a déjà été envoyé ou attend une réponse.".into());
        }
        let socket = self.socket.clone().ok_or("Service de mission absent.")?;
        let profile = self
            .selected()
            .ok_or("Choisissez un contexte de travail.")?;
        if !profile.models.contains(&self.model) {
            return Err("Choisissez un modèle disponible pour ce contexte.".into());
        }
        let request = Request {
            id: format!("mission-{}", ulid::Ulid::new()),
            intent: self.intent.trim().to_owned(),
            profile: self.profile.clone(),
            model: self.model.clone(),
            client: false,
        };
        request.validate()?;
        self.attempt = Some(request.clone());
        self.pending = true;
        self.error = None;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result =
                crate::missions::rpc(socket, "task.prepare", json!(request)).and_then(|v| {
                    serde_json::from_value::<TaskPlan>(v)
                        .map(Box::new)
                        .map_err(|_| "Plan reçu illisible.".into())
                });
            let _ = tx.send(Reply::Plan(result));
            ctx.request_repaint();
        });
        Ok(())
    }

    /// Recherche uniquement le plan de la tentative ; ne renvoie jamais task.prepare.
    pub fn recover(&mut self, ctx: &egui::Context) {
        if self.pending {
            return;
        }
        let (Some(socket), Some(request)) = (self.socket.clone(), self.attempt.clone()) else {
            return;
        };
        self.pending = true;
        self.error = None;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = crate::missions::rpc(socket, "task.inspect", json!({"id":request.id}))
                .and_then(|v| {
                    let info: Inspection = serde_json::from_value(v)
                        .map_err(|_| "Inspection illisible.".to_owned())?;
                    if info.task.id != request.id {
                        return Err("Référence de mission incohérente.".into());
                    }
                    info.plan
                        .map(Box::new)
                        .ok_or("Aucun plan conservé pour cette référence.".into())
                });
            let _ = tx.send(Reply::Plan(result));
            ctx.request_repaint();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn une_confirmation_incoherente_ne_permet_pas_de_selectionner_une_autre_mission() {
        let mut preparation = Preparation {
            attempt: Some(Request {
                id: "attendu".into(),
                intent: "Objectif humain".into(),
                profile: "docs".into(),
                model: "local".into(),
                client: false,
            }),
            pending: true,
            ..Default::default()
        };
        preparation
            .tx
            .send(Reply::Plan(Ok(Box::new(TaskPlan {
                task: "autre".into(),
                intent: "Objectif humain".into(),
                choice: providers::selection::Choice {
                    reference: "local:local".into(),
                    reason: String::new(),
                },
                sandbox_level: 0,
                grants: vec![],
                scopes: vec![],
                limits: Default::default(),
            }))))
            .unwrap();
        preparation.update();
        assert!(preparation.take_prepared().is_none());
        assert!(preparation.error().is_some());
        assert_eq!(preparation.attempted_id(), Some("attendu"));
        assert!(preparation.submit(&egui::Context::default()).is_err());
    }
    #[test]
    fn un_brouillon_en_vol_ne_peut_pas_etre_efface() {
        let mut preparation = Preparation {
            intent: "À conserver".into(),
            pending: true,
            ..Default::default()
        };
        preparation.reset();
        assert_eq!(preparation.intent, "À conserver");
    }

    #[test]
    fn le_modele_d_une_tentative_incertaine_reste_celui_qui_a_ete_envoye() {
        let mut preparation = Preparation {
            profile: "docs".into(),
            model: "envoye".into(),
            attempt: Some(Request {
                id: "tentative".into(),
                intent: "Objectif".into(),
                profile: "docs".into(),
                model: "envoye".into(),
                client: false,
            }),
            options: Some(Options {
                profiles: vec![ProfileView {
                    id: "docs".into(),
                    name: "Docs".into(),
                    description: String::new(),
                    models: vec!["nouveau".into()],
                    preferred: vec![],
                    roles: Default::default(),
                    scopes: vec![],
                    grants: vec![],
                    limits: Default::default(),
                    web: false,
                }],
                model_error: None,
                browser: None,
                pilot: None,
            }),
            ..Default::default()
        };
        preparation.reconcile();
        preparation.discover(&egui::Context::default());
        assert_eq!(preparation.model, "envoye");
        assert!(!preparation.loading());
        assert!(preparation.error().is_none());
    }

    #[test]
    fn une_dictee_rejoint_l_objectif_et_un_echec_est_dit() {
        let mut preparation = Preparation {
            intent: "Écris une note".into(),
            dictating: true,
            ..Default::default()
        };
        preparation
            .tx
            .send(Reply::Dictation(Ok("  dans mes documents.  ".into())))
            .unwrap();
        preparation.update();
        assert_eq!(preparation.intent, "Écris une note dans mes documents.");
        assert!(!preparation.dictating());
        assert!(preparation.error().is_none());

        preparation.dictating = true;
        preparation
            .tx
            .send(Reply::Dictation(Ok("   ".into())))
            .unwrap();
        preparation.update();
        assert_eq!(preparation.intent, "Écris une note dans mes documents.");
        assert!(
            preparation
                .error()
                .unwrap()
                .contains("Rien n'a été compris")
        );

        preparation.dictating = true;
        preparation
            .tx
            .send(Reply::Dictation(Err("aucun modèle de parole".into())))
            .unwrap();
        preparation.update();
        assert!(!preparation.dictating());
        assert!(preparation.error().unwrap().contains("Dictée impossible"));
        // Un brouillon déjà envoyé ne reçoit pas de dictée.
        preparation.pending = true;
        preparation.dictate(&egui::Context::default(), 3);
        assert!(!preparation.dictating());
    }

    /// Les ordres dits pendant l'écoute sont rendus une fois, dans l'ordre, sans toucher à
    /// l'objectif.
    #[test]
    fn les_ordres_dits_sont_rendus_une_fois_dans_l_ordre() {
        let mut preparation = Preparation {
            intent: "Objectif".into(),
            ..Default::default()
        };
        assert!(preparation.take_order().is_none());
        for ordre in [
            voice::Ordre::Preparer,
            voice::Ordre::Lancer,
            voice::Ordre::Resultat,
        ] {
            preparation.tx.send(Reply::Order(ordre)).unwrap();
        }
        preparation.update();
        assert_eq!(preparation.take_order(), Some(voice::Ordre::Preparer));
        assert_eq!(preparation.take_order(), Some(voice::Ordre::Lancer));
        assert_eq!(preparation.take_order(), Some(voice::Ordre::Resultat));
        assert!(preparation.take_order().is_none());
        assert_eq!(preparation.intent, "Objectif");
    }

    /// L'écoute permanente, avec le vrai Whisper et la voix de Piper : un faux enregistreur
    /// livre « Il fait beau » puis « Prophète, écris une note dans mes documents » ; seule la
    /// seconde rejoint l'objectif, et l'écoute s'arrête sur demande.
    #[test]
    #[ignore = "needs_voice_stack: PROPHET_WHISPER_MODEL, PROPHET_WHISPER, PROPHET_PIPER, PROPHET_PIPER_VOICE"]
    fn l_ecoute_permanente_ne_retient_que_la_phrase_qui_commence_par_le_mot() {
        let mut tools = voice::Tools::from_env().unwrap();
        tools.deterministic = true;
        // La langue est dite : la détection automatique de Whisper se trompe sur une phrase
        // courte de synthèse, et l'objectif restait vide (vu en CI le 14 septembre 2026).
        tools.language = Some("fr".into());
        assert!(tools.can_speak(), "{tools:?}");
        let temp = tempfile::tempdir().unwrap();
        let bruit = temp.path().join("bruit.wav");
        let ordre = temp.path().join("ordre.wav");
        let lance = temp.path().join("lance.wav");
        tools.speak("Il fait beau aujourd'hui.", &bruit).unwrap();
        tools
            .speak("Prophète, écris une note dans mes documents.", &ordre)
            .unwrap();
        tools.speak("Prophète, lance la mission.", &lance).unwrap();
        let compteur = temp.path().join("appels");
        let script = temp.path().join("faux-pw-record.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nset -e\nn=0; [ -f {c} ] && n=$(cat {c}); n=$((n+1)); echo $n > {c}\n\
                 for last; do :; done\n\
                 case \"$n\" in 1) cp {b} \"$last\";; 2) cp {o} \"$last\";; *) cp {l} \"$last\";; esac\n",
                c = compteur.display(),
                b = bruit.display(),
                o = ordre.display(),
                l = lance.display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        tools.recorder = Some(voice::Recorder::PipeWire(script));
        let mut preparation = Preparation {
            intent: "Objectif :".into(),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        preparation.listen_with(&ctx, tools, 1);
        assert!(preparation.listening());
        let debut = std::time::Instant::now();
        while debut.elapsed() < std::time::Duration::from_secs(60) {
            preparation.update();
            if preparation.intent.to_lowercase().contains("note") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let intent = preparation.intent.to_lowercase();
        assert!(
            intent.contains("note") && intent.contains("documents"),
            "objectif « {intent} », erreur : {:?}",
            preparation.error()
        );
        assert!(!intent.contains("beau"), "{intent}");
        assert!(!intent.contains("proph"), "{intent}");
        assert!(intent.starts_with("objectif :"), "{intent}");
        // La tranche suivante est un ordre : il va à la supervision, pas à l'objectif.
        let debut = std::time::Instant::now();
        let mut ordre_recu = None;
        while ordre_recu.is_none() && debut.elapsed() < std::time::Duration::from_secs(60) {
            preparation.update();
            ordre_recu = preparation.take_order();
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        assert_eq!(ordre_recu, Some(voice::Ordre::Lancer));
        assert_eq!(preparation.intent.to_lowercase(), intent);
        // Arrêt sur demande : le fil termine sa tranche et le dit.
        preparation.listen_toggle(&ctx);
        let debut = std::time::Instant::now();
        while preparation.listening() && debut.elapsed() < std::time::Duration::from_secs(30) {
            preparation.update();
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        assert!(!preparation.listening());
    }
}
