//! Accès Linux ancrés sur des descripteurs. Aucune canonicalisation suivie d'une réouverture.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::{File, Metadata};
use std::io::{Read, Seek, SeekFrom, Write};
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
        let errno = error
            .raw_os_error()
            .map(rustix::io::Errno::from_raw_os_error);
        // Une erreur du modèle sur la nature d'un chemin se dit comme telle, avec l'outil qui
        // convient : ce n'est ni une panne ni un refus, et il peut la corriger.
        match errno {
            Some(rustix::io::Errno::NOTDIR) => {
                return Self(
                    ErrorCode::Invalid,
                    "ce chemin est un fichier, pas un répertoire : lisez-le avec fs.read".into(),
                );
            }
            Some(rustix::io::Errno::ISDIR) => {
                return Self(
                    ErrorCode::Invalid,
                    "ce chemin est un répertoire, pas un fichier : listez-le avec fs.list".into(),
                );
            }
            _ => {}
        }
        let code = match errno {
            Some(rustix::io::Errno::NOENT) => ErrorCode::NotFound,
            Some(rustix::io::Errno::NAMETOOLONG | rustix::io::Errno::INVAL) => ErrorCode::Invalid,
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
        let chunk = self.read_from(relative, 0, max)?;
        Ok((chunk.text, chunk.total, chunk.next.is_some(), chunk.read))
    }

    /// Lit au plus `max` octets à partir de `offset`, sans couper de caractère : un décalage
    /// tombé au milieu d'un caractère reprend au suivant, et la lecture s'arrête avant un
    /// caractère qui ne tiendrait pas entier.
    fn read_from(&self, relative: &Path, offset: u64, max: usize) -> Result<Chunk> {
        if !self.permits(Act::Read, relative) {
            return Err(denied());
        }
        let mut file = self.selected(relative, OFlags::RDONLY)?;
        let metadata = file.metadata()?;
        if metadata.is_dir() {
            return Err(std::io::Error::from(rustix::io::Errno::ISDIR).into());
        }
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(denied());
        }
        if offset > 0 {
            file.seek(SeekFrom::Start(offset))?;
        }
        let available = metadata.len().saturating_sub(offset) as usize;
        // Trois octets de plus pour sauter une fin de caractère en tête, un pour savoir s'il
        // reste quelque chose après le plafond.
        let mut bytes = Vec::with_capacity(max.min(available).saturating_add(4));
        file.take(max as u64 + 4).read_to_end(&mut bytes)?;
        let read = bytes.len();
        let skip = if offset > 0 {
            bytes
                .iter()
                .take(3)
                .take_while(|byte| is_continuation(**byte))
                .count()
        } else {
            0
        };
        let body = &bytes[skip..];
        let mut end = body.len().min(max);
        while end > 0 && end < body.len() && is_continuation(body[end]) {
            end -= 1;
        }
        let mut text = String::from_utf8_lossy(&body[..end]).into_owned();
        // Des octets invalides s'élargissent en caractères de remplacement : la borne reste.
        if text.len() > max {
            let mut cut = max;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
        }
        let start = offset + skip as u64;
        let total = if read == 0 {
            metadata.len()
        } else {
            metadata.len().max(offset + read as u64)
        };
        let consumed = start + end as u64;
        Ok(Chunk {
            text,
            total,
            read,
            start,
            next: (consumed < total).then_some(consumed),
        })
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
    Edit,
    List,
    Stat,
    Search,
}

/// Les octets d'un fichier du périmètre, lus sous les mêmes règles que `fs.read` : ni lien
/// symbolique, ni lien physique multiple, ni fichier spécial, et une borne dite plutôt que tue.
pub(super) struct Reading {
    /// Chemin logique, tel que l'agent le désigne.
    pub logical: PathBuf,
    /// Les octets lus, au plus `max`.
    pub bytes: Vec<u8>,
    /// Taille totale du fichier.
    pub total: u64,
    /// Le fichier dépassait la borne.
    pub truncated: bool,
}

/// Un morceau de fichier texte lu à partir d'un décalage.
struct Chunk {
    text: String,
    total: u64,
    read: usize,
    /// Décalage réel du premier octet rendu.
    start: u64,
    /// Décalage où reprendre, s'il reste à lire.
    next: Option<u64>,
}

