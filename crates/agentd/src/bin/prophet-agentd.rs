//! `prophet-agentd` — le runtime d'agents, en service.
//!
//! C'est le daemon qui tient les tâches : il les planifie, les suit, les annule, et raconte au
//! journal ce qu'il fait. Tout ce que la surface montre vient d'ici.
//!
//! Il n'émet pas les jetons. Il les demande à `capd`, et c'est important : une seule clé doit
//! signer les jetons de tout le système, sans quoi `egress` jugerait contrefaits les jetons
//! d'`agentd` — et il aurait raison. La bibliothèque sait faire les deux ; le service prend
//! toujours la seconde voie.
//!
//! Il n'écrit pas non plus le journal lui-même. Il pousse ses événements vers `prophet-ledger`,
//! qui est le seul écrivain, parce que le chaînage par hachage ne prouve quelque chose que s'il
//! existe une seule séquence de numéros.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use agentd::runtime::PlanRequest;
use agentd::{EtatPersistant, Publication, Runtime};
use prophet_daemon as commun;
use prophet_ipc::{Client, Error, ErrorCode, Handler, PeerIdentity, Server};
use prophet_types::cap::{Grant, Token};
use prophet_types::manifest::Manifest;
use providers::selection::Availability;
use serde_json::{Value, json};
use time::OffsetDateTime;
use tokio::sync::Mutex;

/// Une séance par mission ; `None` le temps qu'un retrait la conclue.
type Seances =
    Arc<std::sync::Mutex<BTreeMap<String, Arc<std::sync::Mutex<Option<agentd::local::Seance>>>>>>;

struct Agents {
    runtime: Arc<Mutex<Runtime>>,
    jobs: Arc<std::sync::Mutex<BTreeMap<String, Arc<AtomicBool>>>>,
    local_endpoint: Option<String>,
    profiles: Vec<agentd::preparation::Profile>,
    preparing: Mutex<()>,
    reviews: Arc<tokio::sync::Semaphore>,
    /// Une seule publication ou annulation à la fois : SFS verrouille le home, et un second
    /// appel doit recevoir une réponse claire plutôt qu'un refus de verrou.
    publications: Arc<tokio::sync::Semaphore>,
    capd: std::path::PathBuf,
    ledger: std::path::PathBuf,
    egress: std::path::PathBuf,
    /// Navigateur piloté par les outils web, absent par défaut.
    browser: Option<std::path::PathBuf>,
    /// Ce que la sonde de démarrage a dit du navigateur ; `None` si aucun n'est configuré.
    browser_state: Arc<std::sync::RwLock<Option<agentd::preparation::BrowserState>>>,
    /// Profils de navigation, un par tâche, dans l'état privé du service.
    browser_root: std::path::PathBuf,
    /// Socket de l'adaptateur d'accessibilité de la session humaine, s'il est configuré.
    sup_socket: Option<std::path::PathBuf>,
    /// Socket de sandboxd, pour les commandes confinées de `proc.exec`.
    sandboxd: std::path::PathBuf,
    /// Séances d'outils ouvertes pour des clients MCP, une par mission attachée.
    seances: Seances,
    /// Où l'état est écrit entre deux démarrages.
    etat: std::path::PathBuf,
    pairs: commun::Pairs,
}

