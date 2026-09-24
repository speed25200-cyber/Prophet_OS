//! `prophet-ledger` — le journal d'audit, en service.
//!
//! Le journal est en ajout seul. Ce programme en est le seul écrivain : c'est ce qui rend le
//! chaînage par hachage vérifiable, puisqu'une seule séquence de numéros existe.
//!
//! Il publie aussi chaque événement sur un bus. `agentd` et la surface s'y abonneront ; pour
//! l'instant le bus sert à ce que le compte des abonnés soit une vérité et non une promesse.

use std::sync::Arc;

use ledger::{Bus, Filter, Sealer, Store};
use prophet_daemon as commun;
use prophet_ipc::{Error, ErrorCode, Handler, PeerIdentity, Server};
use prophet_types::ledger::{Actor, Draft, EventKind};
use serde_json::{Value, json};
use time::OffsetDateTime;
use tokio::sync::Mutex;

/// Au bout de combien d'événements la chaîne se scelle d'elle-même.
///
/// Un sceau est une signature de la tête de chaîne : il transforme « les événements se suivent »
/// en « personne n'a réécrit ce qui précède ». Trop rare, il laisse une fenêtre ; trop fréquent,
/// il coûte une signature par pas d'agent.
const SEUIL_DE_SCELLEMENT: u64 = 256;

struct Journal {
    store: Mutex<Store>,
    bus: Bus,
    pairs: commun::Pairs,
}

/// À qui chaque méthode s'ouvre (ADR 0044) : écrire et sceller le journal revient aux services,
/// qui y consignent ce que font les tâches ; le lire et le vérifier, à tout pair admis. Un
/// processus de la session humaine — un client officiel compris — ne peut donc pas y inscrire
/// un événement qui n'a pas eu lieu.
fn acces(methode: &str) -> commun::Acces {
    match methode {
        "ledger.append" | "ledger.seal" => commun::Acces::Services,
        _ => commun::Acces::Tous,
    }
}

impl Handler for Journal {
    async fn call(
        &self,
        pair: PeerIdentity,
        _auth: Option<String>,
        methode: String,
        params: Value,
    ) -> Result<Value, Error> {
        if methode != "ping" && !self.pairs.permet(pair, acces(&methode)) {
            tracing::warn!(uid = pair.uid, gid = pair.gid, %methode, "pair refusé");
            return Err(if self.pairs.autorise(pair) {
                self.pairs.refus_pour(&methode, acces(&methode))
            } else {
                self.pairs.refus()
            });
        }

        match methode.as_str() {
            "ping" => Ok(json!("pong")),

            "ledger.append" => {
                let brouillon = brouillon(&params)?;
                let mut store = self.store.lock().await;
                let deja = brouillon
                    .idem
                    .as_deref()
                    .is_some_and(|cle| store.contains_idem(cle));
                let evenement = store.append(brouillon).map_err(interne)?;
                // Déjà écrit : l'émetteur renvoie après une coupure. Rien de neuf à diffuser.
                if deja {
                    return commun::repondre(&evenement);
                }
                // Le scellement suit l'écriture, jamais l'inverse : sceller une tête qu'on n'a pas
                // encore écrite signerait une chaîne qui n'existe pas.
                let scelle = store
                    .maybe_seal(OffsetDateTime::now_utc(), SEUIL_DE_SCELLEMENT, false)
                    .map_err(interne)?;
                drop(store);
                self.bus.publish(evenement.clone());
                if let Some(sceau) = scelle {
                    self.bus.publish(sceau);
                }
                commun::repondre(&evenement)
            }

            "ledger.query" => {
                let filtre = filtre(&params)?;
                let store = self.store.lock().await;
                commun::repondre(&store.query(&filtre).map_err(interne)?)
            }

            // Le journal se vérifie lui-même : chaînage, numérotation, sceaux.
            "ledger.verify" => {
                let store = self.store.lock().await;
                commun::repondre(&store.verify().map_err(interne)?)
            }

            "ledger.seal" => {
                let mut store = self.store.lock().await;
                let sceau = store
                    .maybe_seal(OffsetDateTime::now_utc(), 0, true)
                    .map_err(interne)?;
                match sceau {
                    Some(evenement) => {
                        self.bus.publish(evenement.clone());
                        commun::repondre(&evenement)
                    }
                    // Pas de signataire : le dire, plutôt que de renvoyer « rien » et laisser
                    // croire que la chaîne est scellée.
                    None => Err(Error::new(
                        ErrorCode::Conflict,
                        "ce journal n'a pas de signataire : rien ne peut être scellé",
                    )),
                }
            }

            "ledger.replay_summary" => {
                let tache = commun::texte(&params, "task")?;
                let store = self.store.lock().await;
                Ok(json!({ "summary": store.replay(&tache).map_err(interne)? }))
            }

            "ledger.head" => {
                let store = self.store.lock().await;
                Ok(json!({
                    "hash": store.last_hash(),
                    "subscribers": self.bus.subscribers(),
                }))
            }

            autre => Err(commun::methode_inconnue(autre)),
        }
    }
}

