//! Mesure et mise à l'épreuve.
//!
//! Deux suites, deux questions différentes.
//!
//! [`adversarial`] demande : **que se passe-t-il quand un contenu extérieur retourne l'agent
//! contre son utilisateur ?** C'est la seule question qui compte vraiment pour un système
//! d'exploitation agentique, parce que l'injection de prompt n'est pas un bogue à corriger mais
//! une propriété permanente des modèles. La réponse ne doit donc jamais dépendre du modèle.
//!
//! [`cost`] demande : **combien coûte une observation ?** Elle compare le coût d'un arbre
//! sémantique à celui d'une capture d'écran, sur des mesures réelles.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod adversarial;
pub mod cost;
pub mod tasks;

pub use adversarial::{Attack, Outcome, Scenario, Verdict};
pub use cost::{Measurement, compare};
pub use tasks::{Family, Requires, Run, Task, suite};
