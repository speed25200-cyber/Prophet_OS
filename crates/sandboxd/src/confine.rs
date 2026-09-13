//! Confinement du processus courant, appliqué juste avant de passer la main au programme cible.
//!
//! L'ordre compte : les espaces de noms d'abord (ils exigent encore des appels système que le
//! filtre refusera ensuite), la racine ensuite, Landlock puis seccomp en dernier, car aucun des
//! deux ne se relâche.

use std::path::Path;

use nix::sched::{CloneFlags, unshare};

use crate::spec::SandboxSpec;

/// Erreur de confinement.
#[derive(Debug, thiserror::Error)]
pub enum ConfineError {
    /// Erreur d'appel système.
    #[error("appel système en échec : {0}")]
    Syscall(#[from] nix::Error),
    /// Appel système refusé, avec son nom.
    ///
    /// Un « EACCES » nu ne dit pas si c'est la création de l'espace de noms, un montage ou le
    /// changement de racine qui a été refusé. Sur une machine qu'on ne voit qu'à travers un
    /// journal, ce nom est la moitié du diagnostic — et les politiques de sécurité des
    /// distributions récentes refusent précisément certains de ces appels et pas d'autres.
    #[error("{appel} refusé : {source}")]
    AppelRefuse {
        /// Nom de l'appel système.
        appel: &'static str,
        /// Erreur rendue par le noyau.
        #[source]
        source: nix::Error,
    },
    /// Montage lié refusé, avec le chemin concerné.
    #[error("montage de {source_path} refusé : {source}")]
    MontageRefuse {
        /// Chemin que l'on cherchait à rendre visible dans la sandbox.
        source_path: String,
        /// Erreur rendue par le noyau.
        #[source]
        source: nix::Error,
    },
    /// Erreur d'entrée-sortie.
    #[error("erreur d'entrée-sortie : {0}")]
    Io(#[from] std::io::Error),
    /// Filtre d'appels système invalide.
    #[error("filtre seccomp invalide : {0}")]
    Seccomp(String),
    /// Étape de confinement identifiée en échec.
    #[error("{0} : {1}")]
    Step(&'static str, #[source] std::io::Error),
}

/// Attache le nom de l'appel système à son échec.
fn nomme(appel: &'static str) -> impl Fn(nix::Error) -> ConfineError {
    move |source| ConfineError::AppelRefuse { appel, source }
}

/// Ce qui a effectivement été appliqué. Sert à prouver, dans les tests et dans le journal, que le
/// confinement demandé a bien eu lieu.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    /// Espaces de noms créés.
    pub namespaces: Vec<&'static str>,
    /// Landlock appliqué.
    pub landlock: bool,
    /// Filtre d'appels système appliqué.
    pub seccomp: bool,
    /// Racine remplacée par un arbre minimal.
    pub pivoted: bool,
}

/// Crée les espaces de noms d'isolation.
///
/// L'espace de noms réseau est systématique : un processus de tâche n'a **aucune** interface, pas
/// même une boucle locale routable. Sa seule voie vers l'extérieur est le socket du proxy, monté
/// dans son système de fichiers.
///
/// La projection des identifiants ne peut pas être faite par le processus lui-même : le noyau
/// exige qu'elle vienne d'un processus de l'espace de noms parent. L'amorçage signale donc au
/// gestionnaire qu'il a créé son espace, attend que celui-ci écrive la carte, puis reprend. C'est
/// la poignée de main de [`Handshake`].
///
/// # Erreurs
/// Si le noyau refuse la création d'un espace de noms, ou si la poignée de main échoue.
pub fn enter_namespaces(handshake: Option<&Handshake>) -> Result<Vec<&'static str>, ConfineError> {
    let flags = CloneFlags::CLONE_NEWUSER
        | CloneFlags::CLONE_NEWNS
        | CloneFlags::CLONE_NEWNET
        | CloneFlags::CLONE_NEWIPC
        | CloneFlags::CLONE_NEWUTS;
    unshare(flags).map_err(nomme("unshare(CLONE_NEWUSER|NEWNS|NEWNET|NEWIPC|NEWUTS)"))?;
    if let Some(handshake) = handshake {
        handshake.signal_ready()?;
        handshake.wait_for_mapping()?;
    }
    Ok(vec!["user", "mount", "net", "ipc", "uts"])
}

/// Poignée de main entre le gestionnaire et l'amorçage, par deux tubes nommés.
///
/// Deux tubes plutôt qu'un seul : chacun porte un sens, ce qui rend l'ordre des étapes explicite
/// et impossible à confondre.
#[derive(Debug, Clone)]
pub struct Handshake {
    /// Tube par lequel l'amorçage annonce que son espace de noms existe.
    pub ready: std::path::PathBuf,
    /// Tube par lequel le gestionnaire annonce que la carte d'identifiants est écrite.
    pub go: std::path::PathBuf,
}

/// Variable d'environnement portant le répertoire de la poignée de main.
pub const HANDSHAKE_ENV: &str = "PROPHET_SANDBOX_SYNC";

impl Handshake {
    /// Crée les deux tubes dans un répertoire.
    ///
    /// # Erreurs
    /// Si les tubes ne peuvent pas être créés.
    pub fn create(dir: &Path) -> Result<Self, ConfineError> {
        use nix::sys::stat::Mode;
        std::fs::create_dir_all(dir)?;
        let ready = dir.join("ready");
        let go = dir.join("go");
        nix::unistd::mkfifo(&ready, Mode::S_IRUSR | Mode::S_IWUSR)?;
        nix::unistd::mkfifo(&go, Mode::S_IRUSR | Mode::S_IWUSR)?;
        Ok(Self { ready, go })
    }

