//! Semantic FS : captures privées, examen des versions et publication avec journal de reprise.
//!
//! L'implémentation copie les fichiers, y compris sur btrfs ; la dorsale de snapshots natifs
//! reste à construire. Les ouvertures ancrées et la publication exigent les interfaces Linux
//! décrites dans l'ADR 0022. L'appelant reste responsable de l'autorisation humaine.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod review;
mod snapshot;

mod backend;
mod diff;
mod provenance;
mod publication;
mod publication_metadata;
mod workspace;

pub use backend::{Backend, BackendKind, detect_backend};
pub use diff::{Change, ChangeKind, Diff};
pub use provenance::{Provenance, read_provenance, write_provenance};
pub use review::{FilePreview, FileReview, PreviewContent, ReviewIndex};
pub use workspace::{SfsError, Workspace, WorkspaceState};
