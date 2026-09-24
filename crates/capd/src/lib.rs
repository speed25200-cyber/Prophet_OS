//! `capd` : Capability Broker et Policy Engine.
//!
//! Aucun droit n'existe dans Prophet OS sans un jeton émis ici **et** une politique qui l'autorise.
//! Les deux doivent dire oui ; l'ordre de vérification est fixé par `docs/specs/capability-token.md`
//! et repris dans [`Broker::check`].
//!
//! Trois couches, dans cet ordre :
//! 1. [`policy`] : règles Cedar, y compris des interdits absolus qu'aucun jeton ne peut lever.
//! 2. Le jeton : signature, chaîne de parents, expiration, grant couvrant la demande.
//! 3. [`approvals`] : les actions irréversibles ou externes exigent une décision humaine.
//!
//! [`enforce`] traduit un jeton en règles applicables par le noyau (Landlock, seccomp, domaines).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod approvals;
mod broker;
pub mod enforce;
mod policy;
pub mod presence;
pub mod revocations;

pub use approvals::{
    Approval, ApprovalScope, ApprovalState, Approvals, Decision as ApprovalDecision,
};
pub use broker::{Broker, BrokerError, CheckRequest};
pub use policy::{ActionClass, DEFAULT_POLICIES, PolicyEngine, PolicyError, ResourceFacts};