impl Handler for Agents {
    async fn call(
        &self,
        pair: PeerIdentity,
        _auth: Option<String>,
        methode: String,
        params: Value,
    ) -> Result<Value, Error> {
        if methode != "ping" && !self.pairs.autorise(pair) {
            tracing::warn!(uid = pair.uid, gid = pair.gid, %methode, "pair refusé");
            return Err(self.pairs.refus());
        }

        match methode.as_str() {
            "ping" => Ok(json!("pong")),

            "task.options" => {
                let (models, model_error) = match self.local_models().await {
                    Ok(models) => (models, None),
                    Err(error) => (Vec::new(), Some(error.message)),
                };
                commun::repondre(&agentd::preparation::Options {
                    profiles: self.profiles.iter().map(|p| p.view(&models)).collect(),
                    model_error,
                    browser: self.browser_state(),
                })
            }

            "task.prepare" => {
                let request: agentd::preparation::Request = serde_json::from_value(params)
                    .map_err(|e| Error::new(ErrorCode::InvalidParams, e.to_string()))?;
                request
                    .validate()
                    .map_err(|e| Error::new(ErrorCode::InvalidParams, e))?;
                let profile = self
                    .profiles
                    .iter()
                    .find(|p| p.id == request.profile)
                    .ok_or_else(|| {
                        Error::new(ErrorCode::InvalidParams, "Profil de mission inconnu.")
                    })?;
                let reference = format!("local:{}", request.model);
                if !profile.manifest.model.preferred.contains(&reference) {
                    return Err(Error::new(
                        ErrorCode::PolicyDenied,
                        "Modèle non admis par ce profil.",
                    ));
                }
                // Un contexte web sans navigateur qui répond serait un plan qui échoue au
                // premier outil, une fois la mission lancée : on le dit avant d'émettre un jeton.
                if profile.uses_browser() {
                    match self.browser_state() {
                        Some(state) if state.ready => {}
                        Some(state) => {
                            return Err(Error::new(
                                ErrorCode::Conflict,
                                format!(
                                    "Ce contexte consulte le web par le navigateur piloté, qui ne répond pas : {}.",
                                    state.detail
                                ),
                            ));
                        }
                        None => {
                            return Err(Error::new(
                                ErrorCode::PolicyDenied,
                                "Ce contexte consulte le web par le navigateur piloté ; le service n'en configure aucun.",
                            ));
                        }
                    }
                }
                // Sérialise les préparations pour refuser une seconde émission pour le même id.
                let _preparing = self.preparing.lock().await;
                if self.runtime.lock().await.task(&request.id).is_some() {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Cette référence existe déjà. Relisez son plan.",
                    ));
                }
                // Pour un client MCP, le modèle est celui du client : le moteur local peut
                // être absent, et le modèle du profil n'a pas à y être découvert.
                let models = if request.client {
                    self.local_models().await.unwrap_or_default()
                } else {
                    self.local_models().await?
                };
                if !request.client && !models.contains(&request.model) {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Le modèle choisi n'est plus disponible.",
                    ));
                }
                let models = if request.client && !models.contains(&request.model) {
                    vec![request.model.clone()]
                } else {
                    models
                };
                let mut manifest = profile.manifest.clone();
                manifest.model.preferred = vec![reference];
                let grants = profile
                    .grants()
                    .map_err(|e| Error::new(ErrorCode::InternalError, e))?;
                let user = format!("uid:{}", pair.uid);
                let token = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    self.demander_un_jeton(&manifest, &request.id, &user, &grants),
                )
                .await
                .map_err(|_| {
                    Error::new(
                        ErrorCode::InternalError,
                        "capd ne répond pas dans le délai.",
                    )
                })??;
                let availability = Availability {
                    local_models: models,
                    ..Default::default()
                };
                let scopes: Vec<&str> = profile.scopes.iter().map(String::as_str).collect();
                let plan = {
                    let mut runtime = self.runtime.lock().await;
                    let plan = runtime
                        .plan_with_token(
                            &PlanRequest {
                                id: &request.id,
                                intent: &request.intent,
                                manifest: &manifest,
                                user: &user,
                                requested: &grants,
                                scopes: &scopes,
                                availability: &availability,
                            },
                            token,
                            OffsetDateTime::now_utc(),
                        )
                        .map_err(runtime_erreur)?;
                    runtime
                        .bind_owner(&request.id, pair.uid)
                        .map_err(|e| Error::new(ErrorCode::InternalError, e))?;
                    // Le rôle que ce modèle joue dans le relais du profil, s'il en a un : la
                    // mission le sait, le briefing et le compte par modèle s'y réfèrent.
                    let role = manifest
                        .model
                        .role_of(&plan.choice.reference)
                        .map(str::to_owned);
                    runtime.set_role(&request.id, role);
                    ecrire(&self.etat, &runtime.etat()).map_err(|e| {
                        Error::new(
                            ErrorCode::InternalError,
                            format!("Plan non confirmé sur disque : {e}"),
                        )
                    })?;
                    plan
                };
                self.vider_le_journal().await;
                commun::repondre(&plan)
            }

            // Planifier, c'est décider *avant* : quel pilote, quel niveau d'isolation, quelles
            // capacités, quel budget. Le plan est rendu tel quel pour que l'humain puisse dire non
            // en connaissance de cause, ce qui suppose que tout y soit.
            "task.spawn" => {
                let id = commun::texte(&params, "id")?;
                let _preparing = self.preparing.lock().await;
                if self.runtime.lock().await.task(&id).is_some() {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Cette référence de mission existe déjà.",
                    ));
                }
                let intention = commun::texte(&params, "intent")?;
                let utilisateur = commun::texte(&params, "user")?;
                let manifeste: Manifest = lire(&params, "manifest")?;
                let demandes: Vec<Grant> = lire(&params, "requested")?;
                // Absent veut dire « vide » ; illisible veut dire « erreur ». Les confondre par
                // un `unwrap_or_default` ferait planifier une tâche sans périmètre et sans pilote
                // disponible, puis échouer bien plus loin, sur un message sans rapport.
                let perimetres: Vec<String> = optionnel(&params, "scopes")?.unwrap_or_default();
                let disponible: Availability =
                    optionnel(&params, "availability")?.unwrap_or_default();

                // Le jeton vient de `capd`, et de nulle part ailleurs.
                let jeton = self
                    .demander_un_jeton(&manifeste, &id, &utilisateur, &demandes)
                    .await?;

                let maintenant = OffsetDateTime::now_utc();
                let refs: Vec<&str> = perimetres.iter().map(String::as_str).collect();
                let plan = {
                    let mut runtime = self.runtime.lock().await;
                    let plan = runtime
                        .plan_with_token(
                            &PlanRequest {
                                id: &id,
                                intent: &intention,
                                manifest: &manifeste,
                                user: &utilisateur,
                                requested: &demandes,
                                scopes: &refs,
                                availability: &disponible,
                            },
                            jeton,
                            maintenant,
                        )
                        .map_err(runtime_erreur)?;
                    runtime
                        .bind_owner(&id, pair.uid)
                        .map_err(|e| Error::new(ErrorCode::InternalError, e))?;
                    plan
                };
                self.enregistrer().await?;
                self.vider_le_journal().await;
                tracing::info!(tache = %id, pilote = %plan.choice.reference, "tâche planifiée");
                commun::repondre(&plan)
            }

            "task.list" => {
                let runtime = self.runtime.lock().await;
                commun::repondre(&runtime.tasks())
            }

            "task.status" => {
                let id = commun::texte(&params, "id")?;
                let runtime = self.runtime.lock().await;
                let tache = runtime.task(&id).ok_or_else(|| {
                    Error::new(ErrorCode::NotFound, format!("tâche inconnue : {id}"))
                })?;
                commun::repondre(tache)
            }

            "task.start" => self.start_local(commun::texte(&params, "id")?).await,

            "task.inspect" => {
                let id = commun::texte(&params, "id")?;
                let has_worker = self
                    .jobs
                    .lock()
                    .map_err(|_| {
                        Error::new(ErrorCode::InternalError, "travailleurs indisponibles")
                    })?
                    .contains_key(&id);
                let (inspection, owner, home) = {
                    let runtime = self.runtime.lock().await;
                    let inspection = runtime
                        .inspect(&id, self.local_endpoint.is_some(), has_worker)
                        .map_err(runtime_erreur)?;
                    (
                        inspection,
                        runtime.is_owner(&id, pair.uid),
                        runtime.home().to_path_buf(),
                    )
                };
                // L'état de publication vit dans SFS, pas dans le service : il est relu hors
                // du verrou, seulement pour une mission qui possède des versions conservées.
                let has_review = inspection
                    .result
                    .as_ref()
                    .is_some_and(|result| result.get("review").is_some());
                let observation =
                    mcp_system::tools::Browsing::observation_path(&self.browser_root, &id);
                let task = id.clone();
                let (publication, browsing) = tokio::task::spawn_blocking(move || {
                    let publication = has_review
                        .then(|| sfs::Workspace::open(&home, &task).ok())
                        .flatten()
                        .map(|workspace| workspace.state());
                    // Où l'agent navigue : déposé par les outils web, lu ici sans l'arbre.
                    let browsing = std::fs::read_to_string(observation)
                        .ok()
                        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
                    (publication, browsing)
                })
                .await
                .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))?;
                let mut inspection = inspection.with_publication(owner, publication);
                inspection.browsing = browsing;
                commun::repondre(&inspection)
            }

            // Le créateur publie l'index exact qu'il a examiné, ou annule cette publication.
            // La bibliothèque SFS relit les versions et refuse les conflits ; le service ne
            // fait qu'exiger l'identité, sérialiser les appels et consigner ce qui a eu lieu.
            "task.apply" => {
                self.publish(
                    commun::texte(&params, "id")?,
                    pair.uid,
                    Publication::Applied,
                )
                .await
            }

            "task.undo" => {
                self.publish(commun::texte(&params, "id")?, pair.uid, Publication::Undone)
                    .await
            }

            "task.result" => {
                let id = commun::texte(&params, "id")?;
                let runtime = self.runtime.lock().await;
                let result = runtime
                    .result(&id)
                    .ok_or_else(|| Error::new(ErrorCode::NotFound, "résultat non disponible"))?;
                Ok(result.clone())
            }

            "task.change" => {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Request {
                    id: String,
                    path: String,
                }
                let request: Request = serde_json::from_value(params)
                    .map_err(|e| Error::new(ErrorCode::InvalidParams, e.to_string()))?;
                if request.id.len() > 160 || request.path.len() > 4096 {
                    return Err(Error::new(
                        ErrorCode::InvalidParams,
                        "Référence ou chemin trop long.",
                    ));
                }
                let (home, index) = self
                    .runtime
                    .lock()
                    .await
                    .review_context(&request.id, pair.uid)
                    .map_err(|e| Error::new(ErrorCode::PolicyDenied, e))?;
                let task = request.id.clone();
                let permit = self.reviews.clone().try_acquire_owned().map_err(|_| {
                    Error::new(
                        ErrorCode::Conflict,
                        "Deux fichiers sont déjà en cours de lecture. Réessayez dans un instant.",
                    )
                })?;
                let file = tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    index.read(&home, &request.id, &request.path)
                })
                .await
                .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))?
                .map_err(|e| Error::new(ErrorCode::Conflict, e.to_string()))?;
                commun::repondre(&agentd::ChangeReview { task, file })
            }

            // Une mission active accuse réception de la demande ; son travailleur confirme
            // ensuite l'arrêt dans l'état final, après interruption de la requête au modèle.
            // Un client MCP de l'humain (Claude Code, Codex…) travaille dans une mission
            // préparée : le service tient le jeton, le registre, le travail SFS et le journal ;
            // le client ne reçoit que la liste des outils et leurs résultats.
            "task.attach" => {
                let id = commun::texte(&params, "id")?;
                let client = params
                    .get("client")
                    .and_then(Value::as_str)
                    .unwrap_or("client MCP")
                    .chars()
                    .take(120)
                    .collect::<String>();
                self.attach(id, client, pair.uid).await
            }
            "task.tools" => {
                let id = commun::texte(&params, "id")?;
                let seance = self.seance(&id, pair.uid).await?;
                let tools = hors_tokio("seance-outils", move || {
                    let guard = seance.lock().map_err(|_| seance_indisponible())?;
                    guard
                        .as_ref()
                        .map(agentd::local::Seance::tools)
                        .ok_or_else(seance_terminee)
                })
                .await??;
                Ok(json!({ "tools": tools }))
            }
            "task.call" => {
                let id = commun::texte(&params, "id")?;
                let name = commun::texte(&params, "name")?;
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                if !args.is_object() {
                    return Err(Error::new(
                        ErrorCode::InvalidParams,
                        "arguments : objet attendu",
                    ));
                }
                let seance = self.seance(&id, pair.uid).await?;
                let result = hors_tokio("seance-appel", move || {
                    let mut guard = seance.lock().map_err(|_| seance_indisponible())?;
                    guard
                        .as_mut()
                        .ok_or_else(seance_terminee)?
                        .call(&name, &args)
                        .map_err(|raison| Error::new(ErrorCode::Conflict, raison))
                })
                .await??;
                commun::repondre(&result)
            }
            "task.detach" => {
                let id = commun::texte(&params, "id")?;
                let text = params
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|t| t.chars().take(16_384).collect::<String>());
                let seance = self.seance(&id, pair.uid).await?;
                self.retirer(&id);
                hors_tokio("seance-retrait", move || {
                    let taken = seance
                        .lock()
                        .map_err(|_| seance_indisponible())?
                        .take()
                        .ok_or_else(seance_terminee)?;
                    taken.finish(text);
                    Ok::<(), Error>(())
                })
                .await??;
                self.vider_le_journal().await;
                tracing::info!(tache = %id, "séance d'outils conclue");
                Ok(json!({ "detached": id }))
            }

            "task.cancel" => {
                let id = commun::texte(&params, "id")?;
                let stop = self
                    .jobs
                    .lock()
                    .map_err(|_| {
                        Error::new(ErrorCode::InternalError, "travailleurs indisponibles")
                    })?
                    .get(&id)
                    .cloned();
                if let Some(stop) = stop {
                    stop.store(true, Ordering::Release);
                    // Une séance d'outils n'a pas de boucle qui verrait l'arrêt : le service la
                    // conclut lui-même, et le client apprend l'annulation à son prochain appel.
                    if let Some(seance) = self.retirer(&id) {
                        hors_tokio("seance-annulation", move || {
                            if let Ok(mut guard) = seance.lock()
                                && let Some(taken) = guard.take()
                            {
                                taken.abort("annulation demandée");
                            }
                        })
                        .await?;
                        self.vider_le_journal().await;
                    }
                    return Ok(json!({"cancel_requested":id}));
                }
                let maintenant = OffsetDateTime::now_utc();
                {
                    let mut runtime = self.runtime.lock().await;
                    runtime
                        .cancel_unstarted(&id, maintenant)
                        .map_err(runtime_erreur)?;
                }
                self.enregistrer().await?;
                self.vider_le_journal().await;
                tracing::info!(tache = %id, "tâche annulée");
                Ok(json!({ "cancelled": id }))
            }

            autre => Err(commun::methode_inconnue(autre)),
        }
    }
}

