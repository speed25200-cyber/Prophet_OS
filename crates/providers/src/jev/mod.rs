//! Jev, le modèle « System One » de TypeSafe AI : un état en entrée, des décisions typées en
//! sortie, jamais du texte.
//!
//! Jev ne génère rien. Il répond à des questions fermées posées sur un état : une probabilité
//! (`noul`), un choix parmi des options nommées (`choice`), une note sur des niveaux ordonnés
//! (`score`). Chaque réponse porte sa calibration, ce qui permet à un programme d'agir seul
//! quand la confiance est haute et de remettre la main à un modèle génératif ou à l'humain
//! quand elle ne l'est pas. Une réponse arrive en quelques centaines de millisecondes, là où un
//! tour de LLM en prend plusieurs secondes ; c'est ce qui rend possible une boucle d'interface
//! serrée, à condition que l'état soit structuré — l'arbre SUP l'est.
//!
//! Ce module porte le protocole tel que documenté au 16 septembre 2026 (voir
//! `docs/specs/jev-decisions.md`) : `POST https://api.typesafe.ai/v1/systemone`, corps
//! `{state, model, questions}`, réponse `{model, answers, usage}`. Il ne parle jamais au réseau
//! lui-même : le seul transport réel passe par le proxy de sortie, avec le jeton de la tâche et
//! une référence de secret que seul le proxy sait résoudre ([`egress`]).
//!
//! Jev est un fournisseur de classe C au sens du plan : une clé d'API, jamais requise. Tout ce
//! qui suit fonctionne sans lui, plus lentement.

pub mod egress;
pub mod operator;
pub mod router;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Hôte de l'API. C'est cet hôte, et lui seul, que le transport par egress demande à joindre.
pub const HOST: &str = "api.typesafe.ai";
/// Chemin des décisions.
pub const PATH: &str = "/v1/systemone";
/// Alias du modèle courant ; les alias peuvent changer de cible, la réponse nomme la version.
pub const DEFAULT_MODEL: &str = "jev-latest";
/// Nombre minimal d'options d'un choix.
pub const MIN_CHOICE_OPTIONS: usize = 2;
/// Nombre maximal d'options d'un choix.
pub const MAX_CHOICE_OPTIONS: usize = 255;
/// Nombre minimal de niveaux d'une note.
pub const MIN_SCORE_LEVELS: usize = 2;
/// Nombre maximal de niveaux d'une note.
pub const MAX_SCORE_LEVELS: usize = 10;
/// Tarif annoncé au lancement, en dollars par million de tokens d'entrée ; la sortie est gratuite.
pub const USD_PER_MILLION_INPUT_TOKENS: f64 = 0.042;
/// Taille maximale d'un état sérialisé envoyé à Jev. Au-delà, l'appelant doit résumer.
pub const MAX_STATE_BYTES: usize = 256 * 1024;

/// Erreur d'une décision.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum JevError {
    /// La question ou l'état ne respectent pas les bornes du protocole.
    #[error("demande invalide : {0}")]
    Invalid(String),
    /// La clé a été refusée (401).
    #[error("clé refusée par l'API")]
    Unauthorized,
    /// L'API a rejeté la demande (422).
    #[error("demande rejetée par l'API : {0}")]
    Rejected(String),
    /// Trop de demandes (429) ; reprendre après le délai indiqué s'il y en a un.
    #[error("débit dépassé (429)")]
    RateLimited {
        /// Délai conseillé avant une nouvelle demande, en secondes.
        retry_after_s: Option<u64>,
    },
    /// Service saturé (529).
    #[error("service saturé (529)")]
    Overloaded,
    /// Le proxy de sortie a refusé la requête avant qu'elle ne parte.
    #[error("sortie refusée par le proxy ({code}) : {detail}")]
    Refused {
        /// Code du refus (`PolicyDenied`, `SecretRefused`, …).
        code: String,
        /// Explication du proxy.
        detail: String,
    },
    /// Transport injoignable ou interrompu.
    #[error("transport : {0}")]
    Transport(String),
    /// Réponse illisible ou incohérente avec la demande.
    #[error("réponse inexploitable : {0}")]
    Malformed(String),
}

