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
    let disponibles = Capabilities::probe().user_namespaces && helper().exists();
    // Sur une machine qui doit les avoir (le coureur d'isolation), se taire serait mentir.
    assert!(
        disponibles || std::env::var("PROPHET_EXIGER_ESPACES_DE_NOMS").as_deref() != Ok("1"),
        "espaces de noms ou amorçage indisponibles alors que PROPHET_EXIGER_ESPACES_DE_NOMS=1"
    );
    disponibles
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

/// Ce que la sandbox a réellement produit, à joindre à toute assertion qui échoue.
///
/// Une assertion sur la seule sortie standard dit « attendu FIN, obtenu rien », ce qui ne
/// distingue pas un confinement qui fait son travail d'un confinement qui n'a pas démarré. Sur
/// une machine qu'on ne voit qu'à travers un journal, cette distinction est tout le diagnostic.
fn contexte(code: Option<i32>, stdout: &str, stderr: &str) -> String {
    format!("\n  code de sortie : {code:?}\n  sortie : {stdout:?}\n  erreur : {stderr}")
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
    let (code, stdout, stderr) = executer(rules, "cat /etc/shadow 2>/dev/null; echo FIN");
    assert_eq!(
        stdout.trim(),
        "FIN",
        "le contenu de /etc/shadow ne doit pas sortir{}",
        contexte(code, &stdout, &stderr)
    );
    assert_eq!(code, Some(0), "{}", contexte(code, &stdout, &stderr));
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
    let (code, stdout, stderr) = executer(rules, &script);
    assert!(
        stdout.contains("contenu visible"),
        "le chemin accordé doit être lisible{}",
        contexte(code, &stdout, &stderr)
    );
    assert!(
        !stdout.contains("contenu secret"),
        "le chemin non accordé ne doit pas être lisible{}",
        contexte(code, &stdout, &stderr)
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
    let (code, stdout, stderr) = executer(rules, &script);
    assert!(
        stdout.contains("FIN"),
        "le script n'est pas allé à son terme{}",
        contexte(code, &stdout, &stderr)
    );
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
    let (code, stdout, stderr) = executer(
        Ruleset::default(),
        "cat /proc/net/dev 2>/dev/null | tail -n +3 | wc -l",
    );
    // Replier un compte illisible sur zéro ferait passer ce test quand rien n'a tourné : le
    // silence deviendrait la preuve de l'isolement qu'il est censé mesurer.
    let interfaces: usize = stdout.trim().parse().unwrap_or_else(|_| {
        panic!(
            "le nombre d'interfaces n'a pas pu être lu : la sandbox n'a probablement rien exécuté{}",
            contexte(code, &stdout, &stderr)
        )
    });
    assert!(
        interfaces <= 1,
        "un espace de noms réseau neuf ne contient que la boucle locale, trouvé {interfaces}{}",
        contexte(code, &stdout, &stderr)
    );
}

#[test]
fn le_montage_est_refuse_par_le_filtre_d_appels_systeme() {
    if !namespaces_disponibles() {
        eprintln!("espaces de noms indisponibles : test ignoré");
        return;
    }
    // `mount` figure dans la liste de refus : la commande doit échouer, quel que soit son motif.
    let (code, stdout, stderr) = executer(
        Ruleset::default(),
        "mount -t tmpfs none /mnt 2>/dev/null && echo MONTE || echo REFUSE",
    );
    assert!(
        stdout.contains("REFUSE"),
        "le montage aurait dû être refusé par le filtre{}",
        contexte(code, &stdout, &stderr)
    );
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
    let (code, stdout, stderr) = executer(rules, &script);
    assert!(
        stdout.contains("ECRIT"),
        "l'écriture dans un chemin accordé doit aboutir{}",
        contexte(code, &stdout, &stderr)
    );
    assert_eq!(
        std::fs::read_to_string(travail.join("sortie.txt"))
            .unwrap()
            .trim(),
        "resultat"
    );
}

/// Landlock, au-delà de la racine minimale : même sur ce qui est monté, le noyau ne laisse
/// écrire que dans les chemins accordés en écriture, et n'exécute que ce qui vient des montages
/// en lecture seule. Un binaire déposé dans l'espace de travail ne s'exécute pas.
#[test]
fn landlock_borne_l_ecriture_et_l_execution_au_niveau_zero() {
    if !namespaces_disponibles() {
        eprintln!("espaces de noms indisponibles : test ignoré");
        return;
    }
    if Capabilities::probe().landlock_abi.is_none() {
        assert!(
            std::env::var("PROPHET_EXIGER_LANDLOCK").as_deref() != Ok("1"),
            "Landlock absent alors que PROPHET_EXIGER_LANDLOCK=1"
        );
        eprintln!("Landlock absent de ce noyau : test sans effet");
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
    // La racine de la sandbox est un tmpfs : sans Landlock, on pouvait y créer ce qu'on voulait.
    let (code, stdout, stderr) = executer(
        rules.clone(),
        "mkdir /intrus 2>/dev/null && echo RACINE_ECRITE; (: > /fichier) 2>/dev/null && echo FICHIER_ECRIT; echo FIN",
    );
    assert!(
        stdout.contains("FIN"),
        "le script a tourné{}",
        contexte(code, &stdout, &stderr)
    );
    assert!(
        !stdout.contains("RACINE_ECRITE") && !stdout.contains("FICHIER_ECRIT"),
        "la racine de la sandbox ne doit pas être inscriptible{}",
        contexte(code, &stdout, &stderr)
    );
    // L'écriture accordée reste possible, et la lecture de ce qu'on a écrit aussi.
    let script = format!(
        "cp /bin/true {t}/outil && cat {t}/outil > /dev/null && echo COPIE; {t}/outil && echo EXECUTE; echo FIN",
        t = travail.display()
    );
    let (code, stdout, stderr) = executer(rules, &script);
    assert!(
        stdout.contains("COPIE"),
        "la copie dans l'espace accordé{}",
        contexte(code, &stdout, &stderr)
    );
    assert!(
        !stdout.contains("EXECUTE"),
        "un binaire déposé dans l'espace de travail ne doit pas s'exécuter{}",
        contexte(code, &stdout, &stderr)
    );
    assert!(
        stdout.contains("FIN"),
        "{}",
        contexte(code, &stdout, &stderr)
    );
    // Et le programme lancé depuis un montage en lecture seule, lui, s'exécute : c'est /bin/sh.
    assert!(travail.join("outil").exists());
    assert!(
        !stderr.contains("Landlock indisponible"),
        "l'amorçage doit appliquer Landlock ici{}",
        contexte(code, &stdout, &stderr)
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

    // Geler zéro sandbox est instantané. Sans cette vérification, le test mesurait la vitesse à
    // laquelle on ne fait rien, et la déclarait conforme à l'objectif du plan.
    for handle in &mut handles {
        if let Some(enfant) = handle.child_mut() {
            assert!(
                enfant.try_wait().unwrap().is_none(),
                "les sandboxes doivent encore tourner au moment du gel"
            );
        }
    }

    let start = std::time::Instant::now();
    let frozen = manager.freeze_all();
    let elapsed = start.elapsed();
    eprintln!("gel de {frozen} sandboxes en {elapsed:?}");
    assert_eq!(frozen, 8, "les huit sandboxes doivent avoir été gelées");
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
        // Une sandbox qui meurt au démarrage démarre très vite. Sans cette vérification, ce test
        // décernait sa médaille de vitesse à un confinement qui n'avait pas eu lieu.
        let code = handle.wait().unwrap();
        assert_eq!(
            code,
            Some(0),
            "la sandbox doit réellement exécuter /bin/true, code obtenu : {code:?}"
        );
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
    // Un espace de travail vide : le disque confié à l'invité en est fait (ADR 0038).
    let travail = tempfile::tempdir().unwrap();
    let spec =
        SandboxSpec::new(2, "/bin/true", travail.path().display().to_string()).env("PATH", "/bin");
    let debut = std::time::Instant::now();
    let mut handle = manager.run("task:test", &spec).unwrap();
    let ecoule = debut.elapsed();
    eprintln!("mesure : microVM démarrée à froid en {ecoule:?}");
    assert_eq!(handle.level, 2);
    // Objectif du plan : moins de 2 s à froid, moins de 100 ms depuis un instantané.
    assert!(
        ecoule < std::time::Duration::from_secs(5),
        "démarrage trop lent : {ecoule:?}"
    );
    let _ = manager.kill(&mut handle);
}

/// Le contrat de l'invité (ADR 0038), de bout en bout : un programme Python lit un fichier de
/// l'espace de travail, dit une phrase sur la console, en écrit un autre, rend un code ; l'hôte
/// lit la console entre les marques, rapatrie le disque, et le fichier écrit est là.
#[test]
#[ignore = "needs_kvm"]
fn le_niveau_deux_execute_un_programme_et_rapatrie_ses_fichiers() {
    use std::io::Read as _;
    let caps = Capabilities::probe();
    assert!(
        caps.supports(2),
        "niveau 2 inatteignable : il manque {}",
        caps.missing_for(2).join(", ")
    );
    let travail = tempfile::tempdir().unwrap();
    std::fs::write(travail.path().join("entree.txt"), "3 et 4").unwrap();
    // Un espace de travail de milliers de petits fichiers : le disque doit avoir un inode pour
    // chacun (le ratio par défaut de mkfs n'en donnait qu'un par 16 Kio, et l'image échouait).
    let plein = travail.path().join("plein");
    std::fs::create_dir(&plein).unwrap();
    for i in 0..6000 {
        std::fs::write(plein.join(format!("f{i}")), "x").unwrap();
    }
    let manager = Manager::new(helper().display().to_string());
    let spec = SandboxSpec::new(2, "/usr/bin/python3", travail.path().display().to_string())
        .args([
            "-c",
            "import sys\na, b = open('entree.txt').read().split(' et ')\nprint('somme', int(a) + int(b))\nopen('resultat.txt', 'w').write('fait')\nsys.exit(7)",
        ])
        .env("PATH", "/usr/bin:/bin");
    let debut = std::time::Instant::now();
    let mut handle = manager.run("task:test", &spec).unwrap();
    assert_eq!(handle.level, 2);
    let vm = handle
        .microvm
        .clone()
        .expect("une microVM a un disque de travail");
    let mut console = String::new();
    if let Some(child) = handle_child(&mut handle)
        && let Some(out) = child.stdout.as_mut()
    {
        let _ = out.read_to_string(&mut console);
    }
    let code_moniteur = handle.wait().unwrap();
    let duree = debut.elapsed();
    eprintln!("microVM : {duree:?}, moniteur {code_moniteur:?}");
    assert_eq!(
        code_moniteur,
        Some(0),
        "le moniteur doit sortir quand l'invité redémarre\n{console}"
    );
    let lu = sandboxd::invite::lire_console(&console);
    assert!(lu.fin_vue, "l'invité n'a pas dit sa fin :\n{console}");
    assert_eq!(lu.erreur, None, "{console}");
    assert_eq!(lu.code, Some(7), "{console}");
    assert!(
        lu.sortie.contains("somme 7"),
        "sortie lue : {:?}",
        lu.sortie
    );
    sandboxd::invite::rapatrier(&vm.disque, travail.path()).unwrap();
    assert_eq!(
        std::fs::read_to_string(travail.path().join("resultat.txt")).unwrap(),
        "fait"
    );
    assert_eq!(
        std::fs::read_to_string(travail.path().join("entree.txt")).unwrap(),
        "3 et 4"
    );
    let _ = std::fs::remove_dir_all(&vm.base);
    assert!(
        duree < std::time::Duration::from_secs(20),
        "trop lent : {duree:?}"
    );
}

/// M5-T4, critère d'acceptation : réserve chaude, une microVM de niveau 2 est rendue en moins
/// de 150 ms (médiane), chaque exécution a une machine neuve, le programme s'exécute comme à
/// froid, et la réserve se remplit à nouveau d'elle-même (ADR 0045).
#[test]
#[ignore = "needs_kvm"]
fn la_reserve_rend_une_microvm_de_niveau_deux_en_moins_de_150_ms() {
    let caps = Capabilities::probe();
    assert!(
        caps.supports(2),
        "niveau 2 inatteignable : il manque {}",
        caps.missing_for(2).join(", ")
    );
    let manager = Manager::new(helper().display().to_string()).avec_reserve(2);
    let reserve = manager.reserve().expect("une réserve au niveau 2");
    let debut = std::time::Instant::now();
    assert!(
        reserve.attendre_pleine(std::time::Duration::from_secs(60)),
        "la réserve ne s'est pas remplie : {:?}",
        reserve.statut()
    );
    eprintln!(
        "mesure : réserve pleine en {:?} ({:?})",
        debut.elapsed(),
        reserve.statut()
    );
    let mut durees = Vec::new();
    let mut aleas = Vec::new();
    for tour in 0..5 {
        assert!(
            reserve.attendre_pleine(std::time::Duration::from_secs(30)),
            "tour {tour} : la réserve ne se régénère pas : {:?}",
            reserve.statut()
        );
        let travail = tempfile::tempdir().unwrap();
        std::fs::write(travail.path().join("entree.txt"), format!("{tour} et 4")).unwrap();
        // Une trace dans le /tmp de l'invité : une machine réutilisée la retrouverait.
        let spec = SandboxSpec::new(2, "/usr/bin/python3", travail.path().display().to_string())
            .args([
                "-c",
                "import os, sys\na, b = open('entree.txt').read().split(' et ')\nprint('neuve' if not os.path.exists('/tmp/deja') else 'reprise')\nopen('/tmp/deja', 'w').write('1')\nprint('alea', os.urandom(16).hex())\nprint('somme', int(a) + int(b))\nopen('resultat.txt', 'w').write('fait')\nsys.exit(7)",
            ])
            .env("PATH", "/usr/bin:/bin");
        let lance = std::time::Instant::now();
        let mut handle = manager.run("task:reserve", &spec).unwrap();
        durees.push(lance.elapsed());
        assert_eq!(handle.level, 2);
        let vm = handle.microvm.clone().expect("un disque de travail");
        // La console est lue avec un délai : une machine qui ne verrait pas son disque arriver
        // ne doit pas retenir le coureur des heures durant.
        let sortie = handle_child(&mut handle)
            .and_then(|child| child.stdout.take())
            .expect("console du moniteur");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut sortie = sortie;
            let mut console = String::new();
            let _ = sortie.read_to_string(&mut console);
            let _ = tx.send(console);
        });
        let Ok(console) = rx.recv_timeout(std::time::Duration::from_secs(60)) else {
            let _ = manager.kill(&mut handle);
            panic!(
                "tour {tour} : l'invité n'a pas fini en 60 s ; réserve {:?}",
                reserve.statut()
            );
        };
        let code_moniteur = handle.wait().unwrap();
        let lu = sandboxd::invite::lire_console(&console);
        assert!(
            lu.fin_vue,
            "tour {tour} : l'invité n'a pas dit sa fin :\n{console}"
        );
        assert_eq!(lu.erreur, None, "{console}");
        assert_eq!(lu.code, Some(7), "{console}");
        assert!(
            lu.sortie.contains("neuve"),
            "tour {tour} : machine réutilisée ?\n{console}"
        );
        assert!(
            lu.sortie.contains(&format!("somme {}", tour + 4)),
            "tour {tour} : {:?}",
            lu.sortie
        );
        assert_eq!(code_moniteur, Some(0), "{console}");
        aleas.extend(
            lu.sortie
                .lines()
                .filter_map(|l| l.strip_prefix("alea "))
                .map(str::to_owned),
        );
        sandboxd::invite::rapatrier(&vm.disque, travail.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(travail.path().join("resultat.txt")).unwrap(),
            "fait"
        );
        let _ = std::fs::remove_dir_all(&vm.base);
    }
    // Cinq machines clonées de la même mémoire : si le noyau d'invité n'apprenait pas le
    // clonage (VMGenID), elles tireraient les mêmes octets aléatoires (ADR 0045).
    let mut distincts = aleas.clone();
    distincts.sort();
    distincts.dedup();
    eprintln!(
        "mesure : aléa des clones, {} tirages distincts sur {}",
        distincts.len(),
        aleas.len()
    );
    assert_eq!(aleas.len(), 5, "chaque prise dit son aléa : {aleas:?}");
    assert_eq!(
        distincts.len(),
        aleas.len(),
        "des clones de la réserve tirent les mêmes octets aléatoires : {aleas:?}"
    );
    durees.sort();
    let mediane = durees[durees.len() / 2];
    eprintln!("mesure : microVM rendue par la réserve en {mediane:?} (médiane de {durees:?})");
    assert!(
        mediane < std::time::Duration::from_millis(150),
        "médiane {mediane:?} au-delà de 150 ms : {durees:?} ; réserve {:?}",
        reserve.statut()
    );
    assert!(
        reserve.attendre_pleine(std::time::Duration::from_secs(30)),
        "la réserve ne s'est pas remplie à nouveau : {:?}",
        reserve.statut()
    );
}

/// FRONTIER, isolation : deux microVM de niveau 2 tournent en même temps, chacune sur son
/// disque, et aucune n'a de réseau — pas d'interface hors de la boucle locale, et une connexion
/// vers l'extérieur échoue.
#[test]
#[ignore = "needs_kvm"]
fn deux_microvm_tournent_ensemble_sans_reseau() {
    let caps = Capabilities::probe();
    assert!(
        caps.supports(2),
        "niveau 2 inatteignable : il manque {}",
        caps.missing_for(2).join(", ")
    );
    let manager = Manager::new(helper().display().to_string()).avec_reserve(2);
    let reserve = manager.reserve().expect("une réserve au niveau 2");
    assert!(
        reserve.attendre_pleine(std::time::Duration::from_secs(60)),
        "{:?}",
        reserve.statut()
    );
    let programme = "import os, socket, sys, time\nprint('interfaces', ' '.join(sorted(os.listdir('/sys/class/net'))) if os.path.isdir('/sys/class/net') else 'interfaces aucune')\ntry:\n    socket.create_connection(('1.1.1.1', 80), timeout=2)\n    print('reseau ouvert')\nexcept OSError as e:\n    print('reseau ferme', e.errno)\ntime.sleep(1)\nopen('marque.txt', 'w').write(open('nom.txt').read())\nprint('fin', open('nom.txt').read())";
    let mut en_cours = Vec::new();
    let debut = std::time::Instant::now();
    for nom in ["premiere", "seconde"] {
        let travail = tempfile::tempdir().unwrap();
        std::fs::write(travail.path().join("nom.txt"), nom).unwrap();
        let spec = SandboxSpec::new(2, "/usr/bin/python3", travail.path().display().to_string())
            .args(["-c", programme])
            .env("PATH", "/usr/bin:/bin");
        let mut handle = manager.run("task:ensemble", &spec).unwrap();
        assert_eq!(handle.level, 2);
        let sortie = handle_child(&mut handle)
            .and_then(|child| child.stdout.take())
            .expect("console");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut sortie = sortie;
            let mut console = String::new();
            let _ = sortie.read_to_string(&mut console);
            let _ = tx.send(console);
        });
        en_cours.push((nom, travail, handle, rx));
    }
    for (nom, travail, mut handle, rx) in en_cours {
        let Ok(console) = rx.recv_timeout(std::time::Duration::from_secs(60)) else {
            let _ = manager.kill(&mut handle);
            panic!("{nom} : l'invité n'a pas fini en 60 s");
        };
        let _ = handle.wait();
        let lu = sandboxd::invite::lire_console(&console);
        assert!(lu.fin_vue, "{nom} :\n{console}");
        assert!(
            lu.sortie.contains(&format!("fin {nom}")),
            "{nom} : {:?}",
            lu.sortie
        );
        assert!(
            lu.sortie.contains("reseau ferme"),
            "{nom} : réseau ouvert ?\n{console}"
        );
        let interfaces = lu
            .sortie
            .lines()
            .find_map(|l| l.strip_prefix("interfaces "))
            .unwrap_or_default()
            .to_owned();
        assert!(
            interfaces.split_whitespace().all(|i| i == "lo"),
            "{nom} : interfaces {interfaces:?}"
        );
        let vm = handle.microvm.clone().expect("disque");
        sandboxd::invite::rapatrier(&vm.disque, travail.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(travail.path().join("marque.txt")).unwrap(),
            nom
        );
        let _ = std::fs::remove_dir_all(&vm.base);
    }
    // Deux programmes d'une seconde chacun, ensemble : bien moins que deux secondes bout à bout
    // plus deux démarrages.
    let duree = debut.elapsed();
    eprintln!("mesure : deux microVM ensemble en {duree:?}");
    assert!(duree < std::time::Duration::from_secs(10), "{duree:?}");
}

#[test]
#[ignore = "needs_kvm"]
fn le_niveau_deux_ne_retombe_jamais_sur_le_niveau_zero() {
    // Le défaut que ce test existe pour attraper : accepter une demande de niveau 2 et
    // l'exécuter en niveau 0, donc promettre une isolation qui n'a pas lieu.
    let caps = Capabilities::probe();
    let manager = Manager::new(helper().display().to_string());
    let travail = tempfile::tempdir().unwrap();
    let spec = SandboxSpec::new(2, "/bin/true", travail.path().display().to_string());
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
