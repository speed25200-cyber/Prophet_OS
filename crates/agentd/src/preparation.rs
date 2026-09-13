//! Profils de mission fournis par la configuration du service, jamais par le modèle.
use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::Path;

use prophet_types::cap::{Act, Grant, Res};
use prophet_types::manifest::Manifest;
use serde::{Deserialize, Serialize};

use crate::Limits;

/// Profil installé par l'administrateur du service. Il définit le plafond et le contexte.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Référence stable dans le catalogue.
    pub id: String,
    /// Nom du contexte de travail.
    pub name: String,
    /// Usage du profil présenté à l'humain.
    pub description: String,
    /// Manifeste de confiance issu de la configuration locale.
    pub manifest: Manifest,
    /// Répertoires capturés pour cette mission.
    pub scopes: Vec<String>,
}

/// Vue publique sans clé d'éditeur, jeton ou configuration de connexion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileView {
    /// Identifiant choisi lors de la préparation.
    pub id: String,
    /// Nom affiché.
    pub name: String,
    /// Usage annoncé.
    pub description: String,
    /// Modèles du profil effectivement découverts auprès du moteur du service.
    pub models: Vec<String>,
    /// Contexte fichiers fixé par le profil.
    pub scopes: Vec<String>,
    /// Droits demandés à capd, avant son contrôle.
    pub grants: Vec<String>,
    /// Plafonds prévus.
    pub limits: Limits,
}

/// Catalogue de préparation et erreur éventuelle de découverte du moteur.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Options {
    /// Profils configurés, même si leurs modèles sont actuellement absents.
    pub profiles: Vec<ProfileView>,
    /// Une panne ne doit pas être présentée comme un catalogue vide réussi.
    pub model_error: Option<String>,
}

/// Intention explicite : le client ne fournit ni manifeste, ni identité, ni droits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Référence conservée côté client pour retrouver une réponse perdue.
    pub id: String,
    /// Objectif de l'humain.
    pub intent: String,
    /// Profil installé.
    pub profile: String,
    /// Modèle exact découvert et admis par le profil.
    pub model: String,
}

impl Request {
    /// Refuse les identifiants de chemin et les intentions vides ou démesurées avant tout effet.
    ///
    /// # Errors
    /// Paramètre invalide.
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || self.id.len() > 160
            || self.id == "."
            || self.id == ".."
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
            || self.intent.trim().is_empty()
            || self.intent.len() > 16_384
            || self.profile.is_empty()
            || self.profile.len() > 80
            || self.model.is_empty()
            || self.model.len() > 256
        {
            return Err("Objectif ou référence de préparation invalide.".into());
        }
        Ok(())
    }
}

impl Profile {
    /// Liste des droits du profil. capd les valide à nouveau lors de l'émission.
    ///
    /// # Errors
    /// Manifeste invalide.
    pub fn grants(&self) -> Result<Vec<Grant>, String> {
        self.manifest.ceiling().map_err(|e| e.to_string())
    }

    /// Vue bornée au moteur effectivement disponible.
    #[must_use]
    pub fn view(&self, models: &[String]) -> ProfileView {
        ProfileView {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            models: self
                .manifest
                .model
                .preferred
                .iter()
                .filter_map(|r| r.strip_prefix("local:"))
                .filter(|m| models.iter().any(|v| v == m))
                .map(str::to_owned)
                .collect(),
            scopes: self.scopes.clone(),
            grants: self
                .grants()
                .unwrap_or_default()
                .iter()
                .map(|g| format!("{:?}.{:?} sur {}", g.res, g.act, g.pattern).to_lowercase())
                .collect(),
            limits: Limits {
                tokens: self.manifest.budget.default.tokens,
                wall_time_s: self.manifest.wall_time_seconds().unwrap_or_default(),
                steps: 200,
                approvals: self.manifest.budget.default.approvals,
                cost_eur: self.manifest.budget.default.cost_eur,
            },
        }
    }

    fn validate(&self) -> Result<(), String> {
        self.manifest.validate().map_err(|e| e.to_string())?;
        if self.id.is_empty()
            || self.id.len() > 80
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || self.name.trim().is_empty()
            || self.name.len() > 120
            || self.description.len() > 1024
            || self.manifest.sandbox.min_level != 0
            || self.scopes.is_empty()
            || self.scopes.len() > 16
            || self
                .manifest
                .model
                .preferred
                .iter()
                .any(|m| !m.starts_with("local:"))
        {
            return Err("Profil de mission locale invalide.".into());
        }
        for scope in &self.scopes {
            let Some(relative) = scope.strip_prefix("~/") else {
                return Err("Périmètre relatif au home requis.".into());
            };
            if relative.is_empty()
                || relative.len() > 1024
                || relative
                    .chars()
                    .any(|c| c.is_control() || "*?\\".contains(c))
                || relative
                    .split('/')
                    .any(|c| c.is_empty() || matches!(c, "." | ".." | ".prophet"))
            {
                return Err("Périmètre de profil invalide.".into());
            }
        }
        let grants = self.grants()?;
        for scope in &self.scopes {
            if !grants.iter().any(|g| {
                g.res == Res::Fs
                    && g.act == Act::Read
                    && prophet_types::pattern::covers(
                        prophet_types::pattern::Family::Path,
                        &g.pattern,
                        &format!("{scope}/**"),
                    )
            }) {
                return Err(
                    "Chaque contexte capturé doit être couvert par un droit de lecture.".into(),
                );
            }
        }
        for grant in grants {
            match (grant.res, grant.act) {
                (Res::Fs, Act::Read | Act::Write | Act::List)
                    if self.scopes.iter().any(|s| {
                        prophet_types::pattern::covers(
                            prophet_types::pattern::Family::Path,
                            &format!("{s}/**"),
                            &grant.pattern,
                        )
                    }) => {}
                (Res::Tool, Act::Call)
                    if matches!(
                        grant.pattern.as_str(),
                        "fs.read" | "fs.write" | "fs.list" | "fs.search" | "fs.stat"
                    ) => {}
                _ => {
                    return Err(
                        "Le profil dépasse les outils fichiers natifs et ses périmètres.".into(),
                    );
                }
            }
        }
        Ok(())
    }
}

/// Charge une configuration explicite, bornée à 1 Mio et 32 profils, puis la valide entièrement.
///
/// # Errors
/// Fichier illisible, trop grand, profil invalide ou identifiant dupliqué.
pub fn load(path: &Path) -> Result<Vec<Profile>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("profils de mission : {e}"))?;
    let mut bytes = Vec::new();
    file.take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1_048_576 {
        return Err("Catalogue de profils trop grand.".into());
    }
    let profiles: Vec<Profile> = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if profiles.len() > 32 {
        return Err("Trop de profils de mission.".into());
    }
    let mut ids = BTreeSet::new();
    for profile in &profiles {
        profile.validate()?;
        if !ids.insert(&profile.id) {
            return Err("Identifiant de profil dupliqué.".into());
        }
    }
    Ok(profiles)
}
