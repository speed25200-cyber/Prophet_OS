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
                // Sérialise les préparations pour refuser une seconde émission pour le même id.
                let _preparing = self.preparing.lock().await;
                if self.runtime.lock().await.task(&request.id).is_some() {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Cette référence existe déjà. Relisez son plan.",
                    ));
                }
                let models = self.local_models().await?;
                if !models.contains(&request.model) {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Le modèle choisi n'est plus disponible.",
                    ));
                }
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
                let publication = if inspection
                    .result
                    .as_ref()
                    .is_some_and(|result| result.get("review").is_some())
                {
                    let task = id.clone();
                    tokio::task::spawn_blocking(move || {
                        sfs::Workspace::open(&home, &task)
                            .ok()
                            .map(|workspace| workspace.state())
                    })
                    .await
                    .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))?
                } else {
                    None
                };
                commun::repondre(&inspection.with_publication(owner, publication))
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

impl Agents {
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
        let mission = agentd::local::Mission {
            task,
            token,
            plan,
            home,
            endpoint,
            services: mcp_system::services::Services::new(self.capd.clone(), self.ledger.clone()),
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
