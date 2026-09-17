//! L'opérateur : Jev fait le « computer use » lui-même, sur l'arbre sémantique, sans LLM.
//!
//! Le plan de Prophet OS dit que l'écran est une projection et que l'état d'une application est
//! un arbre : rôles, noms, valeurs, actions typées. Cet arbre est exactement l'entrée que Jev
//! sait lire. À chaque tour, l'opérateur transforme la page en un état structuré et les éléments
//! actionnables en options nommées (`click:n12`, `fill:n4:v1`, `submit:n4`, `done`, `escalate`),
//! puis pose trois questions : quelle est la prochaine action, l'objectif est-il déjà atteint,
//! la page est-elle bloquée. La réponse arrive en quelques centaines de millisecondes et
//! devient un appel d'outil ordinaire — `web.act` — qui passe par le registre, capd et le
//! journal comme n'importe quel appel d'un modèle. L'opérateur ne détient aucun droit de plus.
//!
//! Ce que Jev ne fait pas, il ne fait pas semblant de le faire. Il ne génère pas de texte : un
//! champ n'est rempli qu'avec une valeur connue d'avance — les segments entre guillemets de
//! l'intention, ou une banque fournie par l'appelant. Quand la page demande d'écrire, quand la
//! confiance est basse, quand la page est un mur (connexion, captcha, erreur) ou quand la boucle
//! tourne en rond, l'opérateur **rend la main** ([`DriverError::HandOver`]) et une [`Cascade`]
//! donne la même histoire au modèle génératif. Le rapide décide, le lent écrit.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{Value, json};
use sup::tree::{Node, Role, Tree};

use super::{Answer, JevError, Question, Request, Transport};
use crate::DriverError;
use crate::native::{ModelClient, ModelTurn, Usage};

/// Ce que l'opérateur doit obtenir, et ce qu'il a le droit d'écrire.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Goal {
    /// L'intention, telle que l'humain l'a formulée.
    pub intent: String,
    /// Première adresse à ouvrir, si aucune page ne l'est encore.
    pub start_url: Option<String>,
    /// Valeurs que l'opérateur peut placer dans un champ : (étiquette, valeur). Jev choisit
    /// laquelle va où ; il n'en invente aucune.
    pub values: Vec<(String, String)>,
}

impl Goal {
    /// Objectif dont les valeurs sont les segments entre guillemets de l'intention, et dont la
    /// page de départ est l'adresse donnée ou, à défaut, la première adresse citée dans
    /// l'intention.
    #[must_use]
    pub fn from_intent(intent: &str, start_url: Option<&str>) -> Self {
        Self {
            intent: intent.to_owned(),
            start_url: start_url.map(str::to_owned).or_else(|| first_url(intent)),
            values: values_from_intent(intent),
        }
    }
}

/// Ce que l'opérateur a fait, lisible après la mission : ce que l'humain doit pouvoir relire
/// pour savoir qui a décidé quoi.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Trace {
    /// Décisions demandées à Jev.
    pub decisions: u32,
    /// Actions exécutées sur décision de Jev, dans l'ordre.
    pub actions: Vec<String>,
    /// Raisons pour lesquelles la main a été rendue, dans l'ordre.
    pub handovers: Vec<String>,
    /// Tokens d'entrée facturés par Jev.
    pub input_tokens: u64,
}

/// Seuils au-dessous desquels l'opérateur rend la main plutôt que d'agir.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    /// Confiance minimale pour exécuter l'action choisie.
    pub act: f64,
    /// Probabilité minimale pour déclarer l'objectif atteint.
    pub done: f64,
    /// Probabilité à partir de laquelle la page est tenue pour bloquée.
    pub blocked: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            act: 0.6,
            done: 0.85,
            blocked: 0.8,
        }
    }
}

/// Nombre maximal d'actions décidées par mission avant de rendre la main.
pub const DEFAULT_MAX_ACTIONS: u32 = 40;
/// Nombre de textes de page transmis dans l'état.
const MAX_TEXTS: usize = 40;
/// Longueur maximale d'un texte ou d'un nom transmis.
const MAX_TEXT_CHARS: usize = 160;
/// Actions récentes rappelées à Jev.
const MAX_HISTORY: usize = 6;
/// Préfixe des outils d'interface pilotés : `web.open`, `web.tree`, `web.act`.
const SURFACE: &str = "web";

/// L'opérateur Jev, vu par la boucle native comme un modèle.
pub struct Operator {
    transport: Arc<dyn Transport>,
    goal: Goal,
    model: String,
    thresholds: Thresholds,
    max_actions: u32,
    actions: Vec<String>,
    handed_over: HashSet<String>,
    failures: u32,
    reobserved: bool,
    decisions: u32,
    input_tokens: u64,
}

impl std::fmt::Debug for Operator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Operator")
            .field("transport", &self.transport.name())
            .field("actions", &self.actions.len())
            .finish_non_exhaustive()
    }
}

