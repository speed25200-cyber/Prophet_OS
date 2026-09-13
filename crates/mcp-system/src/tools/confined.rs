//! Accès Linux ancrés sur des descripteurs. Aucune canonicalisation suivie d'une réouverture.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::{File, Metadata};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use prophet_types::cap::Act;
use rand::RngCore;
use rustix::fs::{self, AtFlags, Mode, OFlags, ResolveFlags};
use serde_json::{Value, json};

use crate::protocol::{CallResult, ErrorCode};
use crate::registry::{ResourceAccess, ToolContext};

pub(super) const MAX_READ: usize = 256 * 1024;
pub(super) const MAX_WRITE: usize = 1024 * 1024;
const MAX_VISITED: usize = 10_000;
const MAX_SCAN: usize = 8 * 1024 * 1024;
const MAX_RESULTS_BYTES: usize = 512 * 1024;
const RESOLVE: ResolveFlags = ResolveFlags::BENEATH
    .union(ResolveFlags::NO_SYMLINKS)
    .union(ResolveFlags::NO_XDEV);

type Result<T> = std::result::Result<T, Failure>;

#[derive(Debug)]
struct Failure(ErrorCode, String);

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        let code = match error
            .raw_os_error()
            .map(rustix::io::Errno::from_raw_os_error)
        {
            Some(rustix::io::Errno::NOENT) => ErrorCode::NotFound,
            Some(
                rustix::io::Errno::LOOP
                | rustix::io::Errno::XDEV
                | rustix::io::Errno::ACCESS
                | rustix::io::Errno::PERM
                | rustix::io::Errno::AGAIN,
            ) => ErrorCode::PolicyDenied,
            Some(rustix::io::Errno::NOSYS | rustix::io::Errno::NOTSUP) => ErrorCode::SandboxError,
            _ => ErrorCode::Internal,
        };
        Self(code, format!("accès fichiers : {error}"))
    }
}

impl From<rustix::io::Errno> for Failure {
    fn from(error: rustix::io::Errno) -> Self {
        std::io::Error::from(error).into()
    }
}

fn denied() -> Failure {
    Failure(
        ErrorCode::PolicyDenied,
        "chemin ou type de fichier non autorisé".into(),
    )
}
fn invalid(message: &str) -> Failure {
    Failure(ErrorCode::Invalid, message.into())
}

fn private(relative: &Path) -> bool {
    relative.components().any(|c| matches!(c, Component::Normal(n) if n == ".prophet" || n.as_bytes().starts_with(b".prophet-write-")))
}

pub(super) fn relative(raw: &str, context: &ToolContext) -> Option<PathBuf> {
    if raw.is_empty() || raw.len() > 4096 || raw.contains('\0') {
        return None;
    }
    let home = Path::new(&context.home);
    if !home.is_absolute() || home.components().any(|c| matches!(c, Component::ParentDir)) {
        return None;
    }
    let path = if raw == "~" {
        Path::new("")
    } else if let Some(rest) = raw.strip_prefix("~/") {
        Path::new(rest)
    } else if Path::new(raw).is_absolute() {
        Path::new(raw).strip_prefix(home).ok()?
    } else {
        Path::new(raw)
    };
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => result.push(name),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if private(&result) {
        return None;
    }
    Some(result)
}

pub(super) fn target(raw: &str, context: &ToolContext) -> Option<String> {
    relative(raw, context).map(|p| Path::new(&context.home).join(p).display().to_string())
}

struct View<'a> {
    home: File,
    work: File,
    logical: PathBuf,
    access: &'a dyn ResourceAccess,
}

fn open_root(path: &Path) -> Result<File> {
    Ok(fs::openat2(
        fs::CWD,
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::NO_SYMLINKS,
    )?
    .into())
}