/// Exécute sur un thread propre, hors de Tokio : les services synchrones du registre (capd,
/// ledger) refusent de tourner sur un travailleur bloquant du runtime, comme une mission.
async fn hors_tokio<T: Send + 'static>(
    nom: &str,
    travail: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Error> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name(nom.to_owned())
        .spawn(move || {
            let _ = tx.send(travail());
        })
        .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))?;
    rx.await
        .map_err(|_| Error::new(ErrorCode::InternalError, "travailleur de séance interrompu"))
}

fn seance_indisponible() -> Error {
    Error::new(ErrorCode::InternalError, "séance d'outils indisponible")
}

fn seance_terminee() -> Error {
    Error::new(ErrorCode::Conflict, "cette séance d'outils est terminée")
}

impl Agents {
    /// La séance d'une mission, pour son créateur seulement.
    async fn seance(
        &self,
        id: &str,
        uid: u32,
    ) -> Result<Arc<std::sync::Mutex<Option<agentd::local::Seance>>>, Error> {
        if !self.runtime.lock().await.is_owner(id, uid) {
            return Err(Error::new(
                ErrorCode::Unauthorized,
                "Seul le créateur de la mission dispose de sa séance d'outils.",
            ));
        }
        self.seances
            .lock()
            .map_err(|_| seance_indisponible())?
            .get(id)
            .cloned()
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::NotFound,
                    "Aucune séance d'outils ouverte pour cette mission.",
                )
            })
    }

    /// Retire la séance et son travailleur ; ce qui reste à conclure l'est par l'appelant.
    fn retirer(&self, id: &str) -> Option<Arc<std::sync::Mutex<Option<agentd::local::Seance>>>> {
        let seance = self.seances.lock().ok()?.remove(id);
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.remove(id);
        }
        seance
    }

    /// Ouvre une séance d'outils sur une mission préparée par ce créateur.
    async fn attach(&self, id: String, client: String, uid: u32) -> Result<Value, Error> {
        if !self.runtime.lock().await.is_owner(&id, uid) {
            return Err(Error::new(
                ErrorCode::Unauthorized,
                "Seul le créateur d'une mission préparée peut y attacher un client.",
            ));
        }
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut jobs = self
                .jobs
                .lock()
                .map_err(|_| Error::new(ErrorCode::InternalError, "travailleurs indisponibles"))?;
            if jobs.len() >= 4 || jobs.contains_key(&id) {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "mission déjà lancée ou capacité de travail atteinte",
                ));
            }
            jobs.insert(id.clone(), stop.clone());
        }
        let launch = {
            let mut runtime = self.runtime.lock().await;
            match runtime.begin_local(&id) {
                Ok(launch) => match ecrire(&self.etat, &runtime.etat()) {
                    Ok(()) => Ok(launch),
                    Err(error) => {
                        let mut task = launch.0;
                        task.state = agentd::State::Failed;
                        task.reason = Some("lancement non persisté".into());
                        task.history.push(agentd::State::Failed);
                        runtime.publish_local(task, None);
                        Err(Error::new(
                            ErrorCode::InternalError,
                            format!("lancement non persisté : {error}"),
                        ))
                    }
                },
                Err(error) => Err(runtime_erreur(error)),
            }
        };
        let (task, token, plan, home) = match launch {
            Ok(value) => value,
            Err(error) => {
                self.retirer(&id);
                return Err(error);
            }
        };
        let state = self.runtime.clone();
        let path = self.etat.clone();
        let publish: agentd::local::Publish = Arc::new(move |task, result| {
            let mut runtime = state.blocking_lock();
            runtime.publish_local(task, result);
            ecrire(&path, &runtime.etat()).map_err(|e| format!("état non enregistré : {e}"))
        });
        let mission = agentd::local::Mission {
            task,
            token,
            plan,
            home,
            endpoint: self.local_endpoint.clone().unwrap_or_default(),
            services: mcp_system::services::Services::new(self.capd.clone(), self.ledger.clone()),
            egress: self.egress.clone(),
            browser: self.browser.clone(),
            browser_root: self.browser_root.clone(),
            sup_socket: self.sup_socket.clone(),
            delegate: Some(self.delegator()),
            sandboxd: self.sandboxd.clone(),
            // Une séance n'a pas de modèle côté service : la consigne n'aurait personne à qui
            // parler, le client de l'humain lit la description des outils.
            briefing: None,
            stop,
        };
        let opened =
            hors_tokio("seance-ouverture", move || mission.attach(publish, &client)).await?;
        self.vider_le_journal().await;
        match opened {
            Ok(seance) => {
                let tools = seance.tools().len();
                self.seances
                    .lock()
                    .map_err(|_| seance_indisponible())?
                    .insert(id.clone(), Arc::new(std::sync::Mutex::new(Some(seance))));
                tracing::info!(tache = %id, outils = tools, "séance d'outils ouverte");
                Ok(json!({ "attached": id, "tools": tools }))
            }
            Err(error) => {
                self.retirer(&id);
                Err(Error::new(ErrorCode::Conflict, error))
            }
        }
    }

    /// Dernier état connu du navigateur piloté, sans bloquer sur une sonde en cours.
    fn browser_state(&self) -> Option<agentd::preparation::BrowserState> {
        self.browser_state.read().ok().and_then(|s| s.clone())
    }

    async fn local_models(&self) -> Result<Vec<String>, Error> {
        let endpoint = self.local_endpoint.as_deref().ok_or_else(|| {
            Error::new(
                ErrorCode::Conflict,
                "Le moteur de mission local n'est pas configuré.",
            )
        })?;
        providers::stream::ChatClient::new(endpoint, std::time::Duration::from_secs(3))
            .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))?
            .models()
            .await
            .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))
    }

    /// Applique ou annule les versions examinées d'une mission, sous l'identité du créateur.
    ///
    /// Une publication ou une annulation interrompue laisse SFS en `applying` ou `undoing` ;
    /// la même commande reprend alors l'intention journalisée au lieu d'en créer une autre.
    async fn publish(&self, id: String, uid: u32, outcome: Publication) -> Result<Value, Error> {
        if id.len() > 160 {
            return Err(Error::new(
                ErrorCode::InvalidParams,
                "Référence trop longue.",
            ));
        }
        let (home, review, provenance) = self
            .runtime
            .lock()
            .await
            .publication_context(&id, uid)
            .map_err(|e| Error::new(ErrorCode::PolicyDenied, e))?;
        if outcome == Publication::Applied {
            self.autoriser_la_publication(&id, &home, &review).await?;
        }
        let permit = self.publications.clone().try_acquire_owned().map_err(|_| {
            Error::new(
                ErrorCode::Conflict,
                "Une publication est déjà en cours. Réessayez dans un instant.",
            )
        })?;
        let task = id.clone();
        let diff = tokio::task::spawn_blocking(move || {
            use sfs::WorkspaceState as W;
            let _permit = permit;
            let mut workspace = sfs::Workspace::open(&home, &task)?;
            let now = OffsetDateTime::now_utc();
            match (outcome, workspace.state()) {
                (Publication::Applied, W::Open) => {
                    workspace.commit_review(&review, now, Some(&provenance))
                }
                (Publication::Applied, W::Applying) | (Publication::Undone, W::Undoing) => {
                    workspace.recover_publication()
                }
                (Publication::Undone, W::Committed) => workspace.undo(),
                (_, state) => Err(sfs::SfsError::BadState { state }),
            }
        })
        .await
        .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))?
        .map_err(|e| publication_refusee(outcome, e))?;
        let maintenant = OffsetDateTime::now_utc();
        {
            let mut runtime = self.runtime.lock().await;
            runtime
                .record_publication(&id, outcome, &diff, maintenant)
                .map_err(runtime_erreur)?;
        }
        self.enregistrer().await?;
        self.vider_le_journal().await;
        let (added, modified, deleted) = diff.counts();
        let counts = json!({"added":added,"modified":modified,"deleted":deleted});
        match outcome {
            Publication::Applied => {
                tracing::info!(tache = %id, "versions publiées");
                Ok(json!({"applied":id,"changes":counts}))
            }
            Publication::Undone => {
                tracing::info!(tache = %id, "publication annulée");
                Ok(json!({"undone":id,"state":"rolled_back","changes":counts}))
            }
        }
    }

    /// Fait trancher capd sur chaque fichier de l'index exact avant la première mutation.
    ///
    /// Un jeton neuf est émis avec, pour seuls grants, les chemins de l'index : capd applique
    /// alors son plafond de manifeste, sa politique Cedar et la révocation de la mission au
    /// moment même où l'humain publie, sans dépendre de l'expiration du jeton de la mission.
    /// Un refus est journalisé avant d'être rendu, et rien n'a été écrit.
    async fn autoriser_la_publication(
        &self,
        id: &str,
        home: &std::path::Path,
        review: &sfs::ReviewIndex,
    ) -> Result<(), Error> {
        let (manifest, user, grants) = self
            .runtime
            .lock()
            .await
            .publication_grants(id, review)
            .map_err(|e| Error::new(ErrorCode::PolicyDenied, e))?;
        if grants.is_empty() {
            return Ok(());
        }
        let jeton = match self
            .jeton_avec_duree(&manifest, id, &user, &grants, 120)
            .await
        {
            Ok(jeton) => jeton,
            Err(erreur) => {
                self.refuser_la_publication(id, "*", &erreur.message).await;
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    format!("capd refuse la publication : {}", erreur.message),
                ));
            }
        };
        let client = Client::connect(&self.capd)
            .await
            .map_err(|e| Error::new(ErrorCode::InternalError, format!("capd injoignable ({e})")))?;
        for change in &review.diff().changes {
            let target = home.join(&change.path).display().to_string();
            let decision: prophet_types::cap::Decision = client
                .call(
                    "cap.check",
                    json!({
                        "token": jeton,
                        "res": prophet_types::cap::Res::Fs,
                        "act": prophet_types::cap::Act::Write,
                        "target": target,
                        "sandbox_level": 0,
                        "irreversible": false,
                        "external": false,
                        "context": prophet_types::cap::CheckContext::default(),
                    }),
                )
                .await
                .and_then(|v| {
                    serde_json::from_value(v)
                        .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))
                })?;
            if let prophet_types::cap::Decision::Deny { reason, rule } = decision {
                let motif = rule
                    .map(|rule| format!("{reason:?} ({rule})"))
                    .unwrap_or_else(|| format!("{reason:?}"));
                let chemin = change.path.display().to_string();
                self.refuser_la_publication(id, &chemin, &motif).await;
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    format!("capd refuse la publication de {chemin} : {motif}"),
                ));
            }
        }
        Ok(())
    }

    async fn refuser_la_publication(&self, id: &str, chemin: &str, motif: &str) {
        {
            let mut runtime = self.runtime.lock().await;
            runtime.record_publication_denied(id, chemin, motif, OffsetDateTime::now_utc());
        }
        self.vider_le_journal().await;
        tracing::warn!(tache = %id, chemin, motif, "publication refusée par capd");
    }

    async fn start_local(&self, id: String) -> Result<Value, Error> {
        let endpoint = self.local_endpoint.clone().ok_or_else(|| {
            Error::new(ErrorCode::Conflict, "moteur local du service non configuré")
        })?;
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut jobs = self
                .jobs
                .lock()
                .map_err(|_| Error::new(ErrorCode::InternalError, "travailleurs indisponibles"))?;
            if jobs.len() >= 2 || jobs.contains_key(&id) {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "mission déjà lancée ou capacité de travail atteinte",
                ));
            }
            jobs.insert(id.clone(), stop.clone());
        }
        let launch = {
            let mut runtime = self.runtime.lock().await;
            match runtime.begin_local(&id) {
                Ok(launch) => match ecrire(&self.etat, &runtime.etat()) {
                    Ok(()) => Ok(launch),
                    Err(error) => {
                        let mut task = launch.0;
                        task.state = agentd::State::Failed;
                        task.reason = Some("lancement non persisté".into());
                        task.history.push(agentd::State::Failed);
                        runtime.publish_local(task, None);
                        Err(Error::new(
                            ErrorCode::InternalError,
                            format!("lancement non persisté : {error}"),
                        ))
                    }
                },
                Err(error) => Err(runtime_erreur(error)),
            }
        };
        let (task, token, plan, home) = match launch {
            Ok(value) => value,
            Err(error) => {
                if let Ok(mut jobs) = self.jobs.lock() {
                    jobs.remove(&id);
                }
                return Err(error);
            }
        };
        let state = self.runtime.clone();
        let path = self.etat.clone();
        let publish: agentd::local::Publish = Arc::new(move |task, result| {
            let mut runtime = state.blocking_lock();
            runtime.publish_local(task, result);
            ecrire(&path, &runtime.etat()).map_err(|e| format!("état non enregistré : {e}"))
        });
        let jobs = self.jobs.clone();
        let job_id = id.clone();
        let failure_task = task.clone();
        let failure_publish = publish.clone();
        let briefing = {
            let runtime = self.runtime.lock().await;
            briefing_pour(&runtime, &id, &self.profiles)
        };
        let mission = agentd::local::Mission {
            task,
            token,
            plan,
            home,
            endpoint,
            services: mcp_system::services::Services::new(self.capd.clone(), self.ledger.clone()),
            egress: self.egress.clone(),
            browser: self.browser.clone(),
            browser_root: self.browser_root.clone(),
            sup_socket: self.sup_socket.clone(),
            delegate: Some(self.delegator()),
            sandboxd: self.sandboxd.clone(),
            briefing,
            stop,
        };
        if let Err(error) = std::thread::Builder::new()
            .name(format!("mission-{id}"))
            .spawn(move || {
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| mission.run(publish)))
                    .is_err()
                {
                    let mut task = failure_task;
                    task.state = agentd::State::Failed;
                    task.history.push(agentd::State::Failed);
                    task.reason = Some("travailleur interrompu de manière inattendue".into());
                    let _ = failure_publish(task, None);
                }
                if let Ok(mut jobs) = jobs.lock() {
                    jobs.remove(&job_id);
                }
            })
        {
            if let Ok(mut jobs) = self.jobs.lock() {
                jobs.remove(&id);
            }
            let mut runtime = self.runtime.lock().await;
            if let Some(mut task) = runtime.task(&id).cloned() {
                task.state = agentd::State::Failed;
                task.reason = Some("travailleur non lancé".into());
                task.history.push(agentd::State::Failed);
                runtime.publish_local(task, None);
            }
            ecrire(&self.etat, &runtime.etat())
                .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))?;
            return Err(Error::new(ErrorCode::InternalError, error.to_string()));
        }
        Ok(json!({"started":id,"state":"running"}))
    }

    /// Demande à `capd` le jeton racine de la tâche.
    async fn demander_un_jeton(
        &self,
        manifeste: &Manifest,
        tache: &str,
        utilisateur: &str,
        demandes: &[Grant],
    ) -> Result<Token, Error> {
        let duree = i64::try_from(manifeste.wall_time_seconds().unwrap_or(1200)).unwrap_or(1200);
        self.jeton_avec_duree(manifeste, tache, utilisateur, demandes, duree)
            .await
    }

    async fn jeton_avec_duree(
        &self,
        manifeste: &Manifest,
        tache: &str,
        utilisateur: &str,
        demandes: &[Grant],
        duree: i64,
    ) -> Result<Token, Error> {
        let client = Client::connect(&self.capd).await.map_err(|e| {
            Error::new(
                ErrorCode::InternalError,
                format!("capd injoignable ({e}) : aucune tâche ne peut être planifiée sans jeton"),
            )
        })?;
        let brut = client
            .call(
                "cap.mint",
                json!({
                    "manifest": manifeste,
                    "grants": demandes,
                    "task": tache,
                    "user": utilisateur,
                    "ttl_seconds": duree,
                }),
            )
            .await
            .map_err(|e| Error::new(e.code, e.message))?;
        serde_json::from_value(brut).map_err(|e| {
            Error::new(
                ErrorCode::InternalError,
                format!("jeton illisible rendu par capd : {e}"),
            )
        })
    }

    /// Écrit l'état sur disque, pour qu'un redémarrage ne l'efface pas.
    ///
    /// L'écriture passe par un fichier temporaire puis un renommage : un `systemctl restart` au
    /// mauvais moment laisserait sinon un fichier tronqué, et le daemon suivant refuserait de
    /// démarrer sur un état qu'il ne sait pas lire — perdant tout au lieu d'une écriture.
    async fn enregistrer(&self) -> Result<(), Error> {
        let runtime = self.runtime.lock().await;
        ecrire(&self.etat, &runtime.etat()).map_err(|e| {
            Error::new(
                ErrorCode::InternalError,
                format!("état non enregistré : {e}"),
            )
        })
    }

    /// Pousse les événements accumulés vers le journal.
    ///
    /// Un échec ici n'annule pas ce qui a été fait — la tâche existe — mais il est bruyant : un
    /// système qui agit sans laisser de trace a perdu ce qui permet de revenir en arrière.
    async fn vider_le_journal(&self) {
        let brouillons = {
            let mut runtime = self.runtime.lock().await;
            runtime.retirer_le_journal()
        };
        if brouillons.is_empty() {
            return;
        }
        let Ok(client) = Client::connect(&self.ledger).await else {
            tracing::error!(
                nombre = brouillons.len(),
                "journal injoignable : des événements ne sont pas écrits"
            );
            return;
        };
        for brouillon in brouillons {
            let params = json!({
                "kind": brouillon.kind,
                "task": brouillon.task,
                "step": brouillon.step,
                "actor": brouillon.actor.0,
                "payload": brouillon.payload,
            });
            if let Err(erreur) = client.call("ledger.append", params).await {
                tracing::error!(motif = %erreur.message, "un événement n'a pas été journalisé");
            }
        }
    }
}