/// Ce que l'opérateur a décidé pour un tour, avant traduction en appel d'outil.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Agir : un appel d'outil.
    Act {
        /// Option retenue (`click:n3`, `fill:n4:v1`, `submit:n4`).
        option: String,
        /// Appel d'outil correspondant.
        call: (String, Value),
        /// Confiance de Jev dans ce choix.
        confidence: f64,
    },
    /// L'objectif est atteint.
    Done {
        /// Probabilité rendue par Jev.
        probability: f64,
    },
    /// Rendre la main, avec la raison.
    HandOver(String),
}

impl Operator {
    /// Opérateur pour un objectif, sur un transport et un modèle Jev donnés.
    #[must_use]
    pub fn new(transport: Arc<dyn Transport>, goal: Goal, model: &str) -> Self {
        Self {
            transport,
            goal,
            model: model.to_owned(),
            thresholds: Thresholds::default(),
            max_actions: DEFAULT_MAX_ACTIONS,
            actions: Vec::new(),
            handed_over: HashSet::new(),
            failures: 0,
            reobserved: false,
            decisions: 0,
            input_tokens: 0,
        }
    }

    /// Fixe les seuils.
    #[must_use]
    pub const fn thresholds(mut self, thresholds: Thresholds) -> Self {
        self.thresholds = thresholds;
        self
    }

    /// Fixe le plafond d'actions.
    #[must_use]
    pub const fn max_actions(mut self, max: u32) -> Self {
        self.max_actions = max;
        self
    }

    /// Actions exécutées jusqu'ici, dans l'ordre.
    #[must_use]
    pub fn actions(&self) -> &[String] {
        &self.actions
    }

    /// Décisions demandées et tokens facturés jusqu'ici.
    #[must_use]
    pub const fn spent(&self) -> (u32, u64) {
        (self.decisions, self.input_tokens)
    }

    /// Décide le tour suivant à partir de l'histoire de la boucle native.
    ///
    /// # Errors
    /// Transport en panne ou réponse inexploitable ; l'appelant décide s'il rend la main.
    pub fn decide(&mut self, history: &[Value]) -> Result<(Verdict, Usage), JevError> {
        let Some(last) = history.last() else {
            return Ok((Verdict::HandOver("histoire vide".into()), Usage::default()));
        };
        if last["role"] != "tool" {
            // Rien n'a encore été observé : ouvrir la page de départ, ou laisser un modèle
            // décider par où commencer. Ouvrir ne demande rien à Jev, mais c'est une action
            // de l'opérateur, et elle se relit comme telle.
            return Ok((
                match &self.goal.start_url {
                    Some(url) if history.len() == 1 => {
                        let option = format!("open:{url}");
                        self.actions.push(option.clone());
                        Verdict::Act {
                            option,
                            call: (format!("{SURFACE}.open"), json!({"url": url})),
                            confidence: 1.0,
                        }
                    }
                    _ => Verdict::HandOver("aucune page observée".into()),
                },
                Usage::default(),
            ));
        }
        if last["ok"] != true {
            self.failures += 1;
            if self.failures >= 2 || self.reobserved {
                return Ok((
                    Verdict::HandOver(format!("{} échecs d'outil consécutifs", self.failures)),
                    Usage::default(),
                ));
            }
            // Une action qui échoue laisse souvent une page qui a changé : la relire une fois
            // avant de conclure.
            self.reobserved = true;
            return Ok((
                Verdict::Act {
                    option: "observe".into(),
                    call: (format!("{SURFACE}.tree"), json!({})),
                    confidence: 1.0,
                },
                Usage::default(),
            ));
        }
        self.failures = 0;
        self.reobserved = false;
        let Some(tree) = last["result"]
            .get("tree")
            .and_then(|t| serde_json::from_value::<Tree>(t.clone()).ok())
        else {
            return Ok((
                Verdict::HandOver("dernière observation sans arbre sémantique".into()),
                Usage::default(),
            ));
        };
        let digest = tree.digest();
        if self.handed_over.contains(&digest) {
            return Ok((
                Verdict::HandOver("page déjà remise au modèle génératif".into()),
                Usage::default(),
            ));
        }
        if self.actions.len() as u32 >= self.max_actions {
            return Ok((
                Verdict::HandOver(format!(
                    "{} actions décidées, plafond atteint",
                    self.actions.len()
                )),
                Usage::default(),
            ));
        }
        let url = last["result"]["url"].as_str().unwrap_or("").to_owned();
        let candidates = candidates(&tree, &self.goal.values);
        let request = self.request(&tree, &url, &candidates)?;
        let response = self.transport.decide(&request)?;
        self.decisions += 1;
        self.input_tokens += response.usage.input_tokens;
        let usage = Usage {
            tokens_in: response.usage.input_tokens,
            tokens_out: response.usage.output_tokens,
        };
        let done = response
            .answer("done")
            .and_then(Answer::noul)
            .unwrap_or(0.0);
        let blocked = response
            .answer("blocked")
            .and_then(Answer::noul)
            .unwrap_or(0.0);
        let (option, confidence) = response
            .answer("next")
            .and_then(Answer::choice)
            .map(|(o, c)| (o.to_owned(), c))
            .ok_or_else(|| JevError::Malformed("réponse next absente".into()))?;

        let verdict = if done >= self.thresholds.done || option == "done" {
            Verdict::Done { probability: done }
        } else if blocked >= self.thresholds.blocked {
            Verdict::HandOver(format!("page bloquée selon Jev (p = {blocked:.2})"))
        } else if option == "escalate" {
            Verdict::HandOver("Jev demande un modèle génératif".into())
        } else if confidence < self.thresholds.act {
            Verdict::HandOver(format!(
                "confiance {confidence:.2} sous le seuil {:.2} pour {option}",
                self.thresholds.act
            ))
        } else if self.loops_on(&option) {
            Verdict::HandOver(format!("boucle sur {option}"))
        } else {
            match candidates.iter().find(|c| c.key == option) {
                Some(candidate) => Verdict::Act {
                    option: option.clone(),
                    call: (format!("{SURFACE}.act"), candidate.arguments.clone()),
                    confidence,
                },
                None => Verdict::HandOver(format!("option inconnue {option}")),
            }
        };
        match &verdict {
            Verdict::Act { option, .. } => self.actions.push(option.clone()),
            Verdict::HandOver(_) => {
                self.handed_over.insert(digest);
            }
            Verdict::Done { .. } => {}
        }
        Ok((verdict, usage))
    }

