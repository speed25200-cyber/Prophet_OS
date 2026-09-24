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

/// Une entrée du catalogue des poids : lue, ou refusée avec sa raison.
pub type Poids = Result<providers::weights::Weights, String>;

/// Le poids que le moteur dit servir, repéré dans le catalogue, et la fenêtre qu'il accorde.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Servi {
    /// Rang de l'entrée du catalogue que le moteur a chargée, s'il y en a une.
    pub rang: Option<usize>,
    /// Fenêtre de contexte par requête, en tokens.
    pub fenetre: Option<u64>,
}

/// Une entrée du catalogue des poids du système, telle qu'agentd la rend (`model.catalog`,
/// ADR 0046) : ce que le système sait télécharger, et ce que la machine en a.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct EntreeCatalogue {
    /// Identifiant au catalogue.
    pub id: String,
    /// Nom pour l'humain.
    pub name: String,
    /// Quantification annoncée.
    #[serde(default)]
    pub quantization: Option<String>,
    /// À quoi sert ce modèle ici.
    #[serde(default)]
    pub note: Option<String>,
    /// Taille exacte, quand le catalogue la connaît.
    #[serde(default)]
    pub bytes: Option<u64>,
    /// Posé et vérifié.
    #[serde(default)]
    pub installed: bool,
    /// Le fichier posé, quand il l'est.
    #[serde(default)]
    pub path: Option<std::path::PathBuf>,
    /// Déjà fourni par la configuration du système (le modèle par défaut, dans `/nix/store`).
    #[serde(default)]
    pub provided: Option<std::path::PathBuf>,
    /// Octets reçus d'un téléchargement interrompu, qui reprendra d'ici.
    #[serde(default)]
    pub partial_bytes: Option<u64>,
    /// Le dernier téléchargement depuis le démarrage d'agentd.
    #[serde(default)]
    pub pull: Option<SuiviDePoids>,
    /// La mémoire que le moteur réservera pour le servir, et où elle tombe sur cette machine
    /// (ADR 0047) : dite avant de télécharger.
    #[serde(default)]
    pub memory: Option<providers::memory::Assessment>,
    /// Ce que le routeur du moteur local en dit, s'il connaît ce fichier.
    #[serde(skip)]
    pub au_moteur: Option<AuMoteur>,
}

/// Un poids tel que le routeur du moteur local le connaît.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuMoteur {
    /// Le nom sous lequel le moteur le sert.
    pub nom: String,
    /// Chargé (`loaded`), en cours (`loading`), déchargé…
    pub etat: Option<String>,
}

impl AuMoteur {
    /// Le moteur le sert en ce moment.
    #[must_use]
    pub fn charge(&self) -> bool {
        self.etat.as_deref() == Some("loaded")
    }
}

/// Où en est un téléchargement.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct SuiviDePoids {
    /// `running`, `done`, `failed` ou `cancelled`.
    pub state: String,
    /// Octets reçus.
    #[serde(default)]
    pub received: u64,
    /// Taille totale, si elle est connue.
    #[serde(default)]
    pub total: Option<u64>,
    /// Motif d'un échec.
    #[serde(default)]
    pub error: Option<String>,
}

impl EntreeCatalogue {
    /// Un téléchargement de cette entrée est en cours.
    #[must_use]
    pub fn en_cours(&self) -> bool {
        self.pull.as_ref().is_some_and(|p| p.state == "running")
    }
}

/// Ce que la page Modèles peut demander pour un poids du catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandeDePoids {
    /// Télécharger, ou reprendre.
    Telecharger,
    /// Arrêter un téléchargement en cours.
    Arreter,
    /// Retirer un poids téléchargé.
    Retirer,
    /// Le faire charger par le routeur du moteur local.
    Servir,
}

impl CommandeDePoids {
    const fn methode(self) -> Option<&'static str> {
        match self {
            Self::Telecharger => Some("model.pull"),
            Self::Arreter => Some("model.cancel"),
            Self::Retirer => Some("model.remove"),
            Self::Servir => None,
        }
    }
}

/// Entre deux relectures du catalogue pendant un téléchargement.
const RELECTURE_DU_CATALOGUE: Duration = Duration::from_millis(500);
/// Entre deux relectures du catalogue au repos, à l'image suivante.
const RELECTURE_AU_REPOS: Duration = Duration::from_secs(5);

