//! Traduction d'un jeton en règles applicables sous le processus.
//!
//! Ces règles sont appliquées par le noyau ou par le proxy, pas par le code de l'agent. Même si
//! un agent est compromis ou si son client ignore ses propres permissions, il ne peut pas en
//! sortir.

use prophet_types::cap::{Act, Res, Token};
use prophet_types::pattern::expand_home;
use serde::{Deserialize, Serialize};

/// Droit d'accès à un chemin, destiné à Landlock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathRule {
    /// Racine concernée (préfixe concret, `~` déjà résolu).
    pub path: String,
    /// Lecture autorisée.
    pub read: bool,
    /// Écriture autorisée.
    pub write: bool,
}

/// Ensemble de règles dérivées d'un jeton.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Ruleset {
    /// Règles de chemins pour Landlock.
    pub paths: Vec<PathRule>,
    /// Domaines autorisés pour le proxy de sortie.
    pub egress: Vec<String>,
    /// Binaires exécutables autorisés.
    pub exec: Vec<String>,
    /// Niveau de sandbox minimal imposé par les contraintes du jeton.
    pub min_sandbox_level: u8,
}

/// Chemins toujours lisibles, sans lesquels aucun processus ne démarre.
pub const ALWAYS_READABLE: &[&str] = &["/proc/self", "/dev/null", "/dev/zero", "/dev/urandom"];

/// Dérive les règles noyau d'un jeton.
///
/// Le motif est réduit à sa **racine concrète** : `~/ventes/**` devient `/home/u/ventes`, car
/// Landlock raisonne en sous-arbres, pas en globs. C'est plus large que le motif pour les formes
/// intermédiaires comme `~/ventes/*.csv`, d'où la vérification fine qui reste faite à chaque appel
/// par `capd::Broker::check` : le noyau pose une borne extérieure, le broker applique le motif exact.
#[must_use]
pub fn ruleset_for(token: &Token, home: &str) -> Ruleset {
    let mut rules = Ruleset::default();
    for grant in &token.grants {
        let level = grant.constraints.level.unwrap_or(0);
        rules.min_sandbox_level = rules.min_sandbox_level.max(level);
        match (grant.res, grant.act) {
            (Res::Fs, act @ (Act::Read | Act::Write | Act::List)) => {
                let root = concrete_root(&grant.pattern, home);
                let write = act == Act::Write;
                if let Some(existing) = rules.paths.iter_mut().find(|r| r.path == root) {
                    existing.read |= !write;
                    existing.write |= write;
                } else {
                    rules.paths.push(PathRule {
                        path: root,
                        read: !write,
                        write,
                    });
                }
            }
            (Res::Net, Act::Egress) => rules.egress.push(grant.pattern.clone()),
            (Res::Proc, Act::Exec) => rules.exec.push(concrete_root(&grant.pattern, home)),
            _ => {}
        }
    }
    // Une racine d'écriture implique la lecture : écrire sans pouvoir relire est inutilisable.
    for rule in &mut rules.paths {
        if rule.write {
            rule.read = true;
        }
    }
    rules.paths.sort_by(|a, b| a.path.cmp(&b.path));
    rules.paths.dedup_by(|a, b| {
        if a.path == b.path {
            b.read |= a.read;
            b.write |= a.write;
            true
        } else {
            false
        }
    });
    rules.egress.sort();
    rules.egress.dedup();
    rules.exec.sort();
    rules.exec.dedup();
    rules
}

/// Racine concrète d'un motif de chemin : le plus long préfixe sans métacaractère.
#[must_use]
pub fn concrete_root(pattern: &str, home: &str) -> String {
    let expanded = expand_home(pattern, home);
    let mut root = String::new();
    for segment in expanded.split('/') {
        if segment.contains('*') || segment.contains('?') || segment.contains('[') {
            break;
        }
        if !segment.is_empty() {
            root.push('/');
            root.push_str(segment);
        }
    }
    if root.is_empty() {
        "/".to_owned()
    } else {
        root
    }
}

