//! Motifs de correspondance (`match`) des grants et relation de couverture.
//!
//! Quatre familles de motifs, selon la ressource :
//! - **chemins** (`fs`, `proc`) : globs restreints, `**` uniquement en dernier segment ;
//! - **domaines** (`net`) : `example.com`, `*.example.com`, `example.com:443`, `driver:<pilote>` ;
//! - **noms** (`tool`, `ui`, `memory`, `model`, `ledger`, `task`, `cap`) : nom exact,
//!   `domaine.*`, ou `*`.
//!
//! La relation centrale est [`covers`] : « tout ce que le motif enfant accepte, le motif parent
//! l'accepte aussi ». Elle est **conservatrice** : en cas de doute elle répond `false`, ce qui
//! refuse une délégation au lieu d'en autoriser une trop large.

use crate::Res;

/// Erreur de validation d'un motif.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PatternError {
    /// Le motif est vide.
    #[error("motif vide")]
    Empty,
    /// Le motif contient un segment `..`.
    #[error("motif contenant `..` : {0}")]
    ParentTraversal(String),
    /// Un chemin ne commence ni par `/` ni par `~/`.
    #[error("chemin ni absolu ni relatif au home : {0}")]
    NotAbsolute(String),
    /// `**` apparaît ailleurs qu'en dernier segment.
    #[error("`**` autorisé uniquement en dernier segment : {0}")]
    DoubleStarNotLast(String),
    /// Motif de domaine invalide.
    #[error("domaine invalide : {0}")]
    BadDomain(String),
    /// Motif de nom invalide.
    #[error("nom invalide : {0}")]
    BadName(String),
}

/// Famille de motif déduite de la ressource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Chemin de fichier ou de binaire.
    Path,
    /// Domaine réseau ou référence de pilote.
    Domain,
    /// Nom d'outil, d'application ou d'espace.
    Name,
    /// Programme : un nom (`wc`) ou un chemin absolu de binaire (`/usr/bin/**`).
    Program,
}

impl Family {
    /// Famille de motif attendue pour une ressource.
    #[must_use]
    pub fn of(res: Res) -> Self {
        match res {
            Res::Fs => Self::Path,
            Res::Proc => Self::Program,
            Res::Net => Self::Domain,
            Res::Tool | Res::Ui | Res::Ledger | Res::Memory | Res::Model | Res::Task | Res::Cap => {
                Self::Name
            }
        }
    }
}

/// Valide un motif pour une famille donnée.
///
/// # Erreurs
/// Retourne la règle violée.
pub fn validate(family: Family, pattern: &str) -> Result<(), PatternError> {
    if pattern.is_empty() {
        return Err(PatternError::Empty);
    }
    match family {
        Family::Path => validate_path(pattern),
        Family::Domain => validate_domain(pattern),
        Family::Name => validate_name(pattern),
        Family::Program => validate_program(pattern),
    }
}

/// Un programme : un chemin absolu (règles des chemins) ou un nom simple, jamais un chemin
/// relatif ni un joker.
fn validate_program(pattern: &str) -> Result<(), PatternError> {
    if pattern.starts_with('/') || pattern.starts_with("~/") || pattern == "*" || pattern == "**" {
        return validate_path(pattern);
    }
    if pattern.contains('/')
        || !pattern
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
    {
        return Err(PatternError::NotAbsolute(pattern.to_owned()));
    }
    Ok(())
}

fn validate_path(pattern: &str) -> Result<(), PatternError> {
    if pattern == "*" || pattern == "**" {
        return Ok(());
    }
    if !pattern.starts_with('/') && !pattern.starts_with("~/") {
        return Err(PatternError::NotAbsolute(pattern.to_owned()));
    }
    let segments: Vec<&str> = pattern.split('/').collect();
    for (i, seg) in segments.iter().enumerate() {
        if *seg == ".." {
            return Err(PatternError::ParentTraversal(pattern.to_owned()));
        }
        if seg.contains("**") && (*seg != "**" || i != segments.len() - 1) {
            return Err(PatternError::DoubleStarNotLast(pattern.to_owned()));
        }
    }
    Ok(())
}

fn validate_domain(pattern: &str) -> Result<(), PatternError> {
    if pattern == "*" {
        return Ok(());
    }
    if let Some(driver) = pattern.strip_prefix("driver:") {
        return if driver.is_empty()
            || !driver
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            Err(PatternError::BadDomain(pattern.to_owned()))
        } else {
            Ok(())
        };
    }
    let (host, port) = split_host_port(pattern);
    if let Some(port) = port
        && port.parse::<u16>().is_err()
    {
        return Err(PatternError::BadDomain(pattern.to_owned()));
    }
    let bare = host.strip_prefix("*.").unwrap_or(host);
    if bare.is_empty()
        || bare.starts_with('.')
        || bare.ends_with('.')
        || bare.contains("..")
        || bare.contains('*')
        || !bare
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        || !bare.contains('.')
    {
        return Err(PatternError::BadDomain(pattern.to_owned()));
    }
    Ok(())
}