fn open(root: &File, relative: &Path, flags: OFlags) -> std::io::Result<File> {
    let nonblock = if flags.contains(OFlags::PATH) {
        OFlags::empty()
    } else {
        OFlags::NONBLOCK
    };
    fs::openat2(
        root,
        if relative.as_os_str().is_empty() {
            Path::new(".")
        } else {
            relative
        },
        flags | OFlags::CLOEXEC | OFlags::NOFOLLOW | nonblock,
        Mode::empty(),
        RESOLVE,
    )
    .map(File::from)
    .map_err(Into::into)
}

impl<'a> View<'a> {
    fn new(context: &ToolContext, access: &'a dyn ResourceAccess) -> Result<Self> {
        let task = Path::new(&context.task);
        if task.components().count() != 1
            || !matches!(task.components().next(), Some(Component::Normal(_)))
        {
            return Err(denied());
        }
        let home = Path::new(&context.home);
        let expected = home.join(".prophet/tasks").join(task).join("work");
        if Path::new(&context.workdir) != expected {
            return Err(denied());
        }
        let home_file = open_root(home)?;
        let work = open_root(&expected)?;
        let a = home_file.metadata()?;
        let b = work.metadata()?;
        if (a.dev(), a.ino()) == (b.dev(), b.ino()) {
            return Err(denied());
        }
        Ok(Self {
            home: home_file,
            work,
            logical: home.to_owned(),
            access,
        })
    }

    fn permits(&self, act: Act, relative: &Path) -> bool {
        !private(relative)
            && self
                .access
                .permits(act, &self.logical.join(relative).display().to_string())
    }

