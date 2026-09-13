//! Adaptateur de la session humaine : accessibilité, et clients officiels.
//!
//! Deux choses que `agentd`, service système, ne peut pas faire lui-même et qui n'existent que
//! dans la session de l'humain : lire le bus d'accessibilité de ses applications (ADR 0027), et
//! lancer ses clients officiels, connectés par son abonnement, sur une mission (ADR 0034).
//!
//! Les applications de bureau ne publient pas SUP ; elles publient leur arbre d'accessibilité,
//! sur le bus AT-SPI de la session de l'humain, que rien d'extérieur ne peut joindre. Ce crate
//! tourne donc **dans** la session : il lit ces arbres, les traduit en SUP (avec la confiance
//! qu'une lecture d'accessibilité mérite, et pas davantage), et exécute les actions typées que
//! `agentd` lui demande après avoir fait trancher capd. Il ne prend aucune capture d'écran, ne
//! simule ni clic ni touche : il n'emploie que ce que les applications déclarent elles-mêmes
//! (`Action`, `EditableText`), et refuse ce qu'elles ne déclarent pas.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod a11y;
pub mod client;

pub use a11y::{Desktop, Error};