fn validate_name(pattern: &str) -> Result<(), PatternError> {
    if pattern == "*" {
        return Ok(());
    }
    let base = pattern.strip_suffix(".*").unwrap_or(pattern);
    if base.is_empty()
        || base.contains('*')
        || !base
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' || c == ':')
    {
        return Err(PatternError::BadName(pattern.to_owned()));
    }
    Ok(())
}

fn split_host_port(pattern: &str) -> (&str, Option<&str>) {
    match pattern.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host, Some(port))
        }
        _ => (pattern, None),
    }
}

/// Vrai si `parent` couvre `child` : toute cible acceptée par `child` est acceptée par `parent`.
///
/// Conservateur : répond `false` dès qu'une inclusion ne peut pas être prouvée par les règles
/// du format.
#[must_use]
pub fn covers(family: Family, parent: &str, child: &str) -> bool {
    if parent == child {
        return true;
    }
    match family {
        Family::Path => path_covers(parent, child),
        Family::Domain => domain_covers(parent, child),
        Family::Name => name_covers(parent, child),
        Family::Program => {
            if parent.starts_with('/') || parent == "*" || parent == "**" {
                path_covers(parent, child)
            } else {
                false
            }
        }
    }
}

fn path_covers(parent: &str, child: &str) -> bool {
    if parent == "**" || parent == "*" {
        return true;
    }
    let Some(prefix) = parent.strip_suffix("/**") else {
        return false;
    };
    if child == prefix {
        return true;
    }
    let Some(rest) = child.strip_prefix(prefix) else {
        return false;
    };
    rest.starts_with('/')
}

fn domain_covers(parent: &str, child: &str) -> bool {
    if parent == "*" {
        return true;
    }
    if parent.starts_with("driver:") || child.starts_with("driver:") {
        return false; // égalité déjà traitée
    }
    let (p_host, p_port) = split_host_port(parent);
    let (c_host, c_port) = split_host_port(child);
    let port_ok = match (p_port, c_port) {
        (None, _) => true,
        (Some(p), Some(c)) => p == c,
        (Some(_), None) => false,
    };
    if !port_ok {
        return false;
    }
    let Some(p_suffix) = p_host.strip_prefix("*.") else {
        return p_host == c_host;
    };
    let c_bare = c_host.strip_prefix("*.").unwrap_or(c_host);
    c_bare == p_suffix || c_bare.ends_with(&format!(".{p_suffix}"))
}

fn name_covers(parent: &str, child: &str) -> bool {
    if parent == "*" {
        return true;
    }
    let Some(p_base) = parent.strip_suffix(".*") else {
        return false;
    };
    let c_base = child.strip_suffix(".*").unwrap_or(child);
    c_base == p_base || c_base.starts_with(&format!("{p_base}."))
}

/// Vrai si le motif accepte la cible concrète donnée (pas un motif, une valeur).
///
/// `home` sert à résoudre le préfixe `~/` des chemins.
#[must_use]
pub fn matches(family: Family, pattern: &str, target: &str, home: &str) -> bool {
    match family {
        Family::Path => path_matches(pattern, target, home),
        Family::Domain => domain_matches(pattern, target),
        Family::Name => name_covers(pattern, target) || pattern == target,
        Family::Program => program_matches(pattern, target, home),
    }
}

/// Un motif de programme : un chemin (règles des chemins) ou un nom simple, qui n'accepte que
/// ce nom exact. Un nom ne couvre jamais un chemin dont le nom de base coïncide : le chemin est
/// choisi par l'appelant, le nom est résolu par le PATH du service.
fn program_matches(pattern: &str, target: &str, home: &str) -> bool {
    if pattern.starts_with('/') || pattern.starts_with("~/") || pattern == "*" || pattern == "**" {
        return path_matches(pattern, target, home);
    }
    pattern == target
}

fn path_matches(pattern: &str, target: &str, home: &str) -> bool {
    if pattern == "*" || pattern == "**" {
        return true;
    }
    let expanded = expand_home(pattern, home);
    if expanded == target {
        return true;
    }
    if let Some(prefix) = expanded.strip_suffix("/**") {
        return target == prefix || target.starts_with(&format!("{prefix}/"));
    }
    if let Some(prefix) = expanded.strip_suffix("/*") {
        return target
            .strip_prefix(&format!("{prefix}/"))
            .is_some_and(|rest| !rest.contains('/'));
    }
    false
}

/// Remplace un préfixe `~/` par le home fourni.
#[must_use]
pub fn expand_home(pattern: &str, home: &str) -> String {
    pattern.strip_prefix("~/").map_or_else(
        || pattern.to_owned(),
        |rest| format!("{}/{rest}", home.trim_end_matches('/')),
    )
}