/// Un paramètre facultatif : absent rend `None`, présent mais illisible rend une erreur.
fn optionnel<T: serde::de::DeserializeOwned>(
    params: &Value,
    nom: &str,
) -> Result<Option<T>, Error> {
    match params.get(nom) {
        None | Some(Value::Null) => Ok(None),
        Some(brut) => serde_json::from_value(brut.clone()).map(Some).map_err(|e| {
            Error::new(
                ErrorCode::InvalidParams,
                format!("« {nom} » invalide : {e}"),
            )
        }),
    }
}

/// Écriture atomique : un fichier voisin, puis un renommage.
impl Agents {
    /// La délégation que reçoivent les missions et les séances : tout ce qu'il faut pour créer,
    /// lancer et attendre une sous-mission, sans tenir le service lui-même.
    fn delegator(&self) -> agentd::local::Delegate {
        delegation_fn(Arc::new(DelegationContext {
            runtime: self.runtime.clone(),
            etat: self.etat.clone(),
            profiles: self.profiles.clone(),
            local_endpoint: self.local_endpoint.clone(),
            capd: self.capd.clone(),
            ledger: self.ledger.clone(),
            egress: self.egress.clone(),
            browser: self.browser.clone(),
            browser_root: self.browser_root.clone(),
            sup_socket: self.sup_socket.clone(),
            sandboxd: self.sandboxd.clone(),
            jobs: self.jobs.clone(),
        }))
    }
}

