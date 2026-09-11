//! Journal d'audit : stockage en ajout seul, chaîné par hachage, scellé périodiquement.
//!
//! Voir `docs/specs/ledger-event.md`. Trois propriétés visées :
//! 1. **Intégrité locale** : toute modification, suppression ou insertion rompt la chaîne.
//! 2. **Intégrité globale** : une réécriture complète et cohérente de la chaîne est détectée par
//!    les sceaux signés, qu'un attaquant ne peut pas reproduire sans la clé.
//! 3. **Rejeu** : à partir du journal, une tâche se raconte étape par étape.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod seal;
mod store;
mod subscribe;

pub use prophet_types::ledger::{Actor, Draft, Event, EventKind, GENESIS};
pub use seal::{Seal, Sealer};
pub use store::{Filter, LedgerError, Store, VerifyReport};
pub use subscribe::Bus;
