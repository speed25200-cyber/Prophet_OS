//! Types fondamentaux de Prophet OS.
//!
//! Ce crate ne fait rien : il définit les formats sur lesquels tout le système s'accorde, et les
//! règles qui les gouvernent. Les spécifications correspondantes sont dans `docs/specs/`.
//!
//! - [`canon`] : sérialisation canonique, base de toute signature et de tout hachage.
//! - [`pattern`] : motifs de cible et relation de couverture.
//! - [`cap`] : jetons de capacité, grants, délégation, décisions.
//! - [`manifest`] : manifeste d'agent.
//! - [`ledger`] : événements du journal d'audit.
//! - [`driver`] : contrat des pilotes d'agents.
//! - [`ids`] : identifiants préfixés.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod canon;
pub mod cap;
pub mod driver;
pub mod ids;
pub mod ledger;
pub mod manifest;
pub mod pattern;

pub use cap::{
    Act, Approval, CapError, CheckContext, Constraints, Decision, DenyReason, Grant, Res, Token,
    TokenBuilder,
};
pub use ids::Id;
pub use ledger::{Event, EventKind};
pub use manifest::Manifest;
