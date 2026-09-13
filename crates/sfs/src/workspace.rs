//! Espace de travail d'une tâche : ouverture, diff, validation, annulation.
//!
//! Invariant central : **rien de ce qu'une tâche écrit n'atteint l'espace de l'utilisateur avant
//! une validation explicite**, et toute validation reste annulable tant que le point de
//! restauration existe.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::backend::{Backend, detect_backend};
use crate::diff::{self, ChangeKind, Diff, Fingerprints};
use crate::provenance::{Provenance, write_provenance};

/// Erreur du système de fichiers sémantique.
#[derive(Debug, thiserror::Error)]
pub enum SfsError {
    /// Erreur d'entrée-sortie.
    #[error("erreur d'entrée-sortie : {0}")]
    Io(#[from] std::io::Error),
    /// Sérialisation de l'état.
    #[error("état illisible : {0}")]
    State(#[from] serde_json::Error),
    /// Espace de travail inconnu.
    #[error("espace de travail inconnu pour la tâche {0}")]
    UnknownTask(String),
    /// Opération incompatible avec l'état courant.
    #[error("opération impossible dans l'état {state:?}")]
    BadState {
        /// État courant.
        state: WorkspaceState,
    },
    /// Le périmètre demandé sort du répertoire personnel.
    #[error("périmètre hors du répertoire personnel : {0}")]
    ScopeOutsideHome(String),
    /// Aucun point de restauration.
    #[error("aucun point de restauration pour cette tâche")]
    NothingToUndo,
}

/// État d'un espace de travail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceState {
    /// Ouvert, la tâche peut écrire.
    Open,
    /// Validé : les changements ont atteint l'espace de l'utilisateur.
    Committed,
    /// Annulé après validation.
    RolledBack,
    /// Abandonné sans validation.
    Abandoned,
}

/// État persisté d'un espace de travail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Meta {
    task: String,
    scopes: Vec<PathBuf>,
    state: WorkspaceState,
    #[serde(with = "time::serde::rfc3339")]
    opened: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    committed: Option<OffsetDateTime>,
    base: Fingerprints,
}

/// Espace de travail d'une tâche.
#[derive(Debug)]
pub struct Workspace {
    root: PathBuf,
    home: PathBuf,
    meta: Meta,
    backend: Backend,
}

/// Nom du répertoire de travail dans l'espace de la tâche.
const WORK: &str = "work";
/// Nom du répertoire des copies de restauration.
const RESTORE: &str = "restore";
/// Nom du fichier d'état.
const META: &str = "meta.json";
/// Nom du répertoire des transactions en cours.
///
/// Il est volontairement **hors** de l'arbre de travail : une écriture en cours ne doit apparaître
/// ni dans le diff, ni pour la tâche elle-même, tant que la transaction n'est pas validée.
const TX: &str = "tx";

impl Workspace {
    /// Fige les empreintes des changements d'une capture autorisée, pour leur examen humain.
    ///
    /// # Errors
    /// Capture historique sans versions conservées, lien, fichier spécial ou travail trop grand.
    pub fn seal_review(&self) -> Result<crate::ReviewIndex, SfsError> {
        crate::review::seal(&self.home, &self.meta.task, &self.meta.base).map_err(Into::into)
    }

    /// Capture de service : droits vérifiés par descendant et ouvertures Linux sans liens.
    ///
    /// # Errors
    /// Identifiant existant, périmètre refusé, source instable ou plafond de capture atteint.
    pub fn begin_authorized(
        home: &Path,
        task: &str,
        scopes: &[String],
        now: OffsetDateTime,
        permits: &dyn Fn(&Path) -> bool,
    ) -> Result<Self, SfsError> {
        let mut resolved = Vec::new();
        for scope in scopes {
            let absolute = scope_path(home, scope)?;
            let relative = absolute
                .strip_prefix(home)
                .map_err(|_| SfsError::ScopeOutsideHome(scope.clone()))?
                .to_path_buf();
            if resolved
                .iter()
                .any(|p: &PathBuf| relative.starts_with(p) || p.starts_with(&relative))
            {
                return Err(SfsError::ScopeOutsideHome("périmètres chevauchants".into()));
            }
            resolved.push(relative);
        }
        let base = crate::snapshot::capture(home, task, &resolved, permits)?;
        let workspace = Self {
            root: Self::root_for(home).join(task),
            home: home.into(),
            meta: Meta {
                task: task.into(),
                scopes: resolved,
                state: WorkspaceState::Open,
                opened: now,
                committed: None,
                base,
            },
            backend: detect_backend(home),
        };
        workspace.save()?;
        Ok(workspace)
    }
    /// Racine des espaces de travail d'un utilisateur.
    #[must_use]
    pub fn root_for(home: &Path) -> PathBuf {
        home.join(".prophet/tasks")
    }

