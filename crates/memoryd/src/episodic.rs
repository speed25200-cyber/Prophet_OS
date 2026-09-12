//! Mémoire épisodique : ce qu'une tâche terminée laisse derrière elle.
//!
//! Une tâche produit beaucoup d'événements et peu de savoir. Résumer, c'est décider ce qui mérite
//! de survivre : ce que la tâche a appris de l'environnement, et ce qui a échoué et pourquoi. Le
//! reste vit dans le journal, qui est fait pour ça.
//!
//! Le résumé est **dérivé du journal**, pas du modèle : un modèle qui se trompe sur ce qu'il a
//! fait produirait une mémoire fausse, et une mémoire fausse est pire qu'une mémoire absente.

use prophet_types::ledger::{Event, EventKind};
use serde_json::Value;
use time::OffsetDateTime;

use crate::store::{Kind, MemoryError, NewEntry, Space, Store};

/// Résumé d'une tâche terminée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Episode {
    /// Tâche concernée.
    pub task: String,
    /// Texte du résumé.
    pub text: String,
    /// Nombre d'étapes.
    pub steps: u32,
    /// Outils employés, dans l'ordre de première apparition.
    pub tools: Vec<String>,
    /// Refus rencontrés.
    pub denials: Vec<String>,
    /// Vrai si la tâche a abouti.
    pub succeeded: bool,
}

/// Construit l'épisode d'une tâche à partir de ses événements.
///
/// Retourne `None` si la tâche n'a pas d'événement terminal : une tâche en cours n'a pas encore
/// d'histoire.
#[must_use]
pub fn summarize(task: &str, events: &[Event]) -> Option<Episode> {
    let concernes: Vec<&Event> = events
        .iter()
        .filter(|e| e.task.as_deref() == Some(task))
        .collect();
    let fin = concernes.iter().find(|e| {
        matches!(
            e.kind,
            EventKind::TaskDone | EventKind::TaskFailed | EventKind::TaskCancelled
        )
    })?;

    let mut tools: Vec<String> = Vec::new();
    let mut denials: Vec<String> = Vec::new();
    let mut steps = 0;
    for event in &concernes {
        steps = steps.max(event.step.unwrap_or(0));
        match event.kind {
            EventKind::ToolCall => {
                if let Some(tool) = event.payload.get("tool").and_then(Value::as_str)
                    && !tools.iter().any(|t| t == tool)
                {
                    tools.push(tool.to_owned());
                }
            }
            EventKind::PolicyDeny | EventKind::NetDeny => {
                let quoi = event
                    .payload
                    .get("tool")
                    .or_else(|| event.payload.get("host"))
                    .and_then(Value::as_str)
                    .unwrap_or("action");
                let pourquoi = event
                    .payload
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("refusé");
                let ligne = format!("{quoi} : {pourquoi}");
                if !denials.contains(&ligne) {
                    denials.push(ligne);
                }
            }
            _ => {}
        }
    }

    let succeeded = fin.kind == EventKind::TaskDone;
    let mut text = format!(
        "Tâche {task} : {} en {steps} étape(s)",
        if succeeded {
            "terminée"
        } else if fin.kind == EventKind::TaskCancelled {
            "annulée"
        } else {
            "échouée"
        }
    );
    if !tools.is_empty() {
        text.push_str(&format!(". Outils : {}", tools.join(", ")));
    }
    if !denials.is_empty() {
        // Les refus sont ce qui sert le plus à une tâche suivante : ils disent où sont les murs.
        text.push_str(&format!(". Refus rencontrés : {}", denials.join(" ; ")));
    }
    if let Some(raison) = fin.payload.get("reason").and_then(Value::as_str) {
        text.push_str(&format!(". Raison : {raison}"));
    }
    text.push('.');

    Some(Episode {
        task: task.to_owned(),
        text,
        steps,
        tools,
        denials,
        succeeded,
    })
}

