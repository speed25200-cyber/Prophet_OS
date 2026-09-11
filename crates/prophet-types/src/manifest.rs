//! Manifeste d'agent : identité, plafond de capacités, sandbox, budgets.
//!
//! Voir `docs/specs/agent-manifest.md`. Le manifeste est un **plafond** : `capd` n'émet jamais de
//! jeton dont les grants dépassent `capabilities.max`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::cap::{Act, Grant, Res};
use crate::pattern::{Family, validate};

/// Version de schéma produite par cette implémentation.
pub const SCHEMA_VERSION: u32 = 0;

/// Erreur de validation d'un manifeste.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    /// Version de schéma inconnue.
    #[error("version de schéma inconnue : {0}")]
    UnknownSchema(u32),
    /// Identifiant d'agent invalide.
    #[error("identifiant d'agent invalide : {0}")]
    BadId(String),
    /// Version d'agent non conforme à semver.
    #[error("version non semver : {0}")]
    BadVersion(String),
    /// Clé publique d'éditeur mal formée.
    #[error("clé d'éditeur mal formée : {0}")]
    BadPublisherKey(String),
    /// Référence de modèle invalide.
    #[error("référence de modèle invalide : {0}")]
    BadModelRef(String),
    /// Aucune préférence de modèle.
    #[error("model.preferred ne peut pas être vide")]
    NoModelPreference,
    /// Plafond de capacités vide.
    #[error("capabilities.max ne peut pas être vide")]
    NoCapabilities,
    /// Clé de capacité inconnue.
    #[error("clé de capacité inconnue : {0}")]
    BadCapabilityKey(String),
    /// Motif invalide dans le plafond.
    #[error("motif invalide pour {key} : {source}")]
    BadPattern {
        /// Clé concernée.
        key: String,
        /// Cause.
        source: crate::pattern::PatternError,
    },
    /// Niveau de sandbox hors bornes.
    #[error("sandbox.min_level doit valoir 0, 1 ou 2 (reçu {0})")]
    BadSandboxLevel(u8),
    /// Exécution de processus autorisée sans microVM.
    #[error("proc.exec non vide impose sandbox.code_execution = \"microvm\"")]
    ExecWithoutMicrovm,
    /// Budget nul ou négatif.
    #[error("budget invalide : {0}")]
    BadBudget(String),
    /// Durée mal formée.
    #[error("durée invalide : {0}")]
    BadDuration(String),
}

/// Politique de confidentialité du choix de fournisseur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Privacy {
    /// Aucun fournisseur distant, jamais.
    LocalOnly,
    /// Local par défaut, distant autorisé en dernier recours.
    #[default]
    LocalPreferred,
    /// Aucune préférence.
    Any,
}

/// Politique d'exécution de code arbitraire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CodeExecution {
    /// Exécution de code interdite.
    #[default]
    Forbidden,
    /// Exécution autorisée, obligatoirement en microVM.
    Microvm,
}

/// Section `[agent]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSection {
    /// Identifiant DNS inversé.
    pub id: String,
    /// Version semver.
    pub version: String,
    /// Nom lisible.
    pub name: String,
    /// Description courte.
    #[serde(default)]
    pub description: String,
    /// Clé publique de l'éditeur, `ed25519:<base64>`.
    pub publisher_key: String,
}

/// Section `[model]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSection {
    /// Fournisseurs par ordre de préférence.
    pub preferred: Vec<String>,
    /// Politique de confidentialité.
    #[serde(default)]
    pub privacy: Privacy,
    /// Capacité minimale attendue du modèle.
    #[serde(default = "default_capability")]
    pub min_capability: String,
}

fn default_capability() -> String {
    "standard".to_owned()
}

/// Section `[sandbox]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxSection {
    /// Niveau d'isolation minimal (0, 1 ou 2).
    #[serde(default)]
    pub min_level: u8,
    /// Politique d'exécution de code.
    #[serde(default)]
    pub code_execution: CodeExecution,
}

impl Default for SandboxSection {
    fn default() -> Self {
        Self {
            min_level: 1,
            code_execution: CodeExecution::Forbidden,
        }
    }
}

/// Budgets par défaut d'une tâche.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetSection {
    /// Plafond de tokens.
    #[serde(default = "default_tokens")]
    pub tokens: u64,
    /// Durée maximale, format `30s`, `20m`, `2h`.
    #[serde(default = "default_wall_time")]
    pub wall_time: String,
    /// Nombre maximal d'approbations demandées.
    #[serde(default = "default_approvals")]
    pub approvals: u32,
    /// Coût maximal en euros (fournisseurs facturés au token uniquement).
    #[serde(default)]
    pub cost_eur: f64,
}