const fn is_continuation(byte: u8) -> bool {
    byte & 0xC0 == 0x80
}

/// Lit les octets d'un fichier autorisé, pour les outils qui interprètent un format (documents,
/// images, médias) sans rien exposer de plus que `fs.read`.
pub(super) fn read_bytes(
    raw: &str,
    context: &ToolContext,
    access: &dyn ResourceAccess,
    max: usize,
) -> std::result::Result<Reading, (ErrorCode, String)> {
    let go = || -> Result<Reading> {
        let relative = relative(raw, context).ok_or_else(denied)?;
        let view = View::new(context, access)?;
        let logical = view.logical.join(&relative);
        if !view.permits(Act::Read, &relative) {
            return Err(denied());
        }
        let file = view.selected(&relative, OFlags::RDONLY)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(denied());
        }
        let mut bytes = Vec::with_capacity(max.min(metadata.len() as usize).saturating_add(1));
        file.take(max as u64 + 1).read_to_end(&mut bytes)?;
        let truncated = bytes.len() > max;
        bytes.truncate(max);
        Ok(Reading {
            logical,
            total: metadata.len().max(bytes.len() as u64),
            bytes,
            truncated,
        })
    };
    go().map_err(|Failure(code, detail)| (code, detail))
}

pub(super) fn execute(
    op: Operation,
    args: &Value,
    context: &ToolContext,
    access: &dyn ResourceAccess,
) -> CallResult {
    match run(op, args, context, access) {
        Ok(data) => CallResult::structured(data),
        Err(Failure(ErrorCode::PolicyDenied, detail)) => CallResult::error(
            ErrorCode::PolicyDenied,
            crate::registry::with_scopes(detail, context),
        ),
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
            let offset = match args.get("offset") {
                None => 0,
                Some(value) => value
                    .as_u64()
                    .ok_or_else(|| invalid("offset doit être un entier positif ou nul"))?,
            };
            let chunk = view.read_from(&relative, offset, max)?;
            // Les lignes sont comptées pour le modèle : un petit modèle compte mal les `\n`
            // d'un texte qu'il relit, le service le fait exactement.
            let lines = chunk.text.lines().count();
            let mut result = json!({"path":logical, "content":chunk.text, "lines":lines, "total_bytes":chunk.total, "truncated":chunk.next.is_some()});
            if args.get("offset").is_some() {
                result["offset"] = json!(chunk.start);
            }
            if let Some(next) = chunk.next {
                result["next_offset"] = json!(next);
            }
            Ok(result)
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
        Operation::Edit => edit(&view, &relative, &logical, args),
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

/// Remplace un passage exact d'un fichier texte et écrit le résultat dans l'espace de travail,
/// comme `fs.write` : un petit modèle corrige une faute ou change une valeur sans recopier tout
/// le fichier, et sans le tronquer. Le passage doit apparaître une fois, ou `all` le dit.
fn edit(view: &View<'_>, relative: &Path, logical: &Path, args: &Value) -> Result<Value> {
    let text_arg = |key: &str, what: &str| -> Result<&str> {
        args.get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| invalid(&format!("{what} ({key}) requis")))
    };
    let old = text_arg("old", "texte à remplacer")?;
    let new = text_arg("new", "texte de remplacement")?;
    if old.is_empty() {
        return Err(invalid("le texte à remplacer (old) ne peut pas être vide"));
    }
    let all = match args.get("all") {
        None => false,
        Some(value) => value
            .as_bool()
            .ok_or_else(|| invalid("all doit être vrai ou faux"))?,
    };
    // Le droit d'écrire est vérifié avant de lire : un refus ne révèle rien du contenu.
    if !view.permits(Act::Write, relative) {
        return Err(denied());
    }
    let chunk = view.read_from(relative, 0, MAX_WRITE)?;
    if chunk.next.is_some() {
        return Err(Failure(
            ErrorCode::BudgetExceeded,
            "fichier de plus de 1 Mio : fs.edit ne l'édite pas".into(),
        ));
    }
    // Un fichier qui n'est pas du texte UTF-8 serait réécrit avec des caractères de
    // remplacement : il n'est pas édité.
    if chunk.text.len() as u64 != chunk.total {
        return Err(invalid(
            "fichier qui n'est pas du texte UTF-8 : fs.edit ne l'édite pas",
        ));
    }
    let found = chunk.text.matches(old).count();
    if found == 0 {
        return Err(invalid(
            "texte à remplacer introuvable dans le fichier : relisez-le avec fs.read et \
             recopiez le passage exact",
        ));
    }
    if found > 1 && !all {
        return Err(invalid(&format!(
            "le texte à remplacer apparaît {found} fois : allongez-le pour qu'il n'apparaisse \
             qu'une fois, ou passez all: true pour tout remplacer"
        )));
    }
    let edited = if all {
        chunk.text.replace(old, new)
    } else {
        chunk.text.replacen(old, new, 1)
    };
    if edited.len() > MAX_WRITE {
        return Err(Failure(
            ErrorCode::BudgetExceeded,
            "écriture limitée à 1 Mio".into(),
        ));
    }
    view.write(relative, &edited)?;
    Ok(json!({
        "path": logical,
        "replaced": if all { found } else { 1 },
        "bytes": edited.len(),
        "lines": edited.lines().count(),
        "staged": true,
        "note": "écrit dans l'espace de travail ; la validation reste explicite",
    }))
}

/// Lignes trouvées rendues par fichier, et longueur d'un extrait en caractères.
const MAX_MATCHES: usize = 5;
const EXCERPT_CHARS: usize = 200;

/// Les premières lignes qui contiennent le motif, numérotées depuis 1, chacune réduite à un
/// extrait centré sur le motif ; vrai s'il y en a d'autres.
fn matching_lines(text: &str, needle: &str) -> (Vec<Value>, bool) {
    let mut found = text
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains(needle));
    let lines = found
        .by_ref()
        .take(MAX_MATCHES)
        .map(|(index, line)| json!({"line": index + 1, "text": excerpt(line.trim(), needle)}))
        .collect();
    (lines, found.next().is_some())
}

