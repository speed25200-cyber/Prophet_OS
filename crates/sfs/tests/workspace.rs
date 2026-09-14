//! Tests d'intégration du système de fichiers sémantique.
//!
//! Le scénario de référence du plan (M4-T2) : une tâche modifie cinquante fichiers, on valide,
//! puis on annule, et l'état doit être identique à l'octet près.

use std::path::Path;

use sfs::{ChangeKind, Provenance, Workspace, WorkspaceState};
use time::OffsetDateTime;

fn now() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_789_000_000).unwrap()
}

fn ecrire(root: &Path, relative: &str, contenu: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contenu).unwrap();
}

fn empreinte_du_home(home: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![home.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            // L'espace interne des tâches ne fait pas partie de l'état utilisateur.
            if path.file_name().is_some_and(|n| n == ".prophet") {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                let contenu = std::fs::read(&path).unwrap();
                out.push((
                    path.strip_prefix(home).unwrap().display().to_string(),
                    blake3::hash(&contenu).to_hex().to_string(),
                ));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn cinquante_fichiers_modifies_valides_puis_annules() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    for i in 0..50 {
        ecrire(
            home,
            &format!("ventes/f{i:02}.csv"),
            &format!("origine {i}"),
        );
    }
    ecrire(home, "prive/journal.txt", "ne doit pas bouger");
    let avant = empreinte_du_home(home);

    let mut espace = Workspace::begin(home, "task:01", &["~/ventes"], now()).unwrap();
    let work = espace.workdir();
    for i in 0..50 {
        ecrire(
            &work,
            &format!("ventes/f{i:02}.csv"),
            &format!("modifié {i}"),
        );
    }
    ecrire(&work, "ventes/out/rapport.md", "# Rapport");

    // Rien n'a encore bougé dans l'espace de l'utilisateur.
    assert_eq!(
        empreinte_du_home(home),
        avant,
        "aucune fuite avant validation"
    );

    let diff = espace.diff().unwrap();
    assert_eq!(diff.counts(), (1, 50, 0));

    espace
        .commit(
            now(),
            Some(&Provenance {
                task: "task:01".into(),
                agent: "org.test.agent".into(),
                step: 18,
                model: "local:qwen3-8b".into(),
            }),
        )
        .unwrap();
    assert_eq!(espace.state(), WorkspaceState::Committed);
    let apres = empreinte_du_home(home);
    assert_ne!(apres, avant);
    assert_eq!(
        std::fs::read_to_string(home.join("ventes/f07.csv")).unwrap(),
        "modifié 7"
    );

    espace.undo().unwrap();
    assert_eq!(espace.state(), WorkspaceState::RolledBack);
    assert_eq!(
        empreinte_du_home(home),
        avant,
        "après annulation, l'état doit être identique à l'octet près"
    );
    assert!(!home.join("ventes/out/rapport.md").exists());
}

#[test]
fn suppression_validee_puis_annulee() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    ecrire(home, "ventes/a.csv", "contenu a");
    ecrire(home, "ventes/b.csv", "contenu b");

    let mut espace = Workspace::begin(home, "task:01", &["~/ventes"], now()).unwrap();
    std::fs::remove_file(espace.workdir().join("ventes/a.csv")).unwrap();

    let diff = espace.diff().unwrap();
    assert_eq!(diff.counts(), (0, 0, 1));
    assert_eq!(diff.changes[0].kind, ChangeKind::Deleted);

    espace.commit(now(), None).unwrap();
    assert!(!home.join("ventes/a.csv").exists());

    espace.undo().unwrap();
    assert_eq!(
        std::fs::read_to_string(home.join("ventes/a.csv")).unwrap(),
        "contenu a"
    );
}

#[test]
fn abandon_ne_touche_a_rien() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    ecrire(home, "ventes/a.csv", "origine");
    let avant = empreinte_du_home(home);

    let mut espace = Workspace::begin(home, "task:01", &["~/ventes"], now()).unwrap();
    ecrire(&espace.workdir(), "ventes/a.csv", "saccagé");
    ecrire(&espace.workdir(), "ventes/parasite.txt", "indésirable");

    espace.abandon().unwrap();
    assert_eq!(espace.state(), WorkspaceState::Abandoned);
    assert_eq!(empreinte_du_home(home), avant);
}