fn default_tokens() -> u64 {
    200_000
}
fn default_wall_time() -> String {
    "20m".to_owned()
}
fn default_approvals() -> u32 {
    5
}

impl Default for BudgetSection {
    fn default() -> Self {
        Self {
            tokens: default_tokens(),
            wall_time: default_wall_time(),
            approvals: default_approvals(),
            cost_eur: 0.0,
        }
    }
}

/// Réglage par action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ActionRule {
    /// Approbation humaine obligatoire pour cette action.
    #[serde(default)]
    pub require_approval: bool,
    /// Nombre maximal d'appels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_calls: Option<u64>,
}

/// Section `[memory]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct MemorySection {
    /// Espaces de mémoire accessibles.
    #[serde(default)]
    pub spaces: Vec<String>,
}

/// Manifeste complet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Version de schéma.
    #[serde(default)]
    pub schema_version: u32,
    /// Identité de l'agent.
    pub agent: AgentSection,
    /// Préférences de fournisseur.
    pub model: ModelSection,
    /// Plafond de capacités, indexé par `<res>.<act>`.
    #[serde(default)]
    pub capabilities: Capabilities,
    /// Réglages de sandbox.
    #[serde(default)]
    pub sandbox: SandboxSection,
    /// Budgets.
    #[serde(default)]
    pub budget: Budgets,
    /// Réglages par action.
    #[serde(default)]
    pub actions: BTreeMap<String, ActionRule>,
    /// Mémoire.
    #[serde(default)]
    pub memory: MemorySection,
}

/// Enveloppe `[capabilities]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    /// Plafond, indexé par `<res>.<act>` (par exemple `fs.read`).
    #[serde(default)]
    pub max: BTreeMap<String, Vec<String>>,
}

/// Enveloppe `[budget]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Budgets {
    /// Budgets par défaut.
    #[serde(default)]
    pub default: BudgetSection,
}

/// Traduit une clé `<res>.<act>` en couple typé.
///
/// # Erreurs
/// Si la clé n'est pas reconnue.
pub fn parse_capability_key(key: &str) -> Result<(Res, Act), ManifestError> {
    let (res, act) = key
        .split_once('.')
        .ok_or_else(|| ManifestError::BadCapabilityKey(key.to_owned()))?;
    let res = match res {
        "fs" => Res::Fs,
        "net" => Res::Net,
        "tool" => Res::Tool,
        "proc" => Res::Proc,
        "ui" => Res::Ui,
        "ledger" => Res::Ledger,
        "memory" => Res::Memory,
        "model" => Res::Model,
        "task" => Res::Task,
        "cap" => Res::Cap,
        _ => return Err(ManifestError::BadCapabilityKey(key.to_owned())),
    };
    let act = match act {
        "read" => Act::Read,
        "write" => Act::Write,
        "list" => Act::List,
        "egress" => Act::Egress,
        "call" => Act::Call,
        "exec" => Act::Exec,
        "act" => Act::Act,
        "vision" => Act::Vision,
        "read_all" => Act::ReadAll,
        "use" => Act::Use,
        "spawn" => Act::Spawn,
        "delegate" => Act::Delegate,
        _ => return Err(ManifestError::BadCapabilityKey(key.to_owned())),
    };
    Ok((res, act))
}

/// Convertit une durée `30s` / `20m` / `2h` en secondes.
///
/// # Erreurs
/// Si le format est invalide ou la durée dépasse 24 heures.
pub fn parse_duration(text: &str) -> Result<u64, ManifestError> {
    let bad = || ManifestError::BadDuration(text.to_owned());
    let (value, unit) = text.split_at(text.len().saturating_sub(1));
    let value: u64 = value.parse().map_err(|_| bad())?;
    let seconds = match unit {
        "s" => value,
        "m" => value.checked_mul(60).ok_or_else(bad)?,
        "h" => value.checked_mul(3600).ok_or_else(bad)?,
        _ => return Err(bad()),
    };
    if seconds == 0 || seconds > 86_400 {
        return Err(bad());
    }
    Ok(seconds)
}

impl Manifest {
    /// Analyse un manifeste TOML.
    ///
    /// # Erreurs
    /// Erreur de syntaxe TOML, champ inconnu, ou règle de validation violée.
    pub fn from_toml(text: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let manifest: Self = toml::from_str(text)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Applique toutes les règles de validation de la spécification.
    ///
    /// # Erreurs
    /// La première règle violée.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ManifestError::UnknownSchema(self.schema_version));
        }
        validate_id(&self.agent.id)?;
        validate_semver(&self.agent.version)?;
        validate_publisher_key(&self.agent.publisher_key)?;