/// Appels système refusés au niveau 0, quel que soit le jeton.
///
/// `setsid` et `setpgid` en font partie : une sandbox de niveau 0 est un groupe de processus,
/// que sandboxd gèle, dégèle et tue d'un seul signal ; un programme qui changerait de groupe
/// échapperait à ce geste (ADR 0031).
pub const SECCOMP_DENY_LEVEL0: &[&str] = &[
    "mount",
    "umount2",
    "pivot_root",
    "ptrace",
    "bpf",
    "kexec_load",
    "kexec_file_load",
    "init_module",
    "finit_module",
    "delete_module",
    "reboot",
    "setns",
    "unshare",
    "perf_event_open",
    "process_vm_readv",
    "process_vm_writev",
    "keyctl",
    "add_key",
    "setsid",
    "setpgid",
];

/// Profil seccomp d'un niveau de sandbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeccompProfile {
    /// Niveau concerné.
    pub level: u8,
    /// Appels système refusés par nom.
    pub deny: Vec<String>,
    /// Familles de sockets autorisées.
    pub allowed_socket_families: Vec<String>,
}

/// Profil seccomp correspondant à un niveau de sandbox.
#[must_use]
pub fn seccomp_profile(level: u8) -> SeccompProfile {
    SeccompProfile {
        level,
        deny: SECCOMP_DENY_LEVEL0
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
        // Aucun accès réseau direct : toute sortie passe par le proxy, joint par socket Unix.
        allowed_socket_families: vec!["AF_UNIX".to_owned()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use prophet_types::cap::{Constraints, Grant, TokenBuilder};
    use time::OffsetDateTime;

    fn token(grants: Vec<Grant>) -> Token {
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        TokenBuilder::new("capd@test", "task:01", "a", "u")
            .grants(grants)
            .build(
                &key,
                OffsetDateTime::from_unix_timestamp(1_789_000_000).unwrap(),
                [0; 16],
            )
            .unwrap()
    }

    #[test]
    fn racine_concrete() {
        assert_eq!(concrete_root("~/ventes/**", "/home/u"), "/home/u/ventes");
        assert_eq!(concrete_root("~/ventes/*.csv", "/home/u"), "/home/u/ventes");
        assert_eq!(concrete_root("/etc/prophet/x", "/home/u"), "/etc/prophet/x");
        assert_eq!(concrete_root("**", "/home/u"), "/");
    }

    #[test]
    fn regles_de_chemin_derivees() {
        let t = token(vec![
            Grant::new(Res::Fs, Act::Read, "~/ventes/**"),
            Grant::new(Res::Fs, Act::Write, "~/ventes/out/**"),
        ]);
        let rules = ruleset_for(&t, "/home/u");
        assert_eq!(rules.paths.len(), 2);
        let lecture = rules
            .paths
            .iter()
            .find(|r| r.path == "/home/u/ventes")
            .unwrap();
        assert!(lecture.read && !lecture.write);
        let ecriture = rules
            .paths
            .iter()
            .find(|r| r.path == "/home/u/ventes/out")
            .unwrap();
        assert!(ecriture.write && ecriture.read, "écrire implique relire");
    }

    #[test]
    fn domaines_et_binaires() {
        let t = token(vec![
            Grant::new(Res::Net, Act::Egress, "*.exemple.fr"),
            Grant::new(Res::Net, Act::Egress, "driver:claude-code"),
            Grant::new(Res::Proc, Act::Exec, "/usr/bin/**").with(Constraints {
                level: Some(2),
                ..Constraints::default()
            }),
        ]);
        let rules = ruleset_for(&t, "/home/u");
        assert_eq!(rules.egress, vec!["*.exemple.fr", "driver:claude-code"]);
        assert_eq!(rules.exec, vec!["/usr/bin"]);
        assert_eq!(rules.min_sandbox_level, 2);
    }

    #[test]
    fn jeton_sans_acces_fichier_ne_produit_aucune_racine() {
        let t = token(vec![Grant::new(Res::Tool, Act::Call, "fs.*")]);
        let rules = ruleset_for(&t, "/home/u");
        assert!(rules.paths.is_empty());
        assert!(rules.egress.is_empty());
    }

    #[test]
    fn profil_seccomp_refuse_les_appels_dangereux() {
        let profile = seccomp_profile(0);
        for appel in ["mount", "ptrace", "bpf", "init_module"] {
            assert!(
                profile.deny.iter().any(|d| d == appel),
                "{appel} doit être refusé"
            );
        }
        assert_eq!(profile.allowed_socket_families, vec!["AF_UNIX"]);
    }
}