/// Une question fermée. Les clés ne sont pas envoyées au modèle : elles servent au programme.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    /// Oui ou non : la réponse est la probabilité du oui.
    Noul {
        /// Consigne, texte ou structure.
        instructions: Value,
        /// Rubriques facultatives du oui et du non.
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    /// Une option parmi celles proposées, chacune décrite par une rubrique.
    Choice {
        /// Consigne.
        instructions: Value,
        /// Options nommées, dans l'ordre de présentation.
        criteria: Map<String, Value>,
    },
    /// Une note sur des niveaux ordonnés ; la réponse peut tomber entre deux niveaux.
    Score {
        /// Consigne.
        instructions: Value,
        /// Niveaux, du plus bas au plus haut.
        criteria: Vec<Value>,
    },
}

/// Rubriques du oui et du non d'une question `noul`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NoulCriteria {
    /// Ce qui fait un oui.
    #[serde(rename = "true")]
    pub yes: Value,
    /// Ce qui fait un non.
    #[serde(rename = "false")]
    pub no: Value,
}

impl Question {
    /// Question oui/non.
    #[must_use]
    pub fn noul(instructions: impl Into<Value>) -> Self {
        Self::Noul {
            instructions: instructions.into(),
            criteria: None,
        }
    }

    /// Question oui/non avec ses deux rubriques.
    #[must_use]
    pub fn noul_with(
        instructions: impl Into<Value>,
        yes: impl Into<Value>,
        no: impl Into<Value>,
    ) -> Self {
        Self::Noul {
            instructions: instructions.into(),
            criteria: Some(NoulCriteria {
                yes: yes.into(),
                no: no.into(),
            }),
        }
    }

    /// Choix parmi des options nommées.
    ///
    /// # Errors
    /// Moins de deux ou plus de 255 options, nom vide ou dupliqué.
    pub fn choice(
        instructions: impl Into<Value>,
        options: impl IntoIterator<Item = (impl Into<String>, impl Into<Value>)>,
    ) -> Result<Self, JevError> {
        let mut criteria = Map::new();
        for (name, rubric) in options {
            let name = name.into();
            if name.trim().is_empty() || criteria.insert(name.clone(), rubric.into()).is_some() {
                return Err(JevError::Invalid(format!(
                    "option de choix vide ou dupliquée : {name:?}"
                )));
            }
        }
        if !(MIN_CHOICE_OPTIONS..=MAX_CHOICE_OPTIONS).contains(&criteria.len()) {
            return Err(JevError::Invalid(format!(
                "un choix prend de {MIN_CHOICE_OPTIONS} à {MAX_CHOICE_OPTIONS} options, reçu {}",
                criteria.len()
            )));
        }
        Ok(Self::Choice {
            instructions: instructions.into(),
            criteria,
        })
    }

    /// Note sur des niveaux ordonnés.
    ///
    /// # Errors
    /// Moins de deux ou plus de dix niveaux.
    pub fn score(
        instructions: impl Into<Value>,
        levels: impl IntoIterator<Item = impl Into<Value>>,
    ) -> Result<Self, JevError> {
        let criteria: Vec<Value> = levels.into_iter().map(Into::into).collect();
        if !(MIN_SCORE_LEVELS..=MAX_SCORE_LEVELS).contains(&criteria.len()) {
            return Err(JevError::Invalid(format!(
                "une note prend de {MIN_SCORE_LEVELS} à {MAX_SCORE_LEVELS} niveaux, reçu {}",
                criteria.len()
            )));
        }
        Ok(Self::Score {
            instructions: instructions.into(),
            criteria,
        })
    }

    /// Noms des options d'un choix, dans l'ordre.
    #[must_use]
    pub fn options(&self) -> Vec<&str> {
        match self {
            Self::Choice { criteria, .. } => criteria.keys().map(String::as_str).collect(),
            _ => Vec::new(),
        }
    }
}

/// Une demande complète : un état, un modèle, des questions nommées.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Request {
    /// L'état sur lequel portent les questions : texte, objet ou liste.
    pub state: Value,
    /// Modèle demandé (`jev-latest` par défaut).
    pub model: String,
    /// Questions, par clé de programme.
    pub questions: Map<String, Value>,
    #[serde(skip)]
    shapes: BTreeMap<String, Shape>,
}

/// Forme attendue d'une réponse, pour la vérifier.
#[derive(Debug, Clone, PartialEq)]
enum Shape {
    Noul,
    Choice(Vec<String>),
    Score(usize),
}

