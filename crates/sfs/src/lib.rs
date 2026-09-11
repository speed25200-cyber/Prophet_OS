//! Semantic FS : chaque tâche travaille dans sa branche, et rien de ce qu'elle fait n'est
//! définitif tant qu'un humain n'a pas vu le diff.
//!
//! Voir `docs/specs/` et ADR-0004. Deux dorsales :
//! - **btrfs** : sous-volumes en copie sur écriture, snapshots instantanés. C'est la cible.
//! - **repli** : empreinte de contenu au départ, copie de travail, sauvegarde des seuls fichiers
//!   touchés au moment de la validation. Fonctionne sur n'importe quel système de fichiers, avec
//!   les mêmes garanties d'annulation pour la dernière validation.
//!
//! Les deux exposent la même API, donc le reste du système ne sait pas laquelle est active.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod backend;
mod diff;
mod provenance;
mod workspace;

pub use backend::{Backend, BackendKind, detect_backend};
pub use diff::{Change, ChangeKind, Diff};
pub use provenance::{Provenance, read_provenance, write_provenance};
pub use workspace::{SfsError, Workspace, WorkspaceState};
