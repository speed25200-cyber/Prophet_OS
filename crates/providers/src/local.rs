//! Inférence réelle auprès d'un serveur local compatible Chat Completions.
//!
//! Le serveur de modèles est un service de confiance de la machine. Ce transport n'accepte
//! que la boucle locale : ni DNS distant, ni proxy d'environnement, ni redirection. Un moteur
//! distant doit passer par la politique de sortie du système, pas par ce pilote local.
//!
//! Cette API synchrone s'utilise hors d'un exécuteur asynchrone (ou dans `spawn_blocking`).

use std::io::Read as _;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::{Url, redirect};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::DriverError;
use crate::native::{ModelClient, ModelTurn, Usage};

const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

/// Fonction proposée au modèle. Les droits sont encore contrôlés par l'exécuteur à chaque appel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalTool {
    /// Nom public de l'outil.
    pub name: String,
    /// Description destinée au modèle.
    pub description: String,
    /// Schéma JSON des arguments.
    pub parameters: Value,
}

/// Client d'un moteur installé sur la machine (llama.cpp, Ollama, vLLM…).
#[derive(Debug)]
pub struct LocalModel {
    client: Client,
    endpoint: Url,
    model: String,
    tools: Vec<LocalTool>,
    max_tokens: u32,
}

impl LocalModel {
    /// Se connecte au service HTTP local. `endpoint` est la base d'API, par exemple
    /// `http://127.0.0.1:8080/v1`. Aucune requête n'est envoyée par le constructeur.
    ///
    /// # Errors
    /// Adresse distante, identifiants dans l'URL, modèle vide ou délai nul.
    pub fn new(endpoint: &str, model: &str, timeout: Duration) -> Result<Self, DriverError> {
        let endpoint = local_endpoint(endpoint)?;
        if model.trim().is_empty() || timeout.is_zero() {
            return Err(invalid("nom de modèle et délai positif requis"));
        }
        let client = Client::builder()
            .no_proxy()
            .redirect(redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3).min(timeout))
            .timeout(timeout)
            .build()
            .map_err(transport)?;
        Ok(Self {
            client,
            endpoint,
            model: model.to_owned(),
            tools: Vec::new(),
            max_tokens: 2048,
        })
    }

    /// Définit les seuls outils présentés au moteur.
    ///
    /// # Errors
    /// Un nom est vide, dupliqué ou son schéma n'est pas un objet.
    pub fn with_tools(mut self, tools: Vec<LocalTool>) -> Result<Self, DriverError> {
        let mut names = std::collections::HashSet::new();
        for tool in &tools {
            if tool.name.trim().is_empty()
                || !names.insert(&tool.name)
                || !tool.parameters.is_object()
            {
                return Err(invalid("définition d'outil invalide ou dupliquée"));
            }
        }
        self.tools = tools;
        Ok(self)
    }

    /// Limite le nombre de tokens générés par tour.
    ///
    /// # Errors
    /// La limite vaut zéro.
    pub fn with_max_tokens(mut self, limit: u32) -> Result<Self, DriverError> {
        if limit == 0 {
            return Err(invalid("limite de génération positive requise"));
        }
        self.max_tokens = limit;
        Ok(self)
    }

    /// Liste les modèles réellement exposés par le serveur.
    ///
    /// # Errors
    /// Serveur indisponible ou réponse non conforme.
    pub fn models(&self) -> Result<Vec<String>, DriverError> {
        let response = self
            .client
            .get(self.url("models")?)
            .send()
            .map_err(transport)?;
        let body = read_response(response)?;
        let items = body["data"]
            .as_array()
            .ok_or_else(|| invalid("liste de modèles absente"))?;
        items
            .iter()
            .map(|item| {
                item["id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| invalid("identifiant de modèle absent"))
            })
            .collect()
    }

    fn url(&self, path: &str) -> Result<Url, DriverError> {
        self.endpoint
            .join(path)
            .map_err(|_| invalid("chemin d'API invalide"))
    }
}

impl ModelClient for LocalModel {
    fn model_name(&self) -> String {
        format!("local:{}", self.model)
    }

    fn next_turn(&mut self, history: &[Value]) -> Result<(ModelTurn, Usage), DriverError> {
        let mut body = json!({
            "model": self.model,
            "messages": messages(history)?,
            "stream": false,
            "max_tokens": self.max_tokens,
        });
        if !self.tools.is_empty() {
            body["tools"] = json!(
                self.tools
                    .iter()
                    .map(|tool| json!({
                        "type": "function", "function": tool,
                    }))
                    .collect::<Vec<_>>()
            );
            body["tool_choice"] = json!("auto");
            // NativeDriver valide une action à la fois. Refuser une réponse multiple évite
            // d'en perdre silencieusement la moitié et de falsifier l'historique du modèle.
            body["parallel_tool_calls"] = json!(false);
        }
        let response = self
            .client
            .post(self.url("chat/completions")?)
            .json(&body)
            .send()
            .map_err(transport)?;
        let body = read_response(response)?;
        parse_turn(&body, &self.tools)
    }
}