/// Ce qu'une délégation emploie du service, cloné pour vivre dans le fil de la mission.
struct DelegationContext {
    runtime: Arc<Mutex<Runtime>>,
    etat: std::path::PathBuf,
    profiles: Vec<agentd::preparation::Profile>,
    local_endpoint: Option<String>,
    capd: std::path::PathBuf,
    ledger: std::path::PathBuf,
    egress: std::path::PathBuf,
    browser: Option<std::path::PathBuf>,
    browser_root: std::path::PathBuf,
    sup_socket: Option<std::path::PathBuf>,
    sandboxd: std::path::PathBuf,
    jobs: Arc<std::sync::Mutex<BTreeMap<String, Arc<AtomicBool>>>>,
}

fn delegation_fn(ctx: Arc<DelegationContext>) -> agentd::local::Delegate {
    Arc::new(move |parent, token, request| deleguer(&ctx, parent, token, request))
}

/// La consigne du relais pour une mission qui se lance : son rôle et les contextes qu'elle
/// peut confier, avec les rôles qu'ils savent jouer (ADR 0034). `None` sans relais : la boucle
/// native reste alors ce qu'elle était.
fn briefing_pour(
    runtime: &Runtime,
    id: &str,
    profiles: &[agentd::preparation::Profile],
) -> Option<String> {
    let role = runtime.task(id).and_then(|t| t.role.clone());
    let contexts: Vec<agentd::relay::Context> = runtime
        .spawn_targets(id)
        .iter()
        .filter_map(|target| profiles.iter().find(|p| &p.id == target))
        .map(|p| agentd::relay::Context {
            profile: p.id.clone(),
            roles: p.manifest.model.roles.clone(),
        })
        .collect();
    agentd::relay::briefing(role.as_deref(), &contexts)
}