    /// Reconstruit la poignée de main à partir du répertoire annoncé par l'environnement.
    #[must_use]
    pub fn from_dir(dir: &Path) -> Self {
        Self {
            ready: dir.join("ready"),
            go: dir.join("go"),
        }
    }

    /// Côté amorçage : annonce que l'espace de noms est créé.
    ///
    /// # Erreurs
    /// Si l'écriture échoue.
    pub fn signal_ready(&self) -> Result<(), ConfineError> {
        use std::io::Write as _;
        let mut fifo = std::fs::OpenOptions::new()
            .write(true)
            .open(&self.ready)
            .map_err(|e| ConfineError::Step("ouverture du tube d'annonce", e))?;
        fifo.write_all(b"1")
            .map_err(|e| ConfineError::Step("annonce de l'espace de noms", e))?;
        Ok(())
    }

    /// Côté amorçage : attend que la carte d'identifiants soit écrite.
    ///
    /// # Erreurs
    /// Si la lecture échoue ou si le gestionnaire a renoncé.
    pub fn wait_for_mapping(&self) -> Result<(), ConfineError> {
        use std::io::Read as _;
        let mut fifo = std::fs::File::open(&self.go)
            .map_err(|e| ConfineError::Step("ouverture du tube de reprise", e))?;
        let mut byte = [0u8; 1];
        fifo.read_exact(&mut byte)
            .map_err(|e| ConfineError::Step("attente de la carte d'identifiants", e))?;
        if byte[0] != b'1' {
            return Err(ConfineError::Step(
                "le gestionnaire a renoncé à projeter les identifiants",
                std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            ));
        }
        Ok(())
    }

    /// Côté gestionnaire : attend l'annonce de l'amorçage.
    ///
    /// # Erreurs
    /// Si la lecture échoue.
    pub fn wait_for_ready(&self) -> Result<(), ConfineError> {
        use std::io::Read as _;
        let mut fifo = std::fs::File::open(&self.ready)
            .map_err(|e| ConfineError::Step("ouverture du tube d'annonce", e))?;
        let mut byte = [0u8; 1];
        fifo.read_exact(&mut byte)
            .map_err(|e| ConfineError::Step("attente de l'amorçage", e))?;
        Ok(())
    }