pub(crate) fn local_endpoint(raw: &str) -> Result<Url, DriverError> {
    let mut url = Url::parse(raw).map_err(|_| invalid("adresse de moteur local invalide"))?;
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid(
            "le moteur local exige une URL HTTP sans identifiants ni paramètres",
        ));
    }
    // Remplacer localhost par son adresse empêche une résolution DNS/hosts détournée.
    if url.host_str() == Some("localhost") {
        url.set_ip_host(IpAddr::V4(Ipv4Addr::LOCALHOST))
            .map_err(|()| invalid("hôte local invalide"))?;
    }
    let local = url
        .host_str()
        .and_then(|host| host.trim_matches(['[', ']']).parse::<IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback());
    if !local {
        return Err(invalid(
            "un moteur local doit écouter sur une adresse de boucle locale",
        ));
    }
    let path = format!("{}/", url.path().trim_end_matches('/'));
    url.set_path(&path);
    Ok(url)
}

fn read_response(response: reqwest::blocking::Response) -> Result<Value, DriverError> {
    let status = response.status();
    if !status.is_success() {
        // Ne pas recopier une page de serveur qui peut contenir le prompt ou des données privées.
        return Err(DriverError::Io(format!(
            "le moteur local répond HTTP {status}"
        )));
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(transport)?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(invalid("réponse du moteur trop volumineuse"));
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid("le moteur n'a pas rendu de JSON valide"))
}

pub(crate) fn messages(history: &[Value]) -> Result<Vec<Value>, DriverError> {
    let mut out = Vec::new();
    let mut pending = None;
    for (index, entry) in history.iter().enumerate() {
        match entry["role"].as_str() {
            Some("assistant") if entry.get("tool_call").is_some() => {
                if pending.is_some() {
                    return Err(invalid("résultat d'outil manquant dans l'historique"));
                }
                let call = &entry["tool_call"];
                let name = call["tool"]
                    .as_str()
                    .ok_or_else(|| invalid("nom d'outil absent"))?;
                let id = format!("prophet_call_{index}");
                out.push(json!({"role":"assistant", "content":null, "tool_calls":[{
                    "id": id, "type":"function", "function":{
                        "name": name, "arguments": call["arguments"].to_string()
                    }
                }]}));
                pending = Some(id);
            }
            Some("tool") => {
                let id = pending
                    .take()
                    .ok_or_else(|| invalid("résultat sans appel d'outil"))?;
                out.push(json!({"role":"tool", "tool_call_id":id,
                    "content": json!({"ok":entry["ok"], "result":entry["result"]}).to_string()}));
            }
            Some("user" | "assistant" | "system") if pending.is_none() => out.push(entry.clone()),
            _ => return Err(invalid("historique de conversation incohérent")),
        }
    }
    if pending.is_some() {
        return Err(invalid("résultat d'outil manquant dans l'historique"));
    }
    Ok(out)
}

fn parse_turn(body: &Value, tools: &[LocalTool]) -> Result<(ModelTurn, Usage), DriverError> {
    let choice = body["choices"]
        .as_array()
        .filter(|c| c.len() == 1)
        .and_then(|c| c.first())
        .ok_or_else(|| invalid("une réponse unique du modèle est requise"))?;
    let usage = response_usage(body)?;
    match choice["finish_reason"].as_str() {
        Some("stop" | "tool_calls") => {}
        Some("length") => return Err(invalid("génération interrompue par la limite de tokens")),
        _ => return Err(invalid("le moteur n'a pas terminé normalement")),
    }
    let message = &choice["message"];
    if let Some(calls) = message.get("tool_calls").filter(|v| !v.is_null()) {
        let calls = calls
            .as_array()
            .ok_or_else(|| invalid("liste d'appels invalide"))?;
        if !calls.is_empty() {
            if calls.len() != 1 {
                return Err(invalid(
                    "le moteur doit produire un seul appel d'outil par tour",
                ));
            }
            let call = &calls[0];
            let name = call["function"]["name"]
                .as_str()
                .ok_or_else(|| invalid("nom d'outil absent"))?;
            if call["type"] != "function" || !tools.iter().any(|tool| tool.name == name) {
                return Err(invalid("le moteur demande un outil non proposé"));
            }
            let arguments = call["function"]["arguments"]
                .as_str()
                .ok_or_else(|| invalid("arguments d'outil absents"))?;
            let arguments: Value = serde_json::from_str(arguments)
                .map_err(|_| invalid("arguments d'outil JSON invalides"))?;
            if !arguments.is_object() {
                return Err(invalid("les arguments d'outil doivent être un objet"));
            }
            return Ok((
                ModelTurn::ToolCall {
                    tool: name.to_owned(),
                    arguments,
                },
                usage,
            ));
        }
    }
    if choice["finish_reason"] != "stop" {
        return Err(invalid("le moteur annonce un appel sans en fournir"));
    }
    let text = message["content"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid("réponse finale vide"))?;
    Ok((
        ModelTurn::Final {
            text: text.to_owned(),
        },
        usage,
    ))
}

