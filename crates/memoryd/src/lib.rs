//! Mémoire du système.
//!
//! Trois mémoires, un seul magasin :
//! - **de travail** : le contexte de la tâche en cours, géré par le runtime, pas ici ;
//! - **épisodique** : ce qui s'est passé, tâche par tâche, avec un lien vers le journal ;
//! - **sémantique** : ce que le système sait de l'utilisateur et de sa machine.
//!
//! Deux règles gouvernent tout le reste. Une entrée porte toujours sa **provenance** : d'où elle
//! vient, quand, et avec quelle confiance. Et elle vit dans un **espace** : un agent de travail ne
//! lit pas la mémoire personnelle, et rien ne quitte la machine.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod embed;
pub mod episodic;
mod store;

pub use embed::{Embedder, HashEmbedder, cosine};
pub use episodic::{Episode, record as record_episode, summarize};
pub use store::{Entry, Kind, MemoryError, NewEntry, Query, Space, Store};
