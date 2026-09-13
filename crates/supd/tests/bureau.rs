//! L'adaptateur contre une vraie application GTK, sur un vrai bus d'accessibilité.
//!
//! Le banc d'essai (X virtuel, bus de session, bus d'accessibilité, éditeur) est monté par
//! `tools/bureau-local.sh`, qui lance ce test dedans. Sans lui, le test se tait, sauf si
//! `PROPHET_EXIGER_BUREAU=1` exige sa présence pour qu'un vert veuille dire vrai.

use std::process::{Command, Stdio};
use std::time::Duration;

use sup::session::ActRequest;
use sup::tree::{Node, Role};
use supd::Desktop;

fn banc_present() -> bool {
    std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
        && std::env::var_os("DISPLAY").is_some()
        && std::env::var_os("PROPHET_BUREAU_EDITEUR").is_some()
}

fn passer_ou_exiger() -> bool {
    if banc_present() {
        return true;
    }
    assert!(
        std::env::var("PROPHET_EXIGER_BUREAU").as_deref() != Ok("1"),
        "PROPHET_EXIGER_BUREAU=1 sans banc d'essai : lancez ce test par tools/bureau-local.sh"
    );
    eprintln!("aucun banc d'essai de bureau : test passé sans rien vérifier");
    false
}

/// L'éditeur lancé pour un test, avec un foyer neuf (aucune session à restaurer, aucune
/// question au démarrage), tué et attendu à la fin, même si le test échoue.
struct Editeur {
    enfant: std::process::Child,
    _foyer: tempfile::TempDir,
}

impl Editeur {
    fn lancer(programme: &str, args: &[&str]) -> Self {
        let foyer = tempfile::tempdir().unwrap();
        let enfant = Command::new(programme)
            .args(args)
            .env("GTK_MODULES", "gail:atk-bridge")
            .env("HOME", foyer.path())
            .env("XDG_CONFIG_HOME", foyer.path().join(".config"))
            .env("XDG_DATA_HOME", foyer.path().join(".local/share"))
            .env("XDG_CACHE_HOME", foyer.path().join(".cache"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("lancer l'éditeur");
        Self {
            enfant,
            _foyer: foyer,
        }
    }
}

impl Drop for Editeur {
    fn drop(&mut self) {
        let _ = self.enfant.kill();
        let _ = self.enfant.wait();
    }
}

fn trouver<'a>(node: &'a Node, pred: &dyn Fn(&Node) -> bool) -> Option<&'a Node> {
    if pred(node) {
        return Some(node);
    }
    node.children.iter().find_map(|c| trouver(c, pred))
}