        if self.model.preferred.is_empty() {
            return Err(ManifestError::NoModelPreference);
        }
        for reference in &self.model.preferred {
            validate_model_ref(reference)?;
        }

        if self.capabilities.max.is_empty() {
            return Err(ManifestError::NoCapabilities);
        }
        for (key, patterns) in &self.capabilities.max {
            let (res, _act) = parse_capability_key(key)?;
            for pattern in patterns {
                validate(Family::of(res), pattern).map_err(|source| ManifestError::BadPattern {
                    key: key.clone(),
                    source,
                })?;
            }
        }

        if self.sandbox.min_level > 2 {
            return Err(ManifestError::BadSandboxLevel(self.sandbox.min_level));
        }
        let exec_allowed = self
            .capabilities
            .max
            .get("proc.exec")
            .is_some_and(|v| !v.is_empty());
        if exec_allowed && self.sandbox.code_execution != CodeExecution::Microvm {
            return Err(ManifestError::ExecWithoutMicrovm);
        }

        let budget = &self.budget.default;
        if budget.tokens == 0 {
            return Err(ManifestError::BadBudget("tokens = 0".to_owned()));
        }
        if budget.approvals == 0 {
            return Err(ManifestError::BadBudget("approvals = 0".to_owned()));
        }
        if budget.cost_eur < 0.0 {
            return Err(ManifestError::BadBudget("cost_eur négatif".to_owned()));
        }
        parse_duration(&budget.wall_time)?;
        Ok(())
    }

    /// Plafond de capacités sous forme de grants.
    ///
    /// # Erreurs
    /// Si une clé de capacité est inconnue.
    pub fn ceiling(&self) -> Result<Vec<Grant>, ManifestError> {
        let mut grants = Vec::new();
        for (key, patterns) in &self.capabilities.max {
            let (res, act) = parse_capability_key(key)?;
            for pattern in patterns {
                grants.push(Grant::new(res, act, pattern.clone()));
            }
        }
        Ok(grants)
    }

    /// Durée maximale par défaut, en secondes.
    ///
    /// # Erreurs
    /// Si la durée est mal formée.
    pub fn wall_time_seconds(&self) -> Result<u64, ManifestError> {
        parse_duration(&self.budget.default.wall_time)
    }
}

fn validate_id(id: &str) -> Result<(), ManifestError> {
    let bad = || ManifestError::BadId(id.to_owned());
    if id.len() < 3 || id.len() > 128 {
        return Err(bad());
    }
    if id.split('.').count() < 2 {
        return Err(bad());
    }
    if id.starts_with('.') || id.ends_with('.') || id.contains("..") {
        return Err(bad());
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
    {
        return Err(bad());
    }
    Ok(())
}

fn validate_semver(version: &str) -> Result<(), ManifestError> {
    let bad = || ManifestError::BadVersion(version.to_owned());
    let core = version
        .split_once('-')
        .map_or(version, |(core, _)| core)
        .split_once('+')
        .map_or_else(
            || version.split_once('-').map_or(version, |(c, _)| c),
            |(core, _)| core,
        );
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return Err(bad());
    }
    for part in parts {
        if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
            return Err(bad());
        }
        if part.len() > 1 && part.starts_with('0') {
            return Err(bad());
        }
    }
    Ok(())
}

fn validate_publisher_key(key: &str) -> Result<(), ManifestError> {
    use base64::Engine as _;
    let bad = || ManifestError::BadPublisherKey(key.to_owned());
    let raw = key.strip_prefix("ed25519:").ok_or_else(bad)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(|_| bad())?;
    if bytes.len() != 32 {
        return Err(bad());
    }
    Ok(())
}

