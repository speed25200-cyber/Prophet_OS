//! Les outils système livrés avec Prophet OS.
//!
//! Chacun est écrit pour être appelé par un modèle : description courte, arguments typés,
//! résultat structuré, erreurs nommées. Le registre applique la politique ; les outils
//! fichiers utilisent aussi son contrôleur pour chaque descendant du parcours.

mod clock;
mod confined;
mod doc;
mod fs;
mod http;
mod system;
mod task;
mod ui;
mod web;
mod web_relay;

pub use clock::Now;
pub use doc::Read as DocRead;
pub use fs::{List, Read, Search, Stat, Write};
pub use http::Fetch;
pub use system::{
    Exec, Kill, LedgerQuery, ListModels, ListSecrets, Notify, Recall, Remember, RequestApproval,
    UseSecret, WaitApproval, is_safe_binary, required_level_for,
};
pub use task::{Diff as TaskDiff, Status as TaskStatus};
pub use ui::{Act as UiAct, Apps as UiApps, Desktop, Observe as UiTree};
pub use web::{Act as WebAct, Browsing, Observe as WebTree, Open as WebOpen};

use std::sync::Arc;

use crate::registry::{Registry, Tool};

/// Enregistre l'ensemble des outils système v0.
pub fn register_all(registry: &mut Registry) {
    let tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(doc::Read),
        Arc::new(Read),
        Arc::new(Write),
        Arc::new(List),
        Arc::new(Stat),
        Arc::new(Search),
        Arc::new(Fetch::default()),
        Arc::new(TaskStatus),
        Arc::new(TaskDiff),
        Arc::new(Now),
        Arc::new(Exec),
        Arc::new(Kill),
        Arc::new(RequestApproval),
        Arc::new(WaitApproval),
        Arc::new(LedgerQuery),
        Arc::new(Remember),
        Arc::new(Recall),
        Arc::new(ListSecrets),
        Arc::new(UseSecret),
        Arc::new(Notify),
        Arc::new(ListModels),
    ];
    for tool in tools {
        registry.register(tool);
    }
}
