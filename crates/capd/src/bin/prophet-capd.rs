//! `prophet-capd` — le broker de capacités, en service.
//!
//! La bibliothèque `capd` sait tout décider ; ce programme lui donne une porte. Il ouvre
//! `/run/prophet/capd.sock` et répond aux méthodes que `docs/specs/ipc.md` nomme.
//!
//! La clé de signature vit dans l'état du service, en mode `0600`, et n'en sort jamais : les
//! appelants reçoivent des jetons signés, jamais la clé. C'est le principe du Vault, appliqué à
//! ce qui signe les droits.

use std::path::Path;
use std::sync::Arc;

use capd::{ApprovalScope, Broker, CheckRequest, PolicyEngine};
use prophet_daemon as commun;
use prophet_ipc::{Error, ErrorCode, Handler, PeerIdentity, Server};
use prophet_types::cap::{Act, Grant, Res, Token};
use prophet_types::manifest::Manifest;
use serde_json::{Value, json};
use time::OffsetDateTime;
use tokio::sync::Mutex;

struct Capd {
    broker: Mutex<Broker>,
    pairs: commun::Pairs,
}

impl Handler for Capd {
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

        let maintenant = OffsetDateTime::now_utc();
        match methode.as_str() {
            "ping" => Ok(json!("pong")),

            // La clé publique, pour qu'un service vérifie un jeton sans repasser par ici.
            "cap.public_key" => {
                let broker = self.broker.lock().await;
                Ok(json!({ "key": encoder(broker.verifying_key().as_bytes()) }))
            }

            // Le contrôle d'accès. C'est le seul point d'entrée légitime pour accorder un droit,
            // et l'invariant de tout le système en dépend : rien n'est permis qui ne soit passé
            // par ici.
            //
            // La décision est rendue telle quelle, refus compris, avec son motif. Un refus n'est
            // pas une erreur de protocole : c'est une réponse, et l'appelant a besoin de savoir
            // *pourquoi* pour décider s'il demande une approbation ou s'il abandonne.
            "cap.check" => {
                let jeton = jeton(&params)?;
                let demande = demande(&params)?;
                let broker = self.broker.lock().await;
                let decision = broker
                    .check(&jeton, &demande, maintenant)
                    .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))?;
                tracing::debug!(
                    sujet = %jeton.sub,
                    cible = %demande.target,
                    permis = decision.is_allow(),
                    "contrôle rendu"
                );
                commun::repondre(&decision)
            }

            // Une action refusée faute de décision humaine peut être soumise à un humain. C'est
            // ce qui alimente le panneau de décision de la surface.
            "approval.request" => {
                let jeton = jeton(&params)?;
                let demande = demande(&params)?;
                let resume = commun::texte(&params, "summary")?;
                let mut broker = self.broker.lock().await;
                let approbation = broker.request_approval(&jeton, &demande, resume, maintenant);
                tracing::info!(id = %approbation.id, "approbation demandée");
                commun::repondre(&approbation)
            }

            // Émettre le jeton racine d'une tâche : l'intersection de ce qu'elle demande et de ce
            // que son manifeste plafonne. Une tâche n'obtient jamais plus que son manifeste, même
            // si elle demande plus — et `capd` est le seul endroit où cette intersection est
            // faite, pour qu'il n'y ait pas deux réponses possibles à la même question.
            "cap.mint" => {
                let manifeste: Manifest = lire(&params, "manifest")?;
                let grants: Vec<Grant> = lire(&params, "grants")?;
                let tache = commun::texte(&params, "task")?;
                let utilisateur = commun::texte(&params, "user")?;
                let duree = params
                    .get("ttl_seconds")
                    .and_then(Value::as_i64)
                    .unwrap_or(1800);
                let mut broker = self.broker.lock().await;
                let jeton = broker
                    .mint(&manifeste, &tache, &utilisateur, &grants, duree, maintenant)
                    .map_err(|e| Error::new(ErrorCode::PolicyDenied, e.to_string()))?;
                tracing::info!(%tache, grants = jeton.grants.len(), "jeton émis");
                commun::repondre(&jeton)
            }

            // Déléguer : une sous-tâche reçoit un sous-ensemble des droits de son parent, jamais
            // plus, et pas plus longtemps. C'est ainsi qu'un agent en fait travailler un autre
            // sans pouvoir lui donner ce qu'il n'a pas lui-même (ADR 0029).
            "cap.delegate" => {
                let parent: Token = lire(&params, "parent")?;
                let grants: Vec<Grant> = lire(&params, "grants")?;
                let tache = commun::texte(&params, "task")?;
                let duree = params
                    .get("ttl_seconds")
                    .and_then(Value::as_i64)
                    .unwrap_or(1800);
                let mut broker = self.broker.lock().await;
                let jeton = broker
                    .delegate(&parent, &tache, &grants, duree, maintenant)
                    .map_err(|e| Error::new(ErrorCode::PolicyDenied, e.to_string()))?;
                tracing::info!(%tache, parent = %parent.sub, grants = jeton.grants.len(), "jeton délégué");
                commun::repondre(&jeton)
            }

            "approval.pending" => {
                let broker = self.broker.lock().await;
                commun::repondre(&broker.approvals().pending())
            }
            "approval.rules" => {
                let broker = self.broker.lock().await;
                commun::repondre(&broker.approvals().rules())
            }
            // Le modèle dit pourquoi il veut l'action ; l'humain le lira avant de trancher.
            "approval.explain" => {
                let id = commun::texte(&params, "id")?;
                let motif = commun::texte(&params, "reason")?;
                let mut broker = self.broker.lock().await;
                let demande = broker.explain_approval(&id, &motif).ok_or_else(|| {
                    Error::new(
                        ErrorCode::NotFound,
                        "demande inconnue ou déjà tranchée, ou motif vide",
                    )
                })?;
                commun::repondre(&demande)
            }
            // Celui qui attend une décision la lit ici, sans rien pouvoir trancher.
            "approval.status" => {
                let id = commun::texte(&params, "id")?;
                let broker = self.broker.lock().await;
                let demande = broker.approval_status(&id).ok_or_else(|| {
                    Error::new(ErrorCode::NotFound, "demande inconnue ou oubliée")
                })?;
                commun::repondre(&demande)
            }

            "approval.resolve" => {
                let id = commun::texte(&params, "id")?;
                let decision = decision(&commun::texte(&params, "decision")?)?;
                let portee = portee(&params)?;
                let mut broker = self.broker.lock().await;
                let tranchee = broker
                    .approvals_mut()
                    .resolve(&id, decision, portee, maintenant)
                    .ok_or_else(|| {
                        Error::new(ErrorCode::NotFound, "demande inconnue ou déjà tranchée")
                    })?;
                tracing::info!(%id, ?decision, "approbation tranchée");
                commun::repondre(&tranchee)
            }

            // Les demandes périmées cessent d'attendre. Une approbation qu'on ne peut plus
            // accorder ne doit pas rester à l'écran comme si elle le pouvait.
            "approval.expire" => {
                let mut broker = self.broker.lock().await;
                let perimees = broker.approvals_mut().expire(maintenant);
                Ok(json!({ "expired": perimees.len() }))
            }

            "cap.revoke" => {
                let sujet = commun::texte(&params, "subject")?;
                let mut broker = self.broker.lock().await;
                broker.revoke(&sujet);
                tracing::info!(%sujet, "sujet révoqué");
                Ok(json!({ "revoked": sujet }))
            }

            autre => Err(commun::methode_inconnue(autre)),
        }
    }
}