    /// Ouvre un espace de travail pour une tâche.
    ///
    /// `scopes` liste les répertoires que la tâche pourra modifier, relatifs au home ou absolus
    /// sous le home. Tout ce qui est hors du home est refusé.
    ///
    /// # Erreurs
    /// Périmètre hors du home, ou erreur d'entrée-sortie.
    pub fn begin(
        home: &Path,
        task: &str,
        scopes: &[&str],
        now: OffsetDateTime,
    ) -> Result<Self, SfsError> {
        let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
        let root = Self::root_for(&home).join(task);
        std::fs::create_dir_all(root.join(WORK))?;
        std::fs::create_dir_all(root.join(RESTORE))?;

        let mut resolved = Vec::new();
        let mut base = Fingerprints::new();
        for scope in scopes {
            let absolute = resolve_scope(&home, scope)?;
            let relative = absolute
                .strip_prefix(&home)
                .map_err(|_| SfsError::ScopeOutsideHome(scope.to_string()))?
                .to_path_buf();
            let work_scope = root.join(WORK).join(&relative);
            std::fs::create_dir_all(&work_scope)?;
            copy_tree(&absolute, &work_scope)?;
            for (path, fingerprint) in diff::fingerprint_tree(&absolute)? {
                base.insert(relative.join(path), fingerprint);
            }
            resolved.push(relative);
        }

        let meta = Meta {
            task: task.to_owned(),
            scopes: resolved,
            state: WorkspaceState::Open,
            opened: now,
            committed: None,
            base,
        };
        let backend = detect_backend(&home);
        let workspace = Self {
            root,
            home,
            meta,
            backend,
        };
        workspace.save()?;
        Ok(workspace)
    }

