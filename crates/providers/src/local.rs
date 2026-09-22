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

/// Température des tours de mission. Le moteur sert ses réglages de conversation (0,7 pour
/// Qwen3), faits pour varier ; une mission, elle, choisit des outils, et un petit modèle tiré à
/// 0,7 a pris `doc.read` hors de ses droits une fois sur cinq dans le contexte web, ce qui
/// interrompt la mission sur le refus de capd. À 0,2, le choix n'est plus tiré au sort ; la
/// conversation de l'atelier garde les réglages du moteur.
pub const MISSION_TEMPERATURE: f64 = 0.2;

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
            "temperature": MISSION_TEMPERATURE,
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
    /// Consigne de système placée avant l'intention, si la mission en a une (ADR 0034).
    system: Option<String>,
    /// Condensation des anciens résultats d'outils avant envoi, si demandée.
    condensation: Option<Condensation>,
    /// Fenêtre de contexte du moteur, apprise de son premier refus.
    window: std::sync::Mutex<Option<Window>>,
}

/// Comment condenser l'historique envoyé au moteur : les résultats d'outils plus anciens que
/// les `keep_last` derniers, et plus longs que `max_bytes`, sont remplacés par un résumé qui en
/// donne la taille, l'empreinte et le début. L'historique conservé par la boucle ne change pas ;
/// seul ce qui part vers le modèle est allégé, à chaque tour, de façon déterministe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Condensation {
    /// Nombre de résultats d'outils récents envoyés intacts.
    pub keep_last: usize,
    /// Taille au-delà de laquelle un ancien résultat est condensé.
    pub max_bytes: usize,
}

impl Default for Condensation {
    fn default() -> Self {
        Self {
            keep_last: 2,
            max_bytes: 1024,
        }
    }
}

/// Condense sur place les anciens résultats d'outils d'une liste de messages Chat Completions
/// et rend le nombre d'octets épargnés.
///
/// Un résultat condensé garde `ok`, annonce `condensed: true`, la taille d'origine, une
/// empreinte blake3 et les 160 premiers caractères : le modèle sait qu'il a lu ce résultat, ce
/// qu'il contenait en substance, et qu'il peut le relire par un nouvel appel s'il en a besoin.
#[must_use]
pub fn condense(messages: &mut [Value], policy: Condensation) -> usize {
    let tool_indexes: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m["role"] == "tool")
        .map(|(i, _)| i)
        .collect();
    let old = tool_indexes.len().saturating_sub(policy.keep_last);
    let mut saved = 0;
    for index in tool_indexes.into_iter().take(old) {
        let Some(content) = messages[index]["content"].as_str() else {
            continue;
        };
        if content.len() <= policy.max_bytes {
            continue;
        }
        let ok = serde_json::from_str::<Value>(content)
            .ok()
            .and_then(|v| v["ok"].as_bool())
            .unwrap_or(true);
        let mut head_end = 160.min(content.len());
        while !content.is_char_boundary(head_end) {
            head_end -= 1;
        }
        let summary = json!({
            "ok": ok,
            "condensed": true,
            "bytes": content.len(),
            "digest": format!("blake3:{}", blake3::hash(content.as_bytes()).to_hex()),
            "head": &content[..head_end],
            "note": "résultat ancien condensé par Prophet OS ; relancez l'outil pour le relire en entier"
        })
        .to_string();
        saved += content.len().saturating_sub(summary.len());
        messages[index]["content"] = Value::String(summary);
    }
    saved
}

/// Taille laissée au moins au dernier résultat d'outil quand il faut le tronquer.
const MIN_ROOM: usize = 512;
/// Marge sur l'estimation des octets à retirer : les tokens d'un historique ne se comptent pas
/// sans le tokeniseur du modèle, seulement au rapport que le moteur a laissé voir.
pub(crate) const SAFETY: f64 = 1.3;
/// Envois d'un même tour permis quand le moteur refuse un historique trop long.
pub(crate) const MAX_FIT_ATTEMPTS: u32 = 3;
/// Plafond d'un corps d'erreur lu pour y chercher le refus de fenêtre.
pub(crate) const MAX_ERROR_BYTES: usize = 64 * 1024;

