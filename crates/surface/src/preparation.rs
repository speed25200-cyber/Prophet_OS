//! Brouillon de mission, catalogue du service et confirmation asynchrone du plan.
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use agentd::preparation::{Options, ProfileView, Request};
use agentd::{Inspection, TaskPlan};
use serde_json::json;

enum Reply {
    Options(Result<Options, String>),
    Plan(Result<Box<TaskPlan>, String>),
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
            tx,
            rx,
        }
    }
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
            }
        }
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
                route: None,
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
            }),
            options: Some(Options {
                profiles: vec![ProfileView {
                    id: "docs".into(),
                    name: "Docs".into(),
                    description: String::new(),
                    models: vec!["nouveau".into()],
                    scopes: vec![],
                    grants: vec![],
                    limits: Default::default(),
                }],
                model_error: None,
                jev: None,
            }),
            ..Default::default()
        };
        preparation.reconcile();
        preparation.discover(&egui::Context::default());
        assert_eq!(preparation.model, "envoye");
        assert!(!preparation.loading());
        assert!(preparation.error().is_none());
    }
}
