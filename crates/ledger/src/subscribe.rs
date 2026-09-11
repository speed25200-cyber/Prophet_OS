//! Bus d'événements : diffusion en temps réel aux abonnés (shell, agents autorisés).

use prophet_types::ledger::Event;
use tokio::sync::broadcast;

use crate::store::Filter;

/// Capacité du tampon de diffusion. Un abonné trop lent perd les événements les plus anciens ;
/// il le sait (`RecvError::Lagged`) et peut rattraper par une requête au journal.
const CAPACITY: usize = 1024;

/// Bus de diffusion.
#[derive(Debug, Clone)]
pub struct Bus {
    sender: broadcast::Sender<Event>,
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

impl Bus {
    /// Crée un bus vide.
    #[must_use]
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(CAPACITY);
        Self { sender }
    }

    /// Publie un événement. Sans abonné, l'événement est simplement ignoré.
    pub fn publish(&self, event: Event) {
        let _ = self.sender.send(event);
    }

    /// S'abonne au flux, filtré.
    #[must_use]
    pub fn subscribe(&self, filter: Filter) -> Subscription {
        Subscription {
            receiver: self.sender.subscribe(),
            filter,
        }
    }

    /// Nombre d'abonnés actifs.
    #[must_use]
    pub fn subscribers(&self) -> usize {
        self.sender.receiver_count()
    }
}

/// Abonnement filtré au bus.
#[derive(Debug)]
pub struct Subscription {
    receiver: broadcast::Receiver<Event>,
    filter: Filter,
}

impl Subscription {
    /// Attend le prochain événement satisfaisant le filtre.
    ///
    /// Retourne `None` quand le bus est fermé.
    pub async fn next(&mut self) -> Option<Event> {
        loop {
            match self.receiver.recv().await {
                Ok(event) if self.filter.accepts(&event) => return Some(event),
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prophet_types::ledger::{Actor, Draft, EventKind, GENESIS};
    use serde_json::json;
    use time::OffsetDateTime;

    fn evenement(seq: u64, task: &str) -> Event {
        Event::seal(
            Draft::new(
                OffsetDateTime::from_unix_timestamp(1_789_000_000).unwrap(),
                Actor::daemon("agentd"),
                EventKind::ToolCall,
                json!({"tool": "fs.read"}),
            )
            .task(task),
            seq,
            GENESIS,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn diffusion_filtree() {
        let bus = Bus::new();
        let mut abonne = bus.subscribe(Filter {
            task: Some("task:01".into()),
            ..Filter::default()
        });
        bus.publish(evenement(0, "task:02"));
        bus.publish(evenement(1, "task:01"));
        let recu = abonne.next().await.unwrap();
        assert_eq!(recu.seq, 1);
    }

    #[tokio::test]
    async fn publication_sans_abonne_ne_panique_pas() {
        let bus = Bus::new();
        bus.publish(evenement(0, "task:01"));
        assert_eq!(bus.subscribers(), 0);
    }
}