fn validate_model_ref(reference: &str) -> Result<(), ManifestError> {
    let bad = || ManifestError::BadModelRef(reference.to_owned());
    let Some((kind, rest)) = reference.split_once(':') else {
        return Err(bad());
    };
    if rest.is_empty() {
        return Err(bad());
    }
    match kind {
        "local" | "driver" => Ok(()),
        "api" => {
            if rest.split(':').count() == 2 && !rest.split(':').any(str::is_empty) {
                Ok(())
            } else {
                Err(bad())
            }
        }
        _ => Err(bad()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALIDE: &str = r#"
schema_version = 0

[agent]
id = "org.exemple.analyste-ventes"
version = "1.2.0"
name = "Analyste ventes"
description = "Prépare des rapports."
publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="

[model]
preferred = ["local:qwen3-14b", "driver:claude-code"]
privacy = "local-preferred"

[capabilities.max]
"fs.read" = ["~/ventes/**"]
"fs.write" = ["~/ventes/out/**"]
"net.egress" = ["driver:claude-code", "*.exemple.fr"]
"tool.call" = ["fs.*", "mail.send"]

[sandbox]
min_level = 1
code_execution = "forbidden"

[budget.default]
tokens = 400000
wall_time = "20m"
approvals = 3
cost_eur = 2.0

[actions."mail.send"]
require_approval = true
max_calls = 1

[memory]
spaces = ["work"]
"#;

    #[test]
    fn manifeste_valide() {
        let m = Manifest::from_toml(VALIDE).unwrap();
        assert_eq!(m.agent.id, "org.exemple.analyste-ventes");
        assert_eq!(m.wall_time_seconds().unwrap(), 1200);
        assert_eq!(m.ceiling().unwrap().len(), 6);
        assert!(m.actions["mail.send"].require_approval);
    }

    fn invalide(remplacement: &[(&str, &str)]) -> Result<Manifest, String> {
        let mut text = VALIDE.to_owned();
        for (from, to) in remplacement {
            text = text.replace(from, to);
        }
        Manifest::from_toml(&text).map_err(|e| e.to_string())
    }

    #[test]
    fn identifiant_invalide() {
        let err = invalide(&[("org.exemple.analyste-ventes", "SansPoint")]).unwrap_err();
        assert!(err.contains("identifiant d'agent invalide"), "{err}");
    }

    #[test]
    fn version_non_semver() {
        let err = invalide(&[(r#"version = "1.2.0""#, r#"version = "1.2""#)]).unwrap_err();
        assert!(err.contains("non semver"), "{err}");
    }

    #[test]
    fn cle_editeur_mal_formee() {
        let err = invalide(&[(
            "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            "zzz",
        )])
        .unwrap_err();
        assert!(err.contains("clé d'éditeur"), "{err}");
    }

    #[test]
    fn chemin_relatif_refuse() {
        let err = invalide(&[(r#""~/ventes/**""#, r#""ventes/**""#)]).unwrap_err();
        assert!(err.contains("motif invalide"), "{err}");
    }

    #[test]
    fn traversee_de_repertoire_refusee() {
        let err = invalide(&[(r#""~/ventes/**""#, r#""~/../etc/**""#)]).unwrap_err();
        assert!(err.contains("motif invalide"), "{err}");
    }

    #[test]
    fn exec_sans_microvm_refuse() {
        let err = invalide(&[(
            r#""tool.call" = ["fs.*", "mail.send"]"#,
            "\"tool.call\" = [\"fs.*\"]\n\"proc.exec\" = [\"/usr/bin/**\"]",
        )])
        .unwrap_err();
        assert!(err.contains("microvm"), "{err}");
    }

    #[test]
    fn exec_avec_microvm_accepte() {
        let m = invalide(&[
            (
                r#""tool.call" = ["fs.*", "mail.send"]"#,
                "\"tool.call\" = [\"fs.*\"]\n\"proc.exec\" = [\"/usr/bin/**\"]",
            ),
            (
                r#"code_execution = "forbidden""#,
                r#"code_execution = "microvm""#,
            ),
        ])
        .unwrap();
        assert_eq!(m.sandbox.code_execution, CodeExecution::Microvm);
    }

    #[test]
    fn champ_inconnu_refuse() {
        let err = invalide(&[("[memory]", "[memory]\ninconnu = 1")]).unwrap_err();
        assert!(err.contains("inconnu") || err.contains("unknown"), "{err}");
    }

    #[test]
    fn duree_hors_bornes() {
        assert!(parse_duration("0s").is_err());
        assert!(parse_duration("25h").is_err());
        assert!(parse_duration("20x").is_err());
        assert_eq!(parse_duration("2h").unwrap(), 7200);
        assert_eq!(parse_duration("90s").unwrap(), 90);
    }

    #[test]
    fn capacite_inconnue() {
        assert!(parse_capability_key("fs.teleport").is_err());
        assert!(parse_capability_key("magie.read").is_err());
        assert!(parse_capability_key("fsread").is_err());
        assert_eq!(
            parse_capability_key("fs.read").unwrap(),
            (Res::Fs, Act::Read)
        );
    }

    #[test]
    fn reference_de_modele() {
        assert!(validate_model_ref("local:qwen3-8b").is_ok());
        assert!(validate_model_ref("driver:claude-code").is_ok());
        assert!(validate_model_ref("api:anthropic:claude-opus-5").is_ok());
        assert!(validate_model_ref("claude").is_err());
        assert!(validate_model_ref("api:anthropic").is_err());
    }
}
