//! `sandboxd` : isolation graduée des tâches.
//!
//! Trois niveaux, du moins coûteux au plus isolant :
//! - **0, confiné** : espaces de noms, racine minimale, Landlock quand il existe, seccomp.
//!   Démarrage en quelques millisecondes. Pour les outils système de confiance.
//! - **1, noyau utilisateur** : gVisor. Pour les agents qui manipulent des données non fiables.
//! - **2, microVM** : Firecracker sur KVM. Pour toute exécution de code arbitraire.
//!
//! Le niveau réellement atteignable dépend de la machine ; [`caps::Capabilities::probe`] le dit,
//! et [`Manager`] refuse d'exécuter à un niveau qu'il ne peut pas garantir plutôt que de dégrader
//! en silence.

#![warn(missing_docs)]

pub mod caps;
pub mod confine;
pub mod spec;

mod manager;

pub use caps::Capabilities;
pub use manager::{Manager, SandboxError, SandboxHandle, SandboxState};
pub use spec::{SPEC_ENV, SandboxSpec};
