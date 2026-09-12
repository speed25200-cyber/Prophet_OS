//! `prophet-vault` — le coffre à secrets, en service.
//!
//! Un invariant de Prophet OS tient en une phrase : *le Vault rend des poignées, jamais des
//! valeurs*. Ce programme est l'endroit où cette phrase devient une propriété du système plutôt
//! qu'une intention.
//!
//! Concrètement : `secrets.list_refs` et `vault.put` sont ouverts au groupe système, comme partout
//! ailleurs — mais `vault.reveal` n'est servi qu'à **un seul utilisateur**, celui du proxy de
//! sortie, au moment de la substitution dans une requête sortante. Tout autre pair est refusé, y
//! compris un pair du groupe système, y compris `root`, y compris le compte qui fait tourner les
//! agents.
//!
//! La conséquence voulue : sur une machine où l'utilisateur `egress` n'existe pas, **personne** ne
//! peut révéler quoi que ce soit. Un coffre qui refuse tout le monde vaut mieux qu'un coffre qui
//! s'ouvre parce qu'il n'a pas su à qui il parlait.

use std::sync::Arc;

use prophet_daemon as commun;
use prophet_ipc::{Error, ErrorCode, Handler, PeerIdentity, Server};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use vault::{SecretInfo, Vault};

/// Le seul compte à qui une valeur est révélée.
const COMPTE_DU_PROXY: &str = "egress";

struct Coffre {
    vault: Mutex<Vault>,
    pairs: commun::Pairs,
    /// Identifiant du proxy de sortie, ou `None` s'il n'existe pas sur cette machine — auquel cas
    /// rien n'est jamais révélé.
    uid_du_proxy: Option<u32>,
}

impl Handler for Coffre {
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

            // Ce qu'un agent obtient : des noms, des domaines, un en-tête. Jamais une valeur.
            "secrets.list_refs" => {
                let vault = self.vault.lock().await;
                commun::repondre(&vault.list())
            }

            "vault.put" => {
                let info: SecretInfo = serde_json::from_value(
                    params
                        .get("info")
                        .cloned()
                        .ok_or_else(|| Error::new(ErrorCode::InvalidParams, "« info » attendu"))?,
                )
                .map_err(|e| {
                    Error::new(ErrorCode::InvalidParams, format!("« info » invalide : {e}"))
                })?;
                let valeur = commun::texte(&params, "value")?;
                let nom = info.name.clone();
                let mut vault = self.vault.lock().await;
                let reference = vault
                    .put(info, &valeur)
                    .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))?;
                // Le nom est journalisé, la valeur ne l'est jamais. Un secret qui passe par un
                // journal a cessé d'en être un.
                tracing::info!(secret = %nom, "secret enregistré");
                Ok(json!({ "ref": reference.as_str() }))
            }

            "vault.delete" => {
                let nom = commun::texte(&params, "name")?;
                let mut vault = self.vault.lock().await;
                let retire = vault
                    .delete(&nom)
                    .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))?;
                Ok(json!({ "deleted": retire }))
            }

            "vault.rotate" => {
                let nom = commun::texte(&params, "name")?;
                let valeur = commun::texte(&params, "value")?;
                let mut vault = self.vault.lock().await;
                vault
                    .rotate(&nom, &valeur)
                    .map_err(|e| Error::new(ErrorCode::NotFound, e.to_string()))?;
                tracing::info!(secret = %nom, "secret remplacé");
                Ok(json!({ "rotated": nom }))
            }

            // Ce secret peut-il être présenté à cet hôte ? Une question, pas une valeur : c'est
            // ce que le proxy demande avant d'établir une connexion.
            "secrets.allowed_for" => {
                let nom = commun::texte(&params, "name")?;
                let hote = commun::texte(&params, "host")?;
                let vault = self.vault.lock().await;
                Ok(json!({ "allowed": vault.allowed_for(&nom, &hote) }))
            }

            // Le seul appel qui rend une valeur, et le seul qui ne suffit pas d'appartenir au
            // groupe système pour obtenir.
            "secrets.use" | "vault.reveal" => {
                let Some(uid_du_proxy) = self.uid_du_proxy else {
                    tracing::warn!(
                        compte = COMPTE_DU_PROXY,
                        "révélation refusée : le proxy de sortie n'existe pas sur cette machine"
                    );
                    return Err(Error::new(
                        ErrorCode::Unauthorized,
                        format!(
                            "aucun compte « {COMPTE_DU_PROXY} » sur cette machine : \
                             aucune valeur ne peut être révélée"
                        ),
                    ));
                };
                if pair.uid != uid_du_proxy {
                    tracing::warn!(
                        uid = pair.uid,
                        pid = ?pair.pid,
                        "révélation refusée à un pair qui n'est pas le proxy de sortie"
                    );
                    return Err(Error::new(
                        ErrorCode::Unauthorized,
                        format!(
                            "seul le proxy de sortie ({COMPTE_DU_PROXY}) obtient une valeur ; \
                             les autres obtiennent une référence"
                        ),
                    ));
                }
                let nom = commun::texte(&params, "name")?;
                let vault = self.vault.lock().await;
                let valeur = vault
                    .reveal(&nom)
                    .map_err(|e| Error::new(ErrorCode::NotFound, e.to_string()))?;
                // Le nom, jamais la valeur.
                tracing::info!(secret = %nom, "valeur remise au proxy de sortie");
                Ok(json!({ "value": valeur }))
            }

            autre => Err(commun::methode_inconnue(autre)),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    commun::journaliser();

    let etat = commun::etat("vault");
    let socket = commun::socket("vault");
    std::fs::create_dir_all(&etat)?;

    let vault = Vault::open(etat.join("secrets.json"), etat.join("vault.key"))?;
    let uid_du_proxy = commun::uid_de_l_utilisateur(COMPTE_DU_PROXY);
    if uid_du_proxy.is_none() {
        tracing::warn!(
            compte = COMPTE_DU_PROXY,
            "compte absent : aucune valeur ne sera révélée à quiconque"
        );
    }

    let serveur = Server::bind(&socket)?;
    tracing::info!(socket = %socket.display(), "vault écoute");

    serveur
        .serve(Arc::new(Coffre {
            vault: Mutex::new(vault),
            pairs: commun::Pairs::detecter()?,
            uid_du_proxy,
        }))
        .await?;
    Ok(())
}