/// Les modèles que le moteur sert en ce moment, depuis un fil sans exécuteur.
fn modeles_servis(
    endpoint: &str,
) -> Result<Vec<String>, (mcp_system::protocol::ErrorCode, String)> {
    use mcp_system::protocol::ErrorCode as Code;
    let endpoint = endpoint.to_owned();
    bloquer(async move {
        providers::stream::ChatClient::new(&endpoint, std::time::Duration::from_secs(3))
            .map_err(|e| (Code::SandboxError, e.to_string()))?
            .models()
            .await
            .map_err(|e| {
                (
                    Code::SandboxError,
                    format!("moteur local injoignable : {e}"),
                )
            })
    })
}

/// Une sous-mission, du jeton délégué au résultat rendu (ADR 0029).
///
/// Tourne dans le fil de la mission parente (ou de la séance), qui attend : le parent ne fait
/// rien d'autre pendant que l'enfant travaille, et reçoit son résultat comme celui d'un outil.
fn deleguer(
    ctx: &Arc<DelegationContext>,
    parent_id: &str,
    parent_token: &Token,
    request: agentd::local::Delegation,
) -> Result<Value, (mcp_system::protocol::ErrorCode, String)> {
    use mcp_system::protocol::ErrorCode as Code;
    let profile = ctx
        .profiles
        .iter()
        .find(|p| p.id == request.profile.trim().to_lowercase())
        .ok_or_else(|| (Code::Invalid, "Contexte de mission inconnu.".to_owned()))?;
    let endpoint = ctx.local_endpoint.clone().ok_or_else(|| {
        (
            Code::SandboxError,
            "moteur local du service non configuré".to_owned(),
        )
    })?;
    // L'enfant : son identifiant, l'utilisateur et le propriétaire du parent, le modèle du
    // parent à défaut d'un autre.
    let (child_id, user, owner, parent_model) = {
        let runtime = ctx.runtime.blocking_lock();
        let parent = runtime
            .task(parent_id)
            .ok_or_else(|| (Code::NotFound, "mission parente inconnue".to_owned()))?;
        let n = runtime.children_of(parent_id).len() + 1;
        (
            format!("{parent_id}.{n}"),
            parent.user.clone(),
            runtime.owner_of(parent_id),
            parent
                .driver
                .as_deref()
                .and_then(|d| d.strip_prefix("local:"))
                .map(str::to_owned),
        )
    };
    // Un rôle demandé désigne le modèle que le contexte visé admet pour ce rôle, parmi ceux que
    // le moteur sert en ce moment ; un modèle nommé explicitement l'emporte s'il est admis.
    // Un nom que le contexte n'admet pas est une erreur d'argument que le modèle peut corriger
    // (en nommant un rôle), pas un refus de politique qui arrête la mission (ADR 0034).
    let admitted = |m: &str| {
        profile
            .manifest
            .model
            .preferred
            .contains(&format!("local:{m}"))
    };
    let explication = || {
        format!(
            "modèles admis : {} ; rôles : {}",
            profile
                .manifest
                .model
                .preferred
                .iter()
                .filter_map(|r| r.strip_prefix("local:"))
                .collect::<Vec<_>>()
                .join(", "),
            profile
                .manifest
                .model
                .roles
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let explicit = request.model.clone().filter(|m| admitted(m));
    if let Some(model) = &request.model
        && explicit.is_none()
        && request.role.is_none()
    {
        return Err((
            Code::Invalid,
            format!(
                "Le contexte {} n'admet pas le modèle {model} ; {}.",
                profile.id,
                explication()
            ),
        ));
    }
    let role_model = match (&explicit, &request.role) {
        (None, Some(role)) => {
            let served = modeles_servis(&endpoint)?;
            let reference = agentd::relay::resolve(&profile.manifest.model.roles, role, &served)
                .map_err(|e| (Code::Invalid, format!("Rôle {role} : {e}.")))?;
            reference.strip_prefix("local:").map(str::to_owned)
        }
        _ => None,
    };
    let model = explicit.or(role_model).or(parent_model).ok_or_else(|| {
        (
            Code::Invalid,
            "aucun modèle local pour la sous-mission".to_owned(),
        )
    })?;
    let reference = format!("local:{model}");
    if !admitted(&model) {
        return Err((
            Code::Invalid,
            format!(
                "Le contexte {} n'admet pas le modèle {model} de votre mission ; {}.",
                profile.id,
                explication()
            ),
        ));
    }
    let mut manifest = profile.manifest.clone();
    manifest.model.preferred = vec![reference.clone()];
    let grants = profile.grants().map_err(|e| (Code::SandboxError, e))?;
    let ttl = i64::try_from(manifest.wall_time_seconds().unwrap_or(1200)).unwrap_or(1200);
    // Le jeton de l'enfant est délégué par capd : un sous-ensemble de celui du parent, jamais
    // plus, ni plus longtemps. Un contexte plus large que le parent est refusé ici.
    let capd = ctx.capd.clone();
    let child_for_capd = child_id.clone();
    let child_token: Token = bloquer(async move {
        let client = Client::connect(&capd)
            .await
            .map_err(|e| (Code::SandboxError, format!("capd injoignable : {e}")))?;
        let brut = client
            .call(
                "cap.delegate",
                json!({"parent": parent_token, "grants": grants, "task": child_for_capd, "ttl_seconds": ttl}),
            )
            .await
            .map_err(|e| (Code::PolicyDenied, format!("délégation refusée par capd : {}", e.message)))?;
        serde_json::from_value(brut).map_err(|e| {
            (
                Code::SandboxError,
                format!("jeton illisible rendu par capd : {e}"),
            )
        })
    })?;
    let scopes: Vec<&str> = profile.scopes.iter().map(String::as_str).collect();
    let availability = Availability {
        local_models: vec![model.clone()],
        ..Default::default()
    };
    {
        let mut runtime = ctx.runtime.blocking_lock();
        runtime
            .plan_with_token(
                &PlanRequest {
                    id: &child_id,
                    intent: request.intent.trim(),
                    manifest: &manifest,
                    user: &user,
                    requested: &profile.grants().map_err(|e| (Code::SandboxError, e))?,
                    scopes: &scopes,
                    availability: &availability,
                },
                child_token,
                OffsetDateTime::now_utc(),
            )
            .map_err(|e| (Code::SandboxError, e.to_string()))?;
        runtime
            .link_child(&child_id, parent_id, 0.5)
            .map_err(|e| (Code::PolicyDenied, e.to_string()))?;
        // Le rôle de l'enfant : celui demandé, sinon celui que son contexte donne à ce modèle.
        let role = request
            .role
            .as_deref()
            .map(|r| r.trim().to_lowercase())
            .or_else(|| manifest.model.role_of(&reference).map(str::to_owned));
        runtime.set_role(&child_id, role);
        if let Some(uid) = owner {
            runtime
                .bind_owner(&child_id, uid)
                .map_err(|e| (Code::SandboxError, e))?;
        }
        ecrire(&ctx.etat, &runtime.etat())
            .map_err(|e| (Code::SandboxError, format!("état non enregistré : {e}")))?;
    }
    // Lancement, dans ce fil : le parent attend.
    let stop = Arc::new(AtomicBool::new(false));
    if let Ok(mut jobs) = ctx.jobs.lock() {
        jobs.insert(child_id.clone(), stop.clone());
    }
    let launch = {
        let mut runtime = ctx.runtime.blocking_lock();
        runtime.begin_local(&child_id).and_then(|l| {
            ecrire(&ctx.etat, &runtime.etat()).map(|()| l).map_err(|e| {
                agentd::RuntimeError::Workspace(format!("lancement non persisté : {e}"))
            })
        })
    };
    let (task, token, plan, home) = match launch {
        Ok(value) => value,
        Err(error) => {
            if let Ok(mut jobs) = ctx.jobs.lock() {
                jobs.remove(&child_id);
            }
            return Err((Code::SandboxError, error.to_string()));
        }
    };
    let publish: agentd::local::Publish = {
        let state = ctx.runtime.clone();
        let path = ctx.etat.clone();
        Arc::new(move |task, result| {
            let mut runtime = state.blocking_lock();
            runtime.publish_local(task, result);
            ecrire(&path, &runtime.etat()).map_err(|e| format!("état non enregistré : {e}"))
        })
    };
    let failure_task = task.clone();
    let briefing = {
        let runtime = ctx.runtime.blocking_lock();
        briefing_pour(&runtime, &child_id, &ctx.profiles)
    };
    let mission = agentd::local::Mission {
        task,
        token,
        plan,
        home,
        endpoint,
        services: mcp_system::services::Services::new(ctx.capd.clone(), ctx.ledger.clone()),
        egress: ctx.egress.clone(),
        browser: ctx.browser.clone(),
        browser_root: ctx.browser_root.clone(),
        sup_socket: ctx.sup_socket.clone(),
        delegate: Some(delegation_fn(ctx.clone())),
        sandboxd: ctx.sandboxd.clone(),
        briefing,
        stop,
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        mission.run(publish.clone());
    }));
    if let Ok(mut jobs) = ctx.jobs.lock() {
        jobs.remove(&child_id);
    }
    if outcome.is_err() {
        let mut task = failure_task;
        task.state = agentd::State::Failed;
        task.history.push(agentd::State::Failed);
        task.reason = Some("sous-mission interrompue de manière inattendue".into());
        let _ = publish(task, None);
    }
    let (state, reason, result) = {
        let mut runtime = ctx.runtime.blocking_lock();
        runtime.absorb_child(&child_id, parent_id);
        let _ = ecrire(&ctx.etat, &runtime.etat());
        let task = runtime.task(&child_id).cloned();
        (
            task.as_ref().map(|t| t.state),
            task.and_then(|t| t.reason),
            runtime.result(&child_id).cloned(),
        )
    };
    Ok(json!({
        "task": child_id,
        "state": state,
        "reason": reason,
        "result": result,
        "note": "La sous-mission a travaillé dans son propre espace ; ses changements sont à examiner et à appliquer comme les vôtres.",
    }))
}

