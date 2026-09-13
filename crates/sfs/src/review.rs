//! Versions exactes et lectures bornées pour l'examen humain d'une mission terminée.
use std::collections::BTreeSet;
use std::fs::File;
use std::io::{self, Read as _};
use std::os::unix::fs::MetadataExt as _;
use std::path::{Component, Path, PathBuf};

use rustix::fs::{self, Mode, OFlags, ResolveFlags};
use serde::{Deserialize, Serialize};

use crate::diff::{Change, ChangeKind, Diff, FileFingerprint, Fingerprints};

const MAX_BYTES: u64 = 512 * 1024 * 1024;
const PREVIEW_BYTES: u64 = 65536;
const FLAGS: ResolveFlags = ResolveFlags::BENEATH
    .union(ResolveFlags::NO_SYMLINKS)
    .union(ResolveFlags::NO_XDEV);

/// Contenu intégral borné, ou raison explicite de l'absence d'aperçu textuel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PreviewContent {
    /// Texte UTF-8 complet, sans remplacement de caractères ni troncature.
    Text {
        /// Octets UTF-8 conservés, retours à la ligne compris.
        text: String,
    },
    /// Fichier non UTF-8 ou contenant des octets nuls.
    Binary,
    /// Contenu dépassant la limite d'aperçu de 64 Kio.
    TooLarge,
}

/// Une version vérifiée contre l'empreinte enregistrée lors de la mission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePreview {
    /// Empreinte BLAKE3 des octets complets.
    pub hash: String,
    /// Taille complète en octets.
    pub size: u64,
    /// Mode Unix de la version, indépendant des permissions de sa copie privée.
    pub mode: u32,
    /// Texte complet ou absence explicitement typée.
    pub content: PreviewContent,
}

/// Deux versions issues de la même mission, sans lecture des originaux actuels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReview {
    /// Chemin relatif au home capturé.
    pub path: PathBuf,
    /// Nature du changement.
    pub kind: ChangeKind,
    /// Version capturée au début ; absente pour un ajout.
    pub before: Option<FilePreview>,
    /// Proposition enregistrée à la fin ; absente pour une suppression.
    pub after: Option<FilePreview>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub(crate) path: PathBuf,
    pub(crate) before: Option<FileFingerprint>,
    pub(crate) after: Option<FileFingerprint>,
}

impl Entry {
    fn kind(&self) -> ChangeKind {
        match (&self.before, &self.after) {
            (None, _) => ChangeKind::Added,
            (_, None) => ChangeKind::Deleted,
            _ => ChangeKind::Modified,
        }
    }
}

/// Index de fin de mission à conserver avec le résultat, hors de l'espace modifiable par l'agent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewIndex {
    pub(crate) entries: Vec<Entry>,
}

fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}

pub(crate) fn relative(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|p| matches!(p, Component::Normal(_)))
}

pub(crate) fn open(root: &File, path: &Path, flags: OFlags) -> io::Result<File> {
    fs::openat2(
        root,
        path,
        flags | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
        FLAGS,
    )
    .map(File::from)
    .map_err(Into::into)
}

pub(crate) fn task_root(home: &Path, task: &str) -> io::Result<File> {
    if !home.is_absolute()
        || !relative(Path::new(task))
        || Path::new(task).components().count() != 1
    {
        return Err(invalid("Référence de travail invalide."));
    }
    let home: File = fs::openat2(
        fs::CWD,
        home,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::NO_SYMLINKS,
    )?
    .into();
    open(
        &home,
        &Path::new(".prophet/tasks").join(task),
        OFlags::RDONLY | OFlags::DIRECTORY,
    )
}

pub(crate) fn inspect(root: &File, path: &Path) -> io::Result<(FileFingerprint, PreviewContent)> {
    let mut file = open(root, path, OFlags::RDONLY | OFlags::NONBLOCK)?;
    let initial = file.metadata()?;
    if !initial.is_file() || initial.nlink() != 1 || initial.len() > MAX_BYTES {
        return Err(invalid(
            "Type, lien ou taille de fichier refusé pour l'examen.",
        ));
    }
    let mut hash = blake3::Hasher::new();
    let mut preview = Some(Vec::new());
    let mut size = 0_u64;
    let mut buffer = [0_u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        if size > MAX_BYTES {
            return Err(invalid("Fichier trop grand pour être vérifié."));
        }
        hash.update(&buffer[..n]);
        if size > PREVIEW_BYTES {
            preview = None;
        }
        if let Some(bytes) = &mut preview {
            bytes.extend_from_slice(&buffer[..n]);
        }
    }
    let end = file.metadata()?;
    if size != initial.len()
        || end.len() != initial.len()
        || end.mode() != initial.mode()
        || end.nlink() != 1
        || end.mtime() != initial.mtime()
        || end.mtime_nsec() != initial.mtime_nsec()
        || end.ctime() != initial.ctime()
        || end.ctime_nsec() != initial.ctime_nsec()
    {
        return Err(invalid(
            "Le fichier a changé pendant sa lecture. Actualisez l'examen.",
        ));
    }
    let content = match preview {
        None => PreviewContent::TooLarge,
        Some(bytes) if bytes.contains(&0) => PreviewContent::Binary,
        Some(bytes) => match String::from_utf8(bytes) {
            Ok(text) => PreviewContent::Text { text },
            Err(_) => PreviewContent::Binary,
        },
    };
    Ok((
        FileFingerprint {
            hash: hash.finalize().to_hex().to_string(),
            size,
            mode: initial.mode(),
        },
        content,
    ))
}

