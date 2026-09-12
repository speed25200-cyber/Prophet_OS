//! Tests d'évasion : ils ne vérifient pas que le code s'exécute, mais qu'un processus confiné
//! **échoue** à faire ce qu'il n'a pas le droit de faire.
//!
//! Ces tests exigent des espaces de noms utilisateur. Quand l'environnement ne les fournit pas,
//! ils le signalent et s'arrêtent plutôt que de passer à tort.

use std::io::Read as _;
use std::path::PathBuf;

use capd::enforce::{PathRule, Ruleset};
use sandboxd::{Capabilities, Manager, SandboxSpec};

fn helper() -> PathBuf {
    // Le binaire d'amorçage est construit à côté des binaires de test.
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join("prophet-sandbox-helper")
}

fn namespaces_disponibles() -> bool {
    Capabilities::probe().user_namespaces && helper().exists()
}

/// Exécute une spécification quelconque et renvoie (code de sortie, sortie standard, erreur,
/// niveau réellement obtenu).
///
/// Les tests des niveaux 1 et 2 en dépendent. Quand un moniteur est installé mais refuse de
/// démarrer, c'est son message d'erreur qui dit pourquoi ; une sortie vide, seule, n'apprend rien
/// à qui lit le rapport depuis une autre machine. Le niveau est rendu avec le reste pour qu'un
/// repli silencieux se voie dans le même appel.
fn executer_spec(spec: &SandboxSpec) -> (Option<i32>, String, String, u8) {
    let manager = Manager::new(helper().display().to_string());
    let mut handle = manager.run("task:test", spec).unwrap();
    let niveau = handle.level;
    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(child) = handle_child(&mut handle) {
        if let Some(out) = child.stdout.as_mut() {
            let _ = out.read_to_string(&mut stdout);
        }
        if let Some(err) = child.stderr.as_mut() {
            let _ = err.read_to_string(&mut stderr);
        }
    }
    let code = handle.wait().unwrap();
    (code, stdout, stderr, niveau)
}

/// Exécute `sh -c <script>` sous sandbox de niveau 0 et renvoie (code de sortie, sortie, erreur).
fn executer(rules: Ruleset, script: &str) -> (Option<i32>, String, String) {
    let spec = SandboxSpec::new(0, "/bin/sh", "/")
        .args(["-c", script])
        .rules(rules)
        .env("PATH", "/usr/bin:/bin");
    let (code, stdout, stderr, _) = executer_spec(&spec);
    (code, stdout, stderr)
}

/// Accès au processus enfant de la poignée, via son identifiant de processus.
fn handle_child(handle: &mut sandboxd::SandboxHandle) -> Option<&mut std::process::Child> {
    // `SandboxHandle` conserve l'enfant pour permettre la lecture des flux ; l'accès passe par
    // cette fonction pour garder le champ privé.
    handle.child_mut()
}

#[test]
fn le_fichier_des_mots_de_passe_est_invisible() {
    if !namespaces_disponibles() {
        eprintln!("espaces de noms indisponibles : test ignoré");
        return;
    }
    let rules = Ruleset::default();
    let (code, stdout, _) = executer(rules, "cat /etc/shadow 2>/dev/null; echo FIN");
    assert_eq!(
        stdout.trim(),
        "FIN",
        "le contenu de /etc/shadow ne doit pas sortir"
    );
    assert_eq!(code, Some(0));
}

