//! Pont navigateur : un agent navigue par l'arbre sémantique, jamais par des pixels.
//!
//! La page est observée en construisant un arbre SUP depuis le document, et manipulée par des
//! actions typées qui désignent des éléments par identifiant. Aucune capture d'écran n'intervient,
//! ni pour observer, ni pour vérifier : le résultat d'une action rend directement ce qui a changé.
//!
//! Le navigateur tourne dans une sandbox avec un profil par tâche, et sa sortie réseau passe par
//! le proxy comme celle de n'importe quel processus.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cdp;
mod page;

pub use cdp::{Browser, CdpError};
pub use page::{ActionResult, Page};

/// Script d'extraction de l'arbre sémantique.
pub const EXTRACT_JS: &str = include_str!("extract.js");

/// Script d'exécution d'une action.
pub const ACT_JS: &str = include_str!("act.js");
