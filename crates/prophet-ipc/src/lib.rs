//! IPC interne de Prophet OS : JSON-RPC 2.0, un message par ligne, sur sockets Unix.
//!
//! Voir `docs/specs/ipc.md` et ADR-0003. Un seul codec dans tout le système : le même que MCP en
//! stdio, ce qui permet d'exposer un daemon aux agents sans traduction.
//!
//! Deux mécanismes d'authentification, cumulables :
//! - `SO_PEERCRED` : le noyau atteste l'`uid`/`gid`/`pid` du pair. Les méthodes système exigent
//!   un pair du groupe `prophet-system`.
//! - `params._auth` : jeton de capacité de la tâche appelante, vérifié par le serveur.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod client;
mod codec;
mod server;

pub use client::Client;
pub use codec::{Error, ErrorCode, Notification, Request, Response, extract_auth};
pub use server::{Handler, PeerIdentity, Server};

/// Répertoire des sockets des daemons.
pub const SOCKET_DIR: &str = "/run/prophet";

/// Chemin conventionnel du socket d'un daemon.
#[must_use]
pub fn socket_path(daemon: &str) -> std::path::PathBuf {
    std::path::Path::new(SOCKET_DIR).join(format!("{daemon}.sock"))
}

/// Taille maximale d'un message, en octets.
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
