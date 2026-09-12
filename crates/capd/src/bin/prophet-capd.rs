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

use capd::{ApprovalScope, Broker, PolicyEngine};
use prophet_daemon as commun;
use prophet_ipc::{Error, ErrorCode, Handler, PeerIdentity, Server};
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

            "approval.pending" => {
                let broker = self.broker.lock().await;
                commun::repondre(&broker.approvals().pending())
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
