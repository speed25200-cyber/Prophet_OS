//! Différence entre l'état de départ d'une tâche et son espace de travail.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Empreinte d'un fichier au moment où la tâche a commencé.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFingerprint {
    /// Empreinte du contenu.
    pub hash: String,
    /// Taille en octets.
    pub size: u64,
    /// Bits de permission Unix.
    pub mode: u32,
}

/// État de départ : chemin relatif au périmètre vers empreinte.
pub type Fingerprints = BTreeMap<PathBuf, FileFingerprint>;

/// Nature d'un changement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    /// Fichier créé par la tâche.
    Added,
    /// Fichier modifié.
    Modified,
    /// Fichier supprimé.
    Deleted,
}

/// Un changement, tel qu'il sera montré à l'humain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    /// Chemin relatif au périmètre.
    pub path: PathBuf,
    /// Nature.
    pub kind: ChangeKind,
    /// Taille avant, si le fichier existait.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_before: Option<u64>,
    /// Taille après, si le fichier existe encore.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_after: Option<u64>,
}

/// Ensemble des changements d'une tâche.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Diff {
    /// Changements, triés par chemin.
    pub changes: Vec<Change>,
}

impl Diff {
    /// Vrai si la tâche n'a rien changé.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Compte par nature.
    #[must_use]
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut added = 0;
        let mut modified = 0;
        let mut deleted = 0;
        for change in &self.changes {
            match change.kind {
                ChangeKind::Added => added += 1,
                ChangeKind::Modified => modified += 1,
                ChangeKind::Deleted => deleted += 1,
            }
        }
        (added, modified, deleted)
    }

    /// Volume total écrit.
    #[must_use]
    pub fn bytes_written(&self) -> u64 {
        self.changes.iter().filter_map(|c| c.size_after).sum()
    }

    /// Rendu lisible, destiné à l'humain qui valide.
    #[must_use]
    pub fn render(&self) -> String {
        self.render_limited(usize::MAX)
    }

    /// Rendu lisible borné à `max` changements, suivis du compte de ceux qui ne sont pas
    /// montrés : ce qu'on rend à un modèle doit tenir dans son contexte, quelle que soit la
    /// taille du travail.
    #[must_use]
    pub fn render_limited(&self, max: usize) -> String {
        if self.is_empty() {
            return "aucun changement\n".to_owned();
        }
        let mut out = String::new();
        for change in self.changes.iter().take(max) {
            let marque = match change.kind {
                ChangeKind::Added => "+",
                ChangeKind::Modified => "~",
                ChangeKind::Deleted => "-",
            };
            let taille = match (change.size_before, change.size_after) {
                (Some(before), Some(after)) => format!("{before} → {after} o"),
                (None, Some(after)) => format!("{after} o"),
                (Some(before), None) => format!("{before} o supprimés"),
                (None, None) => String::new(),
            };
            out.push_str(&format!("{marque} {}  {taille}\n", change.path.display()));
        }
        let reste = self.changes.len().saturating_sub(max);
        if reste > 0 {
            out.push_str(&format!("… et {reste} autre(s) changement(s)\n"));
        }
        let (a, m, d) = self.counts();
        out.push_str(&format!(
            "\n{a} ajouté(s), {m} modifié(s), {d} supprimé(s)\n"
        ));
        out
    }
}

/// Calcule l'empreinte d'un fichier.
///
/// # Erreurs
/// Si le fichier est illisible.
pub fn fingerprint(path: &Path) -> std::io::Result<FileFingerprint> {
    use std::os::unix::fs::PermissionsExt as _;
    let content = std::fs::read(path)?;
    let metadata = std::fs::metadata(path)?;
    Ok(FileFingerprint {
        hash: blake3::hash(&content).to_hex().to_string(),
        size: content.len() as u64,
        mode: metadata.permissions().mode(),
    })
}

/// Parcourt un arbre et calcule les empreintes de tous ses fichiers réguliers.
///
/// Les liens symboliques ne sont pas suivis : ils sont ignorés, pour qu'un lien pointant hors du
/// périmètre ne puisse pas l'élargir.
///
/// # Erreurs
/// Si un répertoire est illisible.
pub fn fingerprint_tree(root: &Path) -> std::io::Result<Fingerprints> {
    let mut out = Fingerprints::new();
    if !root.exists() {
        return Ok(out);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                stack.push(path);
            } else if metadata.is_file()
                && let Ok(relative) = path.strip_prefix(root)
            {
                out.insert(relative.to_path_buf(), fingerprint(&path)?);
            }
        }
    }
    Ok(out)
}

