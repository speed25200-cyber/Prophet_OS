//! `prophet-pilot-cage` — la cage d'un client officiel lancé en mission (ADR 0056).
//!
//! Lancé par `prophet-pilotd` sous l'identité de l'humain, il se confine puis lance le client :
//! espaces de noms utilisateur, montage, processus, IPC et nom d'hôte ; racine minimale où ne
//! figurent que le système en lecture seule, le profil privé du client et les lieux de la
//! mission ; Landlock par-dessus. Le processus 1 de la cage attend le client, recueille ses
//! enfants et rend son code de sortie ; tuer le groupe de la cage tue tout ce qu'elle contient.
//!
//! La description vient de `PROPHET_SANDBOX_SPEC`, comme pour l'amorçage des sandboxes : même
//! format, mêmes montages, mêmes règles Landlock. Le réseau de l'hôte reste (phase 1 de l'ADR
//! 0056) ; aucun filtre d'appels système n'est posé, pour que le client garde ses propres
//! sandboxes (Codex confine ses commandes avec les espaces de noms).

use std::os::unix::process::CommandExt as _;
use std::process::{Command, ExitCode};

use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::{ForkResult, Pid, fork};
use sandboxd::confine;
use sandboxd::spec::{SPEC_ENV, SandboxSpec};

/// Code rendu quand la cage n'a pas pu se poser : le client n'a pas été lancé.
const CAGE_IMPOSSIBLE: u8 = 125;

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("prophet-pilot-cage : {error}");
            ExitCode::from(CAGE_IMPOSSIBLE)
        }
    }
}

fn run() -> Result<u8, Box<dyn std::error::Error>> {
    let raw = std::env::var(SPEC_ENV).map_err(|_| format!("variable {SPEC_ENV} absente"))?;
    let spec: SandboxSpec = serde_json::from_str(&raw)?;
    // La racine est posée où le lanceur le dit, pour qu'il la retire après ; sinon, son nom
    // suit ce processus-ci (dans la cage, chacun est le processus 1).
    let racine = std::env::var_os(pilotd::cage::RACINE_ENV).map_or_else(
        || std::env::temp_dir().join(format!("prophet-cage-{}", std::process::id())),
        std::path::PathBuf::from,
    );
    confine::entrer_pour_un_client()?;
    // SAFETY: ce programme n'a lancé aucun fil ; après `fork`, l'enfant n'appelle que des
    // fonctions sûres dans ce contexte (montages, Landlock, un second `fork`, `exec`).
    match unsafe { fork() }? {
        ForkResult::Parent { child } => Ok(code_de(attendre(child)?)),
        ForkResult::Child => {
            let code = match init(&spec, &racine) {
                Ok(code) => code,
                Err(error) => {
                    eprintln!("prophet-pilot-cage : {error}");
                    CAGE_IMPOSSIBLE
                }
            };
            std::process::exit(i32::from(code));
        }
    }
}

/// Le processus 1 de la cage : pose la racine et Landlock, lance le client, recueille les
/// enfants jusqu'à sa fin.
fn init(spec: &SandboxSpec, racine: &std::path::Path) -> Result<u8, Box<dyn std::error::Error>> {
    confine::pivot_to_minimal_root(spec, racine)?;
    if !confine::apply_landlock(spec)? {
        eprintln!(
            "prophet-pilot-cage : Landlock indisponible, restriction assurée par la racine minimale seule"
        );
    }
    // SAFETY: toujours un seul fil ; l'enfant ne fait que passer la main au client.
    match unsafe { fork() }? {
        ForkResult::Child => {
            let erreur = Command::new(&spec.program)
                .args(&spec.args)
                .current_dir(&spec.workdir)
                .env_clear()
                .envs(spec.env.iter().map(|(k, v)| (k.clone(), v.clone())))
                .exec();
            eprintln!("prophet-pilot-cage : {} : {erreur}", spec.program);
            std::process::exit(127);
        }
        ForkResult::Parent { child } => loop {
            // Le processus 1 recueille tout orphelin de la cage ; seul le client dit la fin.
            match waitpid(None, None) {
                Ok(statut) if statut.pid() == Some(child) => return Ok(code_de(statut)),
                Ok(_) | Err(nix::errno::Errno::EINTR) => {}
                Err(erreur) => return Err(Box::new(erreur)),
            }
        },
    }
}

fn attendre(pid: Pid) -> Result<WaitStatus, nix::Error> {
    loop {
        match waitpid(pid, None) {
            Err(nix::errno::Errno::EINTR) => {}
            autre => return autre,
        }
    }
}

/// Le code de sortie d'un processus, à la manière d'un shell : 128 + signal s'il a été tué.
fn code_de(statut: WaitStatus) -> u8 {
    match statut {
        WaitStatus::Exited(_, code) => u8::try_from(code).unwrap_or(1),
        WaitStatus::Signaled(_, signal, _) => 128u8.saturating_add(signal as u8),
        _ => 1,
    }
}
