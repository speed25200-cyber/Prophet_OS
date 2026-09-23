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

/// Les clients officiels que le lanceur de pilotes de la session sait lancer (ADR 0035).
pub const OFFICIAL_DRIVERS: [&str; 3] = ["claude-code", "codex", "gemini"];

/// Vrai pour `driver:<client officiel>`.
#[must_use]
pub fn is_official_driver(reference: &str) -> bool {
    prophet_types::manifest::split_driver(reference)
        .is_some_and(|(client, _)| OFFICIAL_DRIVERS.contains(&client))
}

/// Le nom qu'un humain lit pour un modèle proposé : les clients officiels par leur nom
/// d'usage et leur éditeur, un modèle local par son identifiant.
#[must_use]
pub fn model_label(model: &str) -> String {
    let nom = match driver_name(model) {
        Some("claude-code") => "Claude Code (Anthropic)",
        Some("codex") => "Codex (ChatGPT)",
        Some("gemini") => "Gemini (Google)",
        _ => return model.to_owned(),
    };
    match driver_tier(model) {
        Some(palier) => format!("{nom} · {palier}"),
        None => nom.to_owned(),
    }
}

/// Le palier de modèle demandé à un client (`claude-code@opus` → `opus`), s'il y en a un
/// (ADR 0040).
#[must_use]
pub fn driver_tier(model: &str) -> Option<&str> {
    driver_name(model)?;
    let spec = model.strip_prefix("driver:").unwrap_or(model);
    spec.split_once('@')
        .map(|(_, palier)| palier)
        .filter(|palier| !palier.is_empty())
}

/// Le nom qu'un humain lit pour la référence d'un plan (`driver:codex`, `local:qwen3-1.7b`) :
/// le client par son nom d'usage, le modèle local par son identifiant.
#[must_use]
pub fn reference_label(reference: &str) -> String {
    match reference.strip_prefix("local:") {
        Some(local) => local.to_owned(),
        None => model_label(reference),
    }
}

