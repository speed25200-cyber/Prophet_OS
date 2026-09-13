//! Une validation ou son annulation ne doivent pas perdre une modification indépendante.
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::path::Path;

use sfs::{Workspace, WorkspaceState};
use time::OffsetDateTime;

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read(root: &Path, path: &str) -> String {
    std::fs::read_to_string(root.join(path)).unwrap()
}

fn prepare(home: &Path) -> Workspace {
    write(home, "docs/a.txt", "a initial");
    write(home, "docs/z.txt", "z initial");
    let work =
        Workspace::begin(home, "publication", &["~/docs"], OffsetDateTime::now_utc()).unwrap();
    write(&work.workdir(), "docs/a.txt", "a proposé");
    write(&work.workdir(), "docs/z.txt", "z proposé");
    work
}

#[test]
fn commit_refuse_tout_le_lot_si_un_original_a_change() {
    let home = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    write(home.path(), "docs/z.txt", "z humain");
    assert!(work.commit(OffsetDateTime::now_utc(), None).is_err());
    assert_eq!(read(home.path(), "docs/a.txt"), "a initial");
    assert_eq!(read(home.path(), "docs/z.txt"), "z humain");
    assert_eq!(work.state(), WorkspaceState::Open);
}

#[test]
fn commit_ne_remplace_pas_un_ajout_humain_concurrent() {
    let home = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    write(&work.workdir(), "docs/new.txt", "proposé");
    write(home.path(), "docs/new.txt", "créé par l'humain");
    assert!(work.commit(OffsetDateTime::now_utc(), None).is_err());
    assert_eq!(read(home.path(), "docs/new.txt"), "créé par l'humain");
    assert_eq!(read(home.path(), "docs/a.txt"), "a initial");
}

#[test]
fn commit_ne_supprime_pas_un_fichier_humain_modifie() {
    let home = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    std::fs::remove_file(work.workdir().join("docs/z.txt")).unwrap();
    write(home.path(), "docs/z.txt", "à conserver");
    assert!(work.commit(OffsetDateTime::now_utc(), None).is_err());
    assert_eq!(read(home.path(), "docs/z.txt"), "à conserver");
    assert_eq!(read(home.path(), "docs/a.txt"), "a initial");
}

#[test]
fn commit_refuse_un_ajout_hors_du_perimetre_capture() {
    let home = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    write(&work.workdir(), "hors-perimetre/note.txt", "intrusion");
    assert!(work.commit(OffsetDateTime::now_utc(), None).is_err());
    assert!(!home.path().join("hors-perimetre").exists());
    assert_eq!(read(home.path(), "docs/a.txt"), "a initial");
}

#[test]
fn commit_refuse_un_parent_remplace_par_un_lien() {
    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    write(outside.path(), "a.txt", "a initial");
    write(outside.path(), "z.txt", "z initial");
    std::fs::rename(home.path().join("docs"), home.path().join("originaux")).unwrap();
    symlink(outside.path(), home.path().join("docs")).unwrap();
    assert!(work.commit(OffsetDateTime::now_utc(), None).is_err());
    assert_eq!(read(outside.path(), "a.txt"), "a initial");
    assert_eq!(read(outside.path(), "z.txt"), "z initial");
}

#[test]
fn undo_refuse_tout_le_lot_si_un_fichier_a_ete_retouche() {
    let home = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    work.commit(OffsetDateTime::now_utc(), None).unwrap();
    write(home.path(), "docs/z.txt", "z retouché");
    assert!(work.undo().is_err());
    assert_eq!(read(home.path(), "docs/a.txt"), "a proposé");
    assert_eq!(read(home.path(), "docs/z.txt"), "z retouché");
    assert_eq!(work.state(), WorkspaceState::Committed);
}

#[test]
fn undo_ne_supprime_pas_un_ajout_retravaille() {
    let home = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    write(&work.workdir(), "docs/new.txt", "proposé");
    work.commit(OffsetDateTime::now_utc(), None).unwrap();
    write(home.path(), "docs/new.txt", "travail ultérieur");
    assert!(work.undo().is_err());
    assert_eq!(read(home.path(), "docs/new.txt"), "travail ultérieur");
    assert_eq!(read(home.path(), "docs/a.txt"), "a proposé");
}

#[test]
fn undo_ne_remplace_pas_un_fichier_recree_apres_suppression() {
    let home = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    std::fs::remove_file(work.workdir().join("docs/z.txt")).unwrap();
    work.commit(OffsetDateTime::now_utc(), None).unwrap();
    write(home.path(), "docs/z.txt", "nouveau document");
    assert!(work.undo().is_err());
    assert_eq!(read(home.path(), "docs/z.txt"), "nouveau document");
}