fn excerpt(line: &str, needle: &str) -> String {
    let count = line.chars().count();
    if count <= EXCERPT_CHARS {
        return line.to_owned();
    }
    let at = line[..line.find(needle).unwrap_or(0)].chars().count();
    let needle_chars = needle.chars().count();
    // Deux caractères réservés aux points de suspension, le reste réparti autour du motif.
    let room = EXCERPT_CHARS - 2;
    let before = room.saturating_sub(needle_chars) / 2;
    let start = at.saturating_sub(before).min(count.saturating_sub(room));
    let body: String = line.chars().skip(start).take(room).collect();
    let prefix = if start > 0 { "…" } else { "" };
    let suffix = if start + room < count { "…" } else { "" };
    format!("{prefix}{body}{suffix}")
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
    let found = search_with(view, root, name, content)?;
    // Un modèle cherche souvent par nom ce qui figure dans le contenu (« la référence
    // ZX-99417 ») : quand aucun nom ne correspond, le même texte est cherché dans le contenu,
    // et le résultat le dit. Mêmes droits, mêmes bornes.
    if let (Some(needle), None) = (name, content)
        && found["results"].as_array().is_some_and(Vec::is_empty)
    {
        let mut in_content = search_with(view, root, None, Some(needle))?;
        if in_content["results"]
            .as_array()
            .is_some_and(|results| !results.is_empty())
        {
            in_content["note"] = json!(format!(
                "aucun nom ne contient « {needle} » ; voici les fichiers dont le contenu le contient"
            ));
            return Ok(in_content);
        }
    }
    Ok(found)
}

fn search_with(
    view: &View<'_>,
    root: &Path,
    name: Option<&str>,
    content: Option<&str>,
) -> Result<Value> {
    if !view.permits(Act::Read, root) {
        return Err(denied());
    }
    // Un petit modèle nomme souvent le fichier qu'il veut fouiller : la recherche porte alors
    // sur lui seul, sous les mêmes règles.
    let root_is_file = view.metadata(root).is_ok_and(|m| !m.is_dir());
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
        let listed = if root_is_file && directory == root {
            Ok(vec![root.to_owned()])
        } else {
            view.names(&directory, &mut budget)
        };
        let children = match listed {
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
            let mut found = None;
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
                found = Some(matching_lines(&text, needle));
            }
            let mut value = json!({"path":view.logical.join(&child),"size":m.len()});
            if let Some((lines, more)) = found.take() {
                value["matches"] = json!(lines);
                if more {
                    value["more_matches"] = json!(true);
                }
            }
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