pub(crate) fn seal(home: &Path, task: &str, base: &Fingerprints) -> io::Result<ReviewIndex> {
    let root = task_root(home, task)?;
    open(&root, Path::new("base"), OFlags::RDONLY | OFlags::DIRECTORY)?;
    let work = open(&root, Path::new("work"), OFlags::RDONLY | OFlags::DIRECTORY)?;
    let mut paths = vec![PathBuf::from(".")];
    let mut current = Fingerprints::new();
    let mut visited = 0;
    let mut bytes = 0_u64;
    while let Some(path) = paths.pop() {
        visited += 1;
        if visited > 10000 || path.components().count() > 64 {
            return Err(invalid("Travail trop grand pour l'examen."));
        }
        let file = open(&work, &path, OFlags::PATH)?;
        let meta = file.metadata()?;
        if meta.is_dir() {
            for entry in fs::Dir::new(open(&work, &path, OFlags::RDONLY | OFlags::DIRECTORY)?)? {
                let entry = entry?;
                let name = entry
                    .file_name()
                    .to_str()
                    .map_err(|_| invalid("Nom de fichier non UTF-8."))?;
                if matches!(name, "." | "..") {
                    continue;
                }
                if visited + paths.len() >= 10000 {
                    return Err(invalid("Trop de fichiers à examiner."));
                }
                paths.push(if path == Path::new(".") {
                    PathBuf::from(name)
                } else {
                    path.join(name)
                });
            }
        } else {
            let (fingerprint, _) = inspect(&work, &path)?;
            bytes = bytes.saturating_add(fingerprint.size);
            if bytes > MAX_BYTES {
                return Err(invalid("Travail trop grand pour l'examen."));
            }
            current.insert(path, fingerprint);
        }
    }
    let keys: BTreeSet<_> = base.keys().chain(current.keys()).collect();
    Ok(ReviewIndex {
        entries: keys
            .into_iter()
            .filter_map(|path| {
                let before = base.get(path).cloned();
                let after = current.get(path).cloned();
                (before != after).then(|| Entry {
                    path: path.clone(),
                    before,
                    after,
                })
            })
            .collect(),
    })
}

impl ReviewIndex {
    /// Métadonnées correspondant aux mêmes versions que les aperçus.
    #[must_use]
    pub fn diff(&self) -> Diff {
        Diff {
            changes: self
                .entries
                .iter()
                .map(|entry| Change {
                    path: entry.path.clone(),
                    kind: entry.kind(),
                    size_before: entry.before.as_ref().map(|v| v.size),
                    size_after: entry.after.as_ref().map(|v| v.size),
                })
                .collect(),
        }
    }

    /// Lit un fichier de l'index et vérifie les empreintes avant de livrer le texte.
    ///
    /// # Errors
    /// Chemin inconnu, version modifiée, lien, fichier spécial ou capture historique absente.
    pub fn read(&self, home: &Path, task: &str, path: &str) -> io::Result<FileReview> {
        if !relative(Path::new(path)) || path.split('/').any(|p| matches!(p, "" | "." | "..")) {
            return Err(invalid("Chemin d'examen invalide."));
        }
        let entry = self
            .entries
            .iter()
            .find(|e| e.path == Path::new(path))
            .ok_or_else(|| invalid("Fichier absent des changements de cette mission."))?;
        let root = task_root(home, task)?;
        let base = open(&root, Path::new("base"), OFlags::RDONLY | OFlags::DIRECTORY)?;
        let work = open(&root, Path::new("work"), OFlags::RDONLY | OFlags::DIRECTORY)?;
        let version = |directory: &File, expected: &FileFingerprint, compare_mode| {
            let (actual, content) = inspect(directory, &entry.path)?;
            if actual.hash != expected.hash
                || actual.size != expected.size
                || (compare_mode && actual.mode != expected.mode)
            {
                return Err(invalid(
                    "Le fichier a changé depuis la mission. Cet aperçu ne peut plus être confirmé.",
                ));
            }
            Ok(FilePreview {
                hash: expected.hash.clone(),
                size: expected.size,
                mode: expected.mode,
                content,
            })
        };
        let before = entry
            .before
            .as_ref()
            .map(|v| version(&base, v, false))
            .transpose()?;
        let after = entry
            .after
            .as_ref()
            .map(|v| version(&work, v, true))
            .transpose()?;
        if entry.after.is_none() {
            match open(&work, &entry.path, OFlags::PATH) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                _ => return Err(invalid("Le fichier supprimé est réapparu dans le travail.")),
            }
        }
        Ok(FileReview {
            path: entry.path.clone(),
            kind: entry.kind(),
            before,
            after,
        })
    }
}