fn domain_matches(pattern: &str, target: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let (p_host, p_port) = split_host_port(pattern);
    let (t_host, t_port) = split_host_port(target);
    if p_port.is_some() && p_port != t_port {
        return false;
    }
    p_host.strip_prefix("*.").map_or_else(
        || p_host == t_host,
        |suffix| t_host == suffix || t_host.ends_with(&format!(".{suffix}")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_programme_est_un_nom_nu_ou_un_chemin() {
        assert!(validate(Family::Program, "wc").is_ok());
        assert!(validate(Family::Program, "/usr/bin/**").is_ok());
        assert!(validate(Family::Program, "*").is_ok());
        assert!(validate(Family::Program, "bin/wc").is_err());
        assert!(validate(Family::Program, "w c").is_err());
        assert!(matches(Family::Program, "wc", "wc", "/home/u"));
        assert!(!matches(Family::Program, "wc", "/tmp/x/wc", "/home/u"));
        assert!(!matches(Family::Program, "wc", "cat", "/home/u"));
        assert!(matches(
            Family::Program,
            "/usr/bin/**",
            "/usr/bin/python3",
            "/home/u"
        ));
        assert!(!matches(
            Family::Program,
            "/usr/bin/**",
            "python3",
            "/home/u"
        ));
        assert!(matches(Family::Program, "*", "/tmp/x/wc", "/home/u"));
        assert!(covers(Family::Program, "/usr/bin/**", "/usr/bin/python3"));
        assert!(covers(Family::Program, "*", "wc"));
        assert!(!covers(Family::Program, "wc", "/usr/bin/wc"));
    }

    #[test]
    fn validation_des_chemins() {
        assert!(validate(Family::Path, "~/ventes/**").is_ok());
        assert!(validate(Family::Path, "/etc/prophet/*").is_ok());
        assert_eq!(
            validate(Family::Path, "ventes/**"),
            Err(PatternError::NotAbsolute("ventes/**".into()))
        );
        assert_eq!(
            validate(Family::Path, "~/../etc/**"),
            Err(PatternError::ParentTraversal("~/../etc/**".into()))
        );
        assert_eq!(
            validate(Family::Path, "~/**/out"),
            Err(PatternError::DoubleStarNotLast("~/**/out".into()))
        );
    }

    #[test]
    fn validation_des_domaines() {
        assert!(validate(Family::Domain, "api.exemple.fr").is_ok());
        assert!(validate(Family::Domain, "*.exemple.fr").is_ok());
        assert!(validate(Family::Domain, "exemple.fr:443").is_ok());
        assert!(validate(Family::Domain, "driver:claude-code").is_ok());
        assert!(validate(Family::Domain, "localhost").is_err());
        assert!(validate(Family::Domain, "").is_err());
        assert!(validate(Family::Domain, "exemple.fr:99999").is_err());
        assert!(validate(Family::Domain, "a..b.fr").is_err());
    }

    #[test]
    fn couverture_de_chemins() {
        assert!(covers(Family::Path, "~/ventes/**", "~/ventes/out/**"));
        assert!(covers(Family::Path, "~/ventes/**", "~/ventes"));
        assert!(!covers(Family::Path, "~/ventes/**", "~/autre/**"));
        assert!(!covers(Family::Path, "~/ventes", "~/ventes/**"));
        assert!(covers(Family::Path, "**", "~/n/importe/**"));
        // piège du préfixe partagé : `~/ventes2` n'est pas sous `~/ventes`
        assert!(!covers(Family::Path, "~/ventes/**", "~/ventes2/**"));
    }

    #[test]
    fn couverture_de_domaines() {
        assert!(covers(Family::Domain, "*.exemple.fr", "api.exemple.fr"));
        assert!(covers(Family::Domain, "*.exemple.fr", "*.api.exemple.fr"));
        assert!(covers(Family::Domain, "*.exemple.fr", "exemple.fr"));
        assert!(!covers(
            Family::Domain,
            "*.exemple.fr",
            "exemple.fr.evil.com"
        ));
        assert!(covers(Family::Domain, "exemple.fr", "exemple.fr:443"));
        assert!(!covers(Family::Domain, "exemple.fr:443", "exemple.fr"));
        assert!(!covers(
            Family::Domain,
            "driver:claude-code",
            "api.anthropic.com"
        ));
    }

    #[test]
    fn couverture_de_noms() {
        assert!(covers(Family::Name, "fs.*", "fs.read"));
        assert!(covers(Family::Name, "*", "n.importe"));
        assert!(!covers(Family::Name, "fs.*", "proc.exec"));
        assert!(!covers(Family::Name, "fs.read", "fs.*"));
        assert!(!covers(Family::Name, "fs.*", "fsx.read"));
    }

    #[test]
    fn correspondance_concrete() {
        let home = "/home/u";
        assert!(matches(
            Family::Path,
            "~/ventes/**",
            "/home/u/ventes/q3.csv",
            home
        ));
        assert!(!matches(
            Family::Path,
            "~/ventes/**",
            "/home/u/prive/secret",
            home
        ));
        assert!(matches(
            Family::Path,
            "~/ventes/*",
            "/home/u/ventes/a.csv",
            home
        ));
        assert!(!matches(
            Family::Path,
            "~/ventes/*",
            "/home/u/ventes/sous/a.csv",
            home
        ));
        assert!(matches(
            Family::Domain,
            "*.exemple.fr",
            "api.exemple.fr",
            home
        ));
        assert!(!matches(
            Family::Domain,
            "exemple.fr:443",
            "exemple.fr:80",
            home
        ));
    }
}