/// Compare un état de départ à un arbre courant.
///
/// # Erreurs
/// Si l'arbre est illisible.
pub fn compute(base: &Fingerprints, work_root: &Path) -> std::io::Result<Diff> {
    let current = fingerprint_tree(work_root)?;
    let mut changes = Vec::new();

    for (path, now) in &current {
        match base.get(path) {
            None => changes.push(Change {
                path: path.clone(),
                kind: ChangeKind::Added,
                size_before: None,
                size_after: Some(now.size),
            }),
            Some(before) if before.hash != now.hash || before.mode != now.mode => {
                changes.push(Change {
                    path: path.clone(),
                    kind: ChangeKind::Modified,
                    size_before: Some(before.size),
                    size_after: Some(now.size),
                });
            }
            Some(_) => {}
        }
    }
    for (path, before) in base {
        if !current.contains_key(path) {
            changes.push(Change {
                path: path.clone(),
                kind: ChangeKind::Deleted,
                size_before: Some(before.size),
                size_after: None,
            });
        }
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Diff { changes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ecrire(root: &Path, relative: &str, contenu: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contenu).unwrap();
    }

    #[test]
    fn detection_des_trois_natures() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        ecrire(root, "garde.txt", "identique");
        ecrire(root, "modifie.txt", "avant");
        ecrire(root, "supprime.txt", "à supprimer");
        let base = fingerprint_tree(root).unwrap();

        ecrire(root, "modifie.txt", "après, plus long");
        std::fs::remove_file(root.join("supprime.txt")).unwrap();
        ecrire(root, "sous/dossier/nouveau.txt", "neuf");

        let diff = compute(&base, root).unwrap();
        assert_eq!(diff.counts(), (1, 1, 1));
        let par_chemin: BTreeMap<_, _> = diff
            .changes
            .iter()
            .map(|c| (c.path.display().to_string(), c.kind))
            .collect();
        assert_eq!(par_chemin["modifie.txt"], ChangeKind::Modified);
        assert_eq!(par_chemin["supprime.txt"], ChangeKind::Deleted);
        assert_eq!(par_chemin["sous/dossier/nouveau.txt"], ChangeKind::Added);
        assert!(!par_chemin.contains_key("garde.txt"));
    }

    #[test]
    fn contenu_identique_de_meme_taille_n_est_pas_un_changement() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        ecrire(root, "a.txt", "abc");
        let base = fingerprint_tree(root).unwrap();
        // Réécriture du même contenu : l'empreinte ne change pas, donc aucun changement.
        ecrire(root, "a.txt", "abc");
        assert!(compute(&base, root).unwrap().is_empty());
    }

    #[test]
    fn changement_de_permissions_compte_comme_modification() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        ecrire(root, "script.sh", "#!/bin/sh\n");
        let base = fingerprint_tree(root).unwrap();
        std::fs::set_permissions(
            root.join("script.sh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert_eq!(compute(&base, root).unwrap().counts(), (0, 1, 0));
    }

    #[test]
    fn liens_symboliques_ignores() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        ecrire(root, "reel.txt", "x");
        std::os::unix::fs::symlink("/etc/passwd", root.join("evasion")).unwrap();
        let empreintes = fingerprint_tree(root).unwrap();
        assert_eq!(empreintes.len(), 1);
        assert!(empreintes.contains_key(Path::new("reel.txt")));
    }

    #[test]
    fn rendu_lisible() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let base = fingerprint_tree(root).unwrap();
        ecrire(root, "rapport.pdf", "contenu");
        let rendu = compute(&base, root).unwrap().render();
        assert!(rendu.contains("+ rapport.pdf"), "{rendu}");
        assert!(rendu.contains("1 ajouté"), "{rendu}");
        assert_eq!(Diff::default().render(), "aucun changement\n");
    }

    #[test]
    fn un_rendu_borne_montre_les_premiers_et_compte_le_reste() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let base = fingerprint_tree(root).unwrap();
        for n in 0..5 {
            ecrire(root, &format!("f{n}.txt"), "contenu");
        }
        let diff = compute(&base, root).unwrap();
        let rendu = diff.render_limited(2);
        assert_eq!(
            rendu.lines().filter(|l| l.starts_with("+ ")).count(),
            2,
            "{rendu}"
        );
        assert!(rendu.contains("… et 3 autre(s) changement(s)"), "{rendu}");
        assert!(
            rendu.contains("5 ajouté(s)"),
            "le compte reste entier : {rendu}"
        );
        assert!(!diff.render().contains('…'));
    }

    #[test]
    fn arbre_inexistant_donne_un_etat_vide() {
        assert!(
            fingerprint_tree(Path::new("/n/existe/pas"))
                .unwrap()
                .is_empty()
        );
    }
}
