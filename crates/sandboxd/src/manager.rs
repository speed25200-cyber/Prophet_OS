//! Cycle de vie des sandboxes : lancement, gel, reprise, arrêt.

use std::collections::HashMap;
use std::process::Child;
use std::sync::{Arc, Mutex};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use crate::caps::Capabilities;
use crate::spec::SandboxSpec;

/// Erreur du gestionnaire de sandbox.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// Le niveau demandé n'est pas atteignable sur cette machine.
    #[error("niveau {requested} indisponible : niveau maximal {available}\n{report}")]
    LevelUnavailable {
        /// Niveau demandé.
        requested: u8,
        /// Niveau maximal atteignable.
        available: u8,
        /// Détail de la sonde.
        report: String,
    },
    /// Erreur d'entrée-sortie au lancement.
    #[error("lancement impossible : {0}")]
    Io(#[from] std::io::Error),
    /// Sandbox inconnue.
    #[error("sandbox inconnue : {0}")]
    Unknown(String),
    /// Erreur de signal.
    #[error("signal en échec : {0}")]
    Signal(#[from] nix::Error),
    /// Description non sérialisable.
    #[error("description non sérialisable : {0}")]
    Serialize(#[from] serde_json::Error),
    /// Le confinement n'a pas pu être mis en place.
    #[error("confinement impossible : {0}")]
    Confine(String),
}

/// État d'une sandbox.
///
/// Sérialisable : c'est ce que `prophet-sandboxd` rend à qui demande, et ce que la surface montre.
/// Un état qu'on ne peut pas transmettre ne sert qu'au processus qui le détient.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxState {
    /// En cours d'exécution.
    Running,
    /// Gelée.
    Frozen,
    /// Terminée.
    Exited {
        /// Code de sortie, si le processus s'est terminé normalement.
        code: Option<i32>,
    },
}

/// Poignée sur une sandbox lancée.
#[derive(Debug)]
pub struct SandboxHandle {
    /// Identifiant attribué par le gestionnaire.
    pub id: String,
    /// Tâche propriétaire.
    pub task: String,
    /// Niveau appliqué.
    pub level: u8,
    /// Identifiant de processus.
    pub pid: i32,
    state: SandboxState,
    child: Option<Child>,
}

impl SandboxHandle {
    /// État courant.
    #[must_use]
    pub const fn state(&self) -> SandboxState {
        self.state
    }

    /// Accès au processus, pour lire ses flux de sortie.
    pub fn child_mut(&mut self) -> Option<&mut Child> {
        self.child.as_mut()
    }

    /// Attend la fin et renvoie le code de sortie.
    ///
    /// # Erreurs
    /// Si l'attente échoue.
    pub fn wait(&mut self) -> std::io::Result<Option<i32>> {
        if let Some(child) = &mut self.child {
            let status = child.wait()?;
            self.state = SandboxState::Exited {
                code: status.code(),
            };
            return Ok(status.code());
        }
        Ok(None)
    }
}

/// Gestionnaire de sandboxes.
#[derive(Debug)]
pub struct Manager {
    caps: Capabilities,
    helper: String,
    running: Arc<Mutex<HashMap<String, i32>>>,
}

/// Compteur global au processus : deux gestionnaires vivant côte à côte ne peuvent pas produire
/// le même identifiant, et donc pas le même chemin de poignée de main.
static NEXT_SANDBOX: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl Manager {
    /// Construit un gestionnaire, en sondant la machine.
    ///
    /// `helper` est le chemin du programme d'amorçage `prophet-sandbox-helper`.
    #[must_use]
    pub fn new(helper: impl Into<String>) -> Self {
        Self {
            caps: Capabilities::probe(),
            helper: helper.into(),
            running: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Capacités de la machine.
    #[must_use]
    pub const fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    /// Niveau à appliquer, compte tenu du minimum exigé par le jeton et du manifeste.
    ///
    /// Toute exécution de code arbitraire impose le niveau 2, quelle que soit la demande.
    #[must_use]
    pub fn required_level(manifest_min: u8, token_min: u8, executes_code: bool) -> u8 {
        let base = manifest_min.max(token_min);
        if executes_code { base.max(2) } else { base }
    }

    /// Lance un programme sous sandbox.
    ///
    /// # Erreurs
    /// [`SandboxError::LevelUnavailable`] si le niveau demandé n'est pas garantissable : le
    /// gestionnaire préfère refuser plutôt qu'exécuter avec moins d'isolation qu'annoncé.
    pub fn run(&self, task: &str, spec: &SandboxSpec) -> Result<SandboxHandle, SandboxError> {
        if !self.caps.supports(spec.level) {
            return Err(SandboxError::LevelUnavailable {
                requested: spec.level,
                available: self.caps.max_level(),
                report: self.caps.report(),
            });
        }
        // L'identifiant porte le processus gestionnaire : deux gestionnaires vivant côte à côte
        // (des tests en parallèle, deux daemons sur une machine de développement) ne peuvent pas
        // se marcher dessus sur les chemins de poignée de main.
        let id = format!(
            "sbx-{}-{}",
            std::process::id(),
            NEXT_SANDBOX.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        // Répertoire de poignée de main, détruit avec la sandbox.
        let sync_dir = std::env::temp_dir().join(format!("prophet-sync-{id}"));
        let handshake = crate::confine::Handshake::create(&sync_dir)
            .map_err(|e| SandboxError::Confine(e.to_string()))?;

        // Chaque niveau a son propre chemin de lancement. Le gestionnaire ne retombe jamais sur un
        // niveau inférieur : il refuse, en nommant ce qui manque.
        let launched =
            crate::launch::launch(&self.caps, &self.helper, spec, &sync_dir).map_err(|error| {
                match error {
                    crate::launch::LaunchError::Unreachable { level, missing } => {
                        SandboxError::LevelUnavailable {
                            requested: level,
                            available: self.caps.max_level(),
                            report: format!("il manque {missing}\n{}", self.caps.report()),
                        }
                    }
                    autre => SandboxError::Confine(autre.to_string()),
                }
            })?;
        let mut child = launched.child;
        let pid = i32::try_from(child.id()).unwrap_or(0);

        if launched.needs_handshake {
            // L'amorçage a créé son espace de noms ; c'est au gestionnaire d'y projeter les
            // identifiants, le noyau refusant que le processus le fasse pour lui-même.
            if let Err(error) = handshake
                .wait_for_ready()
                .and_then(|()| crate::confine::map_child_to_root(pid))
            {
                // On libère l'amorçage avec un refus pour qu'il s'arrête proprement, puis on tue.
                let _ = handshake.release(false);
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_dir_all(&sync_dir);
                return Err(SandboxError::Confine(error.to_string()));
            }
            handshake
                .release(true)
                .map_err(|e| SandboxError::Confine(e.to_string()))?;
        }
        let _ = std::fs::remove_dir_all(&sync_dir);
        self.running
            .lock()
            .map(|mut map| map.insert(id.clone(), pid))
            .ok();
        Ok(SandboxHandle {
            id,
            task: task.to_owned(),
            level: spec.level,
            pid,
            state: SandboxState::Running,
            child: Some(child),
        })
    }

    /// Gèle une sandbox.
    ///
    /// # Erreurs
    /// Si le signal échoue.
    pub fn freeze(&self, handle: &mut SandboxHandle) -> Result<(), SandboxError> {
        signaler(handle.pid, Signal::SIGSTOP)?;
        handle.state = SandboxState::Frozen;
        Ok(())
    }

    /// Dégèle une sandbox.
    ///
    /// # Erreurs
    /// Si le signal échoue.
    pub fn thaw(&self, handle: &mut SandboxHandle) -> Result<(), SandboxError> {
        signaler(handle.pid, Signal::SIGCONT)?;
        handle.state = SandboxState::Running;
        Ok(())
    }

    /// Arrête une sandbox.
    ///
    /// # Erreurs
    /// Si le signal échoue.
    pub fn kill(&self, handle: &mut SandboxHandle) -> Result<(), SandboxError> {
        let _ = signaler(handle.pid, Signal::SIGKILL);
        self.running
            .lock()
            .map(|mut map| map.remove(&handle.id))
            .ok();
        handle.state = SandboxState::Exited { code: None };
        Ok(())
    }

    /// Gèle **toutes** les sandboxes en cours.
    ///
    /// C'est le geste d'urgence de l'humain : il doit aboutir en quelques dizaines de
    /// millisecondes, quel que soit le nombre de tâches.
    ///
    /// Retourne le nombre de sandboxes gelées.
    pub fn freeze_all(&self) -> usize {
        let Ok(map) = self.running.lock() else {
            return 0;
        };
        let mut frozen = 0;
        for pid in map.values() {
            if signaler(*pid, Signal::SIGSTOP).is_ok() {
                frozen += 1;
            }
        }
        frozen
    }

    /// Dégèle toutes les sandboxes.
    pub fn thaw_all(&self) -> usize {
        let Ok(map) = self.running.lock() else {
            return 0;
        };
        map.values()
            .filter(|pid| signaler(**pid, Signal::SIGCONT).is_ok())
            .count()
    }

    /// Nombre de sandboxes suivies.
    #[must_use]
    pub fn count(&self) -> usize {
        self.running.lock().map(|m| m.len()).unwrap_or(0)
    }
}

/// Signale la sandbox entière : le groupe de processus dont l'amorçage est le chef, ce qui
/// atteint aussi tout ce qu'elle a lancé. Si le groupe n'existe plus (l'amorçage a été
/// récolté), on retombe sur le processus seul, pour rendre l'erreur qu'il donnerait.
fn signaler(pid: i32, signal: Signal) -> nix::Result<()> {
    kill(Pid::from_raw(-pid), signal).or_else(|_| kill(Pid::from_raw(pid), signal))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn niveau_impose_par_l_execution_de_code() {
        assert_eq!(Manager::required_level(0, 0, false), 0);
        assert_eq!(Manager::required_level(1, 0, false), 1);
        assert_eq!(Manager::required_level(0, 2, false), 2);
        assert_eq!(
            Manager::required_level(0, 0, true),
            2,
            "toute exécution de code arbitraire impose la microVM"
        );
        assert_eq!(Manager::required_level(1, 1, true), 2);
    }

    #[test]
    fn niveau_indisponible_refuse_plutot_que_degrade() {
        let manager = Manager::new("/n/existe/pas");
        let spec = SandboxSpec::new(2, "/bin/true", "/");
        if manager.capabilities().max_level() < 2 {
            let err = manager.run("task:01", &spec).unwrap_err();
            assert!(
                matches!(err, SandboxError::LevelUnavailable { requested: 2, .. }),
                "{err}"
            );
            // Le message doit expliquer ce qui manque, pas seulement échouer.
            assert!(err.to_string().contains("Firecracker"), "{err}");
        }
    }
}