impl Request {
    /// Prépare une demande. L'état est borné : un modèle de décision n'est pas un entrepôt.
    ///
    /// # Errors
    /// Aucune question, état trop grand ou modèle vide.
    pub fn new(
        state: Value,
        model: &str,
        questions: impl IntoIterator<Item = (impl Into<String>, Question)>,
    ) -> Result<Self, JevError> {
        if model.trim().is_empty() {
            return Err(JevError::Invalid("modèle vide".into()));
        }
        let size = serde_json::to_vec(&state).map(|v| v.len()).unwrap_or(0);
        if size > MAX_STATE_BYTES {
            return Err(JevError::Invalid(format!(
                "état de {size} octets, plafond {MAX_STATE_BYTES}"
            )));
        }
        let mut map = Map::new();
        let mut shapes = BTreeMap::new();
        for (key, question) in questions {
            let key = key.into();
            if key.trim().is_empty() {
                return Err(JevError::Invalid("clé de question vide".into()));
            }
            let shape = match &question {
                Question::Noul { .. } => Shape::Noul,
                Question::Choice { criteria, .. } => {
                    Shape::Choice(criteria.keys().cloned().collect())
                }
                Question::Score { criteria, .. } => Shape::Score(criteria.len()),
            };
            let value = serde_json::to_value(&question)
                .map_err(|e| JevError::Invalid(format!("question non sérialisable : {e}")))?;
            if map.insert(key.clone(), value).is_some() {
                return Err(JevError::Invalid(format!(
                    "clé de question dupliquée : {key}"
                )));
            }
            shapes.insert(key, shape);
        }
        if map.is_empty() {
            return Err(JevError::Invalid(
                "au moins une question est requise".into(),
            ));
        }
        Ok(Self {
            state,
            model: model.to_owned(),
            questions: map,
            shapes,
        })
    }

    /// Clés des questions, dans l'ordre.
    #[must_use]
    pub fn keys(&self) -> Vec<&str> {
        self.questions.keys().map(String::as_str).collect()
    }

    /// Corps JSON à envoyer.
    #[must_use]
    pub fn body(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
}

/// Une réponse typée.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    /// Probabilité du oui, de 0 à 1. Ne porte pas de confiance.
    Noul {
        /// Probabilité.
        noul: f64,
    },
    /// Option retenue, distribution complète et confiance de 0 à 1.
    Choice {
        /// Option retenue.
        choice: String,
        /// Probabilité de chaque option.
        probabilities: BTreeMap<String, f64>,
        /// Confiance.
        confidence: f64,
    },
    /// Niveau pondéré, avec la légende des niveaux.
    Score {
        /// Note, de 0 à `niveaux − 1`.
        score: f64,
        /// Description de chaque niveau, par indice.
        #[serde(default)]
        legend: BTreeMap<String, String>,
        /// Probabilité de chaque niveau, par indice.
        #[serde(default)]
        probabilities: BTreeMap<String, f64>,
        /// Confiance.
        confidence: f64,
    },
}

impl Answer {
    /// Probabilité du oui d'une réponse `noul`.
    #[must_use]
    pub fn noul(&self) -> Option<f64> {
        match self {
            Self::Noul { noul } => Some(*noul),
            _ => None,
        }
    }

    /// Option retenue et confiance d'une réponse `choice`.
    #[must_use]
    pub fn choice(&self) -> Option<(&str, f64)> {
        match self {
            Self::Choice {
                choice, confidence, ..
            } => Some((choice, *confidence)),
            _ => None,
        }
    }

    /// Probabilité d'une option, si la réponse est un choix.
    #[must_use]
    pub fn probability_of(&self, option: &str) -> Option<f64> {
        match self {
            Self::Choice { probabilities, .. } => probabilities.get(option).copied(),
            _ => None,
        }
    }

    /// Note d'une réponse `score`, ramenée de 0 à 1 quel que soit le nombre de niveaux.
    #[must_use]
    pub fn score_normalized(&self, levels: usize) -> Option<f64> {
        match self {
            Self::Score { score, .. } => {
                let top = levels.saturating_sub(1).max(1);
                Some(score / top as f64)
            }
            _ => None,
        }
    }
}

/// Consommation rapportée par l'API. La sortie est facturée zéro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Tokens d'entrée, questions comprises.
    #[serde(default)]
    pub input_tokens: u64,
    /// Tokens de sortie.
    #[serde(default)]
    pub output_tokens: u64,
}

