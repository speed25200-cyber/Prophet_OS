//! Publication de fichiers réguliers, avec conflits explicites et reprise par identité d'inode.
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read as _, Write as _};
use std::os::unix::fs::MetadataExt as _;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};

use rustix::fs::{self, FlockOperation, Mode, OFlags, RenameFlags, ResolveFlags};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::diff::{Diff, FileFingerprint};
use crate::publication_metadata::{Metadata, Pair};
use crate::review::{Entry, ReviewIndex, inspect, open, relative, task_root};
use crate::{Provenance, WorkspaceState};

const RECORD: &str = "publication.json";
const REVIEW: &str = "publication-review.json";
const MAX_RECORD: u64 = 8 * 1024 * 1024;
static NEXT_COPY: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Identity {
    device: u64,
    inode: u64,
}

impl Identity {
    fn from_file(file: &File) -> io::Result<Self> {
        let meta = file.metadata()?;
        Ok(Self {
            device: meta.dev(),
            inode: meta.ino(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Pending {
    original: Option<Identity>,
    proposed: Option<Identity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Journal {
    version: u8,
    task: String,
    #[serde(skip)]
    review: ReviewIndex,
    #[serde(skip)]
    metadata: Vec<Pair>,
    review_hash: String,
    pub(crate) state: WorkspaceState,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) committed: OffsetDateTime,
    undo: bool,
    next: usize,
    pending: Option<Pending>,
}

#[derive(Serialize, Deserialize)]
struct CheckedJournal<T> {
    cursor: T,
    checksum: String,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    review: ReviewIndex,
    metadata: Vec<Pair>,
}

fn digest<T: Serialize>(value: &T) -> io::Result<String> {
    Ok(blake3::hash(&serde_json::to_vec(value)?)
        .to_hex()
        .to_string())
}

fn save_journal(root: &File, journal: &Journal) -> io::Result<()> {
    // Seul le curseur borné est réécrit par fichier, pas l'index complet du lot.
    atomic_json(
        root,
        RECORD,
        &CheckedJournal {
            cursor: journal,
            checksum: digest(journal)?,
        },
    )
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

fn conflict(path: &Path) -> io::Error {
    invalid(format!(
        "Conflit sur {} : la version ou l'identité attendue a changé. Les fichiers déplacés restent conservés dans la publication.",
        path.display()
    ))
}

fn home_root(home: &Path) -> io::Result<File> {
    if !home.is_absolute() || home.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(invalid("Répertoire personnel invalide."));
    }
    let file: File = fs::openat2(
        fs::CWD,
        home,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::NO_SYMLINKS,
    )?
    .into();
    // Le verrou ne prétend pas empêcher les éditeurs ordinaires de modifier leurs fichiers.
    fs::flock(&file, FlockOperation::NonBlockingLockExclusive)?;
    Ok(file)
}

fn directory(root: &File, name: &str) -> io::Result<File> {
    directory_mode(root, name, Mode::RWXU)
}

fn directory_mode(root: &File, name: &str, mode: Mode) -> io::Result<File> {
    match fs::mkdirat(root, name, mode) {
        Ok(()) => root.sync_all()?,
        Err(rustix::io::Errno::EXIST) => {}
        Err(e) => return Err(e.into()),
    }
    open(root, Path::new(name), OFlags::RDONLY | OFlags::DIRECTORY)
}

fn parent(root: &File, path: &Path, create: bool, public: bool) -> io::Result<(File, OsString)> {
    if !relative(path)
        || path
            .components()
            .any(|c| matches!(c, Component::Normal(n) if n == ".prophet"))
    {
        return Err(invalid("Chemin de publication invalide."));
    }
    let mut current = root.try_clone()?;
    for component in path.parent().unwrap_or(Path::new("")).components() {
        let Component::Normal(name) = component else {
            return Err(invalid("Parent invalide."));
        };
        current = if create {
            directory_mode(
                &current,
                name.to_str().ok_or_else(|| invalid("Nom non UTF-8."))?,
                if public {
                    Mode::from_bits_truncate(0o777)
                } else {
                    Mode::RWXU
                },
            )?
        } else {
            open(
                &current,
                Path::new(name),
                OFlags::RDONLY | OFlags::DIRECTORY,
            )?
        };
    }
    Ok((
        current,
        path.file_name()
            .ok_or_else(|| invalid("Nom absent."))?
            .into(),
    ))
}

pub(crate) fn atomic_json<T: Serialize>(root: &File, name: &str, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() as u64 > MAX_RECORD {
        return Err(invalid("État trop grand pour être conservé."));
    }
    let temporary = format!("{name}.tmp");
    match open(root, Path::new(&temporary), OFlags::PATH) {
        Ok(file) => {
            let meta = file.metadata()?;
            if !meta.is_file() || meta.nlink() != 1 {
                return Err(invalid("État temporaire inattendu."));
            }
            fs::unlinkat(root, temporary.as_str(), fs::AtFlags::empty())?;
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let mut file: File = fs::openat2(
        root,
        temporary.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
        ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS,
    )?
    .into();
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::renameat(root, temporary.as_str(), root, name)?;
    root.sync_all()
}

pub(crate) fn read_json<T: serde::de::DeserializeOwned>(root: &File, name: &str) -> io::Result<T> {
    let file = open(root, Path::new(name), OFlags::RDONLY | OFlags::NONBLOCK)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.nlink() != 1 || meta.len() > MAX_RECORD {
        return Err(invalid("Fichier d'état invalide ou trop grand."));
    }
    let mut bytes = Vec::new();
    file.take(MAX_RECORD + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECORD {
        return Err(invalid("État trop grand."));
    }
    serde_json::from_slice(&bytes).map_err(Into::into)
}

fn validate(review: &ReviewIndex) -> io::Result<()> {
    if review.entries.len() > 10000 {
        return Err(invalid("Trop de changements."));
    }
    let mut previous = None;
    let mut bytes = 0_u64;
    for entry in &review.entries {
        if !relative(&entry.path)
            || entry.path.components().count() > 64
            || entry
                .path
                .components()
                .any(|c| matches!(c, Component::Normal(n) if n == ".prophet"))
            || previous.is_some_and(|path| path >= &entry.path)
            || entry.before == entry.after
        {
            return Err(invalid("Index de publication invalide."));
        }
        previous = Some(&entry.path);
        for fingerprint in [&entry.before, &entry.after].into_iter().flatten() {
            bytes = bytes.saturating_add(fingerprint.size);
            if bytes > 1024 * 1024 * 1024
                || fingerprint.mode & 0o170000 != 0o100000
                || fingerprint.mode & 0o7000 != 0
                || fingerprint.hash.len() != 64
                || !fingerprint.hash.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(invalid(
                    "Version ou permissions refusées pour la publication.",
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn read_journal(home: &Path, task: &str) -> io::Result<Option<Journal>> {
    let root = task_root(home, task)?;
    let checked: CheckedJournal<Journal> = match read_json(&root, RECORD) {
        Ok(value) => value,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if checked.checksum != digest(&checked.cursor)? {
        return Err(invalid("Empreinte du journal incorrecte."));
    }
    let mut journal = checked.cursor;
    let manifest: Manifest = read_json(&root, REVIEW)?;
    if journal.review_hash != digest(&manifest)? {
        return Err(invalid("Empreinte de l'index de publication incorrecte."));
    }
    journal.review = manifest.review;
    journal.metadata = manifest.metadata;
    validate(&journal.review)?;
    if journal.metadata.len() != journal.review.entries.len()
        || journal
            .review
            .entries
            .iter()
            .zip(&journal.metadata)
            .any(|(entry, metadata)| {
                entry.before.is_some() != metadata.before.is_some()
                    || entry.after.is_some() != metadata.after.is_some()
            })
    {
        return Err(invalid("Métadonnées de publication incohérentes."));
    }
    if journal.version != 1
        || journal.task != task
        || journal.next > journal.review.entries.len()
        || (journal.pending.is_some() && journal.next == journal.review.entries.len())
        || (matches!(
            journal.state,
            WorkspaceState::Committed | WorkspaceState::RolledBack
        ) && (journal.next != journal.review.entries.len() || journal.pending.is_some()))
        || !matches!(
            journal.state,
            WorkspaceState::Applying
                | WorkspaceState::Undoing
                | WorkspaceState::Committed
                | WorkspaceState::RolledBack
                | WorkspaceState::Conflict
        )
    {
        return Err(invalid("Journal de publication incohérent."));
    }
    Ok(Some(journal))
}

fn identity(root: &File, path: &Path) -> io::Result<Option<Identity>> {
    match open(root, path, OFlags::PATH) {
        Ok(file) => Identity::from_file(&file).map(Some),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn check(
    root: &File,
    path: &Path,
    expected: Option<&FileFingerprint>,
) -> io::Result<Option<Identity>> {
    match expected {
        Some(expected) => {
            let (actual, _) = inspect(root, path)?;
            if actual != *expected {
                return Err(conflict(path));
            }
            identity(root, path)
        }
        None => match identity(root, path)? {
            None => Ok(None),
            Some(_) => Err(conflict(path)),
        },
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CopyPolicy {
    Original,
    Proposed,
    Frozen,
}

fn copy_version(
    source: &File,
    path: &Path,
    target: &File,
    destination: &Path,
    expected: &FileFingerprint,
    metadata: &Metadata,
    policy: CopyPolicy,
) -> io::Result<()> {
    let verify_source = policy != CopyPolicy::Proposed;
    if verify_source {
        metadata.check(source, path)?;
    }
    let (parent, name) = parent(target, destination, true, false)?;
    let replace = identity(&parent, Path::new(&name))?.is_some();
    if replace {
        let prior = open(&parent, Path::new(&name), OFlags::PATH)?.metadata()?;
        if !prior.is_file() || prior.nlink() != 1 {
            return Err(conflict(destination));
        }
        let verified = check(&parent, Path::new(&name), Some(expected))
            .and_then(|_| metadata.check(&parent, Path::new(&name)));
        match verified {
            Ok(()) => return Ok(()),
            Err(error) if policy == CopyPolicy::Frozen => return Err(error),
            Err(_) => {}
        }
    }
    let mut from = open(source, path, OFlags::RDONLY | OFlags::NONBLOCK)?;
    let meta = from.metadata()?;
    if !meta.is_file() || meta.nlink() != 1 {
        return Err(conflict(path));
    }
    // Une copie interrompue ne porte jamais le nom de la version complète. Une reprise
    // crée un autre temporaire ; les temporaires orphelins ne sont jamais des originaux.
    let (mut to, temporary) = (0..32)
        .find_map(|_| {
            let temporary = format!(
                ".copy-{}-{}",
                std::process::id(),
                NEXT_COPY.fetch_add(1, Ordering::Relaxed)
            );
            match fs::openat2(
                &parent,
                temporary.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
                ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS,
            ) {
                Ok(fd) => Some(Ok((File::from(fd), temporary))),
                Err(rustix::io::Errno::EXIST) => None,
                Err(e) => Some(Err(io::Error::from(e))),
            }
        })
        .ok_or_else(|| invalid("Trop de fichiers temporaires de publication."))??;
    let count = io::copy(&mut (&mut from).take(expected.size + 1), &mut to)?;
    if count != expected.size {
        return Err(conflict(path));
    }
    if verify_source {
        metadata.check(source, path)?;
    }
    metadata.apply(&to, expected.mode)?;
    to.sync_all()?;
    checkpoint("copy-synced");
    check(&parent, Path::new(&temporary), Some(expected))?;
    fs::renameat_with(
        &parent,
        temporary.as_str(),
        &parent,
        &name,
        if replace {
            RenameFlags::EXCHANGE
        } else {
            RenameFlags::NOREPLACE
        },
    )?;
    parent.sync_all()?;
    check(target, destination, Some(expected))?;
    metadata.check(target, destination)?;
    Ok(())
}

fn versions(entry: &Entry, undo: bool) -> (Option<&FileFingerprint>, Option<&FileFingerprint>) {
    if undo {
        (entry.after.as_ref(), entry.before.as_ref())
    } else {
        (entry.before.as_ref(), entry.after.as_ref())
    }
}

fn preflight(
    home: &File,
    before: &File,
    after: &File,
    review: &ReviewIndex,
    metadata: &[Pair],
    undo: bool,
) -> io::Result<()> {
    for (entry, metadata) in review.entries.iter().zip(metadata) {
        let (expected, _) = versions(entry, undo);
        check(home, &entry.path, expected)?;
        if let Some(expected) = if undo {
            &metadata.after
        } else {
            &metadata.before
        } {
            expected.check(home, &entry.path)?;
        }
        if let Some(version) = &entry.before {
            check(before, &entry.path, Some(version))?;
            metadata
                .before
                .as_ref()
                .ok_or_else(|| invalid("Métadonnées initiales absentes."))?
                .check(before, &entry.path)?;
        }
        if let Some(version) = &entry.after {
            check(after, &entry.path, Some(version))?;
            metadata
                .after
                .as_ref()
                .ok_or_else(|| invalid("Métadonnées proposées absentes."))?
                .check(after, &entry.path)?;
        }
    }
    Ok(())
}

pub(crate) fn apply(
    home: &Path,
    task: &str,
    review: &ReviewIndex,
    now: OffsetDateTime,
    provenance: Option<&Provenance>,
) -> io::Result<Diff> {
    validate(review)?;
    let home_fd = home_root(home)?;
    let root = task_root(home, task)?;
    if read_journal(home, task)?.is_some() {
        return Err(invalid(
            "Une publication existe déjà. Consultez son état ou demandez sa reprise.",
        ));
    }
    let work = open(&root, Path::new("work"), OFlags::RDONLY | OFlags::DIRECTORY)?;
    // Tout le lot est vérifié avant de préparer ou modifier un original.
    for entry in &review.entries {
        check(&home_fd, &entry.path, entry.before.as_ref())?;
        check(&work, &entry.path, entry.after.as_ref())?;
    }
    let before = directory(&root, "restore")?;
    let after = directory(&root, "published")?;
    let mut metadata = Vec::with_capacity(review.entries.len());
    // Borner aussi l'accumulation en mémoire, avant de préparer toutes les copies du lot.
    let mut manifest_bytes = serde_json::to_vec(review)?.len() as u64 + 64;
    for entry in &review.entries {
        let initial = entry
            .before
            .as_ref()
            .map(|_| Metadata::read(&home_fd, &entry.path))
            .transpose()?;
        let proposed = entry
            .after
            .as_ref()
            .map(|version| {
                Metadata::proposed(
                    &home_fd,
                    &work,
                    &entry.path,
                    version.mode,
                    initial.as_ref(),
                    provenance,
                )
            })
            .transpose()?;
        let pair = Pair {
            before: initial,
            after: proposed,
        };
        manifest_bytes = manifest_bytes.saturating_add(serde_json::to_vec(&pair)?.len() as u64 + 1);
        if manifest_bytes > MAX_RECORD {
            return Err(invalid("Index et métadonnées trop grands."));
        }
        if let Some(version) = &entry.before {
            copy_version(
                &home_fd,
                &entry.path,
                &before,
                &entry.path,
                version,
                pair.before
                    .as_ref()
                    .ok_or_else(|| invalid("Métadonnées initiales absentes."))?,
                CopyPolicy::Original,
            )?;
        }
        if let Some(version) = &entry.after {
            copy_version(
                &work,
                &entry.path,
                &after,
                &entry.path,
                version,
                pair.after
                    .as_ref()
                    .ok_or_else(|| invalid("Métadonnées proposées absentes."))?,
                CopyPolicy::Proposed,
            )?;
        }
        metadata.push(pair);
    }
    preflight(&home_fd, &before, &after, review, &metadata, false)?;
    let manifest = Manifest {
        review: review.clone(),
        metadata,
    };
    if serde_json::to_vec(&manifest)?.len() as u64 > MAX_RECORD {
        return Err(invalid("Index et métadonnées trop grands."));
    }
    atomic_json(&root, REVIEW, &manifest)?;
    let mut journal = Journal {
        version: 1,
        task: task.into(),
        review: review.clone(),
        review_hash: digest(&manifest)?,
        metadata: manifest.metadata,
        state: WorkspaceState::Applying,
        committed: now,
        undo: false,
        next: 0,
        pending: None,
    };
    save_journal(&root, &journal)?;
    checkpoint("journal-start");
    run(&home_fd, &root, &before, &after, &mut journal)
}

pub(crate) fn undo(home: &Path, task: &str) -> io::Result<Diff> {
    let home_fd = home_root(home)?;
    let root = task_root(home, task)?;
    let mut journal = read_journal(home, task)?
        .ok_or_else(|| invalid("Aucun journal de publication vérifiable."))?;
    if journal.state != WorkspaceState::Committed {
        return Err(invalid(
            "Cette publication ne peut pas être annulée dans son état courant.",
        ));
    }
    let before = open(
        &root,
        Path::new("restore"),
        OFlags::RDONLY | OFlags::DIRECTORY,
    )?;
    let after = open(
        &root,
        Path::new("published"),
        OFlags::RDONLY | OFlags::DIRECTORY,
    )?;
    preflight(
        &home_fd,
        &before,
        &after,
        &journal.review,
        &journal.metadata,
        true,
    )?;
    journal.undo = true;
    journal.next = 0;
    journal.pending = None;
    journal.state = WorkspaceState::Undoing;
    save_journal(&root, &journal)?;
    checkpoint("journal-start");
    run(&home_fd, &root, &before, &after, &mut journal)
}

pub(crate) fn recover(home: &Path, task: &str) -> io::Result<Diff> {
    let home_fd = home_root(home)?;
    let root = task_root(home, task)?;
    let mut journal =
        read_journal(home, task)?.ok_or_else(|| invalid("Aucune publication à reprendre."))?;
    if matches!(
        journal.state,
        WorkspaceState::Committed | WorkspaceState::RolledBack
    ) {
        return Ok(journal.review.diff());
    }
    let before = open(
        &root,
        Path::new("restore"),
        OFlags::RDONLY | OFlags::DIRECTORY,
    )?;
    let after = open(
        &root,
        Path::new("published"),
        OFlags::RDONLY | OFlags::DIRECTORY,
    )?;
    run(&home_fd, &root, &before, &after, &mut journal)
}

fn run(
    home: &File,
    root: &File,
    before: &File,
    after: &File,
    journal: &mut Journal,
) -> io::Result<Diff> {
    while journal.next < journal.review.entries.len() {
        let result = step(home, root, before, after, journal);
        if let Err(error) = result {
            journal.state = WorkspaceState::Conflict;
            // Ne masquer ni une erreur de synchronisation ni l'état encore incertain sur disque.
            save_journal(root, journal)?;
            return Err(error);
        }
        journal.next += 1;
        journal.pending = None;
        save_journal(root, journal)?;
        checkpoint("step-recorded");
    }
    journal.state = if journal.undo {
        WorkspaceState::RolledBack
    } else {
        WorkspaceState::Committed
    };
    save_journal(root, journal)?;
    checkpoint("journal-finished");
    Ok(journal.review.diff())
}

fn step(
    home: &File,
    root: &File,
    before: &File,
    after: &File,
    journal: &mut Journal,
) -> io::Result<()> {
    let entry = &journal.review.entries[journal.next];
    let (expected, proposed) = versions(entry, journal.undo);
    let pair = &journal.metadata[journal.next];
    let (expected_metadata, proposed_metadata) = if journal.undo {
        (&pair.after, &pair.before)
    } else {
        (&pair.before, &pair.after)
    };
    let slot = format!(
        "{}-{}",
        if journal.undo { "undo" } else { "apply" },
        journal.next
    );
    let slots = directory(root, "displaced")?;
    let source = if journal.undo { before } else { after };
    if journal.pending.is_none() {
        let original = check(home, &entry.path, expected)?;
        if let Some(metadata) = expected_metadata {
            metadata.check(home, &entry.path)?;
        }
        if let Some(version) = proposed {
            copy_version(
                source,
                &entry.path,
                &slots,
                Path::new(&slot),
                version,
                proposed_metadata
                    .as_ref()
                    .ok_or_else(|| invalid("Métadonnées proposées absentes."))?,
                CopyPolicy::Frozen,
            )?;
        } else if identity(&slots, Path::new(&slot))?.is_some() {
            return Err(conflict(&entry.path));
        }
        checkpoint("slot-synced");
        journal.pending = Some(Pending {
            original,
            proposed: identity(&slots, Path::new(&slot))?,
        });
        save_journal(root, journal)?;
        checkpoint("intent-synced");
    }
    let pending = journal
        .pending
        .as_ref()
        .ok_or_else(|| invalid("Intention absente."))?;
    let (target, name) = parent(home, &entry.path, proposed.is_some(), true)?;
    let at_target = identity(&target, Path::new(&name))?;
    let at_slot = identity(&slots, Path::new(&slot))?;
    if at_target == pending.original && at_slot == pending.proposed {
        // Les empreintes sont recontrôlées, mais seule l'opération atomique conserve une
        // éventuelle édition arrivée entre ce contrôle et le changement de nom.
        check(&target, Path::new(&name), expected)?;
        check(&slots, Path::new(&slot), proposed)?;
        if let Some(metadata) = expected_metadata {
            metadata.check(&target, Path::new(&name))?;
        }
        if let Some(metadata) = proposed_metadata {
            metadata.check(&slots, Path::new(&slot))?;
        }
        checkpoint("before-rename");
        match (expected, proposed) {
            (Some(_), Some(_)) => {
                fs::renameat_with(&slots, slot.as_str(), &target, &name, RenameFlags::EXCHANGE)?
            }
            (None, Some(_)) => fs::renameat_with(
                &slots,
                slot.as_str(),
                &target,
                &name,
                RenameFlags::NOREPLACE,
            )?,
            (Some(_), None) => fs::renameat_with(
                &target,
                &name,
                &slots,
                slot.as_str(),
                RenameFlags::NOREPLACE,
            )?,
            (None, None) => return Err(invalid("Changement vide.")),
        }
        checkpoint("name-changed");
    } else if at_target != pending.proposed || at_slot != pending.original {
        return Err(conflict(&entry.path));
    }
    slots.sync_all()?;
    target.sync_all()?;
    checkpoint("directories-synced");
    if identity(&target, Path::new(&name))? != pending.proposed
        || identity(&slots, Path::new(&slot))? != pending.original
    {
        return Err(conflict(&entry.path));
    }
    check(&slots, Path::new(&slot), expected)?;
    check(&target, Path::new(&name), proposed)?;
    if let Some(metadata) = expected_metadata {
        metadata.check(&slots, Path::new(&slot))?;
    }
    if let Some(metadata) = proposed_metadata {
        metadata.check(&target, Path::new(&name))?;
    }
    Ok(())
}

// Point de synchronisation privé aux tests : aucun déclencheur n'est présent en production.
#[cfg(not(test))]
fn checkpoint(_: &str) {}

#[cfg(test)]
fn checkpoint(name: &str) {
    TEST_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook(name);
        }
    });
}

#[cfg(test)]
type TestHook = Box<dyn FnMut(&str)>;
#[cfg(test)]
thread_local! {
    static TEST_HOOK: std::cell::RefCell<Option<TestHook>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workspace;
    use std::os::unix::process::ExitStatusExt as _;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    fn prepared(home: &Path) -> Workspace {
        std::fs::create_dir(home.join("docs")).unwrap();
        std::fs::write(home.join("docs/a.txt"), "initial").unwrap();
        std::fs::write(home.join("docs/delete.txt"), "à restaurer").unwrap();
        let work =
            Workspace::begin(home, "killable", &["~/docs"], OffsetDateTime::now_utc()).unwrap();
        std::fs::write(work.workdir().join("docs/a.txt"), "proposition").unwrap();
        std::fs::remove_file(work.workdir().join("docs/delete.txt")).unwrap();
        std::fs::write(work.workdir().join("docs/new.txt"), "nouveau").unwrap();
        work
    }

    // Lancé uniquement par le parent ci-dessous, dans un home temporaire possédé par le test.
    #[test]
    fn process_child() {
        let Some(home) = std::env::var_os("PROPHET_TEST_PUBLICATION_HOME") else {
            return;
        };
        let home = std::path::PathBuf::from(home);
        let stop = std::env::var("PROPHET_TEST_PUBLICATION_STOP").unwrap();
        let undo = std::env::var("PROPHET_TEST_PUBLICATION_UNDO").unwrap() == "1";
        let marker = home.join("checkpoint");
        let mut started = false;
        TEST_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |name| {
                if name == "journal-start" {
                    started = true;
                }
                if started && name == stop {
                    std::fs::write(&marker, name).unwrap();
                    loop {
                        std::thread::park_timeout(Duration::from_secs(1));
                    }
                }
            }))
        });
        let mut work = Workspace::open(&home, "killable").unwrap();
        if undo {
            work.undo().unwrap();
        } else {
            work.commit(OffsetDateTime::now_utc(), None).unwrap();
        }
    }

    #[test]
    fn reprise_apres_sigkill_a_chaque_frontiere_de_publication_et_annulation() {
        let checkpoints = [
            "journal-start",
            "copy-synced",
            "slot-synced",
            "intent-synced",
            "before-rename",
            "name-changed",
            "directories-synced",
            "step-recorded",
            "journal-finished",
        ];
        for undo in [false, true] {
            for checkpoint in checkpoints {
                let home = tempfile::tempdir().unwrap();
                let mut work = prepared(home.path());
                if undo {
                    work.commit(OffsetDateTime::now_utc(), None).unwrap();
                }
                drop(work);
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "publication::tests::process_child",
                        "--nocapture",
                    ])
                    .env("PROPHET_TEST_PUBLICATION_HOME", home.path())
                    .env("PROPHET_TEST_PUBLICATION_STOP", checkpoint)
                    .env(
                        "PROPHET_TEST_PUBLICATION_UNDO",
                        if undo { "1" } else { "0" },
                    )
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap();
                let deadline = Instant::now() + Duration::from_secs(20);
                while !home.path().join("checkpoint").exists() {
                    if let Some(status) = child.try_wait().unwrap() {
                        panic!("enfant terminé avant {checkpoint}, undo={undo}: {status}");
                    }
                    if Instant::now() >= deadline {
                        child.kill().unwrap();
                        child.wait().unwrap();
                        panic!("point {checkpoint} non atteint, undo={undo}");
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                child.kill().unwrap();
                assert_eq!(child.wait().unwrap().signal(), Some(9));
                let mut recovered = Workspace::open(home.path(), "killable").unwrap();
                recovered
                    .recover_publication()
                    .unwrap_or_else(|error| panic!("{checkpoint}, undo={undo}: {error}"));
                if undo {
                    assert_eq!(recovered.state(), WorkspaceState::RolledBack);
                    assert_eq!(
                        std::fs::read_to_string(home.path().join("docs/a.txt")).unwrap(),
                        "initial"
                    );
                    assert_eq!(
                        std::fs::read_to_string(home.path().join("docs/delete.txt")).unwrap(),
                        "à restaurer"
                    );
                    assert!(!home.path().join("docs/new.txt").exists());
                } else {
                    assert_eq!(recovered.state(), WorkspaceState::Committed);
                    assert_eq!(
                        std::fs::read_to_string(home.path().join("docs/a.txt")).unwrap(),
                        "proposition"
                    );
                    assert!(!home.path().join("docs/delete.txt").exists());
                    assert_eq!(
                        std::fs::read_to_string(home.path().join("docs/new.txt")).unwrap(),
                        "nouveau"
                    );
                }
                std::fs::write(home.path().join("docs/a.txt"), "édition après reprise").unwrap();
                recovered.recover_publication().unwrap();
                assert_eq!(
                    std::fs::read_to_string(home.path().join("docs/a.txt")).unwrap(),
                    "édition après reprise"
                );
            }
        }
    }

    #[test]
    fn un_echange_concurrent_conserve_les_octets_inattendus_et_arrete_le_lot() {
        let home = tempfile::tempdir().unwrap();
        let mut work = prepared(home.path());
        let target = home.path().join("docs/a.txt");
        let mut changed = false;
        TEST_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |name| {
                if name == "before-rename" && !changed {
                    std::fs::write(&target, "édition au dernier instant").unwrap();
                    changed = true;
                }
            }))
        });
        let result = work.commit(OffsetDateTime::now_utc(), None);
        TEST_HOOK.with(|hook| *hook.borrow_mut() = None);
        assert!(result.is_err());
        assert_eq!(work.state(), WorkspaceState::Conflict);
        assert_eq!(
            std::fs::read_to_string(
                home.path()
                    .join(".prophet/tasks/killable/displaced/apply-0")
            )
            .unwrap(),
            "édition au dernier instant"
        );
        assert_eq!(
            std::fs::read_to_string(home.path().join("docs/delete.txt")).unwrap(),
            "à restaurer"
        );
        assert!(!home.path().join("docs/new.txt").exists());
        std::fs::write(
            home.path().join("docs/a.txt"),
            "nouvelle édition après conflit",
        )
        .unwrap();
        assert!(work.recover_publication().is_err());
        assert_eq!(
            std::fs::read_to_string(home.path().join("docs/a.txt")).unwrap(),
            "nouvelle édition après conflit"
        );
    }

    #[test]
    fn un_ajout_concurrent_ne_peut_pas_etre_remplace() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("docs")).unwrap();
        let mut work = Workspace::begin(
            home.path(),
            "killable",
            &["~/docs"],
            OffsetDateTime::now_utc(),
        )
        .unwrap();
        std::fs::write(work.workdir().join("docs/new.txt"), "proposition").unwrap();
        let target = home.path().join("docs/new.txt");
        TEST_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |name| {
                if name == "before-rename" {
                    std::fs::write(&target, "création concurrente").unwrap();
                }
            }))
        });
        let result = work.commit(OffsetDateTime::now_utc(), None);
        TEST_HOOK.with(|hook| *hook.borrow_mut() = None);
        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(home.path().join("docs/new.txt")).unwrap(),
            "création concurrente"
        );
        assert!(work.recover_publication().is_err());
    }

