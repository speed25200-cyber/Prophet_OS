//! `agentd` : le runtime d'agents.
//!
//! Il fait pour les tâches ce qu'un noyau classique fait pour les processus : il les crée, leur
//! attribue des droits et des ressources, les planifie, les arrête et en rend compte.
//!
//! La différence tient en trois points. Une tâche porte une **intention**, pas une ligne de
//! commande. Elle a un **budget** en tokens et en temps, pas seulement en mémoire. Et tout ce
//! qu'elle fait est **réversible et journalisé**.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod budget;
pub mod local;
pub mod runtime;
pub mod task;

pub use budget::{Budget, Dimension, Limits, Spent};
pub use runtime::{EtatPersistant, Inspection, Runtime, RuntimeError, TaskPlan};
pub use task::{State, Task, TaskError};
