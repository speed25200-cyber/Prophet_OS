//! Les outils système livrés avec Prophet OS.
//!
//! Chacun est écrit pour être appelé par un modèle : description courte, arguments typés,
//! résultat structuré, erreurs nommées. Aucun n'applique lui-même la politique : c'est le
//! registre qui le fait avant de leur passer la main.

mod clock;
mod fs;
mod http;
mod task;

pub use clock::Now;
pub use fs::{List, Read, Search, Stat, Write};
pub use http::Fetch;
pub use task::{Diff as TaskDiff, Status as TaskStatus};

use std::sync::Arc;

use crate::registry::{Registry, Tool};

/// Enregistre l'ensemble des outils système v0.
pub fn register_all(registry: &mut Registry) {
    let tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(Read),
        Arc::new(Write),
        Arc::new(List),
        Arc::new(Stat),
        Arc::new(Search),
        Arc::new(Fetch),
        Arc::new(TaskStatus),
        Arc::new(TaskDiff),
        Arc::new(Now),
    ];
    for tool in tools {
        registry.register(tool);
    }
}
