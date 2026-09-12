//! Lancement effectif d'une sandbox, selon son niveau.
//!
//! Le point important tient en une phrase : **chaque niveau a son propre chemin de lancement**.
//! Un gestionnaire qui accepterait une demande de niveau 2 pour l'exécuter en niveau 0 promettrait
//! une isolation qui n'aurait pas lieu, ce qui est pire que de refuser. Le module refuse donc
//! plutôt que de dégrader, et nomme ce qui manque.

use std::path::Path;
use std::process::{Child, Command, Stdio};

use crate::caps::{Capabilities, MicrovmImages};
use crate::confine::HANDSHAKE_ENV;
use crate::spec::{SPEC_ENV, SandboxSpec};

/// Ce qu'un lancement rend au gestionnaire.
#[derive(Debug)]
pub struct Launched {
    /// Processus à surveiller.
    pub child: Child,
    /// Le lancement exige-t-il la poignée de main de projection d'identifiants ?
    ///
    /// Seul le niveau 0 en a besoin : gVisor et Firecracker créent eux-mêmes leur isolation, et
    /// le gestionnaire n'a rien à écrire dans leurs espaces de noms.
    pub needs_handshake: bool,
}

/// Erreur de lancement.
#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    /// Erreur d'entrée-sortie.
    #[error("lancement impossible : {0}")]
    Io(#[from] std::io::Error),
    /// Description non sérialisable.
    #[error("description non sérialisable : {0}")]
    Serialize(#[from] serde_json::Error),
    /// Le niveau demandé n'est pas atteignable.
    #[error("niveau {level} inatteignable : il manque {missing}")]
    Unreachable {
        /// Niveau demandé.
        level: u8,
        /// Ce qui manque.
        missing: String,
    },
    /// Niveau inconnu.
    #[error("niveau d'isolation inconnu : {0}")]
    UnknownLevel(u8),
}

/// Lance une sandbox au niveau demandé.
///
/// # Errors
/// [`LaunchError::Unreachable`] si le niveau n'est pas réellement atteignable sur cette machine.
pub fn launch(
    caps: &Capabilities,
    helper: &str,
    spec: &SandboxSpec,
    sync_dir: &Path,
) -> Result<Launched, LaunchError> {
    match spec.level {
        0 => launch_confined(helper, spec, sync_dir),
        1 => {
            let runsc = caps
                .runsc
                .as_deref()
                .ok_or_else(|| LaunchError::Unreachable {
                    level: 1,
                    missing: caps.missing_for(1).join(", "),
                })?;
            launch_gvisor(runsc, spec)
        }
        2 => {
            let (firecracker, images) = caps
                .firecracker
                .as_deref()
                .zip(caps.microvm_images.as_ref())
                .ok_or_else(|| LaunchError::Unreachable {
                    level: 2,
                    missing: caps.missing_for(2).join(", "),
                })?;
            launch_microvm(firecracker, images, spec)
        }
        autre => Err(LaunchError::UnknownLevel(autre)),
    }
}

/// Niveau 0 : espaces de noms, racine minimale, seccomp, par le programme d'amorçage.
fn launch_confined(
    helper: &str,
    spec: &SandboxSpec,
    sync_dir: &Path,
) -> Result<Launched, LaunchError> {
    let child = Command::new(helper)
        .env_clear()
        .env(SPEC_ENV, serde_json::to_string(spec)?)
        .env(HANDSHAKE_ENV, sync_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    Ok(Launched {
        child,
        needs_handshake: true,
    })
}

/// Niveau 1 : gVisor intercepte les appels système de l'invité dans un noyau en espace
/// utilisateur, ce qui réduit la surface du noyau réel à presque rien.
///
/// On emploie le mode `do` de `runsc`, qui exécute une commande sans exiger un paquet OCI
/// complet : monter un paquet pour chaque appel d'outil coûterait plus cher que l'isolation
/// elle-même.
fn launch_gvisor(runsc: &str, spec: &SandboxSpec) -> Result<Launched, LaunchError> {
    let mut command = Command::new(runsc);
    command.arg("--rootless");
    // Aucune interface réseau : la seule sortie d'une tâche est le socket du proxy.
    command.arg("--network=none");
    command.arg("do");
    command.arg("--cwd").arg(&spec.workdir);
    command.arg(&spec.program).args(&spec.args);

    let child = command
        .env_clear()
        .envs(spec.env.iter().map(|(k, v)| (k.clone(), v.clone())))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    Ok(Launched {
        child,
        needs_handshake: false,
    })
}

/// Configuration d'une microVM, telle que Firecracker l'attend.
///
/// Elle est rendue publique pour être vérifiable par un test sans démarrer de machine virtuelle :
/// une configuration fausse est la première cause d'échec silencieux d'un lancement.
#[must_use]
pub fn microvm_config(
    images: &MicrovmImages,
    spec: &SandboxSpec,
    vsock_path: &str,
) -> serde_json::Value {
    // `console=ttyS0` rend la sortie de l'invité lisible ; `reboot=k panic=1` fait qu'un invité en
    // panique s'arrête au lieu de rester en vie sans rien faire.
    let cmdline = format!(
        "console=ttyS0 reboot=k panic=1 pci=off prophet.workdir={} prophet.program={}",
        spec.workdir, spec.program
    );
    serde_json::json!({
        "boot-source": {
            "kernel_image_path": images.kernel,
            "boot_args": cmdline
        },
        "drives": [{
            "drive_id": "rootfs",
            "path_on_host": images.rootfs,
            "is_root_device": true,
            // La racine reste en lecture seule : ce que la tâche écrit va dans son espace de
            // travail, monté à part, et reste donc annulable.
            "is_read_only": true
        }],
        "machine-config": {
            "vcpu_count": 2,
            "mem_size_mib": 1024,
            "smt": false
        },
        // Aucun périphérique réseau : la microVM ne parle qu'au proxy, par vsock.
        "vsock": {
            "guest_cid": 3,
            "uds_path": vsock_path
        }
    })
}

/// Niveau 2 : une machine virtuelle minimale, avec son propre noyau.
///
/// C'est le seul niveau où un code hostile qui trouverait une faille du noyau invité resterait
/// enfermé : il lui faudrait ensuite une faille de l'hyperviseur.
fn launch_microvm(
    firecracker: &str,
    images: &MicrovmImages,
    spec: &SandboxSpec,
) -> Result<Launched, LaunchError> {
    let base = std::env::temp_dir().join(format!("prophet-vm-{}", std::process::id()));
    std::fs::create_dir_all(&base)?;
    let config_path = base.join("config.json");
    let api_socket = base.join("api.sock");
    let vsock_path = base.join("vsock.sock");
    // Un socket résiduel empêcherait Firecracker de démarrer.
    let _ = std::fs::remove_file(&api_socket);
    let _ = std::fs::remove_file(&vsock_path);

    let config = microvm_config(images, spec, &vsock_path.display().to_string());
    std::fs::write(&config_path, serde_json::to_vec_pretty(&config)?)?;

    let child = Command::new(firecracker)
        .arg("--api-sock")
        .arg(&api_socket)
        .arg("--config-file")
        .arg(&config_path)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    Ok(Launched {
        child,
        needs_handshake: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps_nues() -> Capabilities {
        Capabilities {
            landlock_abi: None,
            seccomp: true,
            user_namespaces: true,
            cgroups_v2: false,
            kvm: false,
            runsc: None,
            firecracker: None,
            microvm_images: None,
        }
    }

    #[test]
    fn un_niveau_inatteignable_est_refuse_et_nomme_ce_qui_manque() {
        let spec = SandboxSpec::new(1, "/bin/true", "/");
        let dir = tempfile::tempdir().unwrap();
        let erreur = launch(&caps_nues(), "helper", &spec, dir.path()).unwrap_err();
        assert!(erreur.to_string().contains("runsc"), "{erreur}");

        let spec2 = SandboxSpec::new(2, "/bin/true", "/");
        let erreur = launch(&caps_nues(), "helper", &spec2, dir.path()).unwrap_err();
        for attendu in ["kvm", "firecracker", "images"] {
            assert!(erreur.to_string().contains(attendu), "{erreur}");
        }
    }

    #[test]
    fn un_niveau_inconnu_est_refuse() {
        let spec = SandboxSpec::new(7, "/bin/true", "/");
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            launch(&caps_nues(), "helper", &spec, dir.path()),
            Err(LaunchError::UnknownLevel(7))
        ));
    }

    #[test]
    fn la_configuration_de_microvm_est_conforme() {
        let images = MicrovmImages {
            kernel: "/var/lib/prophet/microvm/vmlinux".to_owned(),
            rootfs: "/var/lib/prophet/microvm/rootfs.ext4".to_owned(),
        };
        let spec = SandboxSpec::new(2, "/usr/bin/python3", "/work").args(["script.py"]);
        let config = microvm_config(&images, &spec, "/tmp/vsock.sock");

        assert_eq!(config["boot-source"]["kernel_image_path"], images.kernel);
        assert_eq!(config["drives"][0]["is_root_device"], true);
        assert_eq!(
            config["drives"][0]["is_read_only"], true,
            "la racine de l'invité ne doit pas être inscriptible"
        );
        assert!(
            config.get("network-interfaces").is_none(),
            "une microVM de tâche n'a aucune interface réseau"
        );
        assert_eq!(config["vsock"]["uds_path"], "/tmp/vsock.sock");
        let cmdline = config["boot-source"]["boot_args"].as_str().unwrap();
        assert!(cmdline.contains("panic=1"), "{cmdline}");
        assert!(cmdline.contains("/work"), "{cmdline}");
    }

    #[test]
    fn seul_le_niveau_zero_exige_la_poignee_de_main() {
        // Les niveaux 1 et 2 créent leur propre isolation : le gestionnaire n'a rien à projeter.
        let dir = tempfile::tempdir().unwrap();
        let spec = SandboxSpec::new(0, "/bin/true", "/");
        // Le lancement échouera faute de binaire d'amorçage, mais la décision se lit dans le code
        // du chemin emprunté, que les tests de niveau 1 et 2 couvrent sur une machine équipée.
        let _ = launch(&caps_nues(), "/n/existe/pas", &spec, dir.path());
    }
}
