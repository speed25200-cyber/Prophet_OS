//! Programme d'amorçage d'une sandbox.
//!
//! Lancé par `sandboxd`, il se confine lui-même puis remplace son image par celle du programme
//! cible. Le confinement est donc en place **avant** que la moindre instruction du programme
//! confiné ne s'exécute, et le programme n'a aucun moyen de s'y soustraire : les mécanismes
//! employés (espaces de noms, racine minimale, seccomp) ne se relâchent pas.

use std::os::unix::process::CommandExt as _;
use std::process::Command;

use sandboxd::confine;
use sandboxd::spec::{SPEC_ENV, SandboxSpec};

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("prophet-sandbox-helper : {error}");
            std::process::ExitCode::from(127)
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let raw = std::env::var(SPEC_ENV).map_err(|_| format!("variable {SPEC_ENV} absente"))?;
    let spec: SandboxSpec = serde_json::from_str(&raw)?;

    let handshake = std::env::var_os(confine::HANDSHAKE_ENV)
        .map(|dir| confine::Handshake::from_dir(std::path::Path::new(&dir)));
    confine::enter_namespaces(handshake.as_ref())?;

    let root = std::env::temp_dir().join(format!("prophet-root-{}", std::process::id()));
    confine::pivot_to_minimal_root(&spec, &root)?;

    let landlock = confine::apply_landlock(&spec)?;
    confine::apply_seccomp(spec.level)?;

    if !landlock {
        // Sans Landlock, la restriction de chemins repose entièrement sur la racine minimale.
        // On le dit, plutôt que de laisser croire à une protection qui n'existe pas.
        eprintln!(
            "prophet-sandbox-helper : Landlock indisponible, restriction assurée par la racine minimale seule"
        );
    }

    let workdir = if std::path::Path::new(&spec.workdir).exists() {
        spec.workdir.clone()
    } else {
        "/".to_owned()
    };

    let error = Command::new(&spec.program)
        .args(&spec.args)
        .current_dir(workdir)
        .env_clear()
        .envs(spec.env.iter().map(|(k, v)| (k.clone(), v.clone())))
        .exec();
    Err(Box::new(error))
}