    fn loops_on(&self, option: &str) -> bool {
        self.actions.len() >= 2
            && self.actions[self.actions.len() - 2..]
                .iter()
                .all(|a| a == option)
    }

    fn request(
        &self,
        tree: &Tree,
        url: &str,
        candidates: &[Candidate],
    ) -> Result<Request, JevError> {
        let mut texts = Vec::new();
        let mut fields = Vec::new();
        tree.root.walk(&mut |node| match node.role {
            Role::Text | Role::Status | Role::Cell | Role::Item if !node.name.is_empty() => {
                if texts.len() < MAX_TEXTS {
                    texts.push(truncate(&node.name));
                }
            }
            Role::Field | Role::Select | Role::Toggle => fields.push(json!({
                "id": node.id,
                "name": truncate(&node.name),
                "value": node.value.as_deref().map(truncate),
                "disabled": node.disabled,
            })),
            _ => {}
        });
        let recent: Vec<&String> = self
            .actions
            .iter()
            .rev()
            .take(MAX_HISTORY)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let state = json!({
            "goal": self.goal.intent,
            "values": self.goal.values.iter().map(|(k, v)| json!({"key": k, "value": v})).collect::<Vec<_>>(),
            "page": {
                "url": url,
                "title": tree.title,
                "texts": texts,
                "fields": fields,
                "focus": tree.focus,
            },
            "actions_so_far": recent,
            "step": self.actions.len() + 1,
        });
        let options: Vec<(String, Value)> = candidates
            .iter()
            .map(|c| (c.key.clone(), c.rubric.clone()))
            .collect();
        Request::new(
            state,
            &self.model,
            [
                (
                    "next",
                    Question::choice(
                        "You operate a web page through its semantic tree to achieve the goal. \
                         Choose the single next action that makes progress. Choose `done` only if the page \
                         already shows the goal is fully achieved. Choose `escalate` if progress needs text \
                         that is not among the provided values, needs judgement beyond this page, or if no \
                         listed action helps.",
                        options,
                    )?,
                ),
                (
                    "done",
                    Question::noul_with(
                        "Is the goal already fully achieved, as evidenced by the current page?",
                        "The page shows the requested result or confirmation.",
                        "Something remains to be done, or the evidence is missing.",
                    ),
                ),
                (
                    "blocked",
                    Question::noul_with(
                        "Is the page a wall that a human must handle: login, captcha, payment, error, or consent?",
                        "A human must intervene before any progress.",
                        "The page can be operated with the listed actions.",
                    ),
                ),
            ],
        )
    }
}

impl ModelClient for Operator {
    fn next_turn(&mut self, history: &[Value]) -> Result<(ModelTurn, Usage), DriverError> {
        let (verdict, usage) = self
            .decide(history)
            .map_err(|e| DriverError::BadModelOutput(format!("Jev : {e}")))?;
        match verdict {
            Verdict::Act {
                call: (tool, arguments),
                ..
            } => Ok((ModelTurn::ToolCall { tool, arguments }, usage)),
            Verdict::Done { probability } => Ok((
                ModelTurn::Final {
                    text: format!(
                        "Objectif atteint selon Jev (p = {probability:.2}) après {} action(s) : {}",
                        self.actions.len(),
                        self.actions.join(", ")
                    ),
                },
                usage,
            )),
            Verdict::HandOver(reason) => Err(DriverError::HandOver(reason)),
        }
    }