/// Enregistre l'épisode d'une tâche dans la mémoire.
///
/// La confiance reflète l'issue : ce qu'une tâche échouée a cru apprendre mérite plus de
/// circonspection que ce qu'une tâche aboutie a constaté.
///
/// # Errors
/// Si l'écriture échoue.
pub fn record(
    store: &Store,
    space: &Space,
    episode: &Episode,
    now: OffsetDateTime,
) -> Result<String, MemoryError> {
    let confiance = if episode.succeeded { 0.9 } else { 0.6 };
    store.remember(
        &NewEntry {
            kind: Kind::Episode,
            ..NewEntry::fact(space, &episode.text)
        }
        .from_task(&episode.task)
        .with_confidence(confiance),
        now,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::HashEmbedder;
    use prophet_types::ledger::{Actor, Draft, GENESIS};
    use serde_json::json;

    fn now() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_789_000_000).unwrap()
    }

    fn evenement(kind: EventKind, payload: Value, step: u32) -> Event {
        Event::seal(
            Draft::new(now(), Actor::daemon("agentd"), kind, payload)
                .task("task:01")
                .step(step),
            0,
            GENESIS,
        )
        .unwrap()
    }

    #[test]
    fn une_tache_en_cours_n_a_pas_encore_d_histoire() {
        let events = vec![evenement(
            EventKind::ToolCall,
            json!({"tool": "fs.read"}),
            1,
        )];
        assert!(summarize("task:01", &events).is_none());
    }

    #[test]
    fn le_resume_retient_les_outils_et_l_issue() {
        let events = vec![
            evenement(EventKind::ToolCall, json!({"tool": "fs.list"}), 1),
            evenement(EventKind::ToolCall, json!({"tool": "fs.read"}), 2),
            evenement(EventKind::ToolCall, json!({"tool": "fs.read"}), 3),
            evenement(EventKind::TaskDone, json!({}), 3),
        ];
        let episode = summarize("task:01", &events).unwrap();
        assert!(episode.succeeded);
        assert_eq!(episode.steps, 3);
        assert_eq!(episode.tools, vec!["fs.list", "fs.read"], "sans doublon");
        assert!(
            episode.text.contains("terminée en 3 étape"),
            "{}",
            episode.text
        );
    }

    #[test]
    fn les_refus_sont_ce_qui_sert_le_plus_ensuite() {
        let events = vec![
            evenement(
                EventKind::PolicyDeny,
                json!({"tool": "fs.read", "reason": "NoGrant"}),
                1,
            ),
            evenement(
                EventKind::TaskFailed,
                json!({"reason": "périmètre trop étroit"}),
                1,
            ),
        ];
        let episode = summarize("task:01", &events).unwrap();
        assert!(!episode.succeeded);
        assert_eq!(episode.denials, vec!["fs.read : NoGrant"]);
        assert!(
            episode.text.contains("Refus rencontrés"),
            "une tâche suivante doit savoir où sont les murs : {}",
            episode.text
        );
        assert!(episode.text.contains("périmètre trop étroit"));
    }

    #[test]
    fn une_tache_annulee_est_distinguee_d_un_echec() {
        let events = vec![evenement(EventKind::TaskCancelled, json!({}), 2)];
        let episode = summarize("task:01", &events).unwrap();
        assert!(episode.text.contains("annulée"), "{}", episode.text);
        assert!(!episode.succeeded);
    }

    #[test]
    fn l_episode_est_retrouvable_ensuite() {
        let store = Store::in_memory(Box::new(HashEmbedder::default())).unwrap();
        let events = vec![
            evenement(EventKind::ToolCall, json!({"tool": "sheet.read"}), 1),
            evenement(EventKind::TaskDone, json!({}), 1),
        ];
        let episode = summarize("task:01", &events).unwrap();
        record(&store, &Space::work(), &episode, now()).unwrap();

        let trouve = store
            .search(&crate::store::Query::in_space(Space::work(), "task:01"))
            .unwrap();
        assert_eq!(trouve.len(), 1);
        assert_eq!(trouve[0].kind, Kind::Episode);
        assert_eq!(trouve[0].source_task.as_deref(), Some("task:01"));
    }

    #[test]
    fn une_tache_echouee_est_retenue_avec_moins_de_confiance() {
        let store = Store::in_memory(Box::new(HashEmbedder::default())).unwrap();
        let reussie =
            summarize("task:01", &[evenement(EventKind::TaskDone, json!({}), 1)]).unwrap();
        let echouee = Episode {
            task: "task:02".into(),
            succeeded: false,
            ..reussie.clone()
        };
        record(&store, &Space::work(), &reussie, now()).unwrap();
        record(&store, &Space::work(), &echouee, now()).unwrap();

        let entries = store.list(&Space::work()).unwrap();
        let confiances: Vec<f32> = entries.iter().map(|e| e.confidence).collect();
        assert!(confiances.contains(&0.9) && confiances.contains(&0.6));
    }

    #[test]
    fn les_evenements_d_une_autre_tache_sont_ignores() {
        let mut events = vec![evenement(EventKind::TaskDone, json!({}), 1)];
        let mut autre = evenement(EventKind::ToolCall, json!({"tool": "intrus"}), 9);
        autre.task = Some("task:99".into());
        events.push(autre);
        let episode = summarize("task:01", &events).unwrap();
        assert!(episode.tools.is_empty(), "{:?}", episode.tools);
    }
}
