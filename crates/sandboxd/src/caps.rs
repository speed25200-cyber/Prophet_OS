//! Sonde des mécanismes d'isolation réellement disponibles sur cette machine.
//!
//! Prophet OS ne suppose jamais qu'un mécanisme est là : il le teste au démarrage et le déclare.
//! Un mécanisme absent est une limitation annoncée, jamais une protection silencieusement
//! désactivée.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Ce que le noyau et l'environnement offrent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Version d'ABI Landlock, ou `None` si absent.
    pub landlock_abi: Option<i32>,
    /// Filtrage d'appels système disponible.
    pub seccomp: bool,
    /// Espaces de noms utilisateur non privilégiés utilisables.
    pub user_namespaces: bool,
    /// cgroups v2 monté.
    pub cgroups_v2: bool,
    /// `/dev/kvm` accessible.
    pub kvm: bool,
    /// Chemin de `runsc` (gVisor), si trouvé.
    pub runsc: Option<String>,
    /// Chemin de `firecracker`, si trouvé.
    pub firecracker: Option<String>,
}

impl Capabilities {
    /// Sonde l'environnement.
    #[must_use]
    pub fn probe() -> Self {
        Self {
            landlock_abi: probe_landlock(),
            seccomp: probe_seccomp(),
            user_namespaces: probe_user_namespaces(),
            cgroups_v2: Path::new("/sys/fs/cgroup/cgroup.controllers").exists(),
            kvm: Path::new("/dev/kvm").exists(),
            runsc: which("runsc"),
            firecracker: which("firecracker"),
        }
    }

    /// Niveau d'isolation maximal réellement atteignable.
    #[must_use]
    pub fn max_level(&self) -> u8 {
        if self.kvm && self.firecracker.is_some() {
            2
        } else if self.runsc.is_some() {
            1
        } else {
            0
        }
    }

    /// Vrai si le niveau demandé est atteignable.
    #[must_use]
    pub fn supports(&self, level: u8) -> bool {
        level <= self.max_level()
    }

    /// Explication destinée à l'humain, listant ce qui manque.
    #[must_use]
    pub fn report(&self) -> String {
        let mut lines = vec![format!(
            "niveau maximal atteignable : {} (0 confiné, 1 noyau utilisateur, 2 microVM)",
            self.max_level()
        )];
        lines.push(format!(
            "  Landlock      : {}",
            self.landlock_abi.map_or_else(
                || "absent (chemins non restreints par le noyau)".to_owned(),
                |v| format!("ABI {v}")
            )
        ));
        lines.push(format!(
            "  seccomp       : {}",
            if self.seccomp { "disponible" } else { "absent" }
        ));
        lines.push(format!(
            "  namespaces    : {}",
            if self.user_namespaces {
                "disponibles"
            } else {
                "absents"
            }
        ));
        lines.push(format!(
            "  cgroups v2    : {}",
            if self.cgroups_v2 {
                "montés"
            } else {
                "absents (pas de quota de ressources)"
            }
        ));
        lines.push(format!(
            "  gVisor        : {}",
            self.runsc.as_deref().unwrap_or("absent")
        ));
        lines.push(format!(
            "  Firecracker   : {}",
            self.firecracker.as_deref().unwrap_or("absent")
        ));
        lines.push(format!(
            "  /dev/kvm      : {}",
            if self.kvm { "présent" } else { "absent" }
        ));
        lines.join("\n")
    }
}

/// Interroge la version d'ABI Landlock.
fn probe_landlock() -> Option<i32> {
    // `landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION)` renvoie la version.
    const SYS_LANDLOCK_CREATE_RULESET: libc::c_long = 444;
    const LANDLOCK_CREATE_RULESET_VERSION: libc::c_ulong = 1;
    // SAFETY: appel système sans effet de bord, avec un pointeur nul et une taille nulle, qui se
    // contente de renvoyer la version d'ABI ou une erreur.
    let result = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            std::ptr::null::<libc::c_void>(),
            0_usize,
            LANDLOCK_CREATE_RULESET_VERSION,
        )
    };
    if result > 0 {
        i32::try_from(result).ok()
    } else {
        None
    }
}

/// Vérifie la présence du filtrage d'appels système, sans l'activer sur ce processus.
fn probe_seccomp() -> bool {
    // Activer seccomp est irréversible : on lit donc l'état exposé par le noyau plutôt que de
    // tenter un appel.
    std::fs::read_to_string("/proc/self/status")
        .map(|status| status.lines().any(|line| line.starts_with("Seccomp:")))
        .unwrap_or(false)
}

fn probe_user_namespaces() -> bool {
    if std::fs::read_to_string("/proc/sys/user/max_user_namespaces")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .is_some_and(|max| max == 0)
    {
        return false;
    }
    Path::new("/proc/self/ns/user").exists()
}

/// Cherche un exécutable dans `PATH`.
#[must_use]
pub fn which(program: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
        .map(|candidate| candidate.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sonde_coherente() {
        let caps = Capabilities::probe();
        assert!(caps.max_level() <= 2);
        assert!(caps.supports(0));
        if caps.max_level() == 2 {
            assert!(caps.kvm && caps.firecracker.is_some());
        }
        let report = caps.report();
        assert!(report.contains("Landlock"), "{report}");
        assert!(report.contains("seccomp"), "{report}");
    }

    #[test]
    fn niveau_non_atteignable_refuse() {
        let caps = Capabilities {
            landlock_abi: None,
            seccomp: true,
            user_namespaces: true,
            cgroups_v2: false,
            kvm: false,
            runsc: None,
            firecracker: None,
        };
        assert_eq!(caps.max_level(), 0);
        assert!(caps.supports(0));
        assert!(!caps.supports(1));
        assert!(!caps.supports(2));
    }

    #[test]
    fn recherche_dans_le_path() {
        assert!(which("sh").is_some());
        assert!(which("binaire-qui-n-existe-pas-du-tout").is_none());
    }
}
