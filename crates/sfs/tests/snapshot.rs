//! Captures de service : exactitude du diff, droits et racines sans liens.
use sfs::Workspace;
use time::OffsetDateTime;

#[test]
fn la_capture_preserve_les_fichiers_et_le_diff_initial_est_vide() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("docs/sub")).unwrap();
    std::fs::write(dir.path().join("docs/sub/texte"), "contenu").unwrap();
    let w = Workspace::begin_authorized(
        dir.path(),
        "snapshot",
        &["~/docs".into()],
        OffsetDateTime::now_utc(),
        &|_| true,
    )
    .unwrap();
    assert!(w.diff().unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(w.workdir().join("docs/sub/texte")).unwrap(),
        "contenu"
    );
    assert!(
        Workspace::begin_authorized(
            dir.path(),
            "snapshot",
            &["~/docs".into()],
            OffsetDateTime::now_utc(),
            &|_| true
        )
        .is_err()
    );
    std::fs::write(w.workdir().join("docs/sub/texte"), "nouveau").unwrap();
    assert_eq!(w.diff().unwrap().counts(), (0, 1, 0));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("docs/sub/texte")).unwrap(),
        "contenu"
    );
}

#[test]
fn la_capture_refuse_liens_et_descendants_sans_droit() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("prive"), "SECRET-SYNTHETIQUE").unwrap();
    std::os::unix::fs::symlink(dir.path().join("prive"), dir.path().join("docs/lien")).unwrap();
    assert!(
        Workspace::begin_authorized(
            dir.path(),
            "lien",
            &["~/docs".into()],
            OffsetDateTime::now_utc(),
            &|_| true
        )
        .is_err()
    );
    std::fs::remove_file(dir.path().join("docs/lien")).unwrap();
    std::fs::write(dir.path().join("docs/refuse"), "SECRET-SYNTHETIQUE").unwrap();
    assert!(
        Workspace::begin_authorized(
            dir.path(),
            "droit",
            &["~/docs".into()],
            OffsetDateTime::now_utc(),
            &|p| !p.ends_with("refuse")
        )
        .is_err()
    );
    assert!(
        !dir.path()
            .join(".prophet/tasks/droit/work/docs/refuse")
            .exists()
    );
}

#[test]
fn la_capture_ne_cree_rien_via_une_racine_privee_detournee() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join(".prophet")).unwrap();
    assert!(
        Workspace::begin_authorized(dir.path(), "non", &[], OffsetDateTime::now_utc(), &|_| true)
            .is_err()
    );
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
fn un_perimetre_nouveau_ne_cree_aucun_repertoire_dans_le_home() {
    let dir = tempfile::tempdir().unwrap();
    let w = Workspace::begin_authorized(
        dir.path(),
        "creation",
        &["~/absent/nouveau".into()],
        OffsetDateTime::now_utc(),
        &|_| true,
    )
    .unwrap();
    assert!(w.diff().unwrap().is_empty());
    assert!(!dir.path().join("absent").exists());
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("lien")).unwrap();
    assert!(
        Workspace::begin_authorized(
            dir.path(),
            "detour",
            &["~/lien/nouveau".into()],
            OffsetDateTime::now_utc(),
            &|_| true
        )
        .is_err()
    );
    assert!(!outside.path().join("nouveau").exists());
}