/// Attend une opération asynchrone depuis un fil sans exécuteur : chaque délégation en crée un,
/// le temps d'un appel à capd.
fn bloquer<T>(
    fut: impl std::future::Future<Output = Result<T, (mcp_system::protocol::ErrorCode, String)>>,
) -> Result<T, (mcp_system::protocol::ErrorCode, String)> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| (mcp_system::protocol::ErrorCode::SandboxError, e.to_string()))?;
    runtime.block_on(fut)
}

fn ecrire(chemin: &std::path::Path, etat: &EtatPersistant) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    if let Some(parent) = chemin.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let provisoire = chemin.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&provisoire)?;
    let result = (|| {
        file.write_all(&serde_json::to_vec_pretty(etat).map_err(std::io::Error::other)?)?;
        file.sync_all()?;
        std::fs::rename(&provisoire, chemin)?;
        if let Some(parent) = chemin.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&provisoire);
    }
    result
}

/// Relit l'état d'un démarrage précédent.
///
/// Un fichier absent est normal au premier démarrage. Un fichier illisible bloque le lancement
/// pour préserver les données et permettre leur réparation ; il n'est jamais remplacé par du vide.
fn relire(chemin: &std::path::Path) -> anyhow::Result<EtatPersistant> {
    match std::fs::read(chemin) {
        Err(erreur) if erreur.kind() == std::io::ErrorKind::NotFound => {
            Ok(EtatPersistant::default())
        }
        Err(erreur) => Err(erreur.into()),
        Ok(octets) => Ok(serde_json::from_slice(&octets)?),
    }
}