/// Construit un brouillon d'événement à partir des paramètres.
///
/// Le brouillon n'est pas lu tel quel dans les paramètres : l'heure, et plus loin `seq`, `prev`
/// et `hash`, appartiennent au journal. Un appelant qui pourrait les poser pourrait réécrire
/// l'histoire.
fn brouillon(params: &Value) -> Result<Draft, Error> {
    let kind: EventKind = params
        .get("kind")
        .ok_or_else(|| Error::new(ErrorCode::InvalidParams, "paramètre « kind » attendu"))
        .and_then(|v| {
            serde_json::from_value(v.clone()).map_err(|e| {
                Error::new(
                    ErrorCode::InvalidParams,
                    format!("type d'événement inconnu : {e}"),
                )
            })
        })?;

    let acteur = params
        .get("actor")
        .and_then(Value::as_str)
        .map_or_else(Actor::system, |a| Actor(a.to_owned()));

    let mut brouillon = Draft::new(
        OffsetDateTime::now_utc(),
        acteur,
        kind,
        params.get("payload").cloned().unwrap_or(Value::Null),
    );
    if let Some(tache) = params.get("task").and_then(Value::as_str) {
        brouillon = brouillon.task(tache);
    }
    if let Some(etape) = params.get("step").and_then(Value::as_u64) {
        brouillon = brouillon.step(u32::try_from(etape).unwrap_or(u32::MAX));
    }
    // La clé d'idempotence de l'émetteur : renvoyé après une coupure, l'événement n'est écrit
    // qu'une fois (ADR 0059).
    if let Some(cle) = params.get("idem") {
        let cle = cle
            .as_str()
            .filter(|c| !c.is_empty() && c.len() <= 128)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::InvalidParams,
                    "« idem » : une chaîne de 1 à 128 caractères",
                )
            })?;
        brouillon = brouillon.idem(cle);
    }
    Ok(brouillon)
}

fn filtre(params: &Value) -> Result<Filter, Error> {
    let kinds = match params.get("kinds") {
        None | Some(Value::Null) => Vec::new(),
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| {
            Error::new(
                ErrorCode::InvalidParams,
                format!("liste de types d'événements invalide : {e}"),
            )
        })?,
    };
    Ok(Filter {
        task: params
            .get("task")
            .and_then(Value::as_str)
            .map(str::to_owned),
        kinds,
        since_seq: params.get("since_seq").and_then(Value::as_u64),
        until_seq: params.get("until_seq").and_then(Value::as_u64),
        limit: params
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| usize::try_from(n).unwrap_or(usize::MAX)),
    })
}

/// Une erreur du magasin. Un événement refusé pour lui-même — un champ interdit dans sa charge
/// utile — est une faute de l'appelant, définitive : `-32602`, pour qu'un émetteur qui garde ses
/// envois ne le renvoie pas sans fin (ADR 0059). Le reste est une panne du service.
fn interne(erreur: ledger::LedgerError) -> Error {
    match erreur {
        ledger::LedgerError::Event(
            refus @ prophet_types::ledger::EventError::ForbiddenField(_),
        ) => Error::new(ErrorCode::InvalidParams, refus.to_string()),
        autre => Error::new(ErrorCode::InternalError, autre.to_string()),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    commun::journaliser();

    let etat = commun::etat("ledger");
    let socket = commun::socket("ledger");

    // Le signataire est le journal lui-même : sa clé ne quitte pas son état, et sa clé publique
    // est ce qui permet à quiconque de vérifier un sceau sans pouvoir en produire.
    let signataire = Sealer::new(commun::clef(&etat, "seal.key")?);
    let publique = signataire.public();
    let store = Store::open(&etat)?.with_sealer(signataire);

    let serveur = Server::bind(&socket)?;
    tracing::info!(
        socket = %socket.display(),
        cle = %publique,
        "ledger écoute"
    );

    serveur
        .serve(Arc::new(Journal {
            store: Mutex::new(store),
            bus: Bus::new(),
            pairs: commun::Pairs::detecter()?,
        }))
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests_acces {
    use super::*;

    #[test]
    fn ecrire_le_journal_revient_aux_services_et_le_lire_a_tous() {
        assert_eq!(acces("ledger.append"), commun::Acces::Services);
        assert_eq!(acces("ledger.seal"), commun::Acces::Services);
        for methode in [
            "ledger.query",
            "ledger.verify",
            "ledger.replay_summary",
            "ledger.head",
        ] {
            assert_eq!(acces(methode), commun::Acces::Tous, "{methode}");
        }
    }
}
