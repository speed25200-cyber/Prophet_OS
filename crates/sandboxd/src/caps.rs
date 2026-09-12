//! Sonde des mécanismes d'isolation réellement disponibles sur cette machine.
//!
//! Prophet OS ne suppose jamais qu'un mécanisme est là : il le teste au démarrage et le déclare.
//! Un mécanisme absent est une limitation annoncée, jamais une protection silencieusement
//! désactivée.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Images nécessaires au démarrage d'une microVM.
///
/// Firecracker ne démarre pas « tout seul » : il lui faut un noyau et un système de fichiers
/// racine. Les déclarer dans la sonde évite d'annoncer un niveau 2 que la machine ne saurait pas
/// réellement atteindre.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MicrovmImages {
    /// Noyau de l'invité.
    pub kernel: String,
    /// Système de fichiers racine de l'invité.
    pub rootfs: String,
}

/// Emplacement par défaut des images de microVM.
pub const MICROVM_DIR: &str = "/var/lib/prophet/microvm";

/// Cherche les images de microVM, dans les variables d'environnement puis à l'emplacement par
/// défaut.
#[must_use]
pub fn find_microvm_images() -> Option<MicrovmImages> {
    let kernel = std::env::var("PROPHET_MICROVM_KERNEL")
        .unwrap_or_else(|_| format!("{MICROVM_DIR}/vmlinux"));
    let rootfs = std::env::var("PROPHET_MICROVM_ROOTFS")
        .unwrap_or_else(|_| format!("{MICROVM_DIR}/rootfs.ext4"));
    (Path::new(&kernel).is_file() && Path::new(&rootfs).is_file())
        .then_some(MicrovmImages { kernel, rootfs })
}

/// Ce que le noyau et l'environnement offrent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Version d'ABI Landlock, ou `None` si absent.
    pub landlock_abi: Option<i32>,
    /// Filtrage d'appels système disponible.
    pub seccomp: bool,
    /// Espaces de noms utilisateur non privilégiés utilisables.
    pub user_namespaces: bool,
    /// La politique de sécurité de la distribution restreint-elle les espaces de noms ?
    ///
    /// Ubuntu 24.04 et suivantes refusent, par AppArmor, qu'un programme non listé exécute quoi
    /// que ce soit après avoir créé un espace de noms utilisateur. Créer l'espace reste permis,
    /// exécuter dedans ne l'est pas — d'où un système qui paraît capable et ne l'est pas. Le
    /// distinguer évite de chercher le défaut dans le mauvais composant.
    pub userns_restreint_par_politique: bool,
    /// cgroups v2 monté.
    pub cgroups_v2: bool,
    /// `/dev/kvm` accessible.
    pub kvm: bool,
    /// Chemin de `runsc` (gVisor), si trouvé.
    pub runsc: Option<String>,
    /// Chemin de `firecracker`, si trouvé.
    pub firecracker: Option<String>,
    /// Images d'invité de microVM, si présentes.
    pub microvm_images: Option<MicrovmImages>,
}

impl Capabilities {
    /// Sonde l'environnement.
    #[must_use]
    pub fn probe() -> Self {
        Self {
            landlock_abi: probe_landlock(),
            seccomp: probe_seccomp(),
            user_namespaces: probe_user_namespaces(),
            userns_restreint_par_politique: probe_userns_restreint(),
            cgroups_v2: Path::new("/sys/fs/cgroup/cgroup.controllers").exists(),
            kvm: probe_kvm(),
            runsc: which("runsc"),
            firecracker: which("firecracker"),
            microvm_images: find_microvm_images(),
        }
    }

    /// Niveau d'isolation maximal réellement atteignable.
    ///
    /// « Atteignable » veut dire : la sandbox démarrerait vraiment. Un binaire présent sans les
    /// images qu'il lui faut ne compte pas, sinon le système annoncerait une protection qu'il ne
    /// saurait pas mettre en place.
    #[must_use]
    pub fn max_level(&self) -> u8 {
        if self.kvm && self.firecracker.is_some() && self.microvm_images.is_some() {
            2
        } else if self.runsc.is_some() {
            1
        } else {
            0
        }
    }

