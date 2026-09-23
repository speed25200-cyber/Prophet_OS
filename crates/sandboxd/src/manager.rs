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
    /// Répertoire de travail de la tâche, tel que la description le donne.
    pub workdir: String,
    /// Au niveau 2 : la microVM lancée, son dossier et son disque de travail (ADR 0038).
    pub microvm: Option<crate::launch::MicrovmRun>,
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
    /// MicroVM prêtes, restaurées d'un instantané (ADR 0045) ; absente sans niveau 2.
    reserve: Option<Arc<crate::reserve::Reserve>>,
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
            reserve: None,
        }
    }

    /// Garde `taille` microVM prêtes pour le niveau 2, restaurées d'un instantané et
    /// régénérées en arrière-plan (ADR 0045). Sans niveau 2, ou pour une taille nulle, rien ne
    /// change : chaque exécution démarre sa machine à froid. Sous `PROPHET_MICROVM_RESERVE`, la
    /// réserve garde son instantané d'un démarrage à l'autre ; sans elle, tout va dans un
    /// dossier temporaire propre à ce processus, retiré avec la réserve.
    #[must_use]
    pub fn avec_reserve(self, taille: usize) -> Self {
        match std::env::var_os("PROPHET_MICROVM_RESERVE") {
            Some(racine) => self.avec_reserve_dans(taille, racine.into(), true),
            None => {
                let racine = std::env::temp_dir().join(format!(
                    "prophet-reserve-{}-{}",
                    std::process::id(),
                    NEXT_SANDBOX.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ));
                self.avec_reserve_dans(taille, racine, false)
            }
        }
    }

    /// Comme [`Manager::avec_reserve`], sous `racine`. `persistante`, l'instantané y reste
    /// après la réserve et se reprend au démarrage suivant si le moniteur, le noyau et la racine
    /// d'invité n'ont pas changé ; sinon `racine` est retirée avec la réserve.
    #[must_use]
    pub fn avec_reserve_dans(
        mut self,
        taille: usize,
        racine: std::path::PathBuf,
        persistante: bool,
    ) -> Self {
        if taille == 0 || !self.caps.supports(2) {
            return self;
        }
        let (Some(firecracker), Some(images)) = (
            self.caps.firecracker.clone(),
            self.caps.microvm_images.clone(),
        ) else {
            return self;
        };
        self.reserve = Some(Arc::new(crate::reserve::Reserve::demarrer(
            &firecracker,
            &images,
            taille,
            racine,
            persistante,
        )));
        self
    }

    /// La réserve de microVM, si elle existe.
    #[must_use]
    pub fn reserve(&self) -> Option<&crate::reserve::Reserve> {
        self.reserve.as_deref()
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
        // Au niveau 2, une machine de la réserve si elle en a une ; sinon, ou si elle refuse
        // la tâche, une machine démarrée à froid. Dans les deux cas, une microVM : la réserve
        // accélère le niveau 2, elle ne le remplace jamais par un autre.
        let depuis_la_reserve = if spec.level == 2 {
            self.reserve
                .as_ref()
                .and_then(|reserve| reserve.prendre())
                .and_then(|membre| {
                    crate::launch::depuis_la_reserve(membre, spec)
                        .map_err(|erreur| {
                            tracing::warn!(%erreur, "machine de réserve refusée ; démarrage à froid");
                        })
                        .ok()
                })
        } else {
            None
        };
        let launched = match depuis_la_reserve {
            Some(launched) => Ok(launched),
            None => crate::launch::launch(&self.caps, &self.helper, spec, &sync_dir),
        }
        .map_err(|error| match error {
            crate::launch::LaunchError::Unreachable { level, missing } => {
                SandboxError::LevelUnavailable {
                    requested: level,
                    available: self.caps.max_level(),
                    report: format!("il manque {missing}\n{}", self.caps.report()),
                }
            }
            autre => SandboxError::Confine(autre.to_string()),
        })?;
        let microvm = launched.microvm.clone();
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
            workdir: spec.workdir.clone(),
            microvm,
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