    fn selected(&self, relative: &Path, flags: OFlags) -> Result<File> {
        match open(&self.work, relative, flags) {
            Ok(file) => Ok(file),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(open(&self.home, relative, flags)?)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn metadata(&self, relative: &Path) -> Result<Metadata> {
        let file = self.selected(relative, OFlags::PATH)?;
        let metadata = file.metadata()?;
        if (!metadata.is_file() && !metadata.is_dir())
            || (metadata.is_file() && metadata.nlink() != 1)
        {
            return Err(denied());
        }
        Ok(metadata)
    }

    fn read(&self, relative: &Path, max: usize) -> Result<(String, u64, bool, usize)> {
        if !self.permits(Act::Read, relative) {
            return Err(denied());
        }
        let file = self.selected(relative, OFlags::RDONLY)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(denied());
        }
        let mut bytes = Vec::with_capacity(max.min(metadata.len() as usize).saturating_add(1));
        file.take(max as u64 + 1).read_to_end(&mut bytes)?;
        let read = bytes.len();
        let total = metadata.len().max(read as u64);
        let mut text = String::from_utf8_lossy(&bytes[..bytes.len().min(max)]).into_owned();
        let truncated = total > max as u64 || text.len() > max;
        if text.len() > max {
            let mut end = max;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        Ok((text, total, truncated, read))
    }

    fn names(&self, relative: &Path, budget: &mut Budget) -> Result<Vec<PathBuf>> {
        let mut names = BTreeSet::new();
        // Toute erreur autre qu'une absence interrompt la fusion. Un lien de travail ne
        // doit jamais se transformer en repli vers une version plus privilégiée.
        let mut found = false;
        for root in [&self.work, &self.home] {
            let directory = match open(root, relative, OFlags::RDONLY | OFlags::DIRECTORY) {
                Ok(directory) => directory,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            found = true;
            for entry in fs::Dir::new(directory)? {
                if !budget.visit() {
                    break;
                }
                let entry = entry?;
                let name = OsStr::from_bytes(entry.file_name().to_bytes());
                if name == "." || name == ".." {
                    continue;
                }
                // Les arguments sont UTF-8 : ne pas rendre un nom impossible à rouvrir sans
                // perte, ni fusionner deux noms distincts par remplacement de caractères.
                if name.to_str().is_none() {
                    continue;
                }
                let child = relative.join(name);
                if !private(&child) {
                    names.insert(child);
                }
            }
        }
        if !found {
            return Err(Failure(ErrorCode::NotFound, "répertoire absent".into()));
        }
        Ok(names.into_iter().collect())
    }

    fn write(&self, relative: &Path, content: &str) -> Result<()> {
        if !self.permits(Act::Write, relative) {
            return Err(denied());
        }
        let name = relative.file_name().ok_or_else(denied)?;
        let parent = relative.parent().ok_or_else(denied)?;
        let mut prefix = PathBuf::new();
        for component in parent.components() {
            let Component::Normal(part) = component else {
                return Err(denied());
            };
            let directory = open(&self.work, &prefix, OFlags::RDONLY | OFlags::DIRECTORY)?;
            match fs::mkdirat(&directory, part, Mode::RWXU) {
                Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                Err(error) => return Err(error.into()),
            }
            prefix.push(part);
            // Vérification par le noyau après la création ou sa course avec un autre acteur.
            open(&self.work, &prefix, OFlags::RDONLY | OFlags::DIRECTORY)?;
        }
        let directory = open(&self.work, parent, OFlags::RDONLY | OFlags::DIRECTORY)?;
        let temp = format!(
            ".prophet-write-{:016x}{:016x}",
            rand::rngs::OsRng.next_u64(),
            rand::rngs::OsRng.next_u64()
        );
        let fd = fs::openat2(
            &directory,
            temp.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::RUSR | Mode::WUSR,
            RESOLVE,
        )?;
        let mut file = File::from(fd);
        let result = (|| {
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
            if !self.permits(Act::Write, relative) {
                return Err(denied());
            }
            // Remplacer le nom atomiquement évite de tronquer une cible de lien physique.
            // renameat ne suit pas un éventuel lien symbolique au dernier composant.
            fs::renameat(&directory, temp.as_str(), &directory, name)?;
            directory.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::unlinkat(&directory, temp.as_str(), AtFlags::empty());
        }
        result
    }
}

struct Budget {
    visited: usize,
    scanned: usize,
    result_bytes: usize,
    until: Instant,
    truncated: bool,
}

impl Budget {
    fn new() -> Self {
        Self {
            visited: 0,
            scanned: 0,
            result_bytes: 0,
            until: Instant::now() + Duration::from_secs(2),
            truncated: false,
        }
    }
    fn visit(&mut self) -> bool {
        if self.visited >= MAX_VISITED || Instant::now() >= self.until {
            self.truncated = true;
            return false;
        }
        self.visited += 1;
        true
    }
    fn accept(&mut self, value: &Value, count: usize, max: usize) -> bool {
        self.result_bytes += value.to_string().len();
        if count >= max || self.result_bytes > MAX_RESULTS_BYTES {
            self.truncated = true;
            false
        } else {
            true
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Operation {
    Read,
    Write,
    List,
    Stat,
    Search,
}

pub(super) fn execute(
    op: Operation,
    args: &Value,
    context: &ToolContext,
    access: &dyn ResourceAccess,
) -> CallResult {
    match run(op, args, context, access) {
        Ok(data) => CallResult::structured(data),
        Err(Failure(code, detail)) => CallResult::error(code, detail),
    }
}

fn run(
    op: Operation,
    args: &Value,
    context: &ToolContext,
    access: &dyn ResourceAccess,
) -> Result<Value> {
    let key = if matches!(op, Operation::Search) {
        "root"
    } else {
        "path"
    };
    let raw = args
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("chemin texte requis"))?;
    let relative = relative(raw, context).ok_or_else(denied)?;
    let view = View::new(context, access)?;
    let logical = view.logical.join(&relative);
    match op {
        Operation::Read => {
            let max = match args.get("max_bytes") {
                None => MAX_READ,
                Some(value) => value
                    .as_u64()
                    .ok_or_else(|| invalid("max_bytes doit être un entier positif ou nul"))?
                    .min(MAX_READ as u64) as usize,
            };
            let (content, total, truncated, _) = view.read(&relative, max)?;
            Ok(
                json!({"path":logical, "content":content, "total_bytes":total, "truncated":truncated}),
            )
        }
        Operation::Write => {
            let content = args
                .get("content")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("contenu texte requis"))?;
            if content.len() > MAX_WRITE {
                return Err(Failure(
                    ErrorCode::BudgetExceeded,
                    "écriture limitée à 1 Mio".into(),
                ));
            }
            view.write(&relative, content)?;
            Ok(
                json!({"path":logical,"bytes":content.len(),"staged":true,"note":"écrit dans l'espace de travail ; la validation reste explicite"}),
            )
        }
        Operation::Stat => {
            if !view.permits(Act::Read, &relative) {
                return Err(denied());
            }
            let m = view.metadata(&relative)?;
            Ok(
                json!({"path":logical,"kind":if m.is_dir(){"répertoire"}else{"fichier"},"size":m.len(),"readonly":m.permissions().readonly()}),
            )
        }
        Operation::List => {
            if !view.permits(Act::List, &relative) {
                return Err(denied());
            }
            let mut budget = Budget::new();
            let mut entries = vec![];
            for child in view.names(&relative, &mut budget)? {
                if !budget.visit() {
                    break;
                }
                if !view.permits(Act::List, &child) {
                    continue;
                }
                let Ok(m) = view.metadata(&child) else {
                    continue;
                };
                let value = json!({"name":child.file_name().and_then(OsStr::to_str),"kind":if m.is_dir(){"répertoire"}else{"fichier"},"size":m.len()});
                if !budget.accept(&value, entries.len(), 2000) {
                    break;
                }
                entries.push(value);
            }
            Ok(json!({"path":logical,"entries":entries,"truncated":budget.truncated,"scoped":true}))
        }
        Operation::Search => search(&view, &relative, args),
    }
}

fn search(view: &View<'_>, root: &Path, args: &Value) -> Result<Value> {
    let text_arg = |key| -> Result<Option<&str>> {
        args.get(key)
            .map(|v| {
                v.as_str()
                    .ok_or_else(|| invalid("les filtres de recherche doivent être des textes"))
            })
            .transpose()
    };
    let name = text_arg("name_contains")?;
    let content = text_arg("content_contains")?;
    if !view.permits(Act::Read, root) {
        return Err(denied());
    }
    let mut budget = Budget::new();
    let mut results = vec![];
    let mut stack = vec![(root.to_owned(), 0)];
    while let Some((directory, depth)) = stack.pop() {
        if !budget.visit() {
            break;
        }
        if !view.permits(Act::Read, &directory) {
            continue;
        }
        let children = match view.names(&directory, &mut budget) {
            Ok(children) => children,
            Err(error) if directory == root => return Err(error),
            Err(_) => {
                budget.truncated = true;
                continue;
            }
        };
        for child in children {
            if !budget.visit() {
                stack.clear();
                break;
            }
            if !view.permits(Act::Read, &child) {
                continue;
            }
            let Ok(m) = view.metadata(&child) else {
                continue;
            };
            if m.is_dir() {
                if depth >= 32 {
                    budget.truncated = true;
                } else {
                    stack.push((child, depth + 1));
                }
                continue;
            }
            if name.is_some_and(|needle| {
                !child
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .contains(needle)
            }) {
                continue;
            }
            if let Some(needle) = content {
                let remaining = MAX_SCAN.saturating_sub(budget.scanned);
                if remaining == 0 {
                    budget.truncated = true;
                    stack.clear();
                    break;
                }
                let Ok((text, _, truncated, read)) =
                    view.read(&child, MAX_READ.min(remaining.saturating_sub(1)))
                else {
                    continue;
                };
                budget.scanned += read;
                budget.truncated |= truncated;
                if !text.contains(needle) {
                    continue;
                }
            }
            let value = json!({"path":view.logical.join(&child),"size":m.len()});
            if !budget.accept(&value, results.len(), 200) {
                stack.clear();
                break;
            }
            results.push(value);
        }
    }
    results.sort_by_key(|v| v["path"].as_str().unwrap_or_default().to_owned());
    Ok(json!({"results":results,"truncated":budget.truncated,"scoped":true}))
}
