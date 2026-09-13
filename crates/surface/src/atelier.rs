//! État de l'espace de conversation, indépendant du dessin et du fil réseau.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use providers::stream::{ChatClient, Completion, StreamError};
use serde_json::{Value, json};
use tokio::sync::watch;

/// Page active de l'espace de travail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    /// Accueil et composition d'une demande.
    #[default]
    Accueil,
    /// Historique de la conversation courante.
    Conversation,
    /// Modèles réellement exposés par le moteur.
    Modeles,
    /// Tâches et isolation lues auprès des services.
    Activite,
}

/// Une demande et la réponse associée, y compris partielle en cas d'erreur.
#[derive(Debug, Clone)]
pub struct Tour {
    /// Texte fourni par la personne.
    pub demande: String,
    /// Modèle sélectionné pour ce tour.
    pub modele: String,
    /// Texte reçu progressivement.
    pub reponse: String,
    /// Succès mesuré ; absent pendant la génération ou en cas d'échec.
    pub mesure: Option<Completion>,
    /// Panne ou interruption à afficher avec le texte partiel.
    pub erreur: Option<String>,
}

/// Ce qu'un client officiel dit de lui-même, sondé sans jamais lire ses identifiants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCard {
    /// Nom du pilote : `claude-code`, `codex`, `gemini`.
    pub driver: String,
    /// Le programme est installé.
    pub present: bool,
    /// Version rendue par le programme, si sondée.
    pub version: Option<String>,
    /// État de connexion lisible, tel que le pilote le formule.
    pub connection: String,
    /// Une session est ouverte : le client peut travailler pour l'humain.
    pub connected: bool,
}

enum Evenement {
    Modeles(Result<Vec<String>, String>),
    Clients(Vec<ClientCard>),
    Fragment(u64, String),
    Fin(u64, Result<Completion, String>),
}

/// Contrôleur natif. Les messages passent par un canal ; aucune requête ne bloque son appelant.
pub struct Atelier {
    /// Page visible.
    pub page: Page,
    /// Demande en cours de rédaction.
    pub brouillon: String,
    /// Modèles découverts, jamais inventés.
    pub modeles: Vec<String>,
    /// Identifiant choisi.
    pub choisi: String,
    /// Conversation de cette session.
    pub tours: Vec<Tour>,
    /// Découverte en cours.
    pub decouverte: bool,
    /// Génération en cours.
    pub generation: bool,
    /// État d'erreur du moteur ou de la demande.
    pub erreur: Option<String>,
    /// Réduit les animations décoratives.
    pub mouvement_reduit: bool,
    /// La capture est une démonstration explicite.
    pub demonstration: bool,
    /// Base HTTP locale utilisée.
    pub endpoint: String,
    /// Les clients officiels, tels que sondés ; vide tant que la sonde n'a pas répondu.
    pub clients: Vec<ClientCard>,
    /// Une sonde des clients est partie ou a déjà répondu.
    pub clients_sondes: bool,
    tx: Sender<Evenement>,
    rx: Receiver<Evenement>,
    cancel: Option<watch::Sender<bool>>,
    numero: u64,
}

