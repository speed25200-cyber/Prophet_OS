//! Proxy de sortie : l'unique voie réseau d'une tâche.
//!
//! Une sandbox n'a aucune interface réseau. Sa seule sortie est le socket Unix de ce proxy, monté
//! dans son système de fichiers. Tout ce qui sort passe donc ici, où trois choses se produisent,
//! dans cet ordre :
//!
//! 1. [`policy`] décide si l'hôte, la méthode et le volume sont autorisés.
//! 2. [`detect`] examine ce qui sort, pour attraper ce qu'une injection de prompt aurait obtenu.
//! 3. [`inject`] substitue les secrets, au dernier moment, hors de portée du modèle.
//!
//! Aucune de ces trois étapes ne dépend du raisonnement du modèle, et c'est le point : un modèle
//! compromis ne peut pas les influencer.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod detect;
pub mod identity;
pub mod inject;
pub mod policy;
mod proxy;

pub use detect::{Detector, Outbound, Signal, Thresholds};
pub use identity::{AgentIdentity, HEADER as AGENT_HEADER};
pub use inject::{InjectionError, Injector};
pub use policy::{DenyReason, Policy, Verdict};
pub use proxy::{Proxy, ProxyError, RequestLog};