    #[test]
    fn une_publication_cooperante_exclut_les_autres() {
        let home = tempfile::tempdir().unwrap();
        let mut work = prepared(home.path());
        let guard = home_root(home.path()).unwrap();
        assert!(work.commit(OffsetDateTime::now_utc(), None).is_err());
        assert_eq!(work.state(), WorkspaceState::Open);
        drop(guard);
        work.commit(OffsetDateTime::now_utc(), None).unwrap();
    }

    #[test]
    fn une_preparation_refusee_peut_etre_recommencee_sans_reutiliser_des_metadonnees_perimees() {
        let home = tempfile::tempdir().unwrap();
        let mut work = prepared(home.path());
        let target = home.path().join("docs/a.txt");
        let mut copies = 0;
        TEST_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |name| {
                if name == "copy-synced" {
                    copies += 1;
                    if copies == 2 {
                        std::fs::write(&target, "édition pendant la préparation").unwrap();
                    }
                }
            }))
        });
        let result = work.commit(OffsetDateTime::now_utc(), None);
        TEST_HOOK.with(|hook| *hook.borrow_mut() = None);
        assert!(result.is_err());
        assert_eq!(work.state(), WorkspaceState::Open);
        std::fs::write(home.path().join("docs/a.txt"), "initial").unwrap();
        work.commit(OffsetDateTime::now_utc(), None).unwrap();
        work.undo().unwrap();
        assert_eq!(
            std::fs::read_to_string(home.path().join("docs/a.txt")).unwrap(),
            "initial"
        );
    }
}