fn lire<T: serde::de::DeserializeOwned>(params: &Value, nom: &str) -> Result<T, Error> {
    let brut = params
        .get(nom)
        .ok_or_else(|| Error::new(ErrorCode::InvalidParams, format!("« {nom} » attendu")))?;
    serde_json::from_value(brut.clone()).map_err(|e| {
        Error::new(
            ErrorCode::InvalidParams,
            format!("« {nom} » invalide : {e}"),
        )
    })
}

/// Traduit une erreur de runtime en erreur de protocole, en gardant la distinction qui compte :
/// une tâche inconnue n'est pas une capacité refusée, et un état incompatible n'est ni l'un ni
/// l'autre. Les confondre ferait chercher le défaut au mauvais endroit.
fn runtime_erreur(erreur: agentd::RuntimeError) -> Error {
    use agentd::RuntimeError as R;
    let code = match &erreur {
        R::Unknown(_) => ErrorCode::NotFound,
        R::Capability(_) => ErrorCode::PolicyDenied,
        R::Task(_) => ErrorCode::Conflict,
        R::NoDriver(_) | R::Driver(_) | R::Workspace(_) => ErrorCode::InternalError,
    };
    Error::new(code, erreur.to_string())
}

/// Un refus de SFS, dit dans les termes de la commande demandée.
///
/// L'état de l'espace de travail explique ce qui bloque : déjà publié, jamais publié, conflit
/// conservé. Les autres erreurs — version altérée, document retouché, lien — portent déjà
/// leur message et restent des conflits, jamais des erreurs internes.
fn publication_refusee(outcome: Publication, erreur: sfs::SfsError) -> Error {
    use sfs::WorkspaceState as W;
    let message = match (&erreur, outcome) {
        (sfs::SfsError::BadState { state: W::Committed }, Publication::Applied) => {
            "Les versions de cette mission sont déjà publiées.".to_owned()
        }
        (sfs::SfsError::BadState { state: W::Open }, Publication::Undone) => {
            "Cette mission n'a pas encore été publiée.".to_owned()
        }
        (sfs::SfsError::BadState { state: W::RolledBack }, _) => {
            "Cette publication a déjà été annulée.".to_owned()
        }
        (sfs::SfsError::BadState { state: W::Conflict }, _) => {
            "Une publication interrompue sur un conflit conserve ses fichiers déplacés ; elle demande une résolution explicite.".to_owned()
        }
        (sfs::SfsError::BadState { state }, _) => {
            format!("Commande impossible dans l'état de publication {state:?}.")
        }
        (sfs::SfsError::UnknownTask(_), _) => {
            "Les versions de cette mission ne sont plus disponibles.".to_owned()
        }
        _ => erreur.to_string(),
    };
    let code = match erreur {
        sfs::SfsError::UnknownTask(_) => ErrorCode::NotFound,
        _ => ErrorCode::Conflict,
    };
    Error::new(code, message)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    commun::journaliser();

    let socket = commun::socket("agentd");
    let maison = std::env::var("PROPHET_HOME").unwrap_or_else(|_| "/home/prophet".to_owned());
    let capd = chemin("PROPHET_CAPD_SOCKET", "capd");
    let ledger = chemin("PROPHET_LEDGER_SOCKET", "ledger");
    let egress = chemin("PROPHET_EGRESS_SOCKET", "egress");
    let sandboxd = chemin("PROPHET_SANDBOXD_SOCKET", "sandboxd");
    // Un navigateur n'est piloté que s'il est nommé explicitement : ce choix appartient à
    // l'administrateur. Sa sortie réseau est relayée vers egress ; il n'est pas encore confiné
    // au niveau 2 (ADR 0024).
    let browser = std::env::var_os("PROPHET_BROWSER").map(std::path::PathBuf::from);
    let browser_root = commun::etat("agentd").join("navigateurs");
    // Les applications du bureau ne sont pilotées que si l'administrateur nomme le socket de
    // l'adaptateur de session ; sans lui, les outils `ui.*` n'existent pas.
    let sup_socket = std::env::var_os("PROPHET_SUP_SOCKET").map(std::path::PathBuf::from);
    // La sonde tourne sous les contraintes réelles du service, une fois, sans retarder le socket :
    // `task.options` répond « sonde en cours » jusqu'à son verdict.
    let browser_state = Arc::new(std::sync::RwLock::new(
        browser
            .as_deref()
            .map(agentd::preparation::BrowserState::pending),
    ));
    if let Some(program) = browser.clone() {
        let root = browser_root.clone();
        let state = browser_state.clone();
        tokio::task::spawn_blocking(move || {
            let (ready, detail) = match mcp_system::tools::Browsing::probe(&program, &root) {
                Ok(version) => {
                    tracing::info!(programme = %program.display(), version, "navigateur piloté prêt");
                    (true, version)
                }
                Err(raison) => {
                    tracing::warn!(programme = %program.display(), raison, "navigateur piloté indisponible");
                    (false, raison)
                }
            };
            if let Ok(mut etat) = state.write() {
                *etat = Some(agentd::preparation::BrowserState {
                    program: program.display().to_string(),
                    ready,
                    detail,
                });
            }
        });
    }
    let profiles = std::env::var_os("PROPHET_MISSION_PROFILES")
        .map(|path| agentd::preparation::load(std::path::Path::new(&path)))
        .transpose()
        .map_err(anyhow::Error::msg)?
        .unwrap_or_default();

    let fichier_etat = commun::etat("agentd").join("taches.json");
    let mut repris = relire(&fichier_etat)?;
    for task in &mut repris.taches {
        if matches!(
            task.state,
            agentd::State::Running | agentd::State::WaitingApproval | agentd::State::Paused
        ) {
            task.state = agentd::State::Failed;
            task.history.push(agentd::State::Failed);
            task.reason =
                Some("service redémarré pendant la mission ; reprise explicite nécessaire".into());
            repris.results.insert(
                task.id.clone(),
                json!({"state":task.state,"reason":task.reason,"budget":task.budget}),
            );
        }
    }
    let nombre = repris.taches.len();
    let mut runtime = Runtime::sans_broker(&maison);
    runtime.reprendre(repris);
    ecrire(&fichier_etat, &runtime.etat())?;
    if nombre > 0 {
        tracing::info!(nombre, "tâches reprises du démarrage précédent");
    }

    let serveur = Server::bind(&socket)?;
    tracing::info!(
        socket = %socket.display(),
        capd = %capd.display(),
        ledger = %ledger.display(),
        "agentd écoute ; les jetons viennent de capd, le journal part vers ledger"
    );

    serveur
        .serve(Arc::new(Agents {
            runtime: Arc::new(Mutex::new(runtime)),
            jobs: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
            local_endpoint: std::env::var("PROPHET_LOCAL_ENDPOINT").ok(),
            profiles,
            preparing: Mutex::new(()),
            reviews: Arc::new(tokio::sync::Semaphore::new(2)),
            publications: Arc::new(tokio::sync::Semaphore::new(1)),
            capd,
            ledger,
            egress,
            browser,
            browser_state,
            browser_root,
            sup_socket,
            sandboxd,
            seances: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
            etat: fichier_etat,
            pairs: commun::Pairs::detecter()?,
        }))
        .await?;
    Ok(())
}

fn chemin(variable: &str, daemon: &str) -> std::path::PathBuf {
    std::env::var(variable).map_or_else(
        |_| prophet_ipc::socket_path(daemon),
        std::path::PathBuf::from,
    )
}