#[test]
fn ecriture_hors_perimetre_impossible_a_traduire() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    std::fs::create_dir_all(home.join("ventes")).unwrap();
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    let espace = Workspace::begin(home, "task:01", &["~/ventes"], now()).unwrap();

    assert!(espace.to_work_path(&home.join("ventes/q3.csv")).is_some());
    assert!(
        espace.to_work_path(&home.join(".ssh/id_ed25519")).is_none(),
        "hors périmètre : aucune traduction possible"
    );
    assert!(espace.to_work_path(Path::new("/etc/shadow")).is_none());
}

#[test]
fn perimetre_hors_du_home_refuse() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    assert!(Workspace::begin(home, "task:01", &["/etc"], now()).is_err());
    assert!(Workspace::begin(home, "task:02", &["~/../autre"], now()).is_err());
}

#[test]
fn reouverture_apres_redemarrage() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    ecrire(home, "ventes/a.csv", "origine");
    {
        let espace = Workspace::begin(home, "task:01", &["~/ventes"], now()).unwrap();
        ecrire(&espace.workdir(), "ventes/a.csv", "modifié");
    }
    let mut espace = Workspace::open(home, "task:01").unwrap();
    assert_eq!(espace.state(), WorkspaceState::Open);
    assert_eq!(espace.diff().unwrap().counts(), (0, 1, 0));
    espace.commit(now(), None).unwrap();
    assert_eq!(
        std::fs::read_to_string(home.join("ventes/a.csv")).unwrap(),
        "modifié"
    );
}

#[test]
fn double_validation_refusee() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    ecrire(home, "ventes/a.csv", "x");
    let mut espace = Workspace::begin(home, "task:01", &["~/ventes"], now()).unwrap();
    espace.commit(now(), None).unwrap();
    assert!(espace.commit(now(), None).is_err());
    assert!(espace.abandon().is_err());
}

#[test]
fn annulation_sans_validation_refusee() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    std::fs::create_dir_all(home.join("ventes")).unwrap();
    let mut espace = Workspace::begin(home, "task:01", &["~/ventes"], now()).unwrap();
    assert!(espace.undo().is_err());
}

#[test]
fn transaction_publiee_en_une_fois() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    std::fs::create_dir_all(home.join("ventes")).unwrap();
    let espace = Workspace::begin(home, "task:01", &["~/ventes"], now()).unwrap();

    let mut tx = espace.tx_begin("tx1").unwrap();
    tx.write(Path::new("ventes/a.csv"), b"a").unwrap();
    tx.write(Path::new("ventes/b.csv"), b"b").unwrap();
    // Avant validation de la transaction, l'espace de travail ne voit rien.
    assert!(espace.diff().unwrap().is_empty());
    assert_eq!(tx.commit().unwrap(), 2);
    assert_eq!(espace.diff().unwrap().counts(), (2, 0, 0));
}

#[test]
fn transaction_abandonnee_ne_laisse_rien() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    std::fs::create_dir_all(home.join("ventes")).unwrap();
    let espace = Workspace::begin(home, "task:01", &["~/ventes"], now()).unwrap();

    let mut tx = espace.tx_begin("tx1").unwrap();
    tx.write(Path::new("ventes/a.csv"), b"a").unwrap();
    tx.abort().unwrap();
    assert!(espace.diff().unwrap().is_empty());
}

#[test]
fn transaction_interrompue_est_balayee() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    std::fs::create_dir_all(home.join("ventes")).unwrap();
    let espace = Workspace::begin(home, "task:01", &["~/ventes"], now()).unwrap();

    // Simule une tâche tuée au milieu : la transaction reste sur le disque.
    let mut tx = espace.tx_begin("tx-interrompue").unwrap();
    tx.write(Path::new("ventes/partiel.csv"), b"moitie")
        .unwrap();
    std::mem::forget(tx);

    assert!(
        espace.diff().unwrap().is_empty(),
        "un état partiel ne doit jamais apparaître comme un changement"
    );
    assert_eq!(espace.sweep_transactions().unwrap(), 1);
    assert!(espace.diff().unwrap().is_empty());
}