    /// Liste les espaces de travail présents sur le disque, avec leur état.
    ///
    /// Utilisable sans aucun daemon : c'est ce qui permet d'inspecter et d'annuler une tâche
    /// après un incident, quand plus rien d'autre ne tourne.
    ///
    /// # Errors
    /// Si le répertoire des tâches est illisible.
    pub fn list(home: &Path) -> Result<Vec<(String, WorkspaceState)>, SfsError> {
        let racine = Self::root_for(home);
        if !racine.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&racine)? {
            let entry = entry?;
            let meta_path = entry.path().join(META);
            if !meta_path.exists() {
                continue;
            }
            let Ok(meta) = serde_json::from_str::<Meta>(&std::fs::read_to_string(&meta_path)?)
            else {
                continue;
            };
            out.push((meta.task, meta.state));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    /// Rouvre un espace de travail existant.
    ///
    /// # Erreurs
    /// Si la tâche est inconnue ou son état illisible.
    pub fn open(home: &Path, task: &str) -> Result<Self, SfsError> {
        let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
        let root = Self::root_for(&home).join(task);
        let meta_path = root.join(META);
        if !meta_path.exists() {
            return Err(SfsError::UnknownTask(task.to_owned()));
        }
        let meta: Meta = serde_json::from_str(&std::fs::read_to_string(&meta_path)?)?;
        let backend = detect_backend(&home);
        Ok(Self {
            root,
            home,
            meta,
            backend,
        })
    }

    /// Répertoire dans lequel la tâche travaille.
    #[must_use]
    pub fn workdir(&self) -> PathBuf {
        self.root.join(WORK)
    }

    /// État courant.
    #[must_use]
    pub const fn state(&self) -> WorkspaceState {
        self.meta.state
    }

    /// Dorsale active.
    #[must_use]
    pub const fn backend(&self) -> &Backend {
        &self.backend
    }

    /// Traduit un chemin de l'espace utilisateur vers l'espace de travail.
    ///
    /// Retourne `None` si le chemin sort du périmètre : c'est ainsi qu'une tâche ne peut pas
    /// écrire là où elle n'a rien à faire, même si son jeton le lui permettait.
    #[must_use]
    pub fn to_work_path(&self, real: &Path) -> Option<PathBuf> {
        let relative = real.strip_prefix(&self.home).ok()?;
        if !self
            .meta
            .scopes
            .iter()
            .any(|scope| relative.starts_with(scope))
        {
            return None;
        }
        Some(self.root.join(WORK).join(relative))
    }

    fn save(&self) -> Result<(), SfsError> {
        let text = serde_json::to_string_pretty(&self.meta)?;
        let temp = self.root.join(format!("{META}.tmp"));
        std::fs::write(&temp, text)?;
        std::fs::rename(temp, self.root.join(META))?;
        Ok(())
    }

    /// Calcule le diff entre l'état de départ et l'espace de travail.
    ///
    /// # Erreurs
    /// Si l'espace de travail est illisible.
    pub fn diff(&self) -> Result<Diff, SfsError> {
        Ok(diff::compute(&self.meta.base, &self.root.join(WORK))?)
    }

    /// Valide les changements : sauvegarde l'existant puis applique.
    ///
    /// La sauvegarde précède l'application, de sorte qu'une interruption au milieu laisse de quoi
    /// revenir en arrière.
    ///
    /// # Erreurs
    /// Si l'état n'est pas `Open`, ou en cas d'erreur d'entrée-sortie.
    pub fn commit(
        &mut self,
        now: OffsetDateTime,
        provenance: Option<&Provenance>,
    ) -> Result<Diff, SfsError> {
        if self.meta.state != WorkspaceState::Open {
            return Err(SfsError::BadState {
                state: self.meta.state,
            });
        }
        let diff = self.diff()?;
        let restore = self.root.join(RESTORE);
        std::fs::create_dir_all(&restore)?;

        // 1. Sauvegarde de l'existant, pour tout ce qui sera écrasé ou supprimé.
        for change in &diff.changes {
            let real = self.home.join(&change.path);
            if matches!(change.kind, ChangeKind::Modified | ChangeKind::Deleted) && real.exists() {
                let saved = restore.join(&change.path);
                if let Some(parent) = saved.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&real, &saved)?;
            }
        }
        std::fs::write(
            restore.join("changes.json"),
            serde_json::to_string_pretty(&diff)?,
        )?;

        // 2. Application.
        for change in &diff.changes {
            let real = self.home.join(&change.path);
            let work = self.root.join(WORK).join(&change.path);
            match change.kind {
                ChangeKind::Added | ChangeKind::Modified => {
                    if let Some(parent) = real.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::copy(&work, &real)?;
                    if let Some(provenance) = provenance {
                        let _ = write_provenance(&real, provenance);
                    }
                }
                ChangeKind::Deleted => {
                    if real.exists() {
                        std::fs::remove_file(&real)?;
                    }
                }
            }
        }

        self.meta.state = WorkspaceState::Committed;
        self.meta.committed = Some(now);
        self.save()?;
        Ok(diff)
    }

    /// Annule la dernière validation, en restaurant l'état d'avant.
    ///
    /// # Erreurs
    /// Si rien n'a été validé, ou en cas d'erreur d'entrée-sortie.
    pub fn undo(&mut self) -> Result<Diff, SfsError> {
        if self.meta.state != WorkspaceState::Committed {
            return Err(SfsError::BadState {
                state: self.meta.state,
            });
        }
        let restore = self.root.join(RESTORE);
        let changes_path = restore.join("changes.json");
        if !changes_path.exists() {
            return Err(SfsError::NothingToUndo);
        }
        let diff: Diff = serde_json::from_str(&std::fs::read_to_string(&changes_path)?)?;

        for change in &diff.changes {
            let real = self.home.join(&change.path);
            let saved = restore.join(&change.path);
            match change.kind {
                ChangeKind::Added => {
                    if real.exists() {
                        std::fs::remove_file(&real)?;
                    }
                }
                ChangeKind::Modified | ChangeKind::Deleted => {
                    if saved.exists() {
                        if let Some(parent) = real.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        std::fs::copy(&saved, &real)?;
                    }
                }
            }
        }
        self.meta.state = WorkspaceState::RolledBack;
        self.save()?;
        Ok(diff)
    }