#[test]
fn undo_conserve_une_modification_des_permissions() {
    let home = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    work.commit(OffsetDateTime::now_utc(), None).unwrap();
    std::fs::set_permissions(
        home.path().join("docs/z.txt"),
        std::fs::Permissions::from_mode(0o400),
    )
    .unwrap();
    assert!(work.undo().is_err());
    assert_eq!(read(home.path(), "docs/z.txt"), "z proposé");
    assert_eq!(
        std::fs::metadata(home.path().join("docs/z.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o400
    );
}

#[test]
fn undo_refuse_une_sauvegarde_alteree_avant_de_toucher_aux_originaux() {
    let home = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    work.commit(OffsetDateTime::now_utc(), None).unwrap();
    write(
        home.path(),
        ".prophet/tasks/publication/restore/docs/z.txt",
        "sauvegarde altérée",
    );
    assert!(work.undo().is_err());
    assert_eq!(read(home.path(), "docs/a.txt"), "a proposé");
    assert_eq!(read(home.path(), "docs/z.txt"), "z proposé");
}

#[test]
fn la_publication_refuse_un_travail_modifie_depuis_l_examen() {
    let home = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    let reviewed = work.seal_review().unwrap();
    write(&work.workdir(), "docs/z.txt", "autre proposition");
    assert!(
        work.commit_review(&reviewed, OffsetDateTime::now_utc(), None)
            .is_err()
    );
    assert_eq!(read(home.path(), "docs/a.txt"), "a initial");
    assert_eq!(read(home.path(), "docs/z.txt"), "z initial");
}

#[test]
fn undo_utilise_la_version_publiee_meme_si_le_travail_change() {
    let home = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    work.commit(OffsetDateTime::now_utc(), None).unwrap();
    write(
        &work.workdir(),
        "docs/z.txt",
        "autre travail après publication",
    );
    drop(work);
    let mut reopened = Workspace::open(home.path(), "publication").unwrap();
    reopened.undo().unwrap();
    assert_eq!(read(home.path(), "docs/z.txt"), "z initial");
    assert_eq!(read(home.path(), "docs/a.txt"), "a initial");
}

#[test]
fn deux_instances_ne_republient_pas_le_meme_lot() {
    let home = tempfile::tempdir().unwrap();
    let mut first = prepare(home.path());
    let mut second = Workspace::open(home.path(), "publication").unwrap();
    first.commit(OffsetDateTime::now_utc(), None).unwrap();
    assert!(second.commit(OffsetDateTime::now_utc(), None).is_err());
    second.undo().unwrap();
    assert!(first.undo().is_err());
    assert_eq!(read(home.path(), "docs/a.txt"), "a initial");
}

#[test]
fn provenance_publiee_puis_restauree_sur_les_fichiers_reels() {
    let home = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    let original = sfs::Provenance {
        task: "ancienne".into(),
        ..Default::default()
    };
    let supported = sfs::write_provenance(&home.path().join("docs/a.txt"), &original).unwrap();
    let published = sfs::Provenance {
        task: "publication".into(),
        model: "local:essai".into(),
        ..Default::default()
    };
    work.commit(OffsetDateTime::now_utc(), Some(&published))
        .unwrap();
    if supported {
        assert_eq!(
            sfs::read_provenance(&home.path().join("docs/a.txt")).unwrap(),
            Some(published)
        );
    }
    work.undo().unwrap();
    if supported {
        assert_eq!(
            sfs::read_provenance(&home.path().join("docs/a.txt")).unwrap(),
            Some(original)
        );
    }
}

#[test]
fn publication_et_undo_conservent_les_attributs_humains() {
    let home = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    let file = home.path().join("docs/a.txt");
    xattr::set(&file, "user.note", b"information humaine").unwrap();
    work.commit(OffsetDateTime::now_utc(), None).unwrap();
    assert_eq!(
        xattr::get(&file, "user.note").unwrap(),
        Some(b"information humaine".to_vec())
    );
    work.undo().unwrap();
    assert_eq!(
        xattr::get(&file, "user.note").unwrap(),
        Some(b"information humaine".to_vec())
    );
}

#[test]
fn undo_refuse_un_attribut_modifie_apres_publication() {
    let home = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    work.commit(OffsetDateTime::now_utc(), None).unwrap();
    let file = home.path().join("docs/z.txt");
    xattr::set(&file, "user.note", "information ajoutée".as_bytes()).unwrap();
    assert!(work.undo().is_err());
    assert_eq!(read(home.path(), "docs/a.txt"), "a proposé");
    assert_eq!(
        xattr::get(&file, "user.note").unwrap(),
        Some("information ajoutée".as_bytes().to_vec())
    );
}

fn acl(owner: u16, mask: u16, other: u16, named: u16) -> Vec<u8> {
    let mut bytes = 2_u32.to_le_bytes().to_vec();
    for (tag, permission, id) in [
        (1_u16, owner, u32::MAX),
        (2, named, 4242),
        (4, 4, u32::MAX),
        (16, mask, u32::MAX),
        (32, other, u32::MAX),
    ] {
        bytes.extend(tag.to_le_bytes());
        bytes.extend(permission.to_le_bytes());
        bytes.extend(id.to_le_bytes());
    }
    bytes
}

#[test]
fn acl_humaine_preservee_et_modification_ulterieure_refusee() {
    let home = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    let file = home.path().join("docs/z.txt");
    let initial = acl(6, 4, 4, 4);
    xattr::set(&file, "system.posix_acl_access", &initial).unwrap();
    work.commit(OffsetDateTime::now_utc(), None).unwrap();
    assert_eq!(
        xattr::get(&file, "system.posix_acl_access").unwrap(),
        Some(initial.clone())
    );
    xattr::set(&file, "system.posix_acl_access", &acl(6, 4, 4, 6)).unwrap();
    assert!(work.undo().is_err());
    assert_eq!(read(home.path(), "docs/a.txt"), "a proposé");
    xattr::set(&file, "system.posix_acl_access", &initial).unwrap();
    work.undo().unwrap();
    assert_eq!(
        xattr::get(&file, "system.posix_acl_access").unwrap(),
        Some(initial)
    );
}

#[test]
fn nouvel_arbre_herite_de_l_acl_du_dossier_humain() {
    let home = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    xattr::set(
        home.path().join("docs"),
        "system.posix_acl_default",
        &acl(7, 5, 0, 4),
    )
    .unwrap();
    write(&work.workdir(), "docs/new-folder/note.txt", "nouveau");
    std::fs::set_permissions(
        work.workdir().join("docs/new-folder/note.txt"),
        std::fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    work.commit(OffsetDateTime::now_utc(), None).unwrap();
    assert_eq!(
        xattr::get(
            home.path().join("docs/new-folder/note.txt"),
            "system.posix_acl_access"
        )
        .unwrap(),
        Some(acl(6, 4, 0, 4))
    );
    assert_eq!(
        xattr::get(
            home.path().join("docs/new-folder"),
            "system.posix_acl_default"
        )
        .unwrap(),
        Some(acl(7, 5, 0, 4))
    );
    work.undo().unwrap();
    assert!(!home.path().join("docs/new-folder/note.txt").exists());
}

#[test]
fn un_journal_ou_un_index_altere_interdit_la_reprise() {
    for file in ["publication.json", "publication-review.json"] {
        let home = tempfile::tempdir().unwrap();
        let mut work = prepare(home.path());
        work.commit(OffsetDateTime::now_utc(), None).unwrap();
        let path = home.path().join(".prophet/tasks/publication").join(file);
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        if file == "publication.json" {
            value["cursor"]["next"] = 0.into();
        } else {
            value["review"]["entries"][0]["path"] = "docs/z.txt".into();
        }
        std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(Workspace::open(home.path(), "publication").is_err());
        assert!(work.recover_publication().is_err());
        assert_eq!(read(home.path(), "docs/a.txt"), "a proposé");
    }
}

#[test]
fn undo_conserve_la_date_de_modification_initiale() {
    use std::os::unix::fs::MetadataExt as _;
    let home = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    let file = home.path().join("docs/a.txt");
    let initial = std::fs::metadata(&file).unwrap();
    work.commit(OffsetDateTime::now_utc(), None).unwrap();
    work.undo().unwrap();
    let restored = std::fs::metadata(file).unwrap();
    assert_eq!(
        (restored.mtime(), restored.mtime_nsec()),
        (initial.mtime(), initial.mtime_nsec())
    );
    assert_eq!(
        (restored.uid(), restored.gid()),
        (initial.uid(), initial.gid())
    );
}

#[test]
fn la_liste_refuse_les_metadonnees_liees_hors_de_la_capture() {
    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let _work = prepare(home.path());
    let meta = home.path().join(".prophet/tasks/publication/meta.json");
    let external = outside.path().join("metadata.json");
    std::fs::rename(&meta, &external).unwrap();
    symlink(&external, &meta).unwrap();
    assert!(Workspace::list(home.path()).is_err());
}

#[test]
fn la_liste_refuse_un_identifiant_different_du_dossier() {
    let home = tempfile::tempdir().unwrap();
    let _work = prepare(home.path());
    let meta = home.path().join(".prophet/tasks/publication/meta.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&meta).unwrap()).unwrap();
    value["task"] = "intrus".into();
    let intrus = home.path().join(".prophet/tasks/intrus");
    std::fs::create_dir(&intrus).unwrap();
    std::fs::write(meta, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(Workspace::list(home.path()).is_err());
}

#[test]
fn la_liste_signale_un_manifeste_de_publication_manquant() {
    let home = tempfile::tempdir().unwrap();
    let mut work = prepare(home.path());
    work.commit(OffsetDateTime::now_utc(), None).unwrap();
    std::fs::remove_file(
        home.path()
            .join(".prophet/tasks/publication/publication-review.json"),
    )
    .unwrap();
    assert!(Workspace::list(home.path()).is_err());
}