#[test]
fn dorsale_detectee_et_limites_annoncees() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    std::fs::create_dir_all(home.join("ventes")).unwrap();
    let espace = Workspace::begin(home, "task:01", &["~/ventes"], now()).unwrap();
    let backend = espace.backend();
    assert!(!backend.reason.is_empty());
    if !backend.has_native_snapshots() {
        assert!(backend.limitations().contains("dernière validation"));
    }
}

/// Le relais (ADR 0039) : une sous-tâche part de l'espace de travail de son parent, non des
/// fichiers de l'humain, et ce qu'elle change y revient, dans les périmètres du parent ; le
/// parent publie le tout, d'un seul tenant.
#[test]
fn une_sous_tache_part_de_l_espace_du_parent_et_y_rapporte_ses_changements() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let lire = |p: std::path::PathBuf| std::fs::read_to_string(p).unwrap();
    ecrire(home, "docs/a.txt", "a de l'humain");
    ecrire(home, "notes/n.txt", "note de l'humain");
    let mut parent = Workspace::begin(home, "parent", &["docs"], now()).unwrap();
    ecrire(&parent.workdir(), "docs/a.txt", "a du parent");
    ecrire(&parent.workdir(), "docs/b.txt", "b du parent");

    // L'enfant voit le travail du parent ; un périmètre que le parent n'a pas vient du home.
    let enfant = Workspace::begin_authorized_from(
        home,
        "enfant",
        &["docs".into(), "notes".into()],
        now(),
        &|_| true,
        Some(&parent.workdir()),
    )
    .unwrap();
    assert_eq!(lire(enfant.workdir().join("docs/a.txt")), "a du parent");
    assert_eq!(lire(enfant.workdir().join("docs/b.txt")), "b du parent");
    assert_eq!(
        lire(enfant.workdir().join("notes/n.txt")),
        "note de l'humain"
    );
    assert!(enfant.diff().unwrap().is_empty());

    ecrire(&enfant.workdir(), "docs/a.txt", "a de l'enfant");
    ecrire(&enfant.workdir(), "docs/c.txt", "c de l'enfant");
    std::fs::remove_file(enfant.workdir().join("docs/b.txt")).unwrap();
    // Hors des périmètres du parent : reste chez l'enfant.
    ecrire(&enfant.workdir(), "notes/n.txt", "note de l'enfant");
    let rapport = enfant.carry_into(&parent).unwrap();
    let chemins: Vec<String> = rapport
        .changes
        .iter()
        .map(|c| c.path.display().to_string())
        .collect();
    assert_eq!(chemins, ["docs/a.txt", "docs/b.txt", "docs/c.txt"]);
    assert_eq!(lire(parent.workdir().join("docs/a.txt")), "a de l'enfant");
    assert!(!parent.workdir().join("docs/b.txt").exists());
    assert_eq!(lire(parent.workdir().join("docs/c.txt")), "c de l'enfant");
    assert!(!parent.workdir().join("notes").exists());
    assert_eq!(
        lire(enfant.workdir().join("notes/n.txt")),
        "note de l'enfant"
    );
    // Le home n'a pas bougé : seule la publication du parent l'atteindra.
    assert_eq!(lire(home.join("docs/a.txt")), "a de l'humain");
    assert!(!home.join("docs/c.txt").exists());

    let diff = parent.commit(now(), None).unwrap();
    let publies: Vec<(String, ChangeKind)> = diff
        .changes
        .iter()
        .map(|c| (c.path.display().to_string(), c.kind))
        .collect();
    assert_eq!(
        publies,
        [
            ("docs/a.txt".to_owned(), ChangeKind::Modified),
            ("docs/c.txt".to_owned(), ChangeKind::Added)
        ]
    );
    assert_eq!(lire(home.join("docs/a.txt")), "a de l'enfant");
    assert_eq!(lire(home.join("docs/c.txt")), "c de l'enfant");
    assert_eq!(lire(home.join("notes/n.txt")), "note de l'humain");
    // Un parent qui n'est plus ouvert ne reçoit plus rien.
    assert!(matches!(
        enfant.carry_into(&parent),
        Err(sfs::SfsError::BadState {
            state: WorkspaceState::Committed
        })
    ));
}
