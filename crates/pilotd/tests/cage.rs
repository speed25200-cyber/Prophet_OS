//! Les clients lancés dans leur cage (ADR 0056), avec le vrai `prophet-pilot-cage`.
//!
//! Ces essais exigent des espaces de noms utilisateur. Là où l'environnement ne les fournit pas
//! (le coureur « check »), ils le disent et s'arrêtent ; le coureur d'isolation les exige
//! (`PROPHET_EXIGER_ESPACES_DE_NOMS=1`).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pilotd::{Arrets, Error, Launcher, RunRequest};

const CAGE: &str = env!("CARGO_BIN_EXE_prophet-pilot-cage");

fn cage_disponible() -> bool {
    let disponible = sandboxd::Capabilities::probe().user_namespaces;
    assert!(
        disponible || std::env::var("PROPHET_EXIGER_ESPACES_DE_NOMS").as_deref() != Ok("1"),
        "espaces de noms indisponibles alors que PROPHET_EXIGER_ESPACES_DE_NOMS=1"
    );
    if !disponible {
        eprintln!("espaces de noms utilisateur indisponibles : essai de la cage non joué");
    }
    disponible
}

fn launcher(dir: &Path, overrides: &str) -> Launcher {
    Launcher {
        root: dir.join("etat"),
        user: "humain".into(),
        agentd_socket: dir.join("agentd.sock"),
        bridge: PathBuf::from("/bin/true"),
        runtime_dir: dir.join("run"),
        overrides: Launcher::parse_overrides(overrides).unwrap(),
        arrets: Arrets::default(),
        cage: PathBuf::from(CAGE),
        lecture_seule: Vec::new(),
    }
}

fn requete(driver: &str, intent: &str, wall_time_s: u64) -> RunRequest {
    RunRequest {
        task: "m-1".into(),
        driver: driver.into(),
        intent: intent.into(),
        wall_time_s,
        model: None,
    }
}

#[test]
fn un_client_de_remplacement_recoit_l_intention_et_rien_ne_reste_apres_lui() {
    if !cage_disponible() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let launcher = launcher(
        dir.path(),
        r#"{"codex": {"program": "/bin/echo", "args": ["fait :", "{intent}"]}}"#,
    );
    assert_eq!(launcher.status().drivers[1].connection, "simulated");
    let result = launcher.run(&requete("codex", "écrire", 10)).unwrap();
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.text, "fait : écrire");
    assert!(
        !dir.path().join("run/m-1.json").exists(),
        "la configuration est retirée après"
    );
    assert!(
        !dir.path().join("run/m-1.sock").exists(),
        "le socket filtré aussi"
    );
    assert!(
        !dir.path().join("etat/missions/m-1").exists(),
        "et les lieux de la cage"
    );
    assert!(
        !dir.path().join("run/m-1.racine").exists(),
        "et le point de montage de sa racine"
    );
}

#[test]
fn un_client_est_tue_a_la_demande_avec_ce_qu_il_a_lance() {
    if !cage_disponible() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    // Le client lance un sous-processus qui garderait sa sortie ouverte : l'arrêt doit emporter
    // la cage entière, sinon `run` attendrait la fin du sous-processus.
    let launcher = launcher(
        dir.path(),
        r#"{"gemini": {"program": "/bin/sh", "args": ["-c", "sleep 30 & wait"]}}"#,
    );
    let demandeur = launcher.clone();
    let stop = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(600));
        demandeur.stop("m-1")
    });
    let debut = Instant::now();
    let erreur = launcher
        .run(&requete("gemini", "attendre", 30))
        .unwrap_err();
    assert!(matches!(erreur, Error::Stopped { .. }), "{erreur}");
    assert!(
        debut.elapsed() < Duration::from_secs(5),
        "{:?}",
        debut.elapsed()
    );
    assert!(
        stop.join().unwrap(),
        "un client tournait pour cette mission"
    );
    assert!(!launcher.stop("autre"));
}

