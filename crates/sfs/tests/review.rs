//! L'examen humain lit les versions capturées, jamais une nouvelle lecture du document original.
use sfs::{PreviewContent, Workspace};
use std::os::unix::fs::symlink;
use time::OffsetDateTime;

fn workspace(home: &std::path::Path) -> Workspace {
    Workspace::begin_authorized(
        home,
        "review",
        &["~/docs".into()],
        OffsetDateTime::now_utc(),
        &|_| true,
    )
    .unwrap()
}

#[test]
fn les_deux_versions_restent_lisibles_quand_l_original_change() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/note.txt"), "Version initiale\n").unwrap();
    let work = workspace(dir.path());
    std::fs::write(
        work.workdir().join("docs/note.txt"),
        "Proposition nouvelle\n",
    )
    .unwrap();
    let index = work.seal_review().unwrap();
    std::fs::write(
        dir.path().join("docs/note.txt"),
        "Modification humaine ultérieure\n",
    )
    .unwrap();
    let index: sfs::ReviewIndex =
        serde_json::from_str(&serde_json::to_string(&index).unwrap()).unwrap();
    let review = index.read(dir.path(), "review", "docs/note.txt").unwrap();
    assert_eq!(
        review.before.unwrap().content,
        PreviewContent::Text {
            text: "Version initiale\n".into()
        }
    );
    assert_eq!(
        review.after.unwrap().content,
        PreviewContent::Text {
            text: "Proposition nouvelle\n".into()
        }
    );
    assert_eq!(index.diff().counts(), (0, 1, 0));
}

#[test]
fn ajouts_suppressions_et_fichiers_non_textuels_sont_explicites() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/deleted.txt"), "Ancien texte").unwrap();
    let work = workspace(dir.path());
    std::fs::remove_file(work.workdir().join("docs/deleted.txt")).unwrap();
    std::fs::write(work.workdir().join("docs/new.bin"), [0, 255, 1]).unwrap();
    std::fs::write(work.workdir().join("docs/large.txt"), vec![b'x'; 65537]).unwrap();
    let index = work.seal_review().unwrap();
    let deleted = index
        .read(dir.path(), "review", "docs/deleted.txt")
        .unwrap();
    assert!(deleted.before.is_some());
    assert!(deleted.after.is_none());
    let binary = index.read(dir.path(), "review", "docs/new.bin").unwrap();
    assert!(binary.before.is_none());
    assert_eq!(binary.after.unwrap().content, PreviewContent::Binary);
    assert_eq!(
        index
            .read(dir.path(), "review", "docs/large.txt")
            .unwrap()
            .after
            .unwrap()
            .content,
        PreviewContent::TooLarge
    );
    assert_eq!(index.diff().counts(), (2, 0, 1));
}

#[test]
fn le_travail_modifie_ou_un_fichier_supprime_reapparu_sont_refuses() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/deleted.txt"), "Avant").unwrap();
    let work = workspace(dir.path());
    std::fs::remove_file(work.workdir().join("docs/deleted.txt")).unwrap();
    std::fs::write(work.workdir().join("docs/note.txt"), "A").unwrap();
    let index = work.seal_review().unwrap();
    std::fs::write(work.workdir().join("docs/note.txt"), "B").unwrap();
    assert!(index.read(dir.path(), "review", "docs/note.txt").is_err());
    std::fs::write(work.workdir().join("docs/deleted.txt"), "Revenu").unwrap();
    assert!(
        index
            .read(dir.path(), "review", "docs/deleted.txt")
            .is_err()
    );
}

#[test]
fn chemins_hors_index_liens_et_baseline_alteree_sont_refuses() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/note.txt"), "Avant").unwrap();
    let work = workspace(dir.path());
    std::fs::write(work.workdir().join("docs/note.txt"), "Après").unwrap();
    let index = work.seal_review().unwrap();
    for path in [
        "/etc/passwd",
        "../docs/note.txt",
        "docs/other.txt",
        "docs/../docs/note.txt",
    ] {
        assert!(index.read(dir.path(), "review", path).is_err(), "{path}");
    }
    assert!(
        index
            .read(dir.path(), "../review", "docs/note.txt")
            .is_err()
    );
    let baseline = dir.path().join(".prophet/tasks/review/base/docs/note.txt");
    std::fs::write(&baseline, "Autre").unwrap();
    assert!(index.read(dir.path(), "review", "docs/note.txt").is_err());
    std::fs::write(&baseline, "Avant").unwrap();
    std::fs::remove_file(work.workdir().join("docs/note.txt")).unwrap();
    symlink(
        dir.path().join("docs/note.txt"),
        work.workdir().join("docs/note.txt"),
    )
    .unwrap();
    assert!(index.read(dir.path(), "review", "docs/note.txt").is_err());
    assert!(work.seal_review().is_err());
}
