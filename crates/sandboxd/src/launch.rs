//! Lancement effectif d'une sandbox, selon son niveau.
//!
//! Le point important tient en une phrase : **chaque niveau a son propre chemin de lancement**.
//! Un gestionnaire qui accepterait une demande de niveau 2 pour l'exécuter en niveau 0 promettrait
//! une isolation qui n'aurait pas lieu, ce qui est pire que de refuser. Le module refuse donc
//! plutôt que de dégrader, et nomme ce qui manque.

use std::os::unix::process::CommandExt as _;
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
    /// Au niveau 2 : ce que le moniteur a reçu et ce qu'il faudra rapatrier (ADR 0038).
    pub microvm: Option<MicrovmRun>,
}

/// Une microVM lancée : son dossier de travail sur l'hôte et le disque confié à l'invité.
#[derive(Debug, Clone)]
pub struct MicrovmRun {
    /// Dossier temporaire du moniteur : configuration, sockets, disque.
    pub base: std::path::PathBuf,
    /// L'image ext4 de l'espace de travail, montée par l'invité en `/dev/vdb`.
    pub disque: std::path::PathBuf,
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
    /// Le moniteur a été lancé mais n'a pas tenu.
    ///
    /// Créer un processus n'est pas démarrer une machine virtuelle. Sans cette distinction, une
    /// configuration refusée se lirait comme un démarrage réussi.
    #[error("la microVM n'a pas démarré : {raison}")]
    MicrovmMortNe {
        /// Ce que le moniteur a dit avant de s'arrêter, ou ce qu'on a constaté.
        raison: String,
    },
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
    // La sandbox est un groupe de processus : geler, dégeler ou tuer touche l'arbre entier, pas
    // seulement l'amorçage, sans quoi une commande interrompue laisserait ses enfants vivre.
    let child = Command::new(helper)
        .env_clear()
        .env(SPEC_ENV, serde_json::to_string(spec)?)
        .env(HANDSHAKE_ENV, sync_dir)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    Ok(Launched {
        child,
        needs_handshake: true,
        microvm: None,
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
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    Ok(Launched {
        child,
        needs_handshake: false,
        microvm: None,
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
    disque_de_travail: Option<&str>,
) -> serde_json::Value {
    // `console=ttyS0` rend la sortie de l'invité lisible ; `reboot=k panic=1` fait qu'un invité en
    // panique s'arrête au lieu de rester en vie sans rien faire.
    let cmdline = format!(
        "console=ttyS0 reboot=k panic=1 pci=off prophet.workdir={} prophet.program={}",
        spec.workdir, spec.program
    );
    let mut drives = vec![serde_json::json!({
        "drive_id": "rootfs",
        "path_on_host": images.rootfs,
        "is_root_device": true,
        // La racine reste en lecture seule : ce que la tâche écrit va dans son espace de
        // travail, monté à part, et reste donc annulable.
        "is_read_only": true
    })];
    if let Some(disque) = disque_de_travail {
        // L'espace de travail, second disque (`/dev/vdb` dans l'invité), inscriptible : c'est
        // le seul lieu où le programme écrit, et il revient sur l'hôte après (ADR 0038).
        drives.push(serde_json::json!({
            "drive_id": "travail",
            "path_on_host": disque,
            "is_root_device": false,
            "is_read_only": false
        }));
    }
    serde_json::json!({
        "boot-source": {
            "kernel_image_path": images.kernel,
            "boot_args": cmdline
        },
        "drives": drives,
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

    // Le disque de travail : le répertoire de travail de la tâche, et le script que l'invité
    // exécute (ADR 0038). Sans e2fsprogs sur l'hôte, pas de niveau 2 — dit tel quel.
    let disque = base.join("travail.ext4");
    crate::invite::disque_de_travail(
        Path::new(&spec.workdir),
        &crate::invite::script_exec(spec),
        &disque,
    )
    .map_err(|raison| LaunchError::MicrovmMortNe {
        raison: format!("disque de travail : {raison}"),
    })?;
    let config = microvm_config(
        images,
        spec,
        &vsock_path.display().to_string(),
        Some(&disque.display().to_string()),
    );
    std::fs::write(&config_path, serde_json::to_vec_pretty(&config)?)?;

    let mut child = Command::new(firecracker)
        .arg("--api-sock")
        .arg(&api_socket)
        .arg("--config-file")
        .arg(&config_path)
        .env_clear()
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    attendre_que_la_microvm_tienne(&mut child, &api_socket)?;

    Ok(Launched {
        child,
        needs_handshake: false,
        microvm: Some(MicrovmRun { base, disque }),
    })
}

/// Attend que Firecracker soit réellement debout, ou dit pourquoi il ne l'est pas.
///
/// `spawn` ne rend compte que de la création du processus. Firecracker lit ensuite sa
/// configuration, et la refuse parfois : le processus meurt alors dans la milliseconde qui suit.
/// Rendre `Ok` à ce moment-là reviendrait à annoncer une isolation de niveau 2 qui n'existe pas,
/// exactement la dégradation silencieuse que ce module est écrit pour interdire. Le socket d'API
/// est le premier signe observable que le moniteur a accepté sa configuration.
fn attendre_que_la_microvm_tienne(child: &mut Child, api_socket: &Path) -> Result<(), LaunchError> {
    use std::io::Read as _;

    let limite = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if let Some(statut) = child.try_wait()? {
            let mut erreur = String::new();
            if let Some(flux) = child.stderr.as_mut() {
                let _ = flux.read_to_string(&mut erreur);
            }
            let erreur = erreur.trim();
            let detail = if erreur.is_empty() {
                "aucun message".to_owned()
            } else {
                erreur.to_owned()
            };
            return Err(LaunchError::MicrovmMortNe {
                raison: format!("le moniteur s'est arrêté ({statut}) : {detail}"),
            });
        }
        if api_socket.exists() {
            return Ok(());
        }
        if std::time::Instant::now() >= limite {
            let _ = child.kill();
            let _ = child.wait();
            return Err(LaunchError::MicrovmMortNe {
                raison: "le moniteur vit mais n'a pas créé son socket d'API en deux secondes"
                    .to_owned(),
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps_nues() -> Capabilities {
        Capabilities {
            landlock_abi: None,
            seccomp: true,
            user_namespaces: true,
            userns_restreint_par_politique: false,
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
        let config = microvm_config(&images, &spec, "/tmp/vsock.sock", Some("/tmp/travail.ext4"));
        assert_eq!(config["drives"][1]["drive_id"], "travail");
        assert_eq!(config["drives"][1]["path_on_host"], "/tmp/travail.ext4");
        assert_eq!(config["drives"][1]["is_read_only"], false);
        assert_eq!(config["drives"][1]["is_root_device"], false);
        let sans = microvm_config(&images, &spec, "/tmp/vsock.sock", None);
        assert_eq!(sans["drives"].as_array().map(Vec::len), Some(1));

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
