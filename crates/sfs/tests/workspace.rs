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
