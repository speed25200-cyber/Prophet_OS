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

use std::sync::Arc;

use agentd::Runtime;
use agentd::runtime::PlanRequest;
use prophet_daemon as commun;
use prophet_ipc::{Client, Error, ErrorCode, Handler, PeerIdentity, Server};
use prophet_types::cap::{Grant, Token};
use prophet_types::manifest::Manifest;
use providers::selection::Availability;
use serde_json::{Value, json};
use time::OffsetDateTime;
use tokio::sync::Mutex;

struct Agents {
    runtime: Mutex<Runtime>,
    capd: std::path::PathBuf,
    ledger: std::path::PathBuf,
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

            // Planifier, c'est décider *avant* : quel pilote, quel niveau d'isolation, quelles
            // capacités, quel budget. Le plan est rendu tel quel pour que l'humain puisse dire non
            // en connaissance de cause, ce qui suppose que tout y soit.
            "task.spawn" => {
                let id = commun::texte(&params, "id")?;
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
                    runtime
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
                        .map_err(runtime_erreur)?
                };
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

            // L'annulation est un geste de l'humain : elle aboutit. Le pilote est prévenu, puis la
            // tâche est close sans attendre sa confirmation.
            "task.cancel" => {
                let id = commun::texte(&params, "id")?;
                let maintenant = OffsetDateTime::now_utc();
                {
                    let mut runtime = self.runtime.lock().await;
                    let mut aucun = providers::mock::MockDriver::default();
                    runtime
                        .cancel(&id, None, &mut aucun, maintenant)
                        .map_err(runtime_erreur)?;
                }
                self.vider_le_journal().await;
                tracing::info!(tache = %id, "tâche annulée");
                Ok(json!({ "cancelled": id }))
            }

            autre => Err(commun::methode_inconnue(autre)),
        }
    }
}

impl Agents {
    /// Demande à `capd` le jeton racine de la tâche.
    async fn demander_un_jeton(
        &self,
        manifeste: &Manifest,
        tache: &str,
        utilisateur: &str,
        demandes: &[Grant],
    ) -> Result<Token, Error> {
        let client = Client::connect(&self.capd).await.map_err(|e| {
            Error::new(
                ErrorCode::InternalError,
                format!("capd injoignable ({e}) : aucune tâche ne peut être planifiée sans jeton"),
            )
        })?;
        let duree = manifeste.wall_time_seconds().unwrap_or(1200);
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    commun::journaliser();

    let socket = commun::socket("agentd");
    let maison = std::env::var("PROPHET_HOME").unwrap_or_else(|_| "/home/prophet".to_owned());
    let capd = chemin("PROPHET_CAPD_SOCKET", "capd");
    let ledger = chemin("PROPHET_LEDGER_SOCKET", "ledger");

    let serveur = Server::bind(&socket)?;
    tracing::info!(
        socket = %socket.display(),
        capd = %capd.display(),
        ledger = %ledger.display(),
        "agentd écoute ; les jetons viennent de capd, le journal part vers ledger"
    );

    serveur
        .serve(Arc::new(Agents {
            runtime: Mutex::new(Runtime::sans_broker(&maison)),
            capd,
            ledger,
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
