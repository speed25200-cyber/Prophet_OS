//! Profils de mission fournis par la configuration du service, jamais par le modèle.
use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::Path;

use prophet_types::cap::{Act, Grant, Res};
use prophet_types::manifest::Manifest;
use serde::{Deserialize, Serialize};

use crate::Limits;

/// Outils qui pilotent le navigateur ; ils n'existent que si le service en configure un.
const BROWSER_TOOLS: &[&str] = &["web.open", "web.tree", "web.act"];

/// Outils qui sortent sur le réseau, tous par egress. Chacun exige un hôte dans le profil.
const WEB_TOOLS: &[&str] = &["http.fetch", "web.open", "web.tree", "web.act"];

/// Les outils d'interface des applications de bureau (ADR 0027).
const UI_TOOLS: &[&str] = &["ui.apps", "ui.tree", "ui.act"];

/// Un nom d'application tel que l'adaptateur de session l'identifie : minuscules, chiffres,
/// point, tiret, soulignement ; ni joker ni chemin, pour qu'un droit désigne une application.
fn is_app_name(pattern: &str) -> bool {
    // L'écran, le bureau ou la session entière ne sont pas des applications : un droit sur
    // « tout ce qui s'affiche » n'existe pas.
    !matches!(
        pattern,
        "screen" | "desktop" | "display" | "session" | "all" | "any" | "root"
    ) && !pattern.is_empty()
        && pattern.len() <= 64
        && pattern
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
}

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
    /// Modèles que le profil admet, découverts ou non : un client MCP n'a pas besoin du moteur.
    #[serde(default)]
    pub preferred: Vec<String>,
    /// Contexte fichiers fixé par le profil.
    pub scopes: Vec<String>,
    /// Droits demandés à capd, avant son contrôle.
    pub grants: Vec<String>,
    /// Plafonds prévus.
    pub limits: Limits,
    /// Le profil consulte le web par le navigateur piloté (`web.*`) ; il exige donc que le
    /// service en ait un qui répond.
    #[serde(default)]
    pub web: bool,
}

/// État du navigateur piloté, sondé par le service au démarrage et non à chaque appel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserState {
    /// Programme nommé par l'administrateur (`PROPHET_BROWSER`).
    pub program: String,
    /// Le navigateur a démarré sous les contraintes du service et a répondu au pilotage.
    pub ready: bool,
    /// Version rendue par le navigateur, ou raison de l'échec, ou « sonde en cours ».
    pub detail: String,
}

impl BrowserState {
    /// État initial : configuré, pas encore sondé.
    #[must_use]
    pub fn pending(program: &Path) -> Self {
        Self {
            program: program.display().to_string(),
            ready: false,
            detail: "sonde en cours".into(),
        }
    }
}

/// Catalogue de préparation et erreur éventuelle de découverte du moteur.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Options {
    /// Profils configurés, même si leurs modèles sont actuellement absents.
    pub profiles: Vec<ProfileView>,
    /// Une panne ne doit pas être présentée comme un catalogue vide réussi.
    pub model_error: Option<String>,
    /// Navigateur piloté : `None` si le service n'en configure aucun.
    #[serde(default)]
    pub browser: Option<BrowserState>,
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
    /// La mission accueillera un client MCP de l'humain (ADR 0026) : le modèle doit rester
    /// admis par le profil, mais n'a pas à être découvert, et le moteur peut être absent.
    #[serde(default)]
    pub client: bool,
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
            preferred: self
                .manifest
                .model
                .preferred
                .iter()
                .filter_map(|r| r.strip_prefix("local:"))
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
            web: self.uses_browser(),
        }
    }

    /// Le profil demande au moins un outil du navigateur piloté.
    ///
    /// `http.fetch` n'en fait pas partie : il passe par egress sans navigateur.
    #[must_use]
    pub fn uses_browser(&self) -> bool {
        self.grants().unwrap_or_default().iter().any(|g| {
            g.res == Res::Tool && g.act == Act::Call && BROWSER_TOOLS.contains(&g.pattern.as_str())
        })
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
        // Un outil web sans hôte de sortie ne ferait que des refus : le profil le dit d'avance.
        let hosts = grants
            .iter()
            .any(|g| g.res == Res::Net && g.act == Act::Egress);
        // Un outil d'interface sans application nommée ne ferait, lui aussi, que des refus.
        let apps = grants
            .iter()
            .any(|g| g.res == Res::Ui && g.pattern != "browser" && is_app_name(&g.pattern));
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
                // Les hôtes sont validés par le manifeste ; capd tranche à l'émission, egress à
                // chaque requête, et les méthodes qui modifient attendent l'accord humain.
                (Res::Net, Act::Egress) => {}
                // L'interface observée ou manipulée est le navigateur piloté, ou une application
                // de bureau nommée : jamais l'écran, jamais « toutes les applications ».
                (Res::Ui, Act::Read | Act::Act)
                    if grant.pattern == "browser" || is_app_name(&grant.pattern) => {}
                (Res::Tool, Act::Call)
                    if matches!(
                        grant.pattern.as_str(),
                        "fs.read" | "fs.write" | "fs.list" | "fs.search" | "fs.stat"
                    ) => {}
                (Res::Tool, Act::Call) if hosts && WEB_TOOLS.contains(&grant.pattern.as_str()) => {}
                (Res::Tool, Act::Call) if apps && UI_TOOLS.contains(&grant.pattern.as_str()) => {}
                _ => {
                    return Err(
                        "Le profil dépasse les outils fichiers natifs, le web relayé par egress, les applications nommées et ses périmètres."
                            .into(),
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