#[tokio::test]
async fn l_editeur_se_lit_s_ecrit_et_s_enregistre_sans_aucun_pixel() {
    if !passer_ou_exiger() {
        return;
    }
    let editeur = std::env::var("PROPHET_BUREAU_EDITEUR").unwrap();
    let dossier = tempfile::tempdir().unwrap();
    let cible = dossier.path().join("x.txt");
    std::fs::write(&cible, "").unwrap();
    let _enfant = Editeur::lancer(&editeur, &[cible.to_str().unwrap()]);
    let desktop = Desktop::connect().await.expect("bus d'accessibilité");

    // L'application apparaît, avec une fenêtre.
    let mut app = None;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if let Some(a) = desktop
            .applications()
            .await
            .unwrap_or_default()
            .into_iter()
            .find(|a| a.windows > 0)
        {
            app = Some(a.app);
            break;
        }
    }
    let app = app.expect("l'éditeur n'est jamais apparu sur le bus");
    assert_eq!(app, "mousepad", "{app}");

    // Son arbre a un champ de texte modifiable et des actions typées. L'application finit de
    // se construire après s'être annoncée : on relit jusqu'à voir le champ, comme un agent.
    let mut obs = desktop.tree(&app, None).await.expect("arbre");
    for _ in 0..40 {
        if obs.tree.action("set_field").is_some() {
            break;
        }
        eprintln!(
            "fenêtre {} « {} » : {} nœuds, {} laissés, actions {:?}",
            obs.tree.window,
            obs.tree.title,
            obs.tree.root.count(),
            obs.truncated,
            obs.tree
                .actions
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
        obs = desktop.tree(&app, None).await.expect("arbre");
    }
    assert!(obs.confidence < 1.0 && obs.caveat.contains("accessibilité"));
    assert!(
        obs.tree.action("click").is_some() && obs.tree.action("set_field").is_some(),
        "{:?}",
        obs.tree.actions
    );
    let champ = trouver(&obs.tree.root, &|n| n.role == Role::Field && n.actionable)
        .expect("un champ de texte")
        .clone();

    // Écrire « bonjour » : l'arbre relu le dit, sans capture d'écran.
    let out = desktop
        .act(&ActRequest {
            app: app.clone(),
            window: None,
            action: "set_field".into(),
            node: champ.id.clone(),
            value: Some("bonjour".into()),
        })
        .await
        .expect("set_field");
    let apres = out.observation.expect("arbre après l'action");
    let relu = trouver(&apres.tree.root, &|n| n.id == champ.id).expect("le champ existe encore");
    assert_eq!(relu.value.as_deref(), Some("bonjour"), "{}", out.message);

    // Un élément inexistant et une action non déclarée sont des erreurs nommées.
    let err = desktop
        .act(&ActRequest {
            app: app.clone(),
            window: None,
            action: "click".into(),
            node: "999999".into(),
            value: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, supd::Error::UnknownNode(_)), "{err}");
    let err = desktop
        .act(&ActRequest {
            app: app.clone(),
            window: None,
            action: "set_field".into(),
            node: obs.tree.root.id.clone(),
            value: Some("x".into()),
        })
        .await
        .unwrap_err();
    assert!(matches!(err, supd::Error::Unsupported(_)), "{err}");

    // Enregistrer : l'entrée de menu « Save » écrit le document, qui porte déjà son nom.
    //
    // Pas « Enregistrer sous » : une application GTK 3 qui ouvre un dialogue modal depuis une
    // action bloque son pont d'accessibilité jusqu'à la fermeture du dialogue (libdbus n'est pas
    // réentrant), et l'agent ne verrait rien avant que l'humain ne ferme le dialogue. Les
    // applications GTK 4, aux dialogues asynchrones, n'ont pas cette limite.
    let save = trouver(&apres.tree.root, &|n| {
        n.role == Role::Item && n.name.trim().eq_ignore_ascii_case("save")
    })
    .expect("l'entrée Enregistrer")
    .clone();
    let out = desktop
        .act(&ActRequest {
            app: app.clone(),
            window: None,
            action: "click".into(),
            node: save.id,
            value: None,
        })
        .await
        .expect("enregistrer");
    assert!(out.message.contains("activé"), "{}", out.message);
    let mut contenu = None;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if let Ok(c) = std::fs::read_to_string(&cible)
            && c.trim() == "bonjour"
        {
            contenu = Some(c);
            break;
        }
    }
    assert_eq!(contenu.as_deref().map(str::trim), Some("bonjour"));
}

/// Relevé brut, pour lire ce que l'éditeur déclare : `-- --nocapture dump_de_l_arbre_brut`.
#[tokio::test]
async fn dump_de_l_arbre_brut() {
    if !passer_ou_exiger() {
        return;
    }
    let editeur = std::env::var("PROPHET_BUREAU_EDITEUR").unwrap();
    let _enfant = Editeur::lancer(&editeur, &[]);
    let desktop = Desktop::connect().await.unwrap();
    tokio::time::sleep(Duration::from_secs(4)).await;
    let apps = desktop.applications().await.unwrap();
    eprintln!("APPS {apps:?}");
    if let Some(app) = apps.iter().find(|a| a.windows > 0) {
        let (brut, tronque) = desktop.inspect(&app.app, None).await.unwrap();
        fn imprimer(n: &sup::adapter::AccessibleNode, depth: usize) {
            eprintln!(
                "{:indent$}{} « {} » val={:?} états={:?} actions={:?}",
                "",
                n.role,
                n.name.trim(),
                n.value
                    .as_ref()
                    .map(|v| v.chars().take(30).collect::<String>()),
                n.states,
                n.actions,
                indent = depth * 2
            );
            for c in &n.children {
                imprimer(c, depth + 1);
            }
        }
        imprimer(&brut, 0);
        eprintln!("TRONQUÉ {tronque}");
    }
}
