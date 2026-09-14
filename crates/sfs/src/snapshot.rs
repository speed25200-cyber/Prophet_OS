//! Capture bornée avec des ouvertures relatives et un contrôle de droit par fichier.
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use crate::diff::{FileFingerprint, Fingerprints};
use rustix::fs::{self, Mode, OFlags, ResolveFlags};

const FLAGS: ResolveFlags = ResolveFlags::BENEATH
    .union(ResolveFlags::NO_SYMLINKS)
    .union(ResolveFlags::NO_XDEV);
const MAX_BYTES: u64 = 512 * 1024 * 1024;

fn denied() -> std::io::Error {
    std::io::Error::other("capture SFS : chemin, type ou droit refusé")
}

fn open(root: &File, path: &Path, flags: OFlags) -> std::io::Result<File> {
    fs::openat2(
        root,
        if path.as_os_str().is_empty() {
            Path::new(".")
        } else {
            path
        },
        flags | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
        FLAGS,
    )
    .map(File::from)
    .map_err(Into::into)
}

fn directory(root: &File, name: &str) -> std::io::Result<File> {
    match fs::mkdirat(root, name, Mode::RWXU) {
        Ok(()) | Err(rustix::io::Errno::EXIST) => {}
        Err(e) => return Err(e.into()),
    }
    open(root, Path::new(name), OFlags::RDONLY | OFlags::DIRECTORY)
}