    /// Ce qui manque pour atteindre un niveau donné, à afficher à l'utilisateur.
    #[must_use]
    pub fn missing_for(&self, level: u8) -> Vec<String> {
        let mut manques = Vec::new();
        match level {
            2 => {
                if !self.kvm {
                    manques.push(if Path::new("/dev/kvm").exists() {
                        "l'accès à /dev/kvm (présent mais refusé : appartenance au groupe kvm)"
                            .to_owned()
                    } else {
                        "/dev/kvm".to_owned()
                    });
                }
                if self.firecracker.is_none() {
                    manques.push("le binaire firecracker".to_owned());
                }
                if self.microvm_images.is_none() {
                    manques.push(format!(
                        "les images d'invité ({MICROVM_DIR}/vmlinux et rootfs.ext4, ou PROPHET_MICROVM_KERNEL et PROPHET_MICROVM_ROOTFS)"
                    ));
                }
            }
            1 if self.runsc.is_none() => {
                manques.push("le binaire runsc (gVisor)".to_owned());
            }
            _ => {}
        }
        manques
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
        if self.userns_restreint_par_politique {
            lines.push(
                "  ATTENTION     : la politique AppArmor de cette distribution interdit d'exécuter\n\
                 \x20                 quoi que ce soit dans un espace de noms créé par un programme\n\
                 \x20                 non listé. Les niveaux 0 et 1 échoueront sur EACCES. Remède :\n\
                 \x20                 sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0"
                    .to_owned(),
            );
        }
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
            if self.kvm {
                "accessible"
            } else if Path::new("/dev/kvm").exists() {
                "présent mais inaccessible (groupe kvm)"
            } else {
                "absent"
            }
        ));
        lines.push(format!(
            "  images microVM: {}",
            self.microvm_images.as_ref().map_or_else(
                || "absentes".to_owned(),
                |i| format!("{} + {}", i.kernel, i.rootfs)
            )
        ));
        for niveau in [1, 2] {
            let manques = self.missing_for(niveau);
            if !manques.is_empty() {
                lines.push(format!(
                    "  niveau {niveau} : il manque {}",
                    manques.join(", ")
                ));
            }
        }
        lines.join("\n")
    }
}