fn tool_indexes(messages: &[Value]) -> Vec<usize> {
    messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m["role"] == "tool")
        .map(|(i, _)| i)
        .collect()
}

fn content_len(message: &Value) -> usize {
    message["content"].as_str().map_or(0, str::len)
}

/// Octets des résultats d'outils d'une liste de messages.
#[must_use]
pub fn tool_bytes(messages: &[Value]) -> usize {
    tool_indexes(messages)
        .into_iter()
        .map(|i| content_len(&messages[i]))
        .sum()
}

/// Ramène les résultats d'outils d'une liste de messages sous `budget` octets au total et rend
/// vrai s'ils y tiennent.
///
/// Dans l'ordre : les anciens résultats sont condensés comme par [`condense`], puis réduits à
/// leur taille et leur issue, du plus ancien au plus récent, tant qu'ils ne laissent pas
/// [`MIN_ROOM`] octets au dernier ; le dernier, enfin, garde son début et dit au modèle qu'il a
/// été tronqué et comment en lire moins. L'intention, la consigne et les appels ne changent pas.
#[must_use]
pub fn fit(messages: &mut [Value], budget: usize) -> bool {
    if tool_bytes(messages) <= budget {
        return true;
    }
    let _ = condense(
        messages,
        Condensation {
            keep_last: 1,
            max_bytes: MIN_ROOM,
        },
    );
    let indexes = tool_indexes(messages);
    let Some((&last, old)) = indexes.split_last() else {
        return tool_bytes(messages) <= budget;
    };
    let mut others: usize = old.iter().map(|&i| content_len(&messages[i])).sum();
    for &index in old {
        if budget.saturating_sub(others) >= MIN_ROOM {
            break;
        }
        let before = content_len(&messages[index]);
        bare(&mut messages[index]);
        others = others - before + content_len(&messages[index]);
    }
    truncate(
        &mut messages[last],
        budget.saturating_sub(others).max(MIN_ROOM),
    );
    tool_bytes(messages) <= budget
}

/// Réduit un ancien résultat à son issue et à sa taille d'origine, s'il y gagne.
fn bare(message: &mut Value) {
    let Some(content) = message["content"].as_str() else {
        return;
    };
    let parsed = serde_json::from_str::<Value>(content).ok();
    let ok = parsed
        .as_ref()
        .and_then(|v| v["ok"].as_bool())
        .unwrap_or(true);
    let bytes = parsed
        .as_ref()
        .filter(|v| v["condensed"] == true)
        .and_then(|v| v["bytes"].as_u64())
        .unwrap_or(content.len() as u64);
    let bare = json!({"ok": ok, "condensed": true, "bytes": bytes}).to_string();
    if bare.len() < content.len() {
        message["content"] = Value::String(bare);
    }
}

/// Garde le début d'un résultat dans `room` octets, avis compris.
fn truncate(message: &mut Value, room: usize) {
    let Some(content) = message["content"].as_str() else {
        return;
    };
    if content.len() <= room {
        return;
    }
    let total = content.len();
    let digest = blake3::hash(content.as_bytes()).to_hex();
    let notice = |shown: usize| {
        format!(
            "[Prophet OS : résultat tronqué pour tenir dans la fenêtre de contexte du moteur ; \
             {shown} octets sur {total}, empreinte blake3:{digest}. Ne le relisez pas en entier : \
             demandez-en moins, avec max_bytes ou une cible plus précise.]\n"
        )
    };
    let mut shown = room.saturating_sub(notice(room).len()).min(total);
    while !content.is_char_boundary(shown) {
        shown -= 1;
    }
    let text = format!("{}{}", notice(shown), &content[..shown]);
    message["content"] = Value::String(text);
}

