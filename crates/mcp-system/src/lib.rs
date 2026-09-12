//! Serveurs MCP système : la façon dont tout agent agit sur Prophet OS.
//!
//! MCP joue ici le rôle que l'ABI joue pour un programme classique. Un client d'éditeur, la boucle
//! native, ou n'importe quel agent tiers utilisent les mêmes outils, sous les mêmes contrôles.
//!
//! Le point important n'est pas la liste d'outils : c'est que [`registry::Registry`] impose le
//! même passage à tous, sans exception possible.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod native;
pub mod protocol;
pub mod registry;
pub mod server;
pub mod tools;

pub use protocol::{CallResult, Content, ErrorCode, ToolMeta, ToolSpec};
pub use registry::{Journal, MemoryJournal, Registry, Tool, ToolContext};
pub use server::StdioServer;