enum Evenement {
    Modeles(Result<Vec<String>, String>),
    Poids(Vec<Poids>, Option<Servi>, Option<providers::memory::System>),
    Instances(
        Vec<(std::path::PathBuf, providers::memory::Resident)>,
        Option<providers::memory::System>,
    ),
    Catalogue(Result<Vec<EntreeCatalogue>, String>),
    ErreurDePoids(String),
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
    /// Dossier des poids lu par le catalogue (`PROPHET_MODELS_DIR`, sinon celui de l'image).
    pub dossier_des_poids: std::path::PathBuf,
    /// Fichiers de poids que la configuration nomme hors du dossier (`PROPHET_WEIGHTS`).
    pub fichiers_de_poids: Vec<std::path::PathBuf>,
    /// Le catalogue des poids, tel que lu ; vide tant que la lecture n'a pas répondu.
    pub poids: Vec<Poids>,
    /// Le catalogue a été lu (ou n'a pas à l'être, dans une scène d'exemple).
    pub poids_lus: bool,
    /// Ce que le moteur sert, s'il a répondu à la lecture du catalogue.
    pub servi: Option<Servi>,
    /// La mémoire de la machine, relue avec les poids : ce que chaque poids demande s'y mesure.
    pub memoire: Option<providers::memory::System>,
    /// La fenêtre avec laquelle le moteur de cette machine charge un poids.
    pub contexte_local: u64,
    /// Les instances du moteur et le poids que chacune tient, relues avec le catalogue : ce que
    /// la mémoire porte vraiment, à côté de l'estimation.
    pub instances: Vec<(std::path::PathBuf, providers::memory::Resident)>,
    /// Le catalogue du système, tel qu'agentd le rend ; `None` tant qu'il n'a pas répondu.
    pub catalogue: Option<Result<Vec<EntreeCatalogue>, String>>,
    /// Le dernier refus d'une commande sur un poids (téléchargement refusé par capd…).
    pub erreur_de_poids: Option<String>,
    /// Socket d'agentd (`PROPHET_AGENTD_SOCKET`, sinon le défaut).
    pub socket_agentd: std::path::PathBuf,
    lecture_du_catalogue: bool,
    catalogue_lu_a: Option<std::time::Instant>,
    /// Un poids vient d'être servi : la liste des modèles est à relire.
    redecouvrir: bool,
    lecture_des_poids: bool,
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
            dossier_des_poids: providers::weights::dir(),
            fichiers_de_poids: providers::weights::configured(),
            poids: Vec::new(),
            poids_lus: false,
            servi: None,
            memoire: None,
            contexte_local: providers::memory::context(),
            instances: Vec::new(),
            catalogue: None,
            erreur_de_poids: None,
            socket_agentd: std::env::var_os("PROPHET_AGENTD_SOCKET").map_or_else(
                || prophet_ipc::socket_path("agentd"),
                std::path::PathBuf::from,
            ),
            lecture_du_catalogue: false,
            catalogue_lu_a: None,
            redecouvrir: false,
            lecture_des_poids: false,
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

    /// Lit une fois, en arrière-plan, ce que chaque fichier de poids dit de lui-même. Une scène
    /// de démonstration ne lit rien : elle n'a pas de machine à décrire.
    pub fn lire_les_poids(&mut self, ctx: &egui::Context) {
        if self.lecture_des_poids {
            return;
        }
        self.lecture_des_poids = true;
        if self.demonstration {
            self.poids_lus = true;
            return;
        }
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let dossier = self.dossier_des_poids.clone();
        let fichiers = self.fichiers_de_poids.clone();
        let endpoint = self.endpoint.clone();
        std::thread::spawn(move || {
            let poids = providers::weights::installed(&dossier, &fichiers);
            // Le moteur dit quel fichier il a chargé et sa fenêtre par requête ; muet, la page
            // montre le catalogue seul.
            let servi =
                providers::local::LocalModel::new(&endpoint, "catalogue", Duration::from_secs(2))
                    .and_then(|engine| engine.served())
                    .ok()
                    .map(|servi| {
                        let charge = servi.path.and_then(|p| std::fs::canonicalize(p).ok());
                        Servi {
                            rang: charge.and_then(|charge| {
                                poids.iter().position(|w| {
                                    w.as_ref().is_ok_and(|w| {
                                        std::fs::canonicalize(&w.path).ok().as_ref()
                                            == Some(&charge)
                                    })
                                })
                            }),
                            fenetre: servi.n_ctx,
                        }
                    });
            let _ = tx.send(Evenement::Poids(poids, servi, providers::memory::system()));
            ctx.request_repaint();
        });
    }