    fn model_name(&self) -> String {
        format!("jev:{}", self.model)
    }
}

/// Jev d'abord, un modèle génératif quand il rend la main. Le routage se fait par appel : à
/// chaque nouvelle page, l'opérateur a la première main ; ce qu'il refuse, le modèle reprend
/// avec la même histoire, y compris les actions déjà faites.
pub struct Cascade {
    operator: Operator,
    fallback: Box<dyn ModelClient>,
    handovers: Vec<String>,
    trace: Arc<std::sync::Mutex<Trace>>,
}

impl std::fmt::Debug for Cascade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cascade")
            .field("operator", &self.operator)
            .field("fallback", &self.fallback.model_name())
            .finish()
    }
}

impl Cascade {
    /// Cascade d'un opérateur vers un modèle.
    #[must_use]
    pub fn new(operator: Operator, fallback: Box<dyn ModelClient>) -> Self {
        Self {
            operator,
            fallback,
            handovers: Vec::new(),
            trace: Arc::new(std::sync::Mutex::new(Trace::default())),
        }
    }

    /// La trace partagée, mise à jour à chaque tour ; l'appelant la relit après la mission,
    /// quand la boucle native a repris possession de la cascade.
    #[must_use]
    pub fn trace(&self) -> Arc<std::sync::Mutex<Trace>> {
        Arc::clone(&self.trace)
    }

    /// Les raisons pour lesquelles la main a été rendue, dans l'ordre.
    #[must_use]
    pub fn handovers(&self) -> &[String] {
        &self.handovers
    }

    fn record(&self) {
        if let Ok(mut trace) = self.trace.lock() {
            let (decisions, input_tokens) = self.operator.spent();
            trace.decisions = decisions;
            trace.input_tokens = input_tokens;
            trace.actions = self.operator.actions().to_vec();
            trace.handovers.clone_from(&self.handovers);
        }
    }

    /// Actions décidées par Jev.
    #[must_use]
    pub fn jev_actions(&self) -> &[String] {
        self.operator.actions()
    }
}

impl ModelClient for Cascade {
    fn next_turn(&mut self, history: &[Value]) -> Result<(ModelTurn, Usage), DriverError> {
        let turn = match self.operator.next_turn(history) {
            Err(DriverError::HandOver(reason)) => {
                tracing::info!(%reason, "Jev rend la main au modèle génératif");
                self.handovers.push(reason);
                self.record();
                self.fallback.next_turn(history)
            }
            Err(DriverError::BadModelOutput(reason)) => {
                // Une panne de Jev n'est pas une panne de la mission : le modèle continue.
                tracing::warn!(%reason, "Jev indisponible ; le modèle génératif continue");
                self.handovers.push(reason);
                self.record();
                self.fallback.next_turn(history)
            }
            other => other,
        };
        self.record();
        turn
    }

    fn model_name(&self) -> String {
        format!(
            "{}+{}",
            self.operator.model_name(),
            self.fallback.model_name()
        )
    }
}

/// Une option offerte à Jev et l'appel qu'elle déclenche.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Nom de l'option.
    pub key: String,
    /// Rubrique lue par Jev.
    pub rubric: Value,
    /// Arguments de `web.act`.
    pub arguments: Value,
}

/// Les options d'une page : un clic par élément cliquable, un remplissage par champ et par
/// valeur connue, un envoi par champ déjà rempli, puis `done` et `escalate`. Jamais plus de
/// 255, la borne du protocole ; au-delà, les derniers éléments de la page sont tus.
#[must_use]
pub fn candidates(tree: &Tree, values: &[(String, String)]) -> Vec<Candidate> {
    let mut out = Vec::new();
    tree.root.walk(&mut |node: &Node| {
        if !node.actionable || node.disabled {
            return;
        }
        let name = truncate(&node.name);
        match node.role {
            Role::Button | Role::Link | Role::Toggle | Role::Item | Role::Image => {
                out.push(Candidate {
                    key: format!("click:{}", node.id),
                    rubric: json!({"action": "click", "role": node.role, "name": name, "value": node.value.as_deref().map(truncate)}),
                    arguments: json!({"action": "click", "node": node.id}),
                });
            }
            Role::Field | Role::Select | Role::RichText => {
                for (label, value) in values {
                    out.push(Candidate {
                        key: format!("fill:{}:{label}", node.id),
                        rubric: json!({"action": "set_field", "field": name, "current": node.value.as_deref().map(truncate), "value": value}),
                        arguments: json!({"action": "set_field", "node": node.id, "value": value}),
                    });
                }
                if node.value.as_deref().is_some_and(|v| !v.trim().is_empty()) {
                    out.push(Candidate {
                        key: format!("submit:{}", node.id),
                        rubric: json!({"action": "submit", "field": name, "note": "sends the form of this field; requires human approval"}),
                        arguments: json!({"action": "submit", "node": node.id}),
                    });
                }
            }
            _ => {}
        }
    });
    out.truncate(super::MAX_CHOICE_OPTIONS - 2);
    out.push(Candidate {
        key: "done".into(),
        rubric: json!("The goal is already achieved on this page; stop."),
        arguments: Value::Null,
    });
    out.push(Candidate {
        key: "escalate".into(),
        rubric: json!("Hand over to a generative model: text must be written, a value is missing, or none of the actions helps."),
        arguments: Value::Null,
    });
    out
}