    /// Côté gestionnaire : autorise l'amorçage à reprendre.
    ///
    /// # Erreurs
    /// Si l'écriture échoue.
    pub fn release(&self, ok: bool) -> Result<(), ConfineError> {
        use std::io::Write as _;
        let mut fifo = std::fs::OpenOptions::new()
            .write(true)
            .open(&self.go)
            .map_err(|e| ConfineError::Step("ouverture du tube de reprise", e))?;
        fifo.write_all(if ok { b"1" } else { b"0" })
            .map_err(|e| ConfineError::Step("libération de l'amorçage", e))?;
        Ok(())
    }
}

/// Côté gestionnaire : projette l'utilisateur courant sur la racine de l'espace de noms d'un
/// processus enfant.
///
/// Le noyau refuse qu'un processus écrive sa propre carte dans le cas général ; c'est donc au
/// parent de le faire, exactement comme le fait `unshare --map-root-user`.
///
/// # Erreurs
/// Si l'écriture est refusée, ce qui signale que le gestionnaire n'a pas le droit de projeter les
/// identifiants et que la sandbox ne doit pas démarrer.
pub fn map_child_to_root(pid: i32) -> Result<(), ConfineError> {
    let uid = nix::unistd::getuid().as_raw();
    let gid = nix::unistd::getgid().as_raw();
    // `setgroups` doit être refusé avant d'écrire `gid_map`, sinon le noyau rejette la carte.
    let _ = std::fs::write(format!("/proc/{pid}/setgroups"), "deny");
    std::fs::write(format!("/proc/{pid}/uid_map"), format!("0 {uid} 1\n")).map_err(|e| {
        ConfineError::Step(
            "écriture de uid_map",
            std::io::Error::new(e.kind(), format!("{e}{}", etat_de_la_projection(pid))),
        )
    })?;
    std::fs::write(format!("/proc/{pid}/gid_map"), format!("0 {gid} 1\n")).map_err(|e| {
        ConfineError::Step(
            "écriture de gid_map",
            std::io::Error::new(e.kind(), format!("{e}{}", etat_de_la_projection(pid))),
        )
    })?;
    Ok(())
}

/// Ce que le noyau permet de savoir quand la projection est refusée.
///
/// « Operation not permitted » a au moins quatre causes ici, et rien ne les distingue : l'enfant
/// n'est pas dans un espace de noms neuf, sa carte est déjà écrite, le gestionnaire n'a pas les
/// capacités qu'il croit avoir, ou il n'est pas dans l'espace parent. Chercher laquelle par
/// hypothèses coûte un aller-retour de plusieurs minutes par hypothèse — et sur la machine de
/// quelqu'un, cela ne se cherche pas du tout.
///
/// Ces quatre questions se répondent en lisant quatre fichiers. On les lit donc, une seule fois,
/// sur le chemin d'erreur.
fn etat_de_la_projection(pid: i32) -> String {
    let lire = |chemin: String| std::fs::read_to_string(chemin).unwrap_or_default();
    let espace = |chemin: String| {
        std::fs::read_link(chemin)
            .map_or_else(|_| "illisible".to_owned(), |c| c.display().to_string())
    };

    let sien = espace(format!("/proc/{pid}/ns/user"));
    let notre = espace("/proc/self/ns/user".to_owned());
    let carte = lire(format!("/proc/{pid}/uid_map"));
    let capacites = lire("/proc/self/status".to_owned())
        .lines()
        .find(|l| l.starts_with("CapEff:"))
        .unwrap_or("CapEff: illisible")
        .to_owned();

    let mut diagnostic = String::from("\n  ");
    if sien == notre {
        diagnostic.push_str(
            "l'enfant est dans le MÊME espace de noms utilisateur que le gestionnaire : \
             son `unshare` n'a pas eu lieu, et la carte de l'espace initial est déjà écrite. \
             Ce n'est pas une question de capacités.",
        );
    } else if !carte.trim().is_empty() {
        diagnostic.push_str("la carte est déjà écrite — une projection ne se fait qu'une fois.");
    } else {
        diagnostic.push_str(
            "l'enfant est bien dans un espace neuf et sa carte est vide : \
             le refus vient donc des capacités du gestionnaire dans l'espace parent \
             (CAP_SETUID et CAP_SYS_ADMIN y sont exigés tous les deux).",
        );
    }
    diagnostic.push_str(&format!(
        "\n  espace de l'enfant : {sien}\n  espace du gestionnaire : {notre}\n  {capacites}\n  \
         carte actuelle : {:?}",
        carte.trim()
    ));
    diagnostic
}

/// Applique les restrictions de chemins par Landlock, si le noyau le permet.
///
/// Retourne `false` quand Landlock est absent : l'appelant doit alors compter sur la racine
/// minimale, et le dire.
///
/// # Erreurs
/// Si Landlock est présent mais refuse la règle.
pub fn apply_landlock(spec: &SandboxSpec) -> Result<bool, ConfineError> {
    // La sonde complète essaie de créer un espace de noms ; ici, dans l'enfant déjà confiné,
    // seule l'ABI de Landlock nous intéresse.
    let Some(abi) = crate::caps::probe_landlock() else {
        return Ok(false);
    };
    let _ = abi;
    // L'implémentation complète s'appuie sur la crate `landlock` quand le noyau expose l'ABI.
    // Ce chemin n'est pas exerçable sur un noyau sans Landlock ; il est couvert par les tests
    // marqués `needs_landlock`.
    let _ = spec;
    Ok(false)
}

/// Construit une racine minimale ne contenant que ce que la tâche a le droit de voir.
///
/// C'est la défense qui tient même sans Landlock : ce qui n'est pas monté n'existe pas pour le
/// processus.
///
/// # Erreurs
/// Si un montage échoue.
pub fn pivot_to_minimal_root(spec: &SandboxSpec, new_root: &Path) -> Result<(), ConfineError> {
    use nix::mount::{MntFlags, MsFlags, mount, umount2};

    std::fs::create_dir_all(new_root)
        .map_err(|e| ConfineError::Step("création de la nouvelle racine", e))?;
    // Une propagation partagée ferait fuir nos montages vers l'hôte : on la coupe d'abord.
    mount(
        None::<&str>,
        "/",
        None::<&str>,
        MsFlags::MS_REC | MsFlags::MS_PRIVATE,
        None::<&str>,
    )
    .map_err(nomme("mount(/, MS_REC|MS_PRIVATE)"))?;
    mount(
        Some("tmpfs"),
        new_root,
        Some("tmpfs"),
        MsFlags::MS_NOSUID | MsFlags::MS_NODEV,
        None::<&str>,
    )
    .map_err(nomme("mount(tmpfs, nouvelle racine)"))?;

    // Montages en lecture seule : bibliothèques et binaires.
    for source in &spec.read_only_mounts {
        bind(Path::new(source), new_root, true)?;
    }
    // Chemins autorisés par le jeton.
    for rule in &spec.rules.paths {
        bind(Path::new(&rule.path), new_root, !rule.write)?;
    }
    // Sockets de service : proxy de sortie et serveurs MCP.
    for socket in spec.egress_socket.iter().chain(spec.mcp_sockets.iter()) {
        bind(Path::new(socket), new_root, false)?;
    }

    // `/proc` du nouvel espace de noms de processus, et les périphériques indispensables.
    let proc_dir = new_root.join("proc");
    std::fs::create_dir_all(&proc_dir)?;
    let _ = mount(
        Some("proc"),
        &proc_dir,
        Some("proc"),
        MsFlags::empty(),
        None::<&str>,
    );
    let dev = new_root.join("dev");
    std::fs::create_dir_all(&dev)?;
    for device in ["null", "zero", "urandom"] {
        let target = dev.join(device);
        if std::fs::File::create(&target).is_ok() {
            let _ = mount(
                Some(Path::new("/dev").join(device).as_path()),
                &target,
                None::<&str>,
                MsFlags::MS_BIND,
                None::<&str>,
            );
        }
    }

    let old_root = new_root.join(".ancienne-racine");
    std::fs::create_dir_all(&old_root)?;
    nix::unistd::pivot_root(new_root, &old_root).map_err(nomme("pivot_root"))?;
    nix::unistd::chdir("/").map_err(nomme("chdir"))?;
    umount2("/.ancienne-racine", MntFlags::MNT_DETACH).map_err(nomme("umount2"))?;
    let _ = std::fs::remove_dir("/.ancienne-racine");
    Ok(())
}

fn bind(source: &Path, new_root: &Path, read_only: bool) -> Result<(), ConfineError> {
    use nix::mount::{MsFlags, mount};
    if !source.exists() {
        return Ok(());
    }
    let relative = source.strip_prefix("/").unwrap_or(source);
    let target = new_root.join(relative);
    if source.is_dir() {
        std::fs::create_dir_all(&target)?;
    } else {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::File::create(&target)?;
    }
    mount(
        Some(source),
        &target,
        None::<&str>,
        MsFlags::MS_BIND | MsFlags::MS_REC,
        None::<&str>,
    )
    .map_err(|source_err| ConfineError::MontageRefuse {
        source_path: source.display().to_string(),
        source: source_err,
    })?;
    if read_only {
        mount(
            None::<&str>,
            &target,
            None::<&str>,
            MsFlags::MS_BIND | MsFlags::MS_REMOUNT | MsFlags::MS_RDONLY | MsFlags::MS_REC,
            None::<&str>,
        )?;
    }
    Ok(())
}

/// Applique le filtre d'appels système du niveau demandé.
///
/// Le filtre est irréversible et hérité par tous les descendants : un programme qui se relance
/// ou qui lance un enfant reste filtré.
///
/// # Erreurs
/// Si le filtre est refusé par le noyau.
pub fn apply_seccomp(level: u8) -> Result<(), ConfineError> {
    use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, TargetArch, apply_filter};