#[test]
fn un_client_trop_long_est_tue_et_un_client_absent_refuse() {
    if !cage_disponible() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let launcher = launcher(
        dir.path(),
        r#"{"gemini": {"program": "/bin/sleep", "args": ["30"]}}"#,
    );
    let debut = Instant::now();
    let erreur = launcher.run(&requete("gemini", "attendre", 1)).unwrap_err();
    assert!(matches!(erreur, Error::Timeout { .. }), "{erreur}");
    assert!(debut.elapsed() < Duration::from_secs(10));
    let erreur = launcher.run(&requete("claude-code", "x", 1)).unwrap_err();
    assert!(matches!(erreur, Error::NotReady { .. }), "{erreur}");
}

/// Ce que le client voit de la machine : un client de remplacement sonde sa cage et le dit. Il
/// ne voit ni la maison de l'humain, ni le socket de capd, ni les processus de l'hôte ; il écrit
/// dans sa maison, son temporaire et son profil privé, et seul ce dernier survit à la mission.
#[test]
fn le_client_ne_voit_ni_la_maison_ni_les_services_et_n_ecrit_que_dans_sa_cage() {
    if !cage_disponible() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let maison = dir.path().join("maison-humaine");
    std::fs::create_dir_all(maison.join("Documents")).unwrap();
    std::fs::write(maison.join("secret.txt"), "à l'humain").unwrap();
    let capd = dir.path().join("capd.sock");
    let _ecoute = std::os::unix::net::UnixListener::bind(&capd).unwrap();
    let profil = dir.path().join("etat/providers/gemini/humain");
    std::fs::create_dir_all(&profil).unwrap();
    // Le lanceur ne rend que la dernière ligne du client : la sonde dit tout sur une seule.
    let sonde = format!(
        r#"
        [ -e "{secret}" ] && r="maison:visible" || r="maison:cachee"
        touch "{maison}/intrus" 2>/dev/null && r="$r ecriture-maison:permise" || r="$r ecriture-maison:refusee"
        [ -e "{capd}" ] && r="$r capd:visible" || r="$r capd:cache"
        touch "$HOME/note" && r="$r maison-mission:ecrite"
        touch "$TMPDIR/note" && r="$r temporaire:ecrit"
        touch "$GEMINI_CONFIG_DIR/session" && r="$r profil:ecrit"
        touch /intrus 2>/dev/null && r="$r racine:ecrite" || r="$r racine:refusee"
        echo "$r processus:$(ls /proc | grep -c '^[0-9]')"
        "#,
        secret = maison.join("secret.txt").display(),
        maison = maison.display(),
        capd = capd.display(),
    );
    let overrides = serde_json::json!({
        "gemini": {"program": "/bin/sh", "args": ["-c", sonde]}
    })
    .to_string();
    let launcher = launcher(dir.path(), &overrides);
    let result = launcher.run(&requete("gemini", "sonder", 20)).unwrap();
    let texte = result.text;
    for attendu in [
        "maison:cachee",
        "ecriture-maison:refusee",
        "capd:cache",
        "maison-mission:ecrite",
        "temporaire:ecrit",
        "profil:ecrit",
        "racine:refusee",
    ] {
        assert!(texte.contains(attendu), "{attendu} attendu dans :\n{texte}");
    }
    // Le processus 1 de la cage, le shell du client, la sous-commande `$(…)`, `ls` et `grep` :
    // cinq, et aucun de l'hôte, qui en compte des centaines.
    let processus: usize = texte
        .split_whitespace()
        .find_map(|l| l.strip_prefix("processus:"))
        .and_then(|n| n.trim().parse().ok())
        .unwrap();
    assert!(processus <= 6, "{processus} processus visibles :\n{texte}");
    assert!(!maison.join("intrus").exists());
    assert!(
        profil.join("session").exists(),
        "le profil privé survit à la mission"
    );
    assert!(!dir.path().join("etat/missions/m-1").exists());
}

#[test]
fn sans_cage_aucun_client_n_est_lance() {
    let dir = tempfile::tempdir().unwrap();
    let mut launcher = launcher(
        dir.path(),
        r#"{"codex": {"program": "/bin/echo", "args": ["{intent}"]}}"#,
    );
    launcher.cage = dir.path().join("absente");
    let erreur = launcher.run(&requete("codex", "écrire", 5)).unwrap_err();
    assert!(matches!(erreur, Error::Cage { .. }), "{erreur}");
}
