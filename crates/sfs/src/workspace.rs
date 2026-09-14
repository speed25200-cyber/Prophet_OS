//! Espace de travail d'une tâche : ouverture, diff, validation, annulation.
//!
//! Le travail reste privé jusqu'à publication. L'appelant doit autoriser celle-ci ; l'annulation
//! exige des versions publiées et des sauvegardes intactes. Un lot peut être partiellement visible.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::backend::{Backend, detect_backend};
use crate::diff::{self, ChangeKind, Diff, Fingerprints};
use crate::provenance::Provenance;

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
    /// Publication commencée, dont le journal doit être repris après une interruption.
    Applying,
    /// Annulation commencée, dont le journal doit être repris après une interruption.
    Undoing,
    /// Publication interrompue sur une divergence ; les fichiers déplacés sont conservés.
    Conflict,
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
        Self::begin_authorized_from(home, task, scopes, now, permits, None)
    }

    /// Capture de service depuis l'espace de travail d'une mission parente (ADR 0039) : la
    /// sous-mission part de ce que le parent a déjà fait, non des fichiers de l'humain ; un
    /// périmètre que le parent n'a pas vient du répertoire personnel. Les droits se jugent sur
    /// les chemins du répertoire personnel, comme pour toute capture.
    ///
    /// # Errors
    /// Comme [`Self::begin_authorized`] ; source non absolue.
    pub fn begin_authorized_from(
        home: &Path,
        task: &str,
        scopes: &[String],
        now: OffsetDateTime,
        permits: &dyn Fn(&Path) -> bool,
        source: Option<&Path>,
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
        let base = crate::snapshot::capture(home, task, &resolved, permits, source)?;
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
        let scopes: Vec<String> = scopes.iter().map(|scope| (*scope).to_owned()).collect();
        Self::begin_authorized(home, task, &scopes, now, &|_| true)
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
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let Some(task) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            // Même lecture bornée et ancrée que l'ouverture individuelle. Ne jamais suivre
            // meta.json par un chemin ordinaire, ni croire son identifiant sans le comparer.
            let root = crate::review::task_root(home, &task)?;
            let meta: Meta = match crate::publication::read_json(&root, META) {
                Ok(meta) => meta,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            if meta.task != task {
                return Err(SfsError::UnknownTask(task));
            }
            // Une capture inachevée sans meta peut être ignorée ; un journal existant dont
            // le manifeste a disparu doit au contraire signaler son état illisible.
            let journal = crate::publication::read_journal(home, &task)?;
            out.push((task, journal.map_or(meta.state, |journal| journal.state)));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    /// Rouvre un espace de travail existant.
    ///
    /// # Erreurs
    /// Si la tâche est inconnue ou son état illisible.
    pub fn open(home: &Path, task: &str) -> Result<Self, SfsError> {
        let directory = crate::review::task_root(home, task)?;
        let meta: Meta = crate::publication::read_json(&directory, META)?;
        if meta.task != task {
            return Err(SfsError::UnknownTask(task.into()));
        }
        let mut workspace = Self {
            root: Self::root_for(home).join(task),
            home: home.into(),
            meta,
            backend: detect_backend(home),
        };
        workspace.refresh_publication()?;
        Ok(workspace)
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
        let root = crate::review::task_root(&self.home, &self.meta.task)?;
        crate::publication::atomic_json(&root, META, &self.meta).map_err(Into::into)
    }

    fn refresh_publication(&mut self) -> Result<(), SfsError> {
        if let Some(journal) = crate::publication::read_journal(&self.home, &self.meta.task)? {
            self.meta.state = journal.state;
            self.meta.committed = Some(journal.committed);
        }
        Ok(())
    }

    /// Calcule le diff entre l'état de départ et l'espace de travail.
    ///
    /// # Erreurs
    /// Si l'espace de travail est illisible.
    pub fn diff(&self) -> Result<Diff, SfsError> {
        Ok(diff::compute(&self.meta.base, &self.root.join(WORK))?)
    }

    /// Rapporte les changements de cet espace dans celui de la mission parente (ADR 0039) :
    /// les fichiers créés ou modifiés y sont copiés, les supprimés en sont retirés, dans les
    /// périmètres du parent seulement — le reste demeure ici. Le parent continue avec, et
    /// publie le tout, examiné d'un seul tenant. Aucun lien n'est suivi.
    ///
    /// # Errors
    /// Parent qui n'est plus ouvert, chemin refusé, fichier qui n'est pas ordinaire, entrée-sortie.
    pub fn carry_into(&self, parent: &Self) -> Result<Diff, SfsError> {
        if parent.state() != WorkspaceState::Open {
            return Err(SfsError::BadState {
                state: parent.state(),
            });
        }
        let diff = self.diff()?;
        let work = self.workdir();
        let destination = parent.workdir();
        let mut carried = Vec::new();
        for change in diff.changes {
            let relative = &change.path;
            if relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return Err(SfsError::Io(std::io::Error::other(
                    "rapport au parent : chemin refusé",
                )));
            }
            if !parent
                .meta
                .scopes
                .iter()
                .any(|scope| relative.starts_with(scope))
            {
                continue;
            }
            let cible = destination.join(relative);
            match change.kind {
                ChangeKind::Deleted => match std::fs::remove_file(&cible) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                },
                ChangeKind::Added | ChangeKind::Modified => {
                    let origine = work.join(relative);
                    if !std::fs::symlink_metadata(&origine)?.is_file() {
                        return Err(SfsError::Io(std::io::Error::other(
                            "rapport au parent : seul un fichier ordinaire se rapporte",
                        )));
                    }
                    if let Ok(meta) = std::fs::symlink_metadata(&cible)
                        && !meta.is_file()
                    {
                        return Err(SfsError::Io(std::io::Error::other(
                            "rapport au parent : la destination n'est pas un fichier ordinaire",
                        )));
                    }
                    if let Some(dossier) = cible.parent() {
                        std::fs::create_dir_all(dossier)?;
                    }
                    let nom = relative
                        .file_name()
                        .and_then(|n| n.to_str())
                        .ok_or_else(|| {
                            SfsError::Io(std::io::Error::other("rapport au parent : nom refusé"))
                        })?;
                    let provisoire =
                        cible.with_file_name(format!(".{nom}.rapport-{}", std::process::id()));
                    std::fs::copy(&origine, &provisoire)?;
                    std::fs::rename(&provisoire, &cible)?;
                }
            }
            carried.push(change);
        }
        Ok(Diff { changes: carried })
    }

    /// Publie les changements courants avec contrôle des conflits et journal de reprise.
    ///
    /// Le code appelant est responsable de l'autorisation humaine. Pour publier exactement
    /// un index déjà examiné, employer [`Self::commit_review`]. Le lot n'est pas instantané.
    ///
    /// # Errors
    /// État incompatible, conflit, lien, version altérée ou erreur de synchronisation.
    pub fn commit(
        &mut self,
        now: OffsetDateTime,
        provenance: Option<&Provenance>,
    ) -> Result<Diff, SfsError> {
        let review = self.seal_review()?;
        self.commit_review(&review, now, provenance)
    }

    /// Publie uniquement l'index exact fourni par l'appelant, après relecture de ses versions.
    ///
    /// Cette API de fichiers ne délivre aucune autorisation : l'identité, le consentement
    /// et les droits doivent être contrôlés par l'appelant avant cet appel.
    ///
    /// # Errors
    /// Index différent, fichier hors périmètre, conflit ou erreur de stockage.
    pub fn commit_review(
        &mut self,
        review: &crate::ReviewIndex,
        now: OffsetDateTime,
        provenance: Option<&Provenance>,
    ) -> Result<Diff, SfsError> {
        self.refresh_publication()?;
        if self.meta.state != WorkspaceState::Open {
            return Err(SfsError::BadState {
                state: self.meta.state,
            });
        }
        if *review != self.seal_review()?
            || review.entries.iter().any(|entry| {
                !self
                    .meta
                    .scopes
                    .iter()
                    .any(|scope| entry.path.starts_with(scope))
            })
        {
            return Err(SfsError::Io(std::io::Error::other(
                "Les versions examinées ou leur périmètre ont changé.",
            )));
        }
        let result =
            crate::publication::apply(&self.home, &self.meta.task, review, now, provenance);
        self.refresh_publication()?;
        if result.is_ok() {
            self.save()?;
        }
        result.map_err(Into::into)
    }

    /// Annule une publication dont les fichiers correspondent encore aux versions publiées.
    ///
    /// # Errors
    /// Journal absent, état incompatible, document retouché ou sauvegarde altérée.
    pub fn undo(&mut self) -> Result<Diff, SfsError> {
        self.refresh_publication()?;
        if self.meta.state != WorkspaceState::Committed {
            return Err(SfsError::BadState {
                state: self.meta.state,
            });
        }
        let result = crate::publication::undo(&self.home, &self.meta.task);
        self.refresh_publication()?;
        if result.is_ok() {
            self.save()?;
        }
        result.map_err(Into::into)
    }

    /// Reprend explicitement l'intention enregistrée après interruption, sans nouvel index.
    ///
    /// # Errors
    /// Journal absent, identités ambiguës, modification indépendante ou erreur de stockage.
    pub fn recover_publication(&mut self) -> Result<Diff, SfsError> {
        let result = crate::publication::recover(&self.home, &self.meta.task);
        self.refresh_publication()?;
        if result.is_ok() {
            self.save()?;
        }
        result.map_err(Into::into)
    }

    /// Abandonne l'espace de travail sans rien appliquer.
    ///
    /// # Erreurs
    /// Si l'état n'est pas `Open`, ou en cas d'erreur d'entrée-sortie.
    pub fn abandon(&mut self) -> Result<(), SfsError> {
        self.refresh_publication()?;
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
    /// API historique pour appelants de confiance : les chemins ne sont pas confinés ici.
    /// La préparation est privée, mais sa validation déplace les fichiers successivement et
    /// peut laisser un état partiel après interruption. Employer le moteur de publication
    /// journalisée pour les versions destinées aux documents humains.
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
    /// Les déplacements sont successifs, sans atomicité du lot ni journal de reprise.
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
