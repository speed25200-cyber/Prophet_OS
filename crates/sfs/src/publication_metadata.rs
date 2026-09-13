//! Métadonnées humaines conservées par publication ; les attributs du travail ne les remplacent pas.
use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Component, Path};

use rustix::fs::{self, Mode, OFlags};
use serde::{Deserialize, Serialize};
use xattr::FileExt as _;

use crate::Provenance;
use crate::review::open;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Metadata {
    uid: u32,
    gid: u32,
    modified: i64,
    modified_ns: i64,
    attributes: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Pair {
    pub(crate) before: Option<Metadata>,
    pub(crate) after: Option<Metadata>,
}

fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}

fn attributes(file: &File) -> io::Result<BTreeMap<String, Vec<u8>>> {
    let names = match file.list_xattr() {
        Ok(names) => names,
        Err(error) if error.kind() == io::ErrorKind::Unsupported => return Ok(BTreeMap::new()),
        Err(error) => return Err(error),
    };
    let mut result = BTreeMap::new();
    let mut bytes = 0_usize;
    for name in names {
        let name = name
            .to_str()
            .ok_or_else(|| invalid("Attribut non UTF-8."))?;
        if name == "security.capability" {
            return Err(invalid(
                "Un fichier doté de capacités ne peut pas être publié.",
            ));
        }
        let value = file
            .get_xattr(name)?
            .ok_or_else(|| invalid("Attribut modifié pendant la lecture."))?;
        bytes = bytes.saturating_add(name.len() + value.len());
        if result.len() >= 128 || bytes > 1024 * 1024 {
            return Err(invalid("Métadonnées trop grandes."));
        }
        result.insert(name.into(), value);
    }
    Ok(result)
}

impl Metadata {
    pub(crate) fn read(root: &File, path: &Path) -> io::Result<Self> {
        let file = open(root, path, OFlags::RDONLY | OFlags::NONBLOCK)?;
        let before = file.metadata()?;
        if !before.is_file() || before.nlink() != 1 {
            return Err(invalid("Type de fichier refusé."));
        }
        let value = Self {
            uid: before.uid(),
            gid: before.gid(),
            modified: before.mtime(),
            modified_ns: before.mtime_nsec(),
            attributes: attributes(&file)?,
        };
        let after = file.metadata()?;
        if before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
            || before.mtime() != after.mtime()
            || before.mtime_nsec() != after.mtime_nsec()
        {
            return Err(invalid("Métadonnées modifiées pendant la lecture."));
        }
        Ok(value)
    }

    pub(crate) fn proposed(
        home: &File,
        work: &File,
        path: &Path,
        mode: u32,
        before: Option<&Self>,
        provenance: Option<&Provenance>,
    ) -> io::Result<Self> {
        let source = open(work, path, OFlags::RDONLY | OFlags::NONBLOCK)?.metadata()?;
        let mut result = if let Some(before) = before {
            before.clone()
        } else {
            // Les nouveaux sous-répertoires héritent du même ACL par défaut que le
            // parent existant le plus proche. Aucun attribut du travail n'est importé.
            let mut directory = home.try_clone()?;
            for component in path.parent().unwrap_or(Path::new("")).components() {
                let Component::Normal(name) = component else {
                    return Err(invalid("Parent invalide."));
                };
                match open(
                    &directory,
                    Path::new(name),
                    OFlags::RDONLY | OFlags::DIRECTORY,
                ) {
                    Ok(child) => directory = child,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => break,
                    Err(error) => return Err(error),
                }
            }
            let mut values = BTreeMap::new();
            if let Some(default) = directory.get_xattr("system.posix_acl_default")? {
                values.insert("system.posix_acl_access".into(), default);
            }
            // Un contexte MAC nécessite une règle de création explicite, pas la copie
            // arbitraire du contexte d'un répertoire privé vers le document final.
            if directory.get_xattr("security.selinux")?.is_some() {
                return Err(invalid(
                    "La création avec contexte SELinux doit être configurée explicitement.",
                ));
            }
            Self {
                uid: home.metadata()?.uid(),
                gid: directory.metadata()?.gid(),
                modified: source.mtime(),
                modified_ns: source.mtime_nsec(),
                attributes: values,
            }
        };
        result.modified = source.mtime();
        result.modified_ns = source.mtime_nsec();
        if let Some(acl) = result.attributes.get_mut("system.posix_acl_access") {
            apply_mode(acl, mode)?;
        }
        if let Some(provenance) = provenance {
            for (name, value) in [
                ("task", provenance.task.clone()),
                ("agent", provenance.agent.clone()),
                ("step", provenance.step.to_string()),
                ("model", provenance.model.clone()),
            ] {
                if value.len() > 65536 {
                    return Err(invalid("Provenance trop grande."));
                }
                result
                    .attributes
                    .insert(format!("user.prophet.{name}"), value.into_bytes());
            }
        }
        Ok(result)
    }

    pub(crate) fn apply(&self, file: &File, mode: u32) -> io::Result<()> {
        let current = file.metadata()?;
        if current.uid() != self.uid || current.gid() != self.gid {
            fs::fchown(
                file,
                Some(fs::Uid::from_raw(self.uid)),
                Some(fs::Gid::from_raw(self.gid)),
            )?;
        }
        let existing = attributes(file)?;
        for name in existing
            .keys()
            .filter(|name| !self.attributes.contains_key(*name))
        {
            file.remove_xattr(name)?;
        }
        for (name, value) in &self.attributes {
            if existing.get(name) != Some(value) {
                file.set_xattr(name, value)?;
            }
        }
        fs::fchmod(file, Mode::from_bits_truncate(mode & 0o777))?;
        fs::futimens(
            file,
            &fs::Timestamps {
                last_access: fs::Timespec {
                    tv_sec: 0,
                    tv_nsec: fs::UTIME_OMIT,
                },
                last_modification: fs::Timespec {
                    tv_sec: self.modified,
                    tv_nsec: self.modified_ns,
                },
            },
        )?;
        Ok(())
    }

    pub(crate) fn check(&self, root: &File, path: &Path) -> io::Result<()> {
        if Self::read(root, path)? != *self {
            return Err(invalid(
                "Les métadonnées du fichier ont changé. La publication est interrompue.",
            ));
        }
        Ok(())
    }
}

fn apply_mode(acl: &mut [u8], mode: u32) -> io::Result<()> {
    if acl.len() < 4 || acl[..4] != 2_u32.to_le_bytes() || !(acl.len() - 4).is_multiple_of(8) {
        return Err(invalid("ACL POSIX illisible."));
    }
    let masked = acl[4..]
        .chunks_exact(8)
        .any(|entry| u16::from_le_bytes([entry[0], entry[1]]) == 0x10);
    for entry in acl[4..].chunks_exact_mut(8) {
        let tag = u16::from_le_bytes([entry[0], entry[1]]);
        let bits = match tag {
            0x01 => Some((mode >> 6) & 7),
            0x10 => Some((mode >> 3) & 7),
            0x04 if !masked => Some((mode >> 3) & 7),
            0x20 => Some(mode & 7),
            _ => None,
        };
        if let Some(bits) = bits {
            entry[2..4].copy_from_slice(&(bits as u16).to_le_bytes());
        }
    }
    Ok(())
}