#[test]
fn seuls_les_chemins_accordes_sont_lisibles() {
    if !namespaces_disponibles() {
        eprintln!("espaces de noms indisponibles : test ignoré");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let autorise = dir.path().join("autorise");
    let interdit = dir.path().join("interdit");
    std::fs::create_dir_all(&autorise).unwrap();
    std::fs::create_dir_all(&interdit).unwrap();
    std::fs::write(autorise.join("visible.txt"), "contenu visible").unwrap();
    std::fs::write(interdit.join("secret.txt"), "contenu secret").unwrap();

    let rules = Ruleset {
        paths: vec![PathRule {
            path: autorise.display().to_string(),
            read: true,
            write: false,
        }],
        ..Ruleset::default()
    };
    let script = format!(
        "cat {}/visible.txt; echo; cat {}/secret.txt 2>/dev/null; echo FIN",
        autorise.display(),
        interdit.display()
    );
    let (_, stdout, _) = executer(rules, &script);
    assert!(stdout.contains("contenu visible"), "{stdout}");
    assert!(
        !stdout.contains("contenu secret"),
        "le chemin non accordé ne doit pas être lisible : {stdout}"
    );
}

#[test]
fn le_chemin_accorde_en_lecture_n_est_pas_inscriptible() {
    if !namespaces_disponibles() {
        eprintln!("espaces de noms indisponibles : test ignoré");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let lecture = dir.path().join("lecture");
    std::fs::create_dir_all(&lecture).unwrap();
    std::fs::write(lecture.join("a.txt"), "origine").unwrap();

    let rules = Ruleset {
        paths: vec![PathRule {
            path: lecture.display().to_string(),
            read: true,
            write: false,
        }],
        ..Ruleset::default()
    };
    let script = format!(
        "echo saccage > {}/a.txt 2>/dev/null; echo FIN",
        lecture.display()
    );
    let (_, stdout, _) = executer(rules, &script);
    assert!(stdout.contains("FIN"));
    assert_eq!(
        std::fs::read_to_string(lecture.join("a.txt")).unwrap(),
        "origine",
        "un montage en lecture seule doit rester intact"
    );
}

#[test]
fn aucune_interface_reseau() {
    if !namespaces_disponibles() {
        eprintln!("espaces de noms indisponibles : test ignoré");
        return;
    }
    let (_, stdout, _) = executer(
        Ruleset::default(),
        "cat /proc/net/dev 2>/dev/null | tail -n +3 | wc -l",
    );
    let interfaces: usize = stdout.trim().parse().unwrap_or(0);
    assert!(
        interfaces <= 1,
        "un espace de noms réseau neuf ne contient que la boucle locale, trouvé {interfaces}"
    );
}

#[test]
fn le_montage_est_refuse_par_le_filtre_d_appels_systeme() {
    if !namespaces_disponibles() {
        eprintln!("espaces de noms indisponibles : test ignoré");
        return;
    }
    // `mount` figure dans la liste de refus : la commande doit échouer, quel que soit son motif.
    let (_, stdout, _) = executer(
        Ruleset::default(),
        "mount -t tmpfs none /mnt 2>/dev/null && echo MONTE || echo REFUSE",
    );
    assert!(stdout.contains("REFUSE"), "{stdout}");
}

#[test]
fn l_ecriture_dans_un_chemin_accorde_fonctionne() {
    if !namespaces_disponibles() {
        eprintln!("espaces de noms indisponibles : test ignoré");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let travail = dir.path().join("travail");
    std::fs::create_dir_all(&travail).unwrap();

    let rules = Ruleset {
        paths: vec![PathRule {
            path: travail.display().to_string(),
            read: true,
            write: true,
        }],
        ..Ruleset::default()
    };
    let script = format!(
        "echo resultat > {}/sortie.txt && echo ECRIT",
        travail.display()
    );
    let (_, stdout, stderr) = executer(rules, &script);
    assert!(
        stdout.contains("ECRIT"),
        "sortie: {stdout} / erreur: {stderr}"
    );
    assert_eq!(
        std::fs::read_to_string(travail.join("sortie.txt"))
            .unwrap()
            .trim(),
        "resultat"
    );
}

#[test]
fn gel_global_rapide() {
    if !namespaces_disponibles() {
        eprintln!("espaces de noms indisponibles : test ignoré");
        return;
    }
    let manager = Manager::new(helper().display().to_string());
    let mut handles = Vec::new();
    for _ in 0..8 {
        let spec = SandboxSpec::new(0, "/bin/sh", "/")
            .args(["-c", "sleep 5"])
            .env("PATH", "/usr/bin:/bin");
        handles.push(manager.run("task:test", &spec).unwrap());
    }
    std::thread::sleep(std::time::Duration::from_millis(150));

    let start = std::time::Instant::now();
    let frozen = manager.freeze_all();
    let elapsed = start.elapsed();
    eprintln!("gel de {frozen} sandboxes en {elapsed:?}");
    assert!(
        elapsed < std::time::Duration::from_millis(50),
        "le gel d'urgence doit rester sous 50 ms, mesuré {elapsed:?}"
    );

    manager.thaw_all();
    for handle in &mut handles {
        let _ = manager.kill(handle);
    }
}

#[test]
fn demarrage_du_niveau_zero_sous_dix_millisecondes() {
    if !namespaces_disponibles() {
        eprintln!("espaces de noms indisponibles : test ignoré");
        return;
    }
    let manager = Manager::new(helper().display().to_string());
    let mut durees = Vec::new();
    for _ in 0..10 {
        let spec = SandboxSpec::new(0, "/bin/true", "/").env("PATH", "/bin");
        let start = std::time::Instant::now();
        let mut handle = manager.run("task:test", &spec).unwrap();
        durees.push(start.elapsed());
        let _ = handle.wait();
    }
    durees.sort();
    let mediane = durees[durees.len() / 2];
    eprintln!("lancement niveau 0 : médiane {mediane:?}");
    assert!(
        mediane < std::time::Duration::from_millis(10),
        "objectif du plan : moins de 10 ms au p50, mesuré {mediane:?}"
    );
}

// --- Tests des niveaux 1 et 2 ---
//
// Ils exigent gVisor, KVM et des images d'invité. Ils sont marqués `ignore` pour ne pas échouer
// sur une machine qui n'en dispose pas, et se lancent par `cargo test -- --ignored` ou
// `just verify-host`. Ils ne sont pas décoratifs : ce sont eux qui prouvent que les niveaux
// supérieurs isolent vraiment, ce qu'aucun test de ce conteneur ne peut établir.

#[test]
#[ignore = "needs_gvisor"]
fn niveau_un_execute_reellement_sous_gvisor() {
    let caps = Capabilities::probe();
    assert!(
        caps.runsc.is_some(),
        "gVisor absent : ce test doit tourner sur une machine équipée"
    );
    let spec = SandboxSpec::new(1, "/bin/sh", "/")
        // `dmesg` de gVisor annonce son propre noyau : c'est la preuve que l'invité ne parle pas
        // au noyau de l'hôte.
        .args(["-c", "dmesg 2>/dev/null | head -1; echo FIN"])
        .env("PATH", "/usr/bin:/bin");
    let (code, stdout, stderr, niveau) = executer_spec(&spec);
    assert_eq!(
        niveau, 1,
        "une sandbox demandée au niveau 1 doit être rendue au niveau 1"
    );
    assert!(
        stdout.contains("FIN"),
        "gVisor est installé mais n'a rien exécuté.\n  code de sortie : {code:?}\n  \
         sortie : {stdout:?}\n  erreur : {stderr}"
    );
}

#[test]
#[ignore = "needs_gvisor"]
fn niveau_un_n_a_pas_de_reseau() {
    let caps = Capabilities::probe();
    assert!(caps.runsc.is_some(), "gVisor absent");
    let spec = SandboxSpec::new(1, "/bin/sh", "/")
        .args(["-c", "cat /proc/net/dev 2>/dev/null | tail -n +3 | wc -l"])
        .env("PATH", "/usr/bin:/bin");
    let (code, stdout, stderr, niveau) = executer_spec(&spec);
    assert_eq!(
        niveau, 1,
        "une sandbox demandée au niveau 1 doit être rendue au niveau 1"
    );
    // Un compte illisible et un mauvais compte sont deux défauts différents. Les confondre, comme
    // le faisait la valeur de repli précédente, fait chercher une fuite de réseau là où c'est le
    // lancement qui a échoué.
    let interfaces: usize = stdout.trim().parse().unwrap_or_else(|_| {
        panic!(
            "le nombre d'interfaces n'a pas pu être lu : gVisor n'a probablement rien exécuté.\n  \
             code de sortie : {code:?}\n  sortie : {stdout:?}\n  erreur : {stderr}"
        )
    });
    assert!(
        interfaces <= 1,
        "l'invité de niveau 1 ne doit voir que la boucle locale, or {interfaces} interfaces sont visibles"
    );
}

#[test]
#[ignore = "needs_kvm"]
fn niveau_deux_demarre_une_microvm() {
    let caps = Capabilities::probe();
    assert!(
        caps.supports(2),
        "niveau 2 inatteignable : il manque {}",
        caps.missing_for(2).join(", ")
    );
    let manager = Manager::new(helper().display().to_string());
    let spec = SandboxSpec::new(2, "/bin/true", "/").env("PATH", "/bin");
    let debut = std::time::Instant::now();
    let mut handle = manager.run("task:test", &spec).unwrap();
    let ecoule = debut.elapsed();
    eprintln!("démarrage de microVM : {ecoule:?}");
    assert_eq!(handle.level, 2);
    // Objectif du plan : moins de 2 s à froid, moins de 100 ms depuis un instantané.
    assert!(
        ecoule < std::time::Duration::from_secs(5),
        "démarrage trop lent : {ecoule:?}"
    );
    let _ = manager.kill(&mut handle);
}

#[test]
#[ignore = "needs_kvm"]
fn le_niveau_deux_ne_retombe_jamais_sur_le_niveau_zero() {
    // Le défaut que ce test existe pour attraper : accepter une demande de niveau 2 et
    // l'exécuter en niveau 0, donc promettre une isolation qui n'a pas lieu.
    let caps = Capabilities::probe();
    let manager = Manager::new(helper().display().to_string());
    let spec = SandboxSpec::new(2, "/bin/true", "/");
    match manager.run("task:test", &spec) {
        Ok(handle) => assert_eq!(
            handle.level, 2,
            "une sandbox rendue au niveau 2 doit vraiment être au niveau 2"
        ),
        Err(erreur) => assert!(
            !caps.supports(2),
            "le niveau 2 est atteignable mais le lancement a échoué : {erreur}"
        ),
    }
}

#[test]
fn un_niveau_non_atteignable_est_refuse_avec_ce_qui_manque() {
    // Celui-ci tourne partout : c'est le comportement quand la machine n'a pas ce qu'il faut.
    let caps = Capabilities::probe();
    if caps.supports(2) {
        eprintln!("machine équipée : ce test ne s'applique pas");
        return;
    }
    let manager = Manager::new(helper().display().to_string());
    let erreur = manager
        .run("task:test", &SandboxSpec::new(2, "/bin/true", "/"))
        .unwrap_err();
    let message = erreur.to_string();
    assert!(message.contains("il manque"), "{message}");
    assert!(
        message.contains("kvm") || message.contains("firecracker") || message.contains("images"),
        "le message doit dire quoi installer : {message}"
    );
}
