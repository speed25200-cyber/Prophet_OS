//! Semantic UI Protocol : la fin des captures d'écran.
//!
//! Une application expose **ce qu'elle est** (un arbre d'état) et **ce qu'on peut lui faire**
//! (des actions typées), au lieu de laisser un agent deviner à partir de pixels. Le gain n'est pas
//! seulement une question de volume :
//!
//! | | Capture d'écran | Arbre sémantique |
//! |---|---|---|
//! | Observation | mégaoctets d'image | kilooctets de JSON, ou un différentiel |
//! | Action | clic en (x, y) | action nommée, arguments typés |
//! | Vérification | nouvelle capture | résultat structuré immédiat |
//! | Échec | silencieux | erreur nommée |
//!
//! L'arbre sert aussi au rendu et à l'accessibilité : une seule source de vérité, donc pas de
//! dérive entre ce que voit l'humain et ce que voit l'agent.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod diff;
pub mod registry;
pub mod tree;

pub use diff::{Change, TreeDiff};
pub use registry::{Registry, RegistryError, WindowId};
pub use tree::{Action, ActionArg, ArgType, Detail, Node, Role, Tree};