    /// Abandonne l'espace de travail sans rien appliquer.
    ///
    /// # Erreurs
    /// Si l'état n'est pas `Open`, ou en cas d'erreur d'entrée-sortie.
    pub fn abandon(&mut self) -> Result<(), SfsError> {
        if self.meta.state != WorkspaceState::Open {
            return Err(SfsError::BadState {
                state: self.meta.state,
            });
        }
        std::fs::remove_dir_all(self.root.join(WORK))?;
        std::fs::create_dir_all(self.root.join(WORK))?;
        self.meta.state = WorkspaceState::Abandoned;
        self.save()?;
        Ok(())
    }

    /// Ouvre une transaction d'écriture multi-fichiers.
    ///
    /// Les écritures atterrissent dans un répertoire temporaire ; elles n'apparaissent dans
    /// l'espace de travail qu'à la validation de la transaction. Une tâche tuée au milieu ne
    /// laisse donc aucun état partiel.
    ///
    /// # Erreurs
    /// En cas d'erreur d'entrée-sortie.
    pub fn tx_begin(&self, id: &str) -> Result<Transaction, SfsError> {
        let dir = self.root.join(TX).join(id);
        std::fs::create_dir_all(&dir)?;
        Ok(Transaction {
            dir,
            work: self.root.join(WORK),
            files: Vec::new(),
        })
    }

    /// Supprime les transactions inachevées laissées par une interruption.
    ///
    /// # Erreurs
    /// En cas d'erreur d'entrée-sortie.
    pub fn sweep_transactions(&self) -> Result<usize, SfsError> {
        let tx_root = self.root.join(TX);
        if !tx_root.exists() {
            return Ok(0);
        }
        let mut removed = 0;
        for entry in std::fs::read_dir(&tx_root)? {
            std::fs::remove_dir_all(entry?.path())?;
            removed += 1;
        }
        std::fs::remove_dir_all(&tx_root)?;
        Ok(removed)
    }
}

/// Transaction d'écriture multi-fichiers.
#[derive(Debug)]
pub struct Transaction {
    dir: PathBuf,
    work: PathBuf,
    files: Vec<PathBuf>,
}

impl Transaction {
    /// Écrit un fichier dans la transaction.
    ///
    /// # Erreurs
    /// En cas d'erreur d'entrée-sortie.
    pub fn write(&mut self, relative: &Path, content: &[u8]) -> Result<(), SfsError> {
        let staged = self.dir.join(relative);
        if let Some(parent) = staged.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&staged, content)?;
        self.files.push(relative.to_path_buf());
        Ok(())
    }

    /// Publie toutes les écritures dans l'espace de travail.
    ///
    /// # Erreurs
    /// En cas d'erreur d'entrée-sortie.
    pub fn commit(self) -> Result<usize, SfsError> {
        for relative in &self.files {
            let staged = self.dir.join(relative);
            let target = self.work.join(relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&staged, &target)?;
        }
        let count = self.files.len();
        std::fs::remove_dir_all(&self.dir)?;
        Ok(count)
    }

    /// Abandonne la transaction.
    ///
    /// # Erreurs
    /// En cas d'erreur d'entrée-sortie.
    pub fn abort(self) -> Result<(), SfsError> {
        std::fs::remove_dir_all(&self.dir)?;
        Ok(())
    }
}

fn resolve_scope(home: &Path, scope: &str) -> Result<PathBuf, SfsError> {
    let candidate = scope_path(home, scope)?;
    std::fs::create_dir_all(&candidate)?;
    Ok(candidate)
}

fn scope_path(home: &Path, scope: &str) -> Result<PathBuf, SfsError> {
    let candidate = if let Some(rest) = scope.strip_prefix("~/") {
        home.join(rest)
    } else if scope.starts_with('/') {
        PathBuf::from(scope)
    } else {
        home.join(scope)
    };
    if candidate
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(SfsError::ScopeOutsideHome(scope.to_owned()));
    }
    if !candidate.starts_with(home) {
        return Err(SfsError::ScopeOutsideHome(scope.to_owned()));
    }
    Ok(candidate)
}

fn copy_tree(source: &Path, target: &Path) -> std::io::Result<()> {
    if !source.exists() {
        return Ok(());
    }
    let mut stack = vec![source.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.is_symlink() {
                continue;
            }
            let Ok(relative) = path.strip_prefix(source) else {
                continue;
            };
            let destination = target.join(relative);
            if metadata.is_dir() {
                std::fs::create_dir_all(&destination)?;
                stack.push(path);
            } else if metadata.is_file() {
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&path, &destination)?;
            }
        }
    }
    Ok(())
}