/// Les segments entre guillemets d'une intention, dans l'ordre, sans doublon : ce sont les seules
/// chaînes que l'opérateur a le droit d'écrire dans un champ.
#[must_use]
pub fn values_from_intent(intent: &str) -> Vec<(String, String)> {
    let mut values: Vec<(String, String)> = Vec::new();
    let mut rest = intent;
    while values.len() < 16 {
        let Some((open, close, skip)) = [
            ("\"", "\"", 1),
            ("«", "»", '«'.len_utf8()),
            ("“", "”", '“'.len_utf8()),
        ]
        .into_iter()
        .filter_map(|(o, c, skip)| rest.find(o).map(|i| (i, c, skip)))
        .min_by_key(|(i, _, _)| *i) else {
            break;
        };
        let after = &rest[open + skip..];
        let Some(end) = after.find(close) else {
            break;
        };
        let value = after[..end].trim();
        rest = &after[end + close.len()..];
        if value.is_empty() || value.chars().count() > 200 || values.iter().any(|(_, v)| v == value)
        {
            continue;
        }
        values.push((format!("v{}", values.len() + 1), value.to_owned()));
    }
    values
}

/// La première adresse `http://` ou `https://` citée dans un texte, sans la ponctuation qui la
/// suit.
#[must_use]
pub fn first_url(text: &str) -> Option<String> {
    let start = ["https://", "http://"]
        .into_iter()
        .filter_map(|scheme| text.find(scheme))
        .min()?;
    let raw: String = text[start..]
        .chars()
        .take_while(|c| {
            !c.is_whitespace() && !matches!(c, '"' | '«' | '»' | '“' | '”' | '\'' | '<' | '>')
        })
        .collect();
    let trimmed = raw.trim_end_matches(['.', ',', ';', ':', ')', ']', '!', '?']);
    let host = trimmed.split_once("://").map(|(_, r)| r)?;
    if host.is_empty() || host.starts_with('/') {
        return None;
    }
    Some(trimmed.to_owned())
}

