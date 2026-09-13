//! Conversation locale en flux. L'annulation abandonne aussi la requête HTTP en cours.
//!
//! Aucun outil système n'est exposé par cette API de conversation. Le pilote natif reste
//! responsable des tours d'outils, de leurs droits et de leur journalisation.

use std::time::{Duration, Instant};

use reqwest::{Client, Url, redirect};
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::DriverError;
use crate::local::{local_endpoint, messages};
use crate::native::Usage;

const MAX_FRAME: usize = 256 * 1024;
const MAX_TRANSFER: usize = 32 * 1024 * 1024;
const MAX_TEXT: usize = 8 * 1024 * 1024;

/// Résultat d'une génération terminée et mesurée.
#[derive(Debug, Clone)]
pub struct Completion {
    /// Texte assemblé depuis les fragments.
    pub text: String,
    /// Compteurs fournis par le moteur.
    pub usage: Usage,
    /// Temps total, incluant le préremplissage.
    pub elapsed: Duration,
    /// Temps jusqu'au premier fragment de texte visible.
    pub first_token: Option<Duration>,
}

/// Une annulation est distincte d'une panne et d'une réponse réussie.
#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    /// L'appelant a arrêté la génération.
    #[error("génération interrompue")]
    Cancelled,
    /// Le moteur ou son protocole a échoué.
    #[error(transparent)]
    Driver(#[from] DriverError),
}

/// Transport asynchrone réutilisable, réservé à la boucle locale.
#[derive(Debug, Clone)]
pub struct ChatClient {
    client: Client,
    endpoint: Url,
}