/// Le nom d'un client officiel derrière un modèle demandé pour une mission : `codex`,
/// `driver:codex` ; rien pour un modèle local. Les clients sont les modèles principaux
/// (ADR 0035) : l'humain les nomme comme il nomme un modèle local.
#[must_use]
pub fn driver_name(model: &str) -> Option<&str> {
    let spec = model.strip_prefix("driver:").unwrap_or(model);
    // Un palier de modèle peut suivre le client (`claude-code@opus`, ADR 0040).
    let nom = spec.split('@').next().unwrap_or(spec);
    OFFICIAL_DRIVERS.contains(&nom).then_some(nom)
}

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
    /// Modèles proposés pour une mission : les clients officiels que le lanceur de la session
    /// dit connectés, par leur nom (`claude-code`, `codex`), d'abord — ce sont les modèles
    /// principaux (ADR 0035) —, puis les modèles du profil découverts auprès du moteur du
    /// service, le secours.
    pub models: Vec<String>,
    /// Modèles que le profil admet, découverts ou non : un client MCP n'a pas besoin du moteur.
    #[serde(default)]
    pub preferred: Vec<String>,
    /// Rôles du relais (`reflect`, `execute`, `code`) et, pour chacun, les modèles admis
    /// effectivement découverts ; vide sans relais (ADR 0034).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub roles: std::collections::BTreeMap<String, Vec<String>>,
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
    /// Clients officiels que le lanceur de pilotes de la session connaît : `None` si le
    /// service n'en configure aucun ou s'il ne répond pas (ADR 0035).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pilot: Option<pilotd::Status>,
    /// Le décideur rapide du service, s'il est configuré : nom du secret et modèle, jamais
    /// une valeur. Absent quand le service n'en a pas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jev: Option<crate::local::JevSetup>,
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

    /// Vue bornée à ce qui est effectivement disponible : les modèles que le moteur sert, et
    /// les clients officiels que le lanceur de la session dit connectés (`drivers`, sans
    /// préfixe), rendus dans les rôles sous leur référence `driver:<nom>` (ADR 0035).
    #[must_use]
    pub fn view(&self, models: &[String], drivers: &[String]) -> ProfileView {
        ProfileView {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            // Les clients officiels connectés d'abord — ce sont les modèles principaux —,
            // puis les modèles locaux découverts, dans l'ordre du profil (ADR 0035).
            models: self
                .manifest
                .model
                .preferred
                .iter()
                .filter_map(|reference| {
                    reference.strip_prefix("driver:").and_then(|d| {
                        let client = d.split('@').next().unwrap_or(d);
                        drivers.iter().any(|v| v == client).then(|| d.to_owned())
                    })
                })
                .chain(
                    self.manifest
                        .model
                        .preferred
                        .iter()
                        .filter_map(|reference| {
                            reference
                                .strip_prefix("local:")
                                .and_then(|m| models.iter().any(|v| v == m).then(|| m.to_owned()))
                        }),
                )
                .collect(),
            preferred: self
                .manifest
                .model
                .preferred
                .iter()
                .filter_map(|r| r.strip_prefix("local:"))
                .map(str::to_owned)
                .collect(),
            roles: self
                .manifest
                .model
                .roles
                .iter()
                .map(|(role, refs)| {
                    (
                        role.clone(),
                        refs.iter()
                            .filter_map(|r| {
                                if let Some(m) = r.strip_prefix("local:") {
                                    models.iter().any(|v| v == m).then(|| m.to_owned())
                                } else if let Some(d) = r.strip_prefix("driver:") {
                                    let client = d.split('@').next().unwrap_or(d);
                                    drivers.iter().any(|v| v == client).then(|| r.clone())
                                } else {
                                    None
                                }
                            })
                            .collect(),
                    )
                })
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
        // Un rôle ne peut nommer qu'un modèle du plafond : le relais choisit parmi ce que le
        // profil admet déjà, il n'y ajoute rien.
        for (role, models) in &self.manifest.model.roles {
            // Un client à palier (`driver:claude-code@opus`) est admis si le profil admet le
            // client (ADR 0040).
            if let Some(model) = models.iter().find(|m| {
                !self.manifest.model.preferred.contains(m)
                    && !prophet_types::manifest::split_driver(m).is_some_and(|(client, _)| {
                        self.manifest
                            .model
                            .preferred
                            .contains(&format!("driver:{client}"))
                    })
            }) {
                return Err(format!(
                    "Le rôle {role} du profil {} nomme {model}, absent de model.preferred.",
                    self.id
                ));
            }
        }
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
            // Un modèle local du moteur, ou un client officiel lancé dans la session de
            // l'humain par le lanceur de pilotes (ADR 0035) ; jamais une API à clé.
            || self
                .manifest
                .model
                .preferred
                .iter()
                .any(|m| !m.starts_with("local:") && !is_official_driver(m))
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
        // Une commande suppose un programme nommé ; la liste blanche ou la microVM tranchent.
        let execs = grants
            .iter()
            .any(|g| g.res == Res::Proc && g.act == Act::Exec);
        // Déléguer suppose un contexte à confier ; le catalogue vérifie ensuite qu'il existe.
        let spawns = grants
            .iter()
            .any(|g| g.res == Res::Task && g.act == Act::Spawn);
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
                        "fs.read"
                            | "fs.write"
                            | "fs.edit"
                            | "fs.list"
                            | "fs.search"
                            | "fs.stat"
                            | "doc.read"
                    ) => {}
                // L'introspection ne lit que la mission elle-même : son état, son budget et
                // ses propres changements.
                (Res::Tool, Act::Call)
                    if matches!(
                        grant.pattern.as_str(),
                        "task.status" | "task.diff" | "calc.eval"
                    ) => {}
                (Res::Tool, Act::Call) if hosts && WEB_TOOLS.contains(&grant.pattern.as_str()) => {}
                (Res::Tool, Act::Call) if apps && UI_TOOLS.contains(&grant.pattern.as_str()) => {}
                // Une sous-mission se confie à un contexte nommé du catalogue, jamais à « tout ».
                (Res::Task, Act::Spawn) if is_app_name(&grant.pattern) => {}
                // Un programme par son nom, jamais « tout » ni un chemin relatif.
                (Res::Proc, Act::Exec) if is_app_name(&grant.pattern) => {}
                // Qui lance un programme nommé peut l'arrêter : `proc.kill` va avec `proc.exec`.
                (Res::Tool, Act::Call)
                    if execs && matches!(grant.pattern.as_str(), "proc.exec" | "proc.kill") => {}
                (Res::Tool, Act::Call) if spawns && grant.pattern == "task.delegate" => {}
                _ => {
                    return Err(
                        "Le profil dépasse les outils fichiers natifs, le web relayé par egress, les applications nommées, les programmes nommés, la délégation à un contexte nommé et ses périmètres."
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
    // Un contexte ne se confie qu'à un contexte du même catalogue : une cible absente serait un
    // refus certain, autant le dire à l'administrateur qu'à l'agent.
    let connus: BTreeSet<&str> = profiles.iter().map(|p| p.id.as_str()).collect();
    for profile in &profiles {
        for grant in profile.grants()? {
            if grant.res == Res::Task
                && grant.act == Act::Spawn
                && !connus.contains(grant.pattern.as_str())
            {
                return Err(format!(
                    "Le profil {} délègue à un contexte inconnu : {}.",
                    profile.id, grant.pattern
                ));
            }
        }
    }
    Ok(profiles)
}