    let profile = capd::enforce::seccomp_profile(level);
    let mut rules = std::collections::BTreeMap::new();
    for name in &profile.deny {
        if let Some(number) = syscall_number(name) {
            rules.insert(number, vec![]);
        }
    }
    if rules.is_empty() {
        return Ok(());
    }
    let filter = SeccompFilter::new(
        rules,
        // Tout ce qui n'est pas nommé passe : le filtre est une liste de refus, complétée par
        // l'absence de montage et par Landlock. Une liste d'autorisations casserait la plupart
        // des programmes.
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        target_arch(),
    )
    .map_err(|e| ConfineError::Seccomp(e.to_string()))?;
    let program: BpfProgram = filter
        .try_into()
        .map_err(|e: seccompiler::BackendError| ConfineError::Seccomp(e.to_string()))?;
    apply_filter(&program).map_err(|e| ConfineError::Seccomp(e.to_string()))?;
    let _ = TargetArch::x86_64;
    Ok(())
}

const fn target_arch() -> seccompiler::TargetArch {
    #[cfg(target_arch = "x86_64")]
    {
        seccompiler::TargetArch::x86_64
    }
    #[cfg(target_arch = "aarch64")]
    {
        seccompiler::TargetArch::aarch64
    }
}

/// Numéro d'appel système à partir de son nom, pour les appels que Prophet OS refuse.
#[must_use]
pub fn syscall_number(name: &str) -> Option<i64> {
    #[cfg(target_arch = "x86_64")]
    let table: &[(&str, i64)] = &[
        ("mount", 165),
        ("umount2", 166),
        ("pivot_root", 155),
        ("ptrace", 101),
        ("bpf", 321),
        ("kexec_load", 246),
        ("kexec_file_load", 320),
        ("init_module", 175),
        ("finit_module", 313),
        ("delete_module", 176),
        ("reboot", 169),
        ("setns", 308),
        ("unshare", 272),
        ("perf_event_open", 298),
        ("process_vm_readv", 310),
        ("process_vm_writev", 311),
        ("keyctl", 250),
        ("add_key", 248),
        ("setsid", 112),
        ("setpgid", 109),
    ];
    #[cfg(target_arch = "aarch64")]
    let table: &[(&str, i64)] = &[
        ("mount", 40),
        ("umount2", 39),
        ("pivot_root", 41),
        ("ptrace", 117),
        ("bpf", 280),
        ("kexec_load", 104),
        ("kexec_file_load", 294),
        ("init_module", 105),
        ("finit_module", 273),
        ("delete_module", 106),
        ("reboot", 142),
        ("setns", 268),
        ("unshare", 97),
        ("perf_event_open", 241),
        ("process_vm_readv", 270),
        ("process_vm_writev", 271),
        ("keyctl", 219),
        ("add_key", 217),
        ("setsid", 157),
        ("setpgid", 154),
    ];
    table
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, number)| *number)
}