impl ChatClient {
    /// Prépare un client sans proxy ni redirection.
    ///
    /// # Errors
    /// Adresse non locale, délai nul ou configuration HTTP invalide.
    pub fn new(endpoint: &str, timeout: Duration) -> Result<Self, DriverError> {
        let endpoint = local_endpoint(endpoint)?;
        if timeout.is_zero() {
            return Err(bad("délai positif requis"));
        }
        Ok(Self {
            client: Client::builder()
                .no_proxy()
                .redirect(redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3).min(timeout))
                .timeout(timeout)
                .build()
                .map_err(io)?,
            endpoint,
        })
    }

    /// Découvre les modèles exposés par le moteur, avec une réponse de taille bornée.
    ///
    /// # Errors
    /// Moteur inaccessible, réponse trop grande ou liste invalide.
    pub async fn models(&self) -> Result<Vec<String>, DriverError> {
        let mut response = self
            .client
            .get(self.url("models")?)
            .send()
            .await
            .map_err(io)?;
        status(&response)?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(io)? {
            if bytes.len().saturating_add(chunk.len()) > MAX_FRAME {
                return Err(bad("liste de modèles trop volumineuse"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| bad("liste JSON invalide"))?;
        let mut models = value["data"]
            .as_array()
            .ok_or_else(|| bad("liste de modèles absente"))?
            .iter()
            .map(|item| {
                item["id"]
                    .as_str()
                    .filter(|id| !id.trim().is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| bad("identifiant de modèle absent"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        models.sort();
        models.dedup();
        Ok(models)
    }

    /// Produit une réponse sans bloquer la boucle graphique. Le callback doit rester bref.
    ///
    /// Une valeur vraie, ou la disparition de l'émetteur `cancel`, ferme la requête. Le moteur
    /// doit respecter la déconnexion du client pour cesser son calcul ; ce point est testé
    /// séparément avec chaque moteur. Le texte partiel reste disponible via le callback.
    ///
    /// # Errors
    /// Annulation, HTTP en échec, flux invalide, fin manquante ou consommation absente.
    pub async fn generate(
        &self,
        model: &str,
        history: &[Value],
        max_tokens: u32,
        mut cancel: watch::Receiver<bool>,
        mut on_delta: impl FnMut(&str),
    ) -> Result<Completion, StreamError> {
        if model.trim().is_empty() || max_tokens == 0 {
            return Err(bad("modèle et limite de génération requis").into());
        }
        if *cancel.borrow() {
            return Err(StreamError::Cancelled);
        }
        tokio::select! {
            biased;
            () = async {
                loop {
                    if cancel.changed().await.is_err() || *cancel.borrow() { break; }
                }
            } => Err(StreamError::Cancelled),
            result = self.receive(model, history, max_tokens, &mut on_delta) => result.map_err(Into::into),
        }
    }

    fn url(&self, path: &str) -> Result<Url, DriverError> {
        self.endpoint
            .join(path)
            .map_err(|_| bad("chemin d'API invalide"))
    }

    async fn receive(
        &self,
        model: &str,
        history: &[Value],
        max_tokens: u32,
        on_delta: &mut impl FnMut(&str),
    ) -> Result<Completion, DriverError> {
        let started = Instant::now();
        let mut response = self
            .client
            .post(self.url("chat/completions")?)
            .json(&json!({
                "model":model, "messages":messages(history)?, "stream":true,
                "stream_options":{"include_usage":true}, "max_tokens":max_tokens,
            }))
            .send()
            .await
            .map_err(io)?;
        status(&response)?;
        let mut decoder = Decoder::default();
        let mut result = Assembly::default();
        let mut transferred = 0usize;
        while let Some(chunk) = response.chunk().await.map_err(io)? {
            transferred = transferred.saturating_add(chunk.len());
            if transferred > MAX_TRANSFER {
                return Err(bad("flux du moteur trop volumineux"));
            }
            for data in decoder.feed(&chunk)? {
                if data == "[DONE]" {
                    return result.finish(started.elapsed());
                }
                if let Some(delta) = result.event(&data)? {
                    result.first_token.get_or_insert_with(|| started.elapsed());
                    on_delta(&delta);
                }
            }
        }
        Err(bad("connexion interrompue avant la fin du flux"))
    }
}

#[derive(Default)]
struct Assembly {
    text: String,
    usage: Option<Usage>,
    stopped: bool,
    first_token: Option<Duration>,
}

impl Assembly {
    fn event(&mut self, data: &str) -> Result<Option<String>, DriverError> {
        let event: Value =
            serde_json::from_str(data).map_err(|_| bad("événement de flux JSON invalide"))?;
        if event.get("error").is_some() {
            return Err(bad("le moteur a signalé une erreur dans le flux"));
        }
        if let Some(usage) = event.get("usage").filter(|u| !u.is_null()) {
            self.usage = Some(Usage {
                tokens_in: usage["prompt_tokens"]
                    .as_u64()
                    .ok_or_else(|| bad("tokens d'entrée absents"))?,
                tokens_out: usage["completion_tokens"]
                    .as_u64()
                    .ok_or_else(|| bad("tokens de sortie absents"))?,
            });
        }
        let choices = event["choices"]
            .as_array()
            .ok_or_else(|| bad("choix du moteur absent"))?;
        if choices.is_empty() {
            return Ok(None);
        }
        if choices.len() != 1 || choices[0]["index"] != 0 || self.stopped {
            return Err(bad("réponse unique et ordonnée requise"));
        }
        let choice = &choices[0];
        let delta = &choice["delta"];
        if delta
            .get("tool_calls")
            .is_some_and(|calls| !calls.is_null() && calls != &json!([]))
        {
            return Err(bad("la conversation ne dispose pas d'outils système"));
        }
        match choice["finish_reason"].as_str() {
            None if choice["finish_reason"].is_null() => {}
            Some("stop") => self.stopped = true,
            Some("length") => return Err(bad("génération interrompue par la limite de tokens")),
            _ => return Err(bad("le moteur n'a pas terminé normalement")),
        }
        if let Some(content) = delta.get("content").filter(|c| !c.is_null()) {
            let text = content
                .as_str()
                .ok_or_else(|| bad("fragment de texte invalide"))?;
            if self.text.len().saturating_add(text.len()) > MAX_TEXT {
                return Err(bad("texte généré trop volumineux"));
            }
            self.text.push_str(text);
            if !text.is_empty() {
                return Ok(Some(text.to_owned()));
            }
        }
        Ok(None)
    }

    fn finish(self, elapsed: Duration) -> Result<Completion, DriverError> {
        if !self.stopped || self.text.trim().is_empty() {
            return Err(bad("réponse incomplète ou vide"));
        }
        Ok(Completion {
            text: self.text,
            usage: self
                .usage
                .ok_or_else(|| bad("consommation du moteur absente"))?,
            elapsed,
            first_token: self.first_token,
        })
    }
}

#[derive(Default)]
struct Decoder {
    line: Vec<u8>,
    data: String,
}

impl Decoder {
    fn feed(&mut self, chunk: &[u8]) -> Result<Vec<String>, DriverError> {
        let mut events = Vec::new();
        for &byte in chunk {
            if self.line.len() + self.data.len() >= MAX_FRAME {
                return Err(bad("événement du moteur trop volumineux"));
            }
            if byte != b'\n' {
                self.line.push(byte);
                continue;
            }
            let line = std::str::from_utf8(&self.line)
                .map_err(|_| bad("flux UTF-8 invalide"))?
                .trim_end_matches('\r');
            if line.is_empty() {
                if !self.data.is_empty() {
                    self.data.pop(); // dernier saut de ligne ajouté pour chaque champ data
                    events.push(std::mem::take(&mut self.data));
                }
            } else if let Some(data) = line.strip_prefix("data:") {
                self.data.push_str(data.strip_prefix(' ').unwrap_or(data));
                self.data.push('\n');
            }
            self.line.clear();
        }
        Ok(events)
    }
}

fn status(response: &reqwest::Response) -> Result<(), DriverError> {
    if response.status().is_success() {
        Ok(())
    } else {
        Err(DriverError::Io(format!(
            "le moteur local répond HTTP {}",
            response.status()
        )))
    }
}

fn bad(message: &str) -> DriverError {
    DriverError::BadModelOutput(message.to_owned())
}

fn io(error: impl std::fmt::Display) -> DriverError {
    DriverError::Io(format!("moteur local : {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_flux_utf8_invalide_et_un_evenement_sans_limite_sont_refuses() {
        assert!(Decoder::default().feed(b"data: \xff\n\n").is_err());
        assert!(Decoder::default().feed(&vec![b'a'; MAX_FRAME + 1]).is_err());
    }

    #[test]
    fn plusieurs_lignes_data_forment_un_evenement() {
        assert_eq!(
            Decoder::default().feed(b"data: {\ndata: }\n\n").unwrap(),
            vec!["{\n}"]
        );
    }
}