/// Interroge la version d'ABI Landlock.
pub(crate) fn probe_landlock() -> Option<i32> {
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

/// Les espaces de noms utilisateur sont-ils réellement utilisables ?
///
/// Lire `max_user_namespaces` et constater l'existence de `/proc/self/ns/user` ne prouve rien :
/// les deux sont vrais sur presque tout Linux, y compris là où le noyau refuse la création.
/// Ubuntu 24.04 la restreint par défaut, et une sonde qui se contente de lire annoncerait alors
/// une isolation de niveau 0 que la machine ne sait pas mettre en place — la dégradation
/// silencieuse que ce module existe pour interdire, déplacée dans la sonde.
///
/// On essaie donc pour de bon, dans un enfant jetable afin de ne pas déplacer ce processus-ci
/// dans un nouvel espace de noms. Le résultat est retenu : la réponse ne change pas en cours de
/// vie, et l'essai a un coût.
fn probe_user_namespaces() -> bool {
    static RESULTAT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *RESULTAT.get_or_init(|| {
        if std::fs::read_to_string("/proc/sys/user/max_user_namespaces")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .is_some_and(|max| max == 0)
        {
            return false;
        }
        if !Path::new("/proc/self/ns/user").exists() {
            return false;
        }
        essayer_un_espace_de_noms_utilisateur()
    })
}

/// `/dev/kvm` est-il réellement utilisable par ce processus ?
///
/// Constater que le fichier existe ne dit rien du droit de l'ouvrir : il appartient au groupe
/// `kvm`, et un utilisateur qui n'en fait pas partie obtient `EACCES` — au moment de démarrer la
/// machine virtuelle, c'est-à-dire bien trop tard, et sous la forme d'une erreur du moniteur qui
/// n'en est pas la cause. On l'ouvre donc pour de bon.
fn probe_kvm() -> bool {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/kvm")
        .is_ok()
}

/// La distribution restreint-elle l'usage des espaces de noms non privilégiés ?
///
/// Ubuntu expose ce réglage depuis la 24.04. Sa valeur `1` signifie qu'un programme absent des
/// profils AppArmor livrés ne peut rien exécuter dans l'espace de noms qu'il vient de créer. La
/// création, elle, réussit : une sonde qui s'arrête là conclut à tort que tout va bien.
fn probe_userns_restreint() -> bool {
    std::fs::read_to_string("/proc/sys/kernel/apparmor_restrict_unprivileged_userns")
        .is_ok_and(|v| v.trim() == "1")
}

/// Crée un espace de noms utilisateur dans un enfant jetable et dit si le noyau l'a permis.
fn essayer_un_espace_de_noms_utilisateur() -> bool {
    // SAFETY: `fork` n'a pas de précondition. L'enfant n'appelle ensuite que `unshare` et
    // `_exit`, sûrs après un `fork` même dans un processus multi-thread ; il ne touche ni à
    // l'allocateur, ni à un verrou, ni à rien que le parent pourrait détenir.
    let pid = unsafe { libc::fork() };
    match pid {
        // `fork` refusé : on ne sait pas, et on ne prétend pas savoir.
        -1 => false,
        0 => {
            // SAFETY: voir ci-dessus. `_exit` ne revient jamais.
            unsafe {
                let code = i32::from(libc::unshare(libc::CLONE_NEWUSER) != 0);
                libc::_exit(code);
            }
        }
        enfant => {
            let mut statut: libc::c_int = 0;
            // SAFETY: `enfant` est le processus que nous venons de créer, et `statut` est un
            // entier valide dont nous détenons seuls l'adresse.
            let attendu = unsafe { libc::waitpid(enfant, &raw mut statut, 0) };
            attendu == enfant && libc::WIFEXITED(statut) && libc::WEXITSTATUS(statut) == 0
        }
    }
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
            userns_restreint_par_politique: false,
            cgroups_v2: false,
            kvm: false,
            runsc: None,
            firecracker: None,
            microvm_images: None,
        };
        assert_eq!(caps.max_level(), 0);
        assert!(caps.supports(0));
        assert!(!caps.supports(1));
        assert!(!caps.supports(2));
        assert!(caps.missing_for(1).iter().any(|m| m.contains("runsc")));
        assert_eq!(caps.missing_for(2).len(), 3);
    }

    #[test]
    fn un_binaire_sans_ses_images_ne_donne_pas_le_niveau_deux() {
        // Le piège à éviter : annoncer un niveau que la machine ne saurait pas réellement
        // atteindre, donc promettre une isolation qui n'aurait pas lieu.
        let caps = Capabilities {
            landlock_abi: Some(4),
            seccomp: true,
            user_namespaces: true,
            userns_restreint_par_politique: false,
            cgroups_v2: true,
            kvm: true,
            runsc: Some("/usr/bin/runsc".to_owned()),
            firecracker: Some("/usr/bin/firecracker".to_owned()),
            microvm_images: None,
        };
        assert_eq!(
            caps.max_level(),
            1,
            "sans images, le niveau 2 est hors d'atteinte"
        );
        assert!(caps.missing_for(2).iter().any(|m| m.contains("images")));

        let complet = Capabilities {
            microvm_images: Some(MicrovmImages {
                kernel: "/var/lib/prophet/microvm/vmlinux".to_owned(),
                rootfs: "/var/lib/prophet/microvm/rootfs.ext4".to_owned(),
            }),
            ..caps
        };
        assert_eq!(complet.max_level(), 2);
        assert!(complet.missing_for(2).is_empty());
    }

    #[test]
    fn recherche_dans_le_path() {
        assert!(which("sh").is_some());
        assert!(which("binaire-qui-n-existe-pas-du-tout").is_none());
    }

    /// La sonde annonce-t-elle ce que la machine fait vraiment ?
    ///
    /// Comparaison par un chemin indépendant du nôtre : l'outil `unshare` du système. Une sonde
    /// qui se vérifie elle-même ne vérifie rien ; celle-ci se fait contredire par quelqu'un
    /// d'autre ou pas du tout.
    #[test]
    fn la_sonde_des_espaces_de_noms_dit_ce_que_la_machine_fait() {
        let annonce = Capabilities::probe().user_namespaces;
        let Ok(sortie) = std::process::Command::new("unshare")
            .args(["--user", "true"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
        else {
            eprintln!("outil `unshare` absent : rien à comparer");
            return;
        };
        assert_eq!(
            annonce,
            sortie.success(),
            "la sonde annonce {annonce} alors que `unshare --user` {}",
            if sortie.success() {
                "réussit"
            } else {
                "échoue"
            }
        );
    }
}