    /// Lit le catalogue du système auprès d'agentd, en arrière-plan : une fois, puis toutes les
    /// demi-secondes tant qu'un téléchargement court — la page se redessine pour le suivre —, et
    /// au repos toutes les cinq secondes au plus, à l'image suivante, sans en demander : un
    /// téléchargement lancé d'ailleurs (`prophet model pull`) finit par s'y voir. Une scène de
    /// démonstration reçoit un catalogue d'exemple : un poids posé, un autre en cours.
    pub fn lire_le_catalogue(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.redecouvrir) {
            self.decouvrir(ctx);
        }
        let en_cours =
            matches!(&self.catalogue, Some(Ok(e)) if e.iter().any(EntreeCatalogue::en_cours));
        if en_cours {
            ctx.request_repaint_after(RELECTURE_DU_CATALOGUE);
        }
        let delai = if en_cours {
            RELECTURE_DU_CATALOGUE
        } else {
            RELECTURE_AU_REPOS
        };
        let due = self.catalogue_lu_a.is_none_or(|lu| lu.elapsed() >= delai);
        if self.lecture_du_catalogue || !due {
            return;
        }
        if self.demonstration {
            self.catalogue = Some(Ok(catalogue_d_exemple()));
            self.catalogue_lu_a = Some(std::time::Instant::now());
            return;
        }
        self.lecture_du_catalogue = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let socket = self.socket_agentd.clone();
        let endpoint = self.endpoint.clone();
        std::thread::spawn(move || {
            let lu = execution().and_then(|runtime| runtime.block_on(lire_catalogue(&socket)));
            let _ = tx.send(Evenement::Catalogue(lu.map(|e| au_moteur(e, &endpoint))));
            let _ = tx.send(Evenement::Instances(
                providers::memory::engine_instances(),
                providers::memory::system(),
            ));
            ctx.request_repaint();
        });
    }

    /// Envoie une commande sur un poids du catalogue à agentd, puis relit le catalogue.
    pub fn commander_un_poids(&mut self, ctx: &egui::Context, commande: CommandeDePoids, id: &str) {
        self.erreur_de_poids = None;
        if self.demonstration {
            return;
        }
        self.lecture_du_catalogue = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let socket = self.socket_agentd.clone();
        let endpoint = self.endpoint.clone();
        let au_moteur_avant = match &self.catalogue {
            Some(Ok(entrees)) => entrees
                .iter()
                .find(|e| e.id == id)
                .and_then(|e| e.au_moteur.clone()),
            _ => None,
        };
        let id = id.to_owned();
        std::thread::spawn(move || {
            if commande == CommandeDePoids::Servir {
                // Le routeur répond aussitôt ; le chargement se voit ensuite à l'état.
                let charge = au_moteur_avant
                    .ok_or_else(|| "le moteur ne connaît pas ce poids".to_owned())
                    .and_then(|m| {
                        providers::local::LocalModel::new(
                            &endpoint,
                            "catalogue",
                            Duration::from_secs(5),
                        )
                        .and_then(|moteur| moteur.load_model(&m.nom))
                        .map_err(|e| e.to_string())
                    });
                if let Err(erreur) = charge {
                    let _ = tx.send(Evenement::ErreurDePoids(format!("{id} : {erreur}")));
                }
            }
            let lu = execution().and_then(|runtime| {
                runtime.block_on(async {
                    if let Some(methode) = commande.methode()
                        && let Err(erreur) =
                            appeler_agentd(&socket, methode, json!({"id": id})).await
                    {
                        let _ = tx.send(Evenement::ErreurDePoids(format!("{id} : {erreur}")));
                    }
                    lire_catalogue(&socket).await
                })
            });
            let _ = tx.send(Evenement::Catalogue(lu.map(|e| au_moteur(e, &endpoint))));
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
                Evenement::Instances(instances, memoire) => {
                    self.instances = instances;
                    if memoire.is_some() {
                        self.memoire = memoire;
                    }
                }
                Evenement::Poids(poids, servi, memoire) => {
                    self.poids = poids;
                    self.servi = servi;
                    self.memoire = memoire;
                    self.poids_lus = true;
                }
                Evenement::Catalogue(lu) => {
                    let avant = self.poids_poses();
                    let servis_avant = self.poids_servis();
                    self.catalogue = Some(lu);
                    self.lecture_du_catalogue = false;
                    self.catalogue_lu_a = Some(std::time::Instant::now());
                    // Un poids posé ou retiré change les poids installés : on les relit.
                    if self.poids_poses() != avant && self.poids_lus {
                        self.lecture_des_poids = false;
                        self.poids_lus = false;
                    }
                    // Un poids que le moteur vient de charger est un modèle de plus pour le
                    // dialogue : la liste des modèles est relue à l'image suivante.
                    if self
                        .poids_servis()
                        .iter()
                        .any(|p| !servis_avant.contains(p))
                    {
                        self.redecouvrir = true;
                    }
                }
                Evenement::ErreurDePoids(erreur) => self.erreur_de_poids = Some(erreur),
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
        let (history, omis) = match self.historique() {
            Ok(envoi) => envoi,
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
                        // Ce que la page dit oublié : ce qui n'est pas parti, et ce que le
                        // client a retiré pour tenir dans la fenêtre du moteur.
                        .map(|completion| Completion {
                            forgotten: completion.forgotten + omis,
                            ..completion
                        })
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

    /// Ce qui part au moteur : les tours terminés les plus récents qui tiennent en 32 Kio, puis
    /// la demande ; rend aussi le nombre de messages laissés de côté, que la page dit. Le fil
    /// affiché, lui, reste entier.
    fn historique(&self) -> Result<(Vec<Value>, usize), String> {
        const BUDGET: usize = 32_768;
        if self.brouillon.len() > 16_384 || self.tours.len() >= 200 {
            return Err("Cette conversation atteint sa limite. Ouvrez une nouvelle conversation ou raccourcissez la demande.".to_owned());
        }
        let finis: Vec<&Tour> = self.tours.iter().filter(|t| t.mesure.is_some()).collect();
        let mut size = self.brouillon.len();
        let mut gardes = 0;
        for tour in finis.iter().rev() {
            let poids = tour.demande.len() + tour.reponse.len();
            if size + poids > BUDGET {
                break;
            }
            size += poids;
            gardes += 1;
        }
        let omis = (finis.len() - gardes) * 2;
        let mut history = Vec::new();
        for tour in &finis[finis.len() - gardes..] {
            history.push(json!({"role":"user","content":tour.demande}));
            history.push(json!({"role":"assistant","content":tour.reponse}));
        }
        history.push(json!({"role":"user","content":self.brouillon}));
        Ok((history, omis))
    }
}

impl Atelier {
    /// Les entrées du catalogue que le moteur sert en ce moment.
    fn poids_servis(&self) -> Vec<String> {
        match &self.catalogue {
            Some(Ok(entrees)) => entrees
                .iter()
                .filter(|e| e.au_moteur.as_ref().is_some_and(AuMoteur::charge))
                .map(|e| e.id.clone())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Les entrées du catalogue posées sur la machine.
    fn poids_poses(&self) -> Vec<String> {
        match &self.catalogue {
            Some(Ok(entrees)) => entrees
                .iter()
                .filter(|e| e.installed)
                .map(|e| e.id.clone())
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// Un catalogue d'exemple pour les scènes de démonstration : rien n'y est lu. La mémoire y est
/// celle d'une machine de 8 Gio dont 5 sont libres.
fn catalogue_d_exemple() -> Vec<EntreeCatalogue> {
    let machine = providers::memory::System {
        total: 8 << 30,
        available: 5 << 30,
    };
    let memoire = |octets, kv, vocabulaire| {
        providers::memory::need_from(octets, Some(kv), Some(vocabulaire), 4096)
            .map(|n| providers::memory::assess_need(n, Some(&machine)))
    };
    vec![
        EntreeCatalogue {
            id: "qwen3-1.7b-q8".into(),
            name: "Qwen3 1.7B".into(),
            quantization: Some("Q8_0".into()),
            note: Some("Le modèle de réflexion par défaut.".into()),
            bytes: Some(1_834_426_016),
            memory: memoire(1_834_426_016, 114_688, 151_936),
            installed: true,
            au_moteur: Some(AuMoteur {
                nom: "qwen3-1.7b".into(),
                etat: Some("loaded".into()),
            }),
            ..EntreeCatalogue::default()
        },
        EntreeCatalogue {
            id: "qwen3-0.6b-q8".into(),
            name: "Qwen3 0.6B".into(),
            quantization: Some("Q8_0".into()),
            note: Some("Le modèle d'exécution du relais.".into()),
            pull: Some(SuiviDePoids {
                state: "running".into(),
                received: 397_000_000,
                total: Some(640_000_000),
                error: None,
            }),
            memory: memoire(639_446_688, 114_688, 151_936),
            ..EntreeCatalogue::default()
        },
        EntreeCatalogue {
            id: "qwen3-8b-q4".into(),
            name: "Qwen3 8B".into(),
            quantization: Some("Q4_K_M".into()),
            note: Some("Plus de raisonnement ; demande presque toute la mémoire libre.".into()),
            bytes: Some(5_027_783_488),
            memory: memoire(5_027_783_488, 147_456, 151_936),
            ..EntreeCatalogue::default()
        },
    ]
}

async fn appeler_agentd(
    socket: &std::path::Path,
    methode: &str,
    params: Value,
) -> Result<Value, String> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let client = prophet_ipc::Client::connect(socket).await.map_err(|e| {
            format!(
                "agentd injoignable : {}",
                prophet_ipc::motif_de_connexion(&e)
            )
        })?;
        client.call(methode, params).await.map_err(|e| e.message)
    })
    .await
    .map_err(|_| "agentd ne répond pas".to_owned())?
}

/// Ce que le routeur du moteur local dit de chaque poids posé : sous quel nom il le sert, et
/// s'il est chargé. Un moteur muet, ou qui ne connaît pas le fichier, n'ajoute rien.
fn au_moteur(mut entrees: Vec<EntreeCatalogue>, endpoint: &str) -> Vec<EntreeCatalogue> {
    let Ok(modeles) =
        providers::local::LocalModel::new(endpoint, "catalogue", Duration::from_secs(2))
            .and_then(|moteur| moteur.router_models())
    else {
        return entrees;
    };
    for e in &mut entrees {
        let fichier = e.path.clone().or_else(|| e.provided.clone());
        e.au_moteur = fichier
            .and_then(|f| providers::local::router_model_for(&modeles, &f).cloned())
            .map(|m| AuMoteur {
                nom: m.id,
                etat: m.status,
            });
    }
    entrees
}

async fn lire_catalogue(socket: &std::path::Path) -> Result<Vec<EntreeCatalogue>, String> {
    let catalogue = appeler_agentd(socket, "model.catalog", json!({})).await?;
    serde_json::from_value(catalogue["entries"].clone())
        .map_err(|e| format!("catalogue illisible : {e}"))
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

    fn tour_fini(demande: &str, reponse: &str) -> Tour {
        Tour {
            demande: demande.into(),
            modele: "m".into(),
            reponse: reponse.into(),
            mesure: Some(Completion {
                text: reponse.into(),
                usage: providers::native::Usage {
                    tokens_in: 1,
                    tokens_out: 1,
                },
                elapsed: Duration::from_millis(1),
                first_token: None,
                forgotten: 0,
            }),
            erreur: None,
        }
    }

    #[test]
    fn une_longue_conversation_envoie_ses_tours_recents_au_lieu_de_s_arreter() {
        // Soixante échanges d'un kilo-octet : la conversation continue, avec les tours les plus
        // récents qui tiennent, et dit combien de messages sont restés de côté.
        let mut atelier = Atelier::nouveau("http://127.0.0.1:1/v1".into(), false);
        for i in 0..60 {
            atelier.tours.push(tour_fini(
                &format!("question {i} {}", "q".repeat(500)),
                &format!("réponse {i} {}", "r".repeat(500)),
            ));
        }
        atelier.brouillon = "Et maintenant ?".into();
        let (history, omis) = atelier.historique().unwrap();
        let taille: usize = history
            .iter()
            .map(|m| m["content"].as_str().unwrap().len())
            .sum();
        assert!(taille <= 32_768, "{taille}");
        assert!(omis > 0 && omis % 2 == 0, "{omis}");
        assert_eq!(history.len() + omis, 60 * 2 + 1);
        assert_eq!(history[0]["role"], "user");
        assert!(
            history[0]["content"]
                .as_str()
                .unwrap()
                .starts_with(&format!("question {}", omis / 2))
        );
        assert_eq!(history.last().unwrap()["content"], "Et maintenant ?");
        // Une conversation courte part entière.
        let mut courte = Atelier::nouveau("http://127.0.0.1:1/v1".into(), false);
        courte.tours.push(tour_fini("Bonjour", "Salut"));
        courte.brouillon = "Ça va ?".into();
        let (history, omis) = courte.historique().unwrap();
        assert_eq!((history.len(), omis), (3, 0));
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