/// Même contrat que `LocalModel`, avec une requête annulable par abandon du futur.
#[derive(Debug)]
pub struct AsyncLocalModel {
    client: reqwest::Client,
    endpoint: Url,
    model: String,
    tools: Vec<LocalTool>,
    max_tokens: u32,
}

/// Réponse dont les compteurs restent exploitables même si le tour est invalide.
#[derive(Debug)]
pub struct LocalReply {
    /// Tour validé, ou erreur de protocole/génération.
    pub turn: Result<ModelTurn, DriverError>,
    /// Consommation attestée par la réponse du moteur.
    pub usage: Usage,
}

fn response_usage(body: &Value) -> Result<Usage, DriverError> {
    Ok(Usage {
        tokens_in: body["usage"]["prompt_tokens"]
            .as_u64()
            .ok_or_else(|| invalid("compteur de tokens d'entrée absent"))?,
        tokens_out: body["usage"]["completion_tokens"]
            .as_u64()
            .ok_or_else(|| invalid("compteur de tokens de sortie absent"))?,
    })
}

impl AsyncLocalModel {
    /// Prépare un transport local sans proxy ni redirection.
    ///
    /// # Errors
    /// Adresse, délai, modèle ou schéma invalide.
    pub fn new(
        endpoint: &str,
        model: &str,
        tools: Vec<LocalTool>,
        timeout: Duration,
        max_tokens: u32,
    ) -> Result<Self, DriverError> {
        let endpoint = local_endpoint(endpoint)?;
        if model.trim().is_empty() || timeout.is_zero() || max_tokens == 0 {
            return Err(invalid("modèle et plafonds positifs requis"));
        }
        let mut names = std::collections::HashSet::new();
        if tools.iter().any(|t| {
            t.name.trim().is_empty() || !t.parameters.is_object() || !names.insert(&t.name)
        }) {
            return Err(invalid("définition d'outil invalide ou dupliquée"));
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3).min(timeout))
            .timeout(timeout)
            .build()
            .map_err(transport)?;
        Ok(Self {
            client,
            endpoint,
            model: model.into(),
            tools,
            max_tokens,
        })
    }

    /// Produit un tour ; son futur peut être abandonné pour interrompre l'inférence HTTP.
    ///
    /// # Errors
    /// Moteur indisponible, réponse trop grande, incohérente ou incomplète.
    pub async fn next_turn(&self, history: &[Value]) -> Result<LocalReply, DriverError> {
        let mut body = json!({"model":self.model,"messages":messages(history)?,"stream":false,"max_tokens":self.max_tokens});
        if !self.tools.is_empty() {
            body["tools"] = json!(
                self.tools
                    .iter()
                    .map(|t| json!({"type":"function","function":t}))
                    .collect::<Vec<_>>()
            );
            body["tool_choice"] = json!("auto");
            body["parallel_tool_calls"] = json!(false);
        }
        let url = self
            .endpoint
            .join("chat/completions")
            .map_err(|_| invalid("chemin d'API invalide"))?;
        let mut response = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(transport)?;
        if !response.status().is_success() {
            return Err(DriverError::Io(format!(
                "le moteur local répond HTTP {}",
                response.status()
            )));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport)? {
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES as usize {
                return Err(invalid("réponse du moteur trop volumineuse"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let body = serde_json::from_slice(&bytes)
            .map_err(|_| invalid("le moteur n'a pas rendu de JSON valide"))?;
        Ok(LocalReply {
            usage: response_usage(&body)?,
            turn: parse_turn(&body, &self.tools).map(|(turn, _)| turn),
        })
    }
}

fn invalid(message: &str) -> DriverError {
    DriverError::BadModelOutput(message.to_owned())
}

fn transport(error: impl std::fmt::Display) -> DriverError {
    DriverError::Io(format!("moteur local : {error}"))
}
