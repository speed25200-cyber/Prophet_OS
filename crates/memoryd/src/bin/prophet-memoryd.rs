//! `prophet-memoryd` — la mémoire, en service.
//!
//! Une mémoire partagée entre agents pose une question qu'un magasin de clés-valeurs ne pose pas :
//! *qui a le droit de se souvenir de quoi ?* La réponse de Prophet OS tient dans les espaces. Un
//! espace est un cloisonnement, pas une étiquette, et le daemon le tient comme tel : une recherche
//! sans espace ne cherche nulle part, elle ne cherche pas partout.
//!
//! C'est une décision qu'il vaut mieux écrire que découvrir. `Query::spaces` vide signifie
//! « aucun » — le commentaire de la bibliothèque le dit déjà — et le daemon refuse plutôt que de
//! rendre une liste vide qui ressemblerait à « rien trouvé ».

use std::sync::Arc;

use memoryd::{HashEmbedder, Kind, NewEntry, Query, Space, Store};
use prophet_daemon as commun;
use prophet_ipc::{Error, ErrorCode, Handler, PeerIdentity, Server};
use serde_json::{Value, json};
use time::OffsetDateTime;
use tokio::sync::Mutex;

/// Combien de résultats une recherche rend si l'appelant n'en demande pas un nombre.
const RESULTATS_PAR_DEFAUT: usize = 10;

struct Memoire {
    store: Mutex<Store>,
    pairs: commun::Pairs,
}

/// À qui chaque méthode s'ouvre (ADR 0044). Écrire un souvenir revient aux services, qui le
/// tiennent d'une tâche : un processus de la session humaine qui pourrait en écrire glisserait
/// dans la mémoire d'un agent des consignes qu'il relirait comme les siennes. Chercher, lister
/// et oublier restent ouverts : l'humain édite et efface la mémoire de ses agents (M11-T4).
fn acces(methode: &str) -> commun::Acces {
    match methode {
        "memory.remember" => commun::Acces::Services,
        _ => commun::Acces::Tous,
    }
}

impl Handler for Memoire {
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

            "memory.remember" => {
                let espace = espace(&params)?;
                let texte = commun::texte(&params, "text")?;
                let nature = nature(&params)?;
                let etiquettes: Vec<String> = params
                    .get("tags")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "une confiance est un nombre entre 0 et 1 ; la précision d'un f32 y suffit"
                )]
                let confiance = params
                    .get("confidence")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0) as f32;

                let store = self.store.lock().await;
                let id = store
                    .remember(
                        &NewEntry {
                            space: &espace,
                            kind: nature,
                            text: &texte,
                            tags: &etiquettes,
                            source_task: params.get("source_task").and_then(Value::as_str),
                            confidence: confiance.clamp(0.0, 1.0),
                        },
                        OffsetDateTime::now_utc(),
                    )
                    .map_err(interne)?;
                Ok(json!({ "id": id }))
            }

            "memory.search" => {
                let espace = espace(&params)?;
                let requete = Query {
                    spaces: vec![espace],
                    text: commun::texte(&params, "query")?,
                    limit: params
                        .get("limit")
                        .and_then(Value::as_u64)
                        .and_then(|n| usize::try_from(n).ok())
                        .unwrap_or(RESULTATS_PAR_DEFAUT),
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "un seuil de pertinence est un nombre entre 0 et 1"
                    )]
                    min_score: params
                        .get("min_score")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0) as f32,
                };
                let store = self.store.lock().await;
                commun::repondre(&store.search(&requete).map_err(interne)?)
            }

            "memory.list" => {
                let espace = espace(&params)?;
                let store = self.store.lock().await;
                commun::repondre(&store.list(&espace).map_err(interne)?)
            }

            "memory.forget" => {
                let id = commun::texte(&params, "id")?;
                let store = self.store.lock().await;
                let oublie = store.forget(&id).map_err(interne)?;
                if oublie {
                    tracing::info!(%id, "entrée oubliée");
                }
                Ok(json!({ "forgotten": oublie }))
            }

            // Oublier un espace entier. Le nombre est rendu : « oublié » sans quantité laisserait
            // croire à une opération sans effet quand il n'y avait rien.
            "memory.forget_space" => {
                let espace = espace(&params)?;
                let store = self.store.lock().await;
                let nombre = store.forget_space(&espace).map_err(interne)?;
                tracing::info!(espace = %espace.0, nombre, "espace oublié");
                Ok(json!({ "forgotten": nombre }))
            }

            "memory.spaces" => {
                let store = self.store.lock().await;
                commun::repondre(&store.spaces().map_err(interne)?)
            }

            autre => Err(commun::methode_inconnue(autre)),
        }
    }
}

/// L'espace visé, qui n'a pas de défaut.
///
/// Un défaut ferait qu'un appel mal formé écrirait quelque part plutôt que nulle part, et la
/// mémoire d'un agent finirait dans l'espace d'un autre sans que personne ne l'ait demandé.
fn espace(params: &Value) -> Result<Space, Error> {
    Ok(Space::new(commun::texte(params, "space")?))
}

fn nature(params: &Value) -> Result<Kind, Error> {
    match params.get("kind").and_then(Value::as_str) {
        None | Some("fact") => Ok(Kind::Fact),
        Some("episode") => Ok(Kind::Episode),
        Some("preference") => Ok(Kind::Preference),
        Some(autre) => Err(Error::new(
            ErrorCode::InvalidParams,
            format!("nature inconnue : {autre} (attendu fact, episode ou preference)"),
        )),
    }
}

fn interne(erreur: memoryd::MemoryError) -> Error {
    Error::new(ErrorCode::InternalError, erreur.to_string())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    commun::journaliser();

    let etat = commun::etat("memoryd");
    let socket = commun::socket("memoryd");
    std::fs::create_dir_all(&etat)?;

    let store = Store::open(
        &etat.join("memoire.sqlite"),
        Box::new(HashEmbedder::default()),
    )?;

    let serveur = Server::bind(&socket)?;
    tracing::info!(socket = %socket.display(), "memoryd écoute");

    serveur
        .serve(Arc::new(Memoire {
            store: Mutex::new(store),
            pairs: commun::Pairs::detecter()?,
        }))
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests_acces {
    use super::*;

    #[test]
    fn ecrire_un_souvenir_revient_aux_services_et_l_oublier_a_tous() {
        assert_eq!(acces("memory.remember"), commun::Acces::Services);
        for methode in [
            "memory.search",
            "memory.list",
            "memory.forget",
            "memory.forget_space",
            "memory.spaces",
        ] {
            assert_eq!(acces(methode), commun::Acces::Tous, "{methode}");
        }
    }
}
