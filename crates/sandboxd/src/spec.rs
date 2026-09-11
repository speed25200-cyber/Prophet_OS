//! Description d'une exécution sous sandbox, transmise au programme d'amorçage.

use capd::enforce::Ruleset;
use serde::{Deserialize, Serialize};

/// Tout ce dont l'amorçage a besoin pour confiner un processus puis lui passer la main.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxSpec {
    /// Niveau d'isolation demandé.
    pub level: u8,
    /// Programme à exécuter, chemin absolu.
    pub program: String,
    /// Arguments.
    pub args: Vec<String>,
    /// Répertoire de travail à l'intérieur de la sandbox.
    pub workdir: String,
    /// Variables d'environnement transmises. Rien d'autre ne passe.
    pub env: Vec<(String, String)>,
    /// Règles dérivées du jeton de capacité.
    pub rules: Ruleset,
    /// Chemins montés en lecture seule, indispensables à l'exécution (bibliothèques, binaires).
    pub read_only_mounts: Vec<String>,
    /// Socket du proxy de sortie, monté dans la sandbox. Seule voie vers le réseau.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub egress_socket: Option<String>,
    /// Sockets des serveurs MCP système accessibles à la tâche.
    #[serde(default)]
    pub mcp_sockets: Vec<String>,
}

/// Variable d'environnement portant la description, lue par l'amorçage.
pub const SPEC_ENV: &str = "PROPHET_SANDBOX_SPEC";

impl SandboxSpec {
    /// Description minimale pour exécuter un programme au niveau donné.
    #[must_use]
    pub fn new(level: u8, program: impl Into<String>, workdir: impl Into<String>) -> Self {
        Self {
            level,
            program: program.into(),
            args: Vec::new(),
            workdir: workdir.into(),
            env: Vec::new(),
            rules: Ruleset::default(),
            read_only_mounts: default_read_only_mounts(),
            egress_socket: None,
            mcp_sockets: Vec::new(),
        }
    }

    /// Ajoute des arguments.
    #[must_use]
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    /// Attache les règles d'un jeton.
    #[must_use]
    pub fn rules(mut self, rules: Ruleset) -> Self {
        self.rules = rules;
        self
    }

    /// Ajoute une variable d'environnement.
    #[must_use]
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

/// Montages en lecture seule nécessaires au démarrage de la plupart des programmes.
#[must_use]
pub fn default_read_only_mounts() -> Vec<String> {
    [
        "/usr",
        "/lib",
        "/lib64",
        "/bin",
        "/sbin",
        "/etc/ssl",
        "/nix/store",
    ]
    .into_iter()
    .filter(|path| std::path::Path::new(path).exists())
    .map(ToOwned::to_owned)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn description_serialisable() {
        let spec = SandboxSpec::new(0, "/bin/sh", "/work")
            .args(["-c", "echo bonjour"])
            .env("LANG", "fr_FR.UTF-8");
        let json = serde_json::to_string(&spec).unwrap();
        let back: SandboxSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(spec, back);
    }

    #[test]
    fn montages_par_defaut_existent() {
        for mount in default_read_only_mounts() {
            assert!(std::path::Path::new(&mount).exists(), "{mount}");
        }
    }
}