fn truncate(text: &str) -> String {
    if text.chars().count() <= MAX_TEXT_CHARS {
        text.to_owned()
    } else {
        text.chars()
            .take(MAX_TEXT_CHARS - 1)
            .chain(std::iter::once('…'))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jev::{JevError, Scripted, choice_answer, response};
    use crate::native::ScriptedModel;

    fn page(title: &str, field_value: Option<&str>) -> Tree {
        let mut field = Node::new("n4", Role::Field, "Ville").actionable();
        if let Some(v) = field_value {
            field = field.value(v);
        }
        Tree::new(
            "browser",
            "main",
            title,
            Node::new("n1", Role::Group, "").children([
                Node::new("n2", Role::Text, "Réserver un billet"),
                field,
                Node::new("n5", Role::Link, "Voir les horaires").actionable(),
                Node::new("n6", Role::Button, "Désactivé").actionable(),
                Node::new("n7", Role::Button, "Confirmer").actionable(),
            ]),
        )
    }

    fn observed(tree: &Tree, url: &str) -> Value {
        json!({"role":"tool","ok":true,"result":{"url":url,"title":tree.title,"tree":tree}})
    }

    fn goal() -> Goal {
        Goal::from_intent(
            "Indique la ville « Paris » puis ouvre les horaires.",
            Some("http://127.0.0.1:1/reservation"),
        )
    }

    fn jev_disant(next: &'static str, confidence: f64, done: f64, blocked: f64) -> Arc<Scripted> {
        Arc::new(Scripted::new(move |_| {
            Ok(response(
                [
                    (
                        "next",
                        choice_answer(next, &[(next, confidence)], confidence),
                    ),
                    ("done", Answer::Noul { noul: done }),
                    ("blocked", Answer::Noul { noul: blocked }),
                ],
                800,
            ))
        }))
    }

    #[test]
    fn les_valeurs_viennent_des_guillemets_de_l_intention() {
        assert_eq!(
            values_from_intent(
                "Cherche \"Prophet OS\" et « Jev » puis \"Prophet OS\" encore ; l'agent n'écrit rien d'autre."
            ),
            vec![
                ("v1".to_owned(), "Prophet OS".to_owned()),
                ("v2".to_owned(), "Jev".to_owned())
            ]
        );
        assert!(values_from_intent("sans guillemets").is_empty());
        assert!(values_from_intent("ouvert \"jamais fermé").is_empty());
    }

    #[test]
    fn la_page_de_depart_est_la_premiere_adresse_citee() {
        assert_eq!(
            first_url("Va sur https://exemple.fr/horaires?jour=demain, puis réserve.").as_deref(),
            Some("https://exemple.fr/horaires?jour=demain")
        );
        assert_eq!(
            first_url("Ouvre « http://127.0.0.1:8000/ » et lis.").as_deref(),
            Some("http://127.0.0.1:8000/")
        );
        assert_eq!(first_url("rien à ouvrir"), None);
        assert_eq!(first_url("https:// n'est pas une adresse"), None);
        let goal = Goal::from_intent("Ouvre https://a.fr et cherche \"x\"", None);
        assert_eq!(goal.start_url.as_deref(), Some("https://a.fr"));
        assert_eq!(goal.values, vec![("v1".to_owned(), "x".to_owned())]);
        let explicite = Goal::from_intent("Ouvre https://a.fr", Some("https://b.fr"));
        assert_eq!(explicite.start_url.as_deref(), Some("https://b.fr"));
    }

    #[test]
    fn les_options_couvrent_les_elements_actionnables_et_jamais_plus_de_255() {
        let mut page = page("Réservation", Some("Paris"));
        let options = candidates(&page, &[("v1".into(), "Paris".into())]);
        let keys: Vec<&str> = options.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "fill:n4:v1",
                "submit:n4",
                "click:n5",
                "click:n6",
                "click:n7",
                "done",
                "escalate"
            ]
        );
        assert_eq!(
            options[0].arguments,
            json!({"action":"set_field","node":"n4","value":"Paris"})
        );
        // Un champ vide n'offre pas d'envoi ; un élément désactivé n'offre rien.
        page.root.children[1].value = None;
        page.root.children[3].disabled = true;
        let keys: Vec<String> = candidates(&page, &[("v1".into(), "Paris".into())])
            .into_iter()
            .map(|c| c.key)
            .collect();
        assert_eq!(
            keys,
            ["fill:n4:v1", "click:n5", "click:n7", "done", "escalate"]
        );
        let many = Tree::new(
            "browser",
            "main",
            "Grande page",
            Node::new("r", Role::Group, "").children((0..300).map(|i| {
                Node::new(format!("b{i}"), Role::Button, format!("Bouton {i}")).actionable()
            })),
        );
        let options = candidates(&many, &[]);
        assert_eq!(options.len(), 255);
        assert_eq!(options[254].key, "escalate");
    }

    #[test]
    fn l_operateur_ouvre_la_page_puis_remplit_puis_clique_puis_conclut() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let vu = seen.clone();
        let transport = Arc::new(Scripted::new(move |request| {
            let page = request.state["page"]["title"]
                .as_str()
                .unwrap_or("")
                .to_owned();
            let filled = request.state["page"]["fields"][0]["value"] == "Paris";
            vu.lock().unwrap().push(request.state.clone());
            let (next, done) = match (page.as_str(), filled) {
                ("Réservation", false) => ("fill:n4:v1", 0.05),
                ("Réservation", true) => ("click:n5", 0.1),
                ("Horaires de Paris", _) => ("done", 0.97),
                _ => ("escalate", 0.0),
            };
            Ok(response(
                [
                    ("next", choice_answer(next, &[(next, 0.9)], 0.92)),
                    ("done", Answer::Noul { noul: done }),
                    ("blocked", Answer::Noul { noul: 0.02 }),
                ],
                640,
            ))
        }));
        let mut operator = Operator::new(transport, goal(), "jev-latest");
        let mut history = vec![json!({"role":"user","content":goal().intent})];

        let (turn, usage) = operator.next_turn(&history).unwrap();
        assert_eq!(
            turn,
            ModelTurn::ToolCall {
                tool: "web.open".into(),
                arguments: json!({"url":"http://127.0.0.1:1/reservation"})
            }
        );
        assert_eq!(usage, Usage::default(), "ouvrir ne consulte pas Jev");
        history.push(
            json!({"role":"assistant","tool_call":{"tool":"web.open","arguments":{"url":"x"}}}),
        );
        history.push(observed(
            &page("Réservation", None),
            "http://127.0.0.1:1/reservation",
        ));

        let (turn, usage) = operator.next_turn(&history).unwrap();
        assert_eq!(
            turn,
            ModelTurn::ToolCall {
                tool: "web.act".into(),
                arguments: json!({"action":"set_field","node":"n4","value":"Paris"})
            }
        );
        assert_eq!(usage.tokens_in, 640);
        history.push(json!({"role":"assistant","tool_call":{"tool":"web.act","arguments":{}}}));
        history.push(observed(
            &page("Réservation", Some("Paris")),
            "http://127.0.0.1:1/reservation",
        ));

        let (turn, _) = operator.next_turn(&history).unwrap();
        assert_eq!(
            turn,
            ModelTurn::ToolCall {
                tool: "web.act".into(),
                arguments: json!({"action":"click","node":"n5"})
            }
        );
        history.push(json!({"role":"assistant","tool_call":{"tool":"web.act","arguments":{}}}));
        history.push(observed(
            &page("Horaires de Paris", Some("Paris")),
            "http://127.0.0.1:1/horaires",
        ));

        let (turn, _) = operator.next_turn(&history).unwrap();
        match turn {
            ModelTurn::Final { text } => {
                assert!(text.contains("p = 0.97"), "{text}");
                assert!(
                    text.contains("open:http://127.0.0.1:1/reservation, fill:n4:v1, click:n5"),
                    "{text}"
                );
            }
            other => panic!("{other:?}"),
        }
        let states = seen.lock().unwrap();
        assert_eq!(states.len(), 3);
        assert_eq!(states[0]["goal"], goal().intent);
        assert_eq!(states[0]["values"][0]["value"], "Paris");
        assert_eq!(states[0]["page"]["texts"][0], "Réserver un billet");
        assert_eq!(
            states[2]["actions_so_far"],
            json!([
                "open:http://127.0.0.1:1/reservation",
                "fill:n4:v1",
                "click:n5"
            ])
        );
        assert_eq!(states[2]["step"], 4);
    }

    #[test]
    fn une_confiance_basse_une_escalade_ou_un_mur_rendent_la_main() {
        let history = vec![
            json!({"role":"user","content":"x"}),
            json!({"role":"assistant","tool_call":{}}),
            observed(&page("Réservation", None), "u"),
        ];
        let cases = [
            (jev_disant("click:n5", 0.3, 0.0, 0.0), "confiance 0.30"),
            (jev_disant("escalate", 0.9, 0.0, 0.0), "modèle génératif"),
            (jev_disant("click:n5", 0.9, 0.0, 0.95), "bloquée"),
        ];
        for (transport, attendu) in cases {
            let mut operator = Operator::new(transport, goal(), "jev-latest");
            match operator.next_turn(&history) {
                Err(DriverError::HandOver(reason)) => assert!(reason.contains(attendu), "{reason}"),
                other => panic!("{other:?}"),
            }
            assert!(operator.actions().is_empty());
            // La même page, une fois remise, n'est pas redemandée à Jev.
            assert!(
                matches!(operator.next_turn(&history), Err(DriverError::HandOver(r)) if r.contains("déjà remise"))
            );
        }
        // Une option que personne n'a proposée est arrêtée par la vérification de la réponse,
        // avant même que l'opérateur ne la lise : c'est une réponse inexploitable, pas une action.
        let mut operator = Operator::new(
            jev_disant("click:n999", 0.9, 0.0, 0.0),
            goal(),
            "jev-latest",
        );
        assert!(
            matches!(operator.next_turn(&history), Err(DriverError::BadModelOutput(r)) if r.contains("n999"))
        );
    }

    #[test]
    fn une_boucle_et_un_plafond_rendent_la_main() {
        let history = vec![
            json!({"role":"user","content":"x"}),
            json!({"role":"assistant","tool_call":{}}),
            observed(&page("Réservation", None), "u"),
        ];
        let mut operator =
            Operator::new(jev_disant("click:n5", 0.9, 0.0, 0.0), goal(), "jev-latest");
        assert!(operator.next_turn(&history).is_ok());
        assert!(operator.next_turn(&history).is_ok());
        assert!(
            matches!(operator.next_turn(&history), Err(DriverError::HandOver(r)) if r.contains("boucle"))
        );

        let mut operator =
            Operator::new(jev_disant("click:n5", 0.9, 0.0, 0.0), goal(), "jev-latest")
                .max_actions(1);
        assert!(operator.next_turn(&history).is_ok());
        let autre = observed(&page("Autre", None), "u2");
        let history2 = [history[0].clone(), history[1].clone(), autre];
        assert!(
            matches!(operator.next_turn(&history2), Err(DriverError::HandOver(r)) if r.contains("plafond"))
        );
    }

    #[test]
    fn un_echec_d_outil_fait_relire_la_page_une_fois_puis_rend_la_main() {
        let mut operator =
            Operator::new(jev_disant("click:n5", 0.9, 0.0, 0.0), goal(), "jev-latest");
        let mut history = vec![
            json!({"role":"user","content":"x"}),
            json!({"role":"assistant","tool_call":{}}),
            json!({"role":"tool","ok":false,"result":{"code":"NotFound","detail":"n5"}}),
        ];
        let (turn, _) = operator.next_turn(&history).unwrap();
        assert_eq!(
            turn,
            ModelTurn::ToolCall {
                tool: "web.tree".into(),
                arguments: json!({})
            }
        );
        history.push(json!({"role":"assistant","tool_call":{}}));
        history
            .push(json!({"role":"tool","ok":false,"result":{"code":"SandboxError","detail":"x"}}));
        assert!(
            matches!(operator.next_turn(&history), Err(DriverError::HandOver(r)) if r.contains("échecs"))
        );
    }

    #[test]
    fn sans_adresse_de_depart_ni_arbre_l_operateur_rend_la_main() {
        let goal = Goal::from_intent("Lis le fichier", None);
        let mut operator = Operator::new(jev_disant("done", 0.9, 0.9, 0.0), goal, "jev-latest");
        let history = vec![json!({"role":"user","content":"x"})];
        assert!(matches!(
            operator.next_turn(&history),
            Err(DriverError::HandOver(_))
        ));
        let history = vec![
            json!({"role":"user","content":"x"}),
            json!({"role":"assistant","tool_call":{}}),
            json!({"role":"tool","ok":true,"result":{"content":"texte d'un fichier"}}),
        ];
        assert!(
            matches!(operator.next_turn(&history), Err(DriverError::HandOver(r)) if r.contains("sans arbre"))
        );
    }

    #[test]
    fn la_cascade_donne_la_main_au_modele_puis_la_reprend_sur_une_nouvelle_page() {
        let transport = Arc::new(Scripted::new(|request| {
            let next = if request.state["page"]["title"] == "Formulaire" {
                "escalate"
            } else {
                "done"
            };
            Ok(response(
                [
                    ("next", choice_answer(next, &[(next, 0.9)], 0.9)),
                    (
                        "done",
                        Answer::Noul {
                            noul: if next == "done" { 0.95 } else { 0.1 },
                        },
                    ),
                    ("blocked", Answer::Noul { noul: 0.0 }),
                ],
                100,
            ))
        }));
        let goal = Goal::from_intent(
            "Remplis le formulaire avec le bon message",
            Some("http://x/"),
        );
        let llm = ScriptedModel::new(
            "local:qwen",
            vec![(
                ModelTurn::ToolCall {
                    tool: "web.act".into(),
                    arguments: json!({"action":"set_field","node":"n4","value":"écrit par le LLM"}),
                },
                Usage {
                    tokens_in: 50,
                    tokens_out: 5,
                },
            )],
        );
        let mut cascade = Cascade::new(Operator::new(transport, goal, "jev-latest"), Box::new(llm));
        assert_eq!(cascade.model_name(), "jev:jev-latest+local:qwen");
        let mut history = vec![json!({"role":"user","content":"x"})];
        let (turn, _) = cascade.next_turn(&history).unwrap();
        assert!(matches!(turn, ModelTurn::ToolCall { tool, .. } if tool == "web.open"));
        history.push(json!({"role":"assistant","tool_call":{}}));
        history.push(observed(&page("Formulaire", None), "http://x/"));
        // Jev ne sait pas écrire : le LLM prend le tour, avec la même histoire.
        let (turn, usage) = cascade.next_turn(&history).unwrap();
        assert_eq!(usage.tokens_out, 5);
        assert!(
            matches!(turn, ModelTurn::ToolCall { arguments, .. } if arguments["value"] == "écrit par le LLM")
        );
        assert_eq!(cascade.handovers().len(), 1);
        history.push(json!({"role":"assistant","tool_call":{}}));
        history.push(observed(
            &page("Merci", Some("écrit par le LLM")),
            "http://x/merci",
        ));
        // Nouvelle page : Jev reprend la main et conclut.
        let (turn, _) = cascade.next_turn(&history).unwrap();
        assert!(matches!(turn, ModelTurn::Final { .. }), "{turn:?}");
        let trace = cascade.trace();
        let trace = trace.lock().unwrap();
        assert_eq!(trace.decisions, 2);
        assert_eq!(trace.input_tokens, 200);
        assert_eq!(trace.actions, vec!["open:http://x/".to_owned()]);
        assert_eq!(trace.handovers.len(), 1);
    }

    #[test]
    fn une_panne_de_jev_laisse_le_modele_continuer() {
        let transport = Arc::new(Scripted::failing(JevError::Overloaded));
        let llm = ScriptedModel::new(
            "local:qwen",
            vec![(
                ModelTurn::Final {
                    text: "fini par le LLM".into(),
                },
                Usage::default(),
            )],
        );
        let mut cascade = Cascade::new(
            Operator::new(transport, Goal::from_intent("x", None), "jev-latest"),
            Box::new(llm),
        );
        let history = vec![
            json!({"role":"user","content":"x"}),
            json!({"role":"assistant","tool_call":{}}),
            observed(&page("P", None), "u"),
        ];
        let (turn, _) = cascade.next_turn(&history).unwrap();
        assert_eq!(
            turn,
            ModelTurn::Final {
                text: "fini par le LLM".into()
            }
        );
        assert!(
            cascade.handovers()[0].contains("saturé"),
            "{:?}",
            cascade.handovers()
        );
    }
}