/// Fenêtre de contexte d'un moteur : sa taille en tokens et le rapport octets par token observé
/// sur l'historique qu'il a refusé.
#[derive(Debug, Clone, Copy)]
struct Window {
    n_ctx: u64,
    bytes_per_token: f64,
}

/// Refus d'un historique plus long que la fenêtre du moteur, avec ses nombres s'il les donne.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Overflow {
    pub(crate) n_prompt: Option<u64>,
    pub(crate) n_ctx: Option<u64>,
}

impl Overflow {
    /// Lit le refus de llama-server (`exceed_context_size_error`) ; n'en garde que les nombres.
    pub(crate) fn parse(body: &[u8]) -> Option<Self> {
        let body: Value = serde_json::from_slice(body).ok()?;
        let error = &body["error"];
        let message = error["message"]
            .as_str()
            .unwrap_or_default()
            .to_ascii_lowercase();
        (error["type"] == "exceed_context_size_error" || message.contains("context size")).then(
            || Self {
                n_prompt: error["n_prompt_tokens"].as_u64(),
                n_ctx: error["n_ctx"].as_u64(),
            },
        )
    }

    pub(crate) fn error(self) -> DriverError {
        DriverError::Io(match (self.n_prompt, self.n_ctx) {
            (Some(prompt), Some(ctx)) => format!(
                "l'historique dépasse la fenêtre de contexte du moteur local ({prompt} tokens \
                 pour {ctx}), même resserré"
            ),
            _ => "l'historique dépasse la fenêtre de contexte du moteur local, même resserré"
                .to_owned(),
        })
    }
}