pub(crate) fn capture(
    home: &Path,
    task: &str,
    scopes: &[PathBuf],
    permits: &dyn Fn(&Path) -> bool,
    source_root: Option<&Path>,
) -> std::io::Result<Fingerprints> {
    if !home.is_absolute()
        || home.components().any(|c| matches!(c, Component::ParentDir))
        || task.is_empty()
        || Path::new(task).components().count() != 1
        || !matches!(
            Path::new(task).components().next(),
            Some(Component::Normal(_))
        )
    {
        return Err(denied());
    }
    let source: File = fs::openat2(
        fs::CWD,
        home,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::NO_SYMLINKS,
    )?
    .into();
    // L'espace de travail d'une mission parente, s'il y en a un (ADR 0039) : la capture y lit
    // d'abord, et retombe sur le répertoire personnel pour un périmètre que le parent n'a pas.
    // Les droits, eux, se jugent toujours sur les chemins du répertoire personnel.
    let parent: Option<File> = match source_root {
        Some(root) => {
            if !root.is_absolute() || root.components().any(|c| matches!(c, Component::ParentDir)) {
                return Err(denied());
            }
            Some(
                fs::openat2(
                    fs::CWD,
                    root,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                    Mode::empty(),
                    ResolveFlags::NO_SYMLINKS,
                )?
                .into(),
            )
        }
        None => None,
    };
    let state = directory(&source, ".prophet")?;
    let tasks = directory(&state, "tasks")?;
    // Un identifiant existant n'est jamais réutilisé ni remplacé pendant une capture.
    fs::mkdirat(&tasks, task, Mode::RWXU)?;
    let task_root = open(&tasks, Path::new(task), OFlags::RDONLY | OFlags::DIRECTORY)?;
    let work = directory(&task_root, "work")?;
    let original = directory(&task_root, "base")?;
    directory(&task_root, "restore")?;
    let mut base = Fingerprints::new();
    let mut pending: Vec<_> = scopes
        .iter()
        .map(|p| (p.clone(), 0_u32, parent.is_some()))
        .collect();
    let mut visited = 0;
    let mut bytes = 0_u64;
    while let Some((relative, depth, du_parent)) = pending.pop() {
        visited += 1;
        if visited > 10000 || depth > 64 {
            return Err(std::io::Error::other("capture SFS trop grande"));
        }
        if relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
            || relative
                .components()
                .any(|c| matches!(c,Component::Normal(n) if n==".prophet"))
        {
            return Err(denied());
        }
        if !permits(&home.join(&relative)) {
            return Err(denied());
        }
        let racine_de = |du_parent: bool| -> &File {
            match (&parent, du_parent) {
                (Some(p), true) => p,
                _ => &source,
            }
        };
        let (from, du_parent) = match open(racine_de(du_parent), &relative, OFlags::PATH) {
            Ok(f) => (f, du_parent),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && du_parent && depth == 0 => {
                // Le parent n'a pas ce périmètre : il vient du répertoire personnel.
                match open(&source, &relative, OFlags::PATH) {
                    Ok(f) => (f, false),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        let racine = racine_de(du_parent);
        let meta = from.metadata()?;
        let mut target_parent = work.try_clone()?;
        let mut original_parent = original.try_clone()?;
        for component in relative.parent().unwrap_or(Path::new("")).components() {
            let Component::Normal(name) = component else {
                return Err(denied());
            };
            target_parent = directory(&target_parent, name.to_str().ok_or_else(denied)?)?;
            original_parent = directory(&original_parent, name.to_str().ok_or_else(denied)?)?;
        }
        let name = relative.file_name().and_then(|n| n.to_str());
        if meta.is_dir() {
            if let Some(name) = name {
                directory(&target_parent, name)?;
                directory(&original_parent, name)?;
            }
            let dir = open(racine, &relative, OFlags::RDONLY | OFlags::DIRECTORY)?;
            for entry in fs::Dir::new(dir)? {
                if visited + pending.len() > 10000 {
                    return Err(std::io::Error::other("capture SFS trop grande"));
                }
                let entry = entry?;
                let name = entry.file_name().to_str().map_err(|_| denied())?;
                if matches!(name, "." | ".." | ".prophet") {
                    continue;
                }
                pending.push((relative.join(name), depth + 1, du_parent));
            }
        } else if meta.is_file() && meta.nlink() == 1 {
            let mut from = open(racine, &relative, OFlags::RDONLY | OFlags::NONBLOCK)?;
            let meta = from.metadata()?;
            if !meta.is_file() || meta.nlink() != 1 || bytes.saturating_add(meta.len()) > MAX_BYTES
            {
                return Err(denied());
            }
            let mut to: File = fs::openat2(
                &target_parent,
                Path::new(name.ok_or_else(denied)?),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
                FLAGS,
            )?
            .into();
            let mut hash = blake3::Hasher::new();
            let mut saved: File = fs::openat2(
                &original_parent,
                Path::new(name.ok_or_else(denied)?),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
                FLAGS,
            )?
            .into();
            let mut buffer = [0_u8; 65536];
            let mut size = 0_u64;
            loop {
                if !permits(&home.join(&relative)) {
                    return Err(denied());
                }
                let n = from.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                bytes = bytes.saturating_add(n as u64);
                if bytes > MAX_BYTES {
                    return Err(std::io::Error::other("capture SFS trop grande"));
                }
                hash.update(&buffer[..n]);
                to.write_all(&buffer[..n])?;
                saved.write_all(&buffer[..n])?;
                size += n as u64;
            }
            fs::fchmod(&to, Mode::from_bits_truncate(meta.mode() & 0o777))?;
            to.sync_all()?;
            saved.sync_all()?;
            // Une source modifiée pendant sa copie demande une nouvelle capture.
            let after = from.metadata()?;
            if meta.len() != size
                || meta.mtime() != after.mtime()
                || meta.mtime_nsec() != after.mtime_nsec()
                || meta.ctime() != after.ctime()
                || meta.ctime_nsec() != after.ctime_nsec()
            {
                return Err(std::io::Error::other("source modifiée pendant la capture"));
            }
            base.insert(
                relative,
                FileFingerprint {
                    hash: hash.finalize().to_hex().to_string(),
                    size,
                    mode: meta.mode(),
                },
            );
        } else {
            return Err(denied());
        }
    }
    work.sync_all()?;
    original.sync_all()?;
    Ok(base)
}
