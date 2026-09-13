//! Choix de la dorsale de stockage.

use std::path::Path;

/// Dorsale disponible pour un chemin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    /// btrfs détecté ; le moteur actuel utilise encore des copies de fichiers.
    Btrfs,
    /// Repli portable : empreintes et sauvegarde des fichiers touchés.
    Portable,
}

/// Dorsale active, avec la raison du choix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backend {
    /// Type retenu.
    pub kind: BackendKind,
    /// Explication, affichée par `prophet fs status`.
    pub reason: String,
}

impl Backend {
    /// Vrai seulement lorsqu'une dorsale de snapshots natifs est effectivement implémentée.
    #[must_use]
    pub const fn has_native_snapshots(&self) -> bool {
        false
    }

    /// Limites connues de la dorsale, à afficher à l'utilisateur.
    #[must_use]
    pub const fn limitations(&self) -> &'static str {
        "l'annulation ne couvre que la dernière validation de chaque tâche et refuse les \
         modifications ultérieures ; les fichiers sont copiés, même sur btrfs ; \
         les lots publiés ne sont pas atomiques dans leur ensemble"
    }
}

/// Détecte la dorsale utilisable pour un chemin.
///
/// La détection lit `/proc/self/mounts` : elle n'exige aucun privilège et ne dépend pas d'un
/// binaire externe.
#[must_use]
pub fn detect_backend(path: &Path) -> Backend {
    match filesystem_type(path) {
        Some(fs) if fs == "btrfs" => Backend {
            kind: BackendKind::Btrfs,
            reason: "btrfs détecté : moteur de copies, snapshots natifs non implémentés".to_owned(),
        },
        Some(fs) => Backend {
            kind: BackendKind::Portable,
            reason: format!("système de fichiers {fs} : repli portable"),
        },
        None => Backend {
            kind: BackendKind::Portable,
            reason: "système de fichiers indéterminé : repli portable".to_owned(),
        },
    }
}

/// Type du système de fichiers portant un chemin, d'après `/proc/self/mounts`.
///
/// Retourne le point de montage le plus spécifique qui soit un préfixe du chemin.
#[must_use]
pub fn filesystem_type(path: &Path) -> Option<String> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mounts = std::fs::read_to_string("/proc/self/mounts").ok()?;
    let mut best: Option<(usize, String)> = None;
    for line in mounts.lines() {
        let mut fields = line.split_whitespace();
        let _device = fields.next()?;
        let mount_point = fields.next()?;
        let fs_type = fields.next()?;
        let mount_path = Path::new(mount_point);
        if canonical.starts_with(mount_path) {
            let depth = mount_path.components().count();
            if best.as_ref().is_none_or(|(d, _)| depth > *d) {
                best = Some((depth, fs_type.to_owned()));
            }
        }
    }
    best.map(|(_, fs)| fs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_sans_privilege() {
        let dir = tempfile::tempdir().unwrap();
        let backend = detect_backend(dir.path());
        // Le type exact dépend de la machine ; la détection doit aboutir dans tous les cas.
        assert!(!backend.reason.is_empty());
        assert!(!backend.limitations().is_empty());
    }

    #[test]
    fn type_de_systeme_de_fichiers_de_la_racine() {
        assert!(filesystem_type(Path::new("/")).is_some());
    }

    #[test]
    fn chemin_inexistant_ne_panique_pas() {
        let backend = detect_backend(Path::new("/n/existe/pas/du/tout"));
        assert_eq!(backend.kind, BackendKind::Portable);
    }

    #[test]
    fn btrfs_ne_promet_pas_une_dorsale_non_implementee() {
        let backend = Backend {
            kind: BackendKind::Btrfs,
            reason: "btrfs détecté".into(),
        };
        assert!(!backend.has_native_snapshots());
        assert_ne!(backend.limitations(), "aucune");
    }
}
