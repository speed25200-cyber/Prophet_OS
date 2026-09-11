//! Provenance : chaque fichier touché par un agent porte la trace de qui l'a écrit et pourquoi.
//!
//! Stockée en attributs étendus. Quand le système de fichiers ne les supporte pas, l'écriture est
//! silencieusement ignorée et la provenance reste consultable par le journal : c'est une
//! commodité, jamais une source de vérité pour la sécurité.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Préfixe des attributs étendus posés par Prophet OS.
pub const XATTR_PREFIX: &str = "user.prophet.";

/// Origine d'un fichier écrit par un agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Provenance {
    /// Tâche à l'origine de l'écriture.
    pub task: String,
    /// Agent.
    pub agent: String,
    /// Étape de la boucle agentique.
    pub step: u32,
    /// Modèle ou pilote employé.
    pub model: String,
}

/// Pose la provenance sur un fichier.
///
/// Retourne `Ok(false)` si le système de fichiers ne supporte pas les attributs étendus.
///
/// # Erreurs
/// Si le fichier est inaccessible.
pub fn write_provenance(path: &Path, provenance: &Provenance) -> std::io::Result<bool> {
    let pairs = [
        ("task", provenance.task.as_str()),
        ("agent", provenance.agent.as_str()),
        ("model", provenance.model.as_str()),
    ];
    for (key, value) in pairs {
        match xattr::set(path, format!("{XATTR_PREFIX}{key}"), value.as_bytes()) {
            Ok(()) => {}
            Err(error) if unsupported(&error) => return Ok(false),
            Err(error) => return Err(error),
        }
    }
    match xattr::set(
        path,
        format!("{XATTR_PREFIX}step"),
        provenance.step.to_string().as_bytes(),
    ) {
        Ok(()) => Ok(true),
        Err(error) if unsupported(&error) => Ok(false),
        Err(error) => Err(error),
    }
}

/// Lit la provenance d'un fichier, si elle existe.
///
/// # Erreurs
/// Si le fichier est inaccessible pour une autre raison que l'absence d'attributs.
pub fn read_provenance(path: &Path) -> std::io::Result<Option<Provenance>> {
    let read = |key: &str| -> std::io::Result<Option<String>> {
        match xattr::get(path, format!("{XATTR_PREFIX}{key}")) {
            Ok(Some(bytes)) => Ok(String::from_utf8(bytes).ok()),
            Ok(None) => Ok(None),
            Err(error) if unsupported(&error) => Ok(None),
            Err(error) => Err(error),
        }
    };
    let Some(task) = read("task")? else {
        return Ok(None);
    };
    Ok(Some(Provenance {
        task,
        agent: read("agent")?.unwrap_or_default(),
        model: read("model")?.unwrap_or_default(),
        step: read("step")?
            .and_then(|s| s.parse().ok())
            .unwrap_or_default(),
    }))
}

/// Vrai si l'erreur signale une absence de support des attributs étendus.
fn unsupported(error: &std::io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc_enotsup) if libc_enotsup == 95 || libc_enotsup == 45 || libc_enotsup == 1
    ) || error.kind() == std::io::ErrorKind::Unsupported
        || error.kind() == std::io::ErrorKind::PermissionDenied
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aller_retour_ou_degradation_silencieuse() {
        let dir = tempfile::tempdir().unwrap();
        let fichier = dir.path().join("rapport.pdf");
        std::fs::write(&fichier, b"x").unwrap();

        let provenance = Provenance {
            task: "task:01".into(),
            agent: "org.test.agent".into(),
            step: 18,
            model: "local:qwen3-8b".into(),
        };
        let pose = write_provenance(&fichier, &provenance).unwrap();
        let relu = read_provenance(&fichier).unwrap();
        if pose {
            assert_eq!(relu, Some(provenance));
        } else {
            // Système de fichiers sans attributs étendus : la dégradation doit être propre.
            assert_eq!(relu, None);
        }
    }

    #[test]
    fn fichier_sans_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let fichier = dir.path().join("neutre.txt");
        std::fs::write(&fichier, b"x").unwrap();
        assert_eq!(read_provenance(&fichier).unwrap(), None);
    }
}