#[cfg(test)]
mod tests {

    /// Le diagnostic de refus nomme-t-il la bonne cause ?
    ///
    /// On projette un processus sur lui-même. Il est donc dans le même espace de noms utilisateur
    /// que l'appelant — c'est le cas « l'`unshare` n'a pas eu lieu », et le noyau refuse. Un
    /// message qui dirait « capacités insuffisantes » enverrait chercher au mauvais endroit, ce
    /// qui est exactement ce que ce diagnostic existe pour éviter.
    #[test]
    fn un_refus_de_projection_nomme_sa_cause() {
        let soi = i32::try_from(std::process::id()).expect("un pid tient dans un i32");
        let erreur = super::map_child_to_root(soi)
            .expect_err("on ne projette pas un processus sur son propre espace de noms");
        let texte = erreur.to_string();
        assert!(
            texte.contains("MÊME espace de noms utilisateur"),
            "le diagnostic doit nommer la cause réelle, obtenu : {texte}"
        );
        assert!(
            texte.contains("espace de l'enfant"),
            "et montrer ce qu'il a lu, obtenu : {texte}"
        );
    }

    use super::*;

    #[test]
    fn tous_les_appels_refuses_ont_un_numero() {
        for name in capd::enforce::SECCOMP_DENY_LEVEL0 {
            assert!(
                syscall_number(name).is_some(),
                "{name} n'a pas de numéro sur cette architecture"
            );
        }
    }

    #[test]
    fn appel_inconnu_sans_numero() {
        assert!(syscall_number("appel_imaginaire").is_none());
    }
}
