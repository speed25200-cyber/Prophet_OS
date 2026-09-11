//! Identifiants préfixés : `task:<ulid>`, `run:<ulid>`, `apr:<ulid>`.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Préfixes d'identifiant reconnus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Tâche.
    Task,
    /// Exécution de pilote.
    Run,
    /// Demande d'approbation.
    Approval,
    /// Entrée de mémoire.
    Memory,
}

impl Kind {
    /// Préfixe textuel.
    #[must_use]
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Run => "run",
            Self::Approval => "apr",
            Self::Memory => "mem",
        }
    }
}

/// Identifiant préfixé et trié chronologiquement (ULID).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(String);

impl Id {
    /// Génère un nouvel identifiant.
    #[must_use]
    pub fn new(kind: Kind) -> Self {
        Self(format!("{}:{}", kind.prefix(), ulid::Ulid::new()))
    }

    /// Construit un identifiant à partir d'une chaîne existante, sans validation de l'ULID.
    #[must_use]
    pub fn from_raw(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// Vue textuelle.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Vrai si l'identifiant porte le préfixe attendu.
    #[must_use]
    pub fn is(&self, kind: Kind) -> bool {
        self.0
            .split_once(':')
            .is_some_and(|(p, _)| p == kind.prefix())
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<Id> for String {
    fn from(id: Id) -> Self {
        id.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixe_et_unicite() {
        let a = Id::new(Kind::Task);
        let b = Id::new(Kind::Task);
        assert!(a.is(Kind::Task));
        assert!(!a.is(Kind::Run));
        assert_ne!(a, b);
        assert!(a.as_str().starts_with("task:"));
    }

    #[test]
    fn ordre_chronologique() {
        let a = Id::new(Kind::Task);
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = Id::new(Kind::Task);
        assert!(a < b, "{a} devrait précéder {b}");
    }
}