pub(crate) fn serialized_len(messages: &[Value]) -> usize {
    serde_json::to_string(messages).map_or(0, |s| s.len())
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
            system: None,
            condensation: None,
            window: std::sync::Mutex::new(None),
        })
    }

    /// Place une consigne de système avant l'intention, à chaque tour. Une consigne vide
    /// n'en pose aucune.
    #[must_use]
    pub fn with_system(mut self, system: Option<String>) -> Self {
        self.system = system.filter(|s| !s.trim().is_empty());
        self
    }

    /// Condense les anciens résultats d'outils avant chaque envoi.
    #[must_use]
    pub const fn with_condensation(mut self, policy: Condensation) -> Self {
        self.condensation = Some(policy);
        self
    }

    /// Messages tels qu'ils partent vers le moteur : consigne, historique condensé s'il y a lieu.
    ///
    /// # Errors
    /// Historique incohérent.
    pub fn outgoing(&self, history: &[Value]) -> Result<Vec<Value>, DriverError> {
        let mut messages = messages(history)?;
        if let Some(policy) = self.condensation {
            let _ = condense(&mut messages, policy);
        }
        if let Some(system) = &self.system {
            messages.insert(0, json!({"role":"system","content":system}));
        }
        Ok(messages)
    }

    /// Produit un tour ; son futur peut être abandonné pour interrompre l'inférence HTTP.
    ///
    /// Si le moteur refuse l'historique parce qu'il dépasse sa fenêtre de contexte, les
    /// résultats d'outils sont resserrés à la mesure qu'il donne et le tour renvoyé, jusqu'à
    /// [`MAX_FIT_ATTEMPTS`] envois ; la fenêtre apprise sert ensuite d'emblée aux tours suivants.
    /// L'historique de la boucle ne change pas : seul l'envoi est allégé.
    ///
    /// # Errors
    /// Moteur indisponible, réponse trop grande, incohérente ou incomplète, historique qui ne
    /// tient pas dans la fenêtre même resserré.
    pub async fn next_turn(&self, history: &[Value]) -> Result<LocalReply, DriverError> {
        let mut messages = self.outgoing(history)?;
        self.fit_to_window(&mut messages, None);
        let mut attempts = 0;
        loop {
            attempts += 1;
            let overflow = match self.send(&messages).await? {
                Ok(reply) => return Ok(reply),
                Err(overflow) => overflow,
            };
            let before = tool_bytes(&messages);
            self.learn(&messages, overflow);
            self.fit_to_window(&mut messages, Some(overflow));
            if attempts >= MAX_FIT_ATTEMPTS || tool_bytes(&messages) >= before {
                return Err(overflow.error());
            }
        }
    }

    /// Retient la fenêtre qu'un refus révèle, et le rapport octets par token de ce qui est parti.
    fn learn(&self, messages: &[Value], overflow: Overflow) {
        let (Some(n_prompt), Some(n_ctx)) = (overflow.n_prompt, overflow.n_ctx) else {
            return;
        };
        if n_prompt == 0 || n_ctx == 0 {
            return;
        }
        let window = Window {
            n_ctx,
            bytes_per_token: (serialized_len(messages) as f64 / n_prompt as f64).max(0.5),
        };
        if let Ok(mut slot) = self.window.lock() {
            *slot = Some(window);
        }
    }

    /// Resserre les résultats d'outils pour que l'historique tienne dans la fenêtre connue, en
    /// laissant la place de la réponse ; sans fenêtre connue, un refus en retire la moitié.
    fn fit_to_window(&self, messages: &mut [Value], overflow: Option<Overflow>) {
        let window = self.window.lock().ok().and_then(|w| *w);
        let results = tool_bytes(messages);
        let budget = match (window, overflow) {
            (Some(window), _) => {
                let estimate = overflow.and_then(|o| o.n_prompt).map_or_else(
                    || serialized_len(messages) as f64 / window.bytes_per_token,
                    |n| n as f64,
                );
                let reserve = u64::from(self.max_tokens).min(window.n_ctx / 4);
                let target = window.n_ctx.saturating_sub(reserve) as f64;
                if estimate <= target {
                    return;
                }
                let cut = ((estimate - target) * window.bytes_per_token * SAFETY).ceil();
                // Borné par la taille des résultats : la conversion ne peut déborder.
                results.saturating_sub(cut.min(results as f64) as usize)
            }
            (None, Some(_)) => results / 2,
            (None, None) => return,
        };
        let _ = fit(messages, budget);
    }

    /// Un envoi : la réponse, le refus de fenêtre, ou l'erreur.
    async fn send(&self, messages: &[Value]) -> Result<Result<LocalReply, Overflow>, DriverError> {
        let mut body = json!({"model":self.model,"messages":messages,"stream":false,"max_tokens":self.max_tokens,"temperature":MISSION_TEMPERATURE});
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
        let status = response.status();
        if !status.is_success() {
            // Seul le refus de fenêtre est lu, et seulement ses nombres : le corps d'une erreur
            // peut reprendre l'historique, qui ne doit pas ressortir dans un message.
            if status == reqwest::StatusCode::BAD_REQUEST
                && let Ok(bytes) = read_body(&mut response, MAX_ERROR_BYTES).await
                && let Some(overflow) = Overflow::parse(&bytes)
            {
                return Ok(Err(overflow));
            }
            return Err(DriverError::Io(format!(
                "le moteur local répond HTTP {status}"
            )));
        }
        let bytes = read_body(&mut response, MAX_RESPONSE_BYTES as usize).await?;
        let body = serde_json::from_slice(&bytes)
            .map_err(|_| invalid("le moteur n'a pas rendu de JSON valide"))?;
        Ok(Ok(LocalReply {
            usage: response_usage(&body)?,
            turn: parse_turn(&body, &self.tools).map(|(turn, _)| turn),
        }))
    }
}

pub(crate) async fn read_body(
    response: &mut reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, DriverError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport)? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(invalid("réponse du moteur trop volumineuse"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn invalid(message: &str) -> DriverError {
    DriverError::BadModelOutput(message.to_owned())
}

fn transport(error: impl std::fmt::Display) -> DriverError {
    DriverError::Io(format!("moteur local : {error}"))
}