/// Le jeton présenté par l'appelant.
///
/// Il arrive tel quel, signature comprise. Rien n'est cru sur parole : `Broker::check` vérifie la
/// signature, la chaîne de parents, l'expiration et la révocation avant de regarder les grants.
fn jeton(params: &Value) -> Result<Token, Error> {
    let brut = params
        .get("token")
        .ok_or_else(|| Error::new(ErrorCode::InvalidParams, "paramètre « token » attendu"))?;
    serde_json::from_value(brut.clone())
        .map_err(|e| Error::new(ErrorCode::InvalidParams, format!("jeton illisible : {e}")))
}

/// La demande de contrôle.
///
/// `irreversible` et `external` valent `false` par défaut, ce qui est le choix sûr : un appelant
/// qui *oublie* de dire qu'une action est irréversible obtient une décision plus stricte, pas
/// plus laxiste — la classification de `capd` reclasse ensuite selon ses propres faits.
fn demande(params: &Value) -> Result<CheckRequest, Error> {
    let res: Res = lire(params, "res")?;
    let act: Act = lire(params, "act")?;
    let mut demande = CheckRequest::new(res, act, commun::texte(params, "target")?);
    demande.sandbox_level = params
        .get("sandbox_level")
        .and_then(Value::as_u64)
        .and_then(|n| u8::try_from(n).ok())
        .unwrap_or(0);
    demande.irreversible = params
        .get("irreversible")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    demande.external = params
        .get("external")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if params.get("context").is_some() {
        demande.context = lire(params, "context")?;
        // La racine vient de la configuration de capd, jamais du contrôleur distant.
        demande.context.home.clear();
        demande.context.target.clear();
    }
    Ok(demande)
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

fn decision(texte: &str) -> Result<capd::ApprovalDecision, Error> {
    match texte {
        "allow" | "approve" => Ok(capd::ApprovalDecision::Allow),
        "deny" | "refuse" => Ok(capd::ApprovalDecision::Deny),
        autre => Err(Error::new(
            ErrorCode::InvalidParams,
            format!("décision inconnue : {autre} (attendu « allow » ou « deny »)"),
        )),
    }
}

/// La portée d'une décision humaine.
///
/// Le défaut est la plus étroite. Élargir une autorisation est une décision en soi : elle se
/// demande, elle ne se déduit pas d'un champ absent.
fn portee(params: &Value) -> Result<ApprovalScope, Error> {
    match params.get("scope").and_then(Value::as_str) {
        None | Some("once") => Ok(ApprovalScope::Once),
        Some("task") => Ok(ApprovalScope::Task),
        Some("agent") => Ok(ApprovalScope::Agent {
            days: u16::try_from(params.get("days").and_then(Value::as_u64).unwrap_or(7))
                .unwrap_or(7),
        }),
        Some(autre) => Err(Error::new(
            ErrorCode::InvalidParams,
            format!("portée inconnue : {autre}"),
        )),
    }
}

fn encoder(octets: &[u8]) -> String {
    use base64::Engine as _;
    format!(
        "ed25519:{}",
        base64::engine::general_purpose::STANDARD.encode(octets)
    )
}

/// Charge les politiques de `/etc/prophet/policies`, ou celles par défaut si le répertoire est
/// absent ou vide.
///
/// Un fichier de politique illisible arrête le service. Continuer avec les règles par défaut
/// laisserait croire qu'une politique s'applique alors qu'elle a été ignorée, et c'est exactement
/// le genre de silence qui rend une autorisation fausse.
fn politiques(repertoire: &Path) -> anyhow::Result<PolicyEngine> {
    if !repertoire.is_dir() {
        tracing::info!(chemin = %repertoire.display(), "aucune politique locale, défauts appliqués");
        return Ok(PolicyEngine::with_defaults()?);
    }
    let mut fichiers: Vec<_> = std::fs::read_dir(repertoire)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "cedar"))
        .collect();
    fichiers.sort();
    if fichiers.is_empty() {
        tracing::info!(chemin = %repertoire.display(), "répertoire vide, défauts appliqués");
        return Ok(PolicyEngine::with_defaults()?);
    }
    let mut texte = String::new();
    for fichier in &fichiers {
        texte.push_str(&std::fs::read_to_string(fichier)?);
        texte.push('\n');
    }
    tracing::info!(nombre = fichiers.len(), "politiques locales chargées");
    Ok(PolicyEngine::new(&texte)?)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    commun::journaliser();

    let etat = commun::etat("capd");
    let socket = commun::socket("capd");
    let maison = std::env::var("PROPHET_HOME").unwrap_or_else(|_| "/home/prophet".to_owned());
    let repertoire_politiques =
        std::env::var("PROPHET_POLICIES").unwrap_or_else(|_| "/etc/prophet/policies".to_owned());

    let broker = Broker::new(commun::clef(&etat, "signing.key")?, "capd", maison)?
        .with_policy(politiques(Path::new(&repertoire_politiques))?);

    let serveur = Server::bind(&socket)?;
    tracing::info!(socket = %socket.display(), "capd écoute");

    serveur
        .serve(Arc::new(Capd {
            broker: Mutex::new(broker),
            pairs: commun::Pairs::detecter()?,
        }))
        .await?;
    Ok(())
}

#[cfg(test)]
mod service_context_tests {
    use super::*;

    #[test]
    fn un_pair_ne_peut_pas_remapper_la_racine_ou_la_cible_par_le_contexte() {
        let request = demande(&json!({
            "res":"fs","act":"read","target":"/home/prophet/docs/note.txt",
            "context":{"home":"/outside","target":"/allowed","bytes":42}
        }))
        .unwrap();
        assert!(request.context.home.is_empty());
        assert!(request.context.target.is_empty());
        assert_eq!(request.target, "/home/prophet/docs/note.txt");
        assert_eq!(request.context.bytes, Some(42));
    }
}