impl Atelier {
    /// Crée le contrôleur. La découverte explicite reste séparée pour les captures et les tests.
    #[must_use]
    pub fn nouveau(endpoint: String, demonstration: bool) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            page: Page::Accueil,
            brouillon: String::new(),
            modeles: Vec::new(),
            choisi: String::new(),
            tours: Vec::new(),
            decouverte: false,
            generation: false,
            erreur: None,
            mouvement_reduit: false,
            demonstration,
            endpoint,
            clients: Vec::new(),
            clients_sondes: false,
            tx,
            rx,
            cancel: None,
            numero: 0,
        }
    }

    /// Sonde une fois les clients officiels, en arrière-plan : version et état de session,
    /// par leurs propres commandes, sans lire ni copier leurs fichiers d'identifiants.
    ///
    /// Une scène de démonstration reçoit des cartes d'exemple, jamais une vraie sonde.
    pub fn sonder_les_clients(&mut self, ctx: &egui::Context) {
        if self.clients_sondes {
            return;
        }
        self.clients_sondes = true;
        if self.demonstration {
            self.clients = vec![
                ClientCard {
                    driver: "claude-code".into(),
                    present: true,
                    version: Some("exemple".into()),
                    connection: "connecté".into(),
                    connected: true,
                },
                ClientCard {
                    driver: "codex".into(),
                    present: true,
                    version: Some("exemple".into()),
                    connection: "connexion requise".into(),
                    connected: false,
                },
                ClientCard {
                    driver: "gemini".into(),
                    present: false,
                    version: None,
                    connection: "client absent".into(),
                    connected: false,
                },
            ];
            return;
        }
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            use providers::official::{ClientProfile, ConnectionState, OfficialDriver};
            let root = std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("/nonexistent"))
                .join(".local/state/prophet");
            let user = std::env::var("USER").unwrap_or_else(|_| "inconnu".to_owned());
            let cards = ClientProfile::all()
                .into_iter()
                .map(|profile| {
                    let diagnostic = OfficialDriver::new(profile, &root, &user).diagnostic();
                    ClientCard {
                        driver: diagnostic.driver,
                        present: diagnostic.executable.is_some(),
                        version: diagnostic.version,
                        connection: diagnostic.connection.label().to_owned(),
                        connected: diagnostic.connection == ConnectionState::Connected,
                    }
                })
                .collect();
            let _ = tx.send(Evenement::Clients(cards));
            ctx.request_repaint();
        });
    }

    /// Interroge le moteur en arrière-plan, avec un délai court pour une découverte.
    pub fn decouvrir(&mut self, ctx: &egui::Context) {
        if self.decouverte || self.demonstration {
            return;
        }
        self.decouverte = true;
        self.erreur = None;
        let tx = self.tx.clone();
        let endpoint = self.endpoint.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = execution().and_then(|runtime| {
                runtime.block_on(async {
                    ChatClient::new(&endpoint, Duration::from_secs(5))
                        .map_err(|e| e.to_string())?
                        .models()
                        .await
                        .map_err(|e| e.to_string())
                })
            });
            let _ = tx.send(Evenement::Modeles(result));
            ctx.request_repaint();
        });
    }

    /// Draine les réponses sans attendre ; les événements d'une ancienne génération sont ignorés.
    pub fn actualiser(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Evenement::Modeles(result) => {
                    self.decouverte = false;
                    match result {
                        Ok(models) => {
                            if !models.contains(&self.choisi) {
                                self.choisi = models.first().cloned().unwrap_or_default();
                            }
                            self.modeles = models;
                            self.erreur = None;
                        }
                        Err(error) => {
                            self.modeles.clear();
                            self.choisi.clear();
                            self.erreur = Some(error);
                        }
                    }
                }
                Evenement::Clients(cards) => {
                    self.clients = cards;
                }
                Evenement::Fragment(id, text) if id == self.numero => {
                    if let Some(tour) = self.tours.last_mut() {
                        tour.reponse.push_str(&text);
                    }
                }
                Evenement::Fin(id, result) if id == self.numero => {
                    self.generation = false;
                    self.cancel = None;
                    if let Some(tour) = self.tours.last_mut() {
                        match result {
                            Ok(completion) => {
                                tour.reponse.clone_from(&completion.text);
                                tour.mesure = Some(completion);
                            }
                            Err(error) => tour.erreur = Some(error),
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Envoie la demande avec les seuls tours antérieurs terminés correctement.
    pub fn envoyer(&mut self, ctx: &egui::Context) {
        if self.generation || self.demonstration || self.brouillon.trim().is_empty() {
            return;
        }
        if !self.modeles.contains(&self.choisi) {
            self.erreur = Some("Choisissez un modèle disponible avant d'envoyer.".to_owned());
            return;
        }
        let history = match self.historique() {
            Ok(history) => history,
            Err(error) => {
                self.erreur = Some(error);
                return;
            }
        };
        self.numero += 1;
        let id = self.numero;
        let model = self.choisi.clone();
        let endpoint = self.endpoint.clone();
        self.tours.push(Tour {
            demande: std::mem::take(&mut self.brouillon),
            modele: model.clone(),
            reponse: String::new(),
            mesure: None,
            erreur: None,
        });
        self.generation = true;
        self.erreur = None;
        self.page = Page::Conversation;
        let (cancel, receiver) = watch::channel(false);
        self.cancel = Some(cancel);
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = execution().and_then(|runtime| {
                runtime.block_on(async {
                    let client = ChatClient::new(&endpoint, Duration::from_secs(180))
                        .map_err(|e| e.to_string())?;
                    client
                        .generate(&model, &history, 2048, receiver, |text| {
                            let _ = tx.send(Evenement::Fragment(id, text.to_owned()));
                            ctx.request_repaint();
                        })
                        .await
                        .map_err(|e| match e {
                            StreamError::Cancelled => {
                                "Génération interrompue. La réponse est partielle.".to_owned()
                            }
                            other => other.to_string(),
                        })
                })
            });
            let _ = tx.send(Evenement::Fin(id, result));
            ctx.request_repaint();
        });
    }

    /// Demande l'interruption du transport. L'état actif ne disparaît qu'après son acquittement.
    pub fn interrompre(&mut self) {
        if let Some(cancel) = &self.cancel {
            let _ = cancel.send(true);
        }
    }

    /// Ouvre une conversation vide. Aucune réponse tardive ne peut y être ajoutée.
    pub fn nouvelle(&mut self) {
        self.interrompre();
        self.numero += 1;
        self.generation = false;
        self.cancel = None;
        self.tours.clear();
        self.brouillon.clear();
        self.erreur = None;
        self.page = Page::Accueil;
    }

    fn historique(&self) -> Result<Vec<Value>, String> {
        if self.brouillon.len() > 16_384 || self.tours.len() >= 32 {
            return Err("Cette conversation atteint sa limite. Ouvrez une nouvelle conversation ou raccourcissez la demande.".to_owned());
        }
        let mut history = Vec::new();
        let mut size = self.brouillon.len();
        for tour in &self.tours {
            if tour.mesure.is_some() {
                size += tour.demande.len() + tour.reponse.len();
                history.push(json!({"role":"user","content":tour.demande}));
                history.push(json!({"role":"assistant","content":tour.reponse}));
            }
        }
        if size > 32_768 {
            return Err(
                "Le contexte est trop long pour cette session. Ouvrez une nouvelle conversation."
                    .to_owned(),
            );
        }
        history.push(json!({"role":"user","content":self.brouillon}));
        Ok(history)
    }
}

impl Drop for Atelier {
    fn drop(&mut self) {
        self.interrompre();
    }
}

fn execution() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_reponse_tardive_ne_contamine_pas_la_nouvelle_conversation() {
        let mut atelier = Atelier::nouveau("http://127.0.0.1:1/v1".into(), false);
        atelier.nouvelle();
        atelier
            .tx
            .send(Evenement::Fragment(0, "ancien résultat".into()))
            .unwrap();
        atelier.actualiser();
        assert!(atelier.tours.is_empty());
        assert!(!atelier.generation);
    }

    #[test]
    fn le_modele_absent_et_le_contexte_trop_grand_ne_declenchent_pas_de_requete() {
        let mut atelier = Atelier::nouveau("http://127.0.0.1:1/v1".into(), false);
        atelier.brouillon = "Bonjour".into();
        atelier.envoyer(&egui::Context::default());
        assert!(!atelier.generation);
        assert!(atelier.erreur.is_some());
        atelier.brouillon = "a".repeat(16_385);
        assert!(atelier.historique().is_err());
    }
}