impl Usage {
    /// Coût estimé en dollars, au tarif annoncé.
    #[must_use]
    pub fn cost_usd(self) -> f64 {
        self.input_tokens as f64 * USD_PER_MILLION_INPUT_TOKENS / 1_000_000.0
    }
}

/// Une demande répondue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// Modèle qui a réellement répondu (version résolue de l'alias).
    #[serde(default)]
    pub model: String,
    /// Réponses, par clé de question.
    pub answers: BTreeMap<String, Answer>,
    /// Consommation.
    #[serde(default)]
    pub usage: Usage,
}

impl Response {
    /// Lit une réponse et la confronte à la demande : chaque question a sa réponse, du bon
    /// type, dans ses bornes. Une réponse qui cite une option non proposée est refusée, parce
    /// qu'un programme qui l'exécuterait ferait quelque chose que personne n'a offert.
    ///
    /// # Errors
    /// JSON illisible ou incohérent avec la demande.
    pub fn parse(bytes: &[u8], request: &Request) -> Result<Self, JevError> {
        let response: Self = serde_json::from_slice(bytes)
            .map_err(|e| JevError::Malformed(format!("JSON illisible : {e}")))?;
        response.check_against(request)?;
        Ok(response)
    }

    fn check_against(&self, request: &Request) -> Result<(), JevError> {
        let unit = |x: f64, what: &str| {
            if x.is_finite() && (0.0..=1.0).contains(&x) {
                Ok(())
            } else {
                Err(JevError::Malformed(format!("{what} hors de [0, 1] : {x}")))
            }
        };
        for (key, shape) in &request.shapes {
            let answer = self
                .answers
                .get(key)
                .ok_or_else(|| JevError::Malformed(format!("réponse absente pour {key}")))?;
            match (shape, answer) {
                (Shape::Noul, Answer::Noul { noul }) => unit(*noul, "noul")?,
                (
                    Shape::Choice(options),
                    Answer::Choice {
                        choice,
                        probabilities,
                        confidence,
                    },
                ) => {
                    if !options.iter().any(|o| o == choice) {
                        return Err(JevError::Malformed(format!(
                            "{key} : option non proposée {choice:?}"
                        )));
                    }
                    unit(*confidence, "confiance")?;
                    for (option, p) in probabilities {
                        if !options.iter().any(|o| o == option) {
                            return Err(JevError::Malformed(format!(
                                "{key} : probabilité d'une option non proposée {option:?}"
                            )));
                        }
                        unit(*p, "probabilité")?;
                    }
                }
                (
                    Shape::Score(levels),
                    Answer::Score {
                        score,
                        confidence,
                        probabilities,
                        ..
                    },
                ) => {
                    let top = levels.saturating_sub(1) as f64;
                    if !score.is_finite() || *score < 0.0 || *score > top {
                        return Err(JevError::Malformed(format!(
                            "{key} : note {score} hors de [0, {top}]"
                        )));
                    }
                    unit(*confidence, "confiance")?;
                    for p in probabilities.values() {
                        unit(*p, "probabilité")?;
                    }
                }
                _ => {
                    return Err(JevError::Malformed(format!(
                        "{key} : type de réponse différent de la question"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Réponse à une clé.
    #[must_use]
    pub fn answer(&self, key: &str) -> Option<&Answer> {
        self.answers.get(key)
    }
}

/// Ce qu'un transport rend : la réponse, ou l'erreur qui l'a empêchée.
pub type Decided = Result<Response, JevError>;

/// Un moyen d'obtenir des décisions. Synchrone, comme le reste de la boucle native.
pub trait Transport: Send + Sync {
    /// Soumet la demande et rend la réponse vérifiée.
    ///
    /// # Errors
    /// Toute erreur de transport, de refus ou de forme.
    fn decide(&self, request: &Request) -> Decided;

    /// Nom du transport, pour le journal.
    fn name(&self) -> String;
}

/// Transport scripté : une fonction décide à la place de l'API. C'est ce qui rend l'opérateur et
/// le routeur vérifiables sans clé ni réseau, y compris dans leurs échecs.
pub struct Scripted {
    decide: Box<dyn Fn(&Request) -> Decided + Send + Sync>,
    seen: std::sync::Mutex<Vec<Request>>,
}

impl std::fmt::Debug for Scripted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scripted").finish_non_exhaustive()
    }
}

impl Scripted {
    /// Transport dont chaque décision vient de la fonction donnée.
    #[must_use]
    pub fn new(decide: impl Fn(&Request) -> Decided + Send + Sync + 'static) -> Self {
        Self {
            decide: Box::new(decide),
            seen: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Transport qui rend toujours la même erreur : un service absent ou en panne.
    #[must_use]
    pub fn failing(error: JevError) -> Self {
        Self::new(move |_| Err(error.clone()))
    }

    /// Les demandes reçues, dans l'ordre.
    #[must_use]
    pub fn requests(&self) -> Vec<Request> {
        self.seen.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl Transport for Scripted {
    fn decide(&self, request: &Request) -> Decided {
        if let Ok(mut seen) = self.seen.lock() {
            seen.push(request.clone());
        }
        let response = (self.decide)(request)?;
        response.check_against(request)?;
        Ok(response)
    }

    fn name(&self) -> String {
        "jev:scripted".into()
    }
}

/// Construit une réponse de choix, pour les scripts et les tests.
#[must_use]
pub fn choice_answer(choice: &str, probabilities: &[(&str, f64)], confidence: f64) -> Answer {
    Answer::Choice {
        choice: choice.to_owned(),
        probabilities: probabilities
            .iter()
            .map(|(k, v)| ((*k).to_owned(), *v))
            .collect(),
        confidence,
    }
}

/// Construit une réponse complète à partir de réponses nommées, pour les scripts et les tests.
#[must_use]
pub fn response(
    answers: impl IntoIterator<Item = (impl Into<String>, Answer)>,
    tokens: u64,
) -> Response {
    Response {
        model: "jev-1.13.0".into(),
        answers: answers.into_iter().map(|(k, v)| (k.into(), v)).collect(),
        usage: Usage {
            input_tokens: tokens,
            output_tokens: 0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn demande() -> Request {
        Request::new(
            json!({"porte":"ouverte depuis 40 minutes, personne à la maison"}),
            DEFAULT_MODEL,
            [
                ("warn", Question::noul("Faut-il prévenir quelqu'un ?")),
                (
                    "area",
                    Question::choice(
                        "De quel domaine s'agit-il ?",
                        [
                            ("security", "portes, serrures, alarmes"),
                            ("climate", "chauffage et ventilation"),
                        ],
                    )
                    .unwrap(),
                ),
                (
                    "urgency",
                    Question::score("Quelle urgence ?", ["ignorer", "aujourd'hui", "maintenant"])
                        .unwrap(),
                ),
            ],
        )
        .unwrap()
    }

    #[test]
    fn le_corps_suit_le_protocole_documente() {
        let body: Value = serde_json::from_slice(&demande().body()).unwrap();
        assert_eq!(body["model"], "jev-latest");
        assert_eq!(
            body["state"]["porte"],
            "ouverte depuis 40 minutes, personne à la maison"
        );
        assert_eq!(body["questions"]["warn"]["type"], "noul");
        assert_eq!(body["questions"]["warn"].get("criteria"), None);
        assert_eq!(body["questions"]["area"]["type"], "choice");
        assert_eq!(
            body["questions"]["area"]["criteria"]["security"],
            "portes, serrures, alarmes"
        );
        assert_eq!(body["questions"]["urgency"]["type"], "score");
        assert_eq!(body["questions"]["urgency"]["criteria"][2], "maintenant");
        // L'ordre des options est celui de la présentation, pas l'ordre alphabétique.
        let keys: Vec<&String> = body["questions"]["area"]["criteria"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(keys, ["security", "climate"]);
    }

    #[test]
    fn les_bornes_du_protocole_sont_appliquees_avant_l_envoi() {
        assert!(Question::choice("x", [("seule", "")]).is_err());
        assert!(Question::choice("x", [("a", ""), ("a", "")]).is_err());
        assert!(Question::choice("x", [("", ""), ("b", "")]).is_err());
        assert!(Question::score("x", ["seul"]).is_err());
        assert!(Question::score("x", (0..11).map(|i| i.to_string())).is_err());
        let large: Vec<(String, &str)> = (0..256).map(|i| (format!("o{i}"), "")).collect();
        assert!(Question::choice("x", large).is_err());
        assert!(Request::new(json!({}), "jev-latest", Vec::<(&str, Question)>::new()).is_err());
        assert!(Request::new(json!({}), "", [("q", Question::noul("?"))]).is_err());
        let big = json!({"t": "x".repeat(MAX_STATE_BYTES + 1)});
        assert!(Request::new(big, "jev-latest", [("q", Question::noul("?"))]).is_err());
    }

    #[test]
    fn une_reponse_conforme_est_lue_et_typee() {
        let raw = json!({
            "model":"jev-1.13.0",
            "answers":{
                "warn":{"type":"noul","noul":0.94},
                "area":{"type":"choice","choice":"security","probabilities":{"security":0.97,"climate":0.03},"confidence":0.95},
                "urgency":{"type":"score","score":1.8,"legend":{"0":"ignorer","1":"aujourd'hui","2":"maintenant"},"probabilities":{"0":0.05,"1":0.1,"2":0.85},"confidence":0.8}
            },
            "usage":{"input_tokens":521,"output_tokens":0}
        });
        let response = Response::parse(raw.to_string().as_bytes(), &demande()).unwrap();
        assert_eq!(response.model, "jev-1.13.0");
        assert_eq!(response.answer("warn").unwrap().noul(), Some(0.94));
        assert_eq!(
            response.answer("area").unwrap().choice(),
            Some(("security", 0.95))
        );
        assert_eq!(
            response.answer("area").unwrap().probability_of("climate"),
            Some(0.03)
        );
        let score = response
            .answer("urgency")
            .unwrap()
            .score_normalized(3)
            .unwrap();
        assert!((score - 0.9).abs() < 1e-9);
        assert_eq!(response.usage.input_tokens, 521);
        assert!((response.usage.cost_usd() - 521.0 * 0.042 / 1e6).abs() < 1e-12);
    }

    #[test]
    fn une_option_non_proposee_est_refusee() {
        let raw = json!({"answers":{
            "warn":{"type":"noul","noul":0.5},
            "area":{"type":"choice","choice":"kitchen","probabilities":{},"confidence":0.9},
            "urgency":{"type":"score","score":1.0,"probabilities":{},"confidence":0.9}
        }});
        let err = Response::parse(raw.to_string().as_bytes(), &demande()).unwrap_err();
        assert!(
            matches!(&err, JevError::Malformed(m) if m.contains("kitchen")),
            "{err}"
        );
    }

    #[test]
    fn une_reponse_incomplete_ou_hors_bornes_est_refusee() {
        let sans_reponse = json!({"answers":{"warn":{"type":"noul","noul":0.5}}});
        assert!(Response::parse(sans_reponse.to_string().as_bytes(), &demande()).is_err());
        let mauvais_type = json!({"answers":{
            "warn":{"type":"choice","choice":"security","probabilities":{},"confidence":0.9},
            "area":{"type":"choice","choice":"security","probabilities":{},"confidence":0.9},
            "urgency":{"type":"score","score":1.0,"probabilities":{},"confidence":0.9}
        }});
        assert!(Response::parse(mauvais_type.to_string().as_bytes(), &demande()).is_err());
        let hors_bornes = json!({"answers":{
            "warn":{"type":"noul","noul":1.4},
            "area":{"type":"choice","choice":"security","probabilities":{},"confidence":0.9},
            "urgency":{"type":"score","score":1.0,"probabilities":{},"confidence":0.9}
        }});
        assert!(Response::parse(hors_bornes.to_string().as_bytes(), &demande()).is_err());
        let note_trop_haute = json!({"answers":{
            "warn":{"type":"noul","noul":0.4},
            "area":{"type":"choice","choice":"security","probabilities":{},"confidence":0.9},
            "urgency":{"type":"score","score":2.5,"probabilities":{},"confidence":0.9}
        }});
        assert!(Response::parse(note_trop_haute.to_string().as_bytes(), &demande()).is_err());
        assert!(Response::parse(b"pas du json", &demande()).is_err());
    }

    #[test]
    fn le_transport_scripte_verifie_aussi_ses_propres_reponses() {
        let transport =
            Scripted::new(|_| Ok(response([("area", choice_answer("kitchen", &[], 0.9))], 10)));
        let request = Request::new(
            json!("x"),
            DEFAULT_MODEL,
            [(
                "area",
                Question::choice("?", [("security", ""), ("climate", "")]).unwrap(),
            )],
        )
        .unwrap();
        assert!(matches!(
            transport.decide(&request),
            Err(JevError::Malformed(_))
        ));
        assert_eq!(transport.requests().len(), 1);
    }
}
