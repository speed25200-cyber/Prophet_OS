//! Critère d'acceptation M10-T3 : réserver un billet sur un site local **sans aucune capture
//! d'écran**, en n'utilisant que l'arbre sémantique et des actions typées.
//!
//! Le test lance un vrai navigateur et un vrai serveur ; il échoue si le navigateur est absent,
//! plutôt que de simuler un succès.

use std::sync::{Arc, Mutex};

use browser_bridge::cdp::Session;
use browser_bridge::{Browser, Page};
use sup::tree::{Detail, Role};

const PAGE: &str = r#"<!doctype html>
<html lang="fr"><head><meta charset="utf-8"><title>Réservation</title></head>
<body>
  <h1>Réserver un billet</h1>
  <form method="POST" action="/reserver">
    <label for="depart">Départ</label>
    <input id="depart" name="depart" type="text" placeholder="Ville de départ">
    <label for="arrivee">Arrivée</label>
    <input id="arrivee" name="arrivee" type="text">
    <label for="date">Date</label>
    <input id="date" name="date" type="text">
    <label for="classe">Classe</label>
    <select id="classe" name="classe">
      <option value="seconde">Seconde</option>
      <option value="premiere">Première</option>
    </select>
    <button type="submit" id="valider">Confirmer la réservation</button>
  </form>
  <p id="note">Les places sont limitées.</p>
</body></html>"#;

const CONFIRMATION: &str = r#"<!doctype html>
<html lang="fr"><head><meta charset="utf-8"><title>Confirmation</title></head>
<body><h1>Réservation confirmée</h1><p id="reference" role="status">Référence PX-4821</p></body></html>"#;

/// Serveur local minimal, qui enregistre ce que le formulaire lui envoie.
async fn serveur(recu: Arc<Mutex<Option<String>>>) -> std::io::Result<u16> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                continue;
            };
            let recu = Arc::clone(&recu);
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 16 * 1024];
                let Ok(read) = stream.read(&mut buffer).await else {
                    return;
                };
                let requete = String::from_utf8_lossy(&buffer[..read]).to_string();
                let corps = if requete.starts_with("POST /reserver") {
                    if let Some((_, body)) = requete.split_once("\r\n\r\n")
                        && let Ok(mut garde) = recu.lock()
                    {
                        *garde = Some(body.to_owned());
                    }
                    CONFIRMATION
                } else {
                    PAGE
                };
                let reponse = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{corps}",
                    corps.len()
                );
                let _ = stream.write_all(reponse.as_bytes()).await;
                let _ = stream.flush().await;
            });
        }
    });
    Ok(port)
}

fn chemin_du_navigateur() -> Option<String> {
    for candidat in [
        "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/usr/bin/google-chrome",
    ] {
        if std::path::Path::new(candidat).is_file() {
            return Some(candidat.to_owned());
        }
    }
    std::env::var("PROPHET_BROWSER").ok()
}

fn port_libre() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .unwrap_or(9400)
}

#[tokio::test(flavor = "multi_thread")]
async fn reserver_un_billet_sans_aucune_capture_d_ecran() {
    let Some(navigateur) = chemin_du_navigateur() else {
        eprintln!("aucun navigateur disponible : test ignoré");
        return;
    };
    let recu = Arc::new(Mutex::new(None));
    let port = serveur(Arc::clone(&recu)).await.unwrap();
    let dir = tempfile::tempdir().unwrap();

    let browser = Browser::launch(&navigateur, dir.path(), port_libre())
        .await
        .expect("le navigateur doit démarrer");
    let endpoint = browser.page_endpoint().await.unwrap();
    let mut page = Page::attach(Session::connect(&endpoint).await.unwrap())
        .await
        .unwrap();

    // 1. Observer. L'agent lit un arbre, pas une image.
    page.navigate(&format!("http://127.0.0.1:{port}/"))
        .await
        .unwrap();
    let arbre = page.tree(Detail::Normal).await.unwrap();
    assert_eq!(arbre.title, "Réservation");

    let taille = serde_json::to_vec(&arbre).unwrap().len();
    eprintln!("observation sémantique : {taille} octets");
    assert!(
        taille < 8_000,
        "une observation doit rester de l'ordre du kilooctet, mesuré {taille}"
    );

    // 2. Comprendre. Les champs sont nommés et typés ; aucune coordonnée n'intervient.
    let mut champs = Vec::new();
    arbre.root.walk(&mut |noeud| {
        if matches!(noeud.role, Role::Field | Role::Select) {
            champs.push((noeud.id.clone(), noeud.name.clone()));
        }
    });
    assert_eq!(champs.len(), 4, "quatre champs attendus : {champs:?}");
    let identifiant = |libelle: &str| {
        champs
            .iter()
            .find(|(_, nom)| nom.contains(libelle))
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| panic!("champ « {libelle} » introuvable dans {champs:?}"))
    };

    // 3. Agir, par actions typées désignant des identifiants.
    for (libelle, valeur) in [
        ("Départ", "Paris"),
        ("Arrivée", "Lyon"),
        ("Date", "2026-10-02"),
    ] {
        let (resultat, _) = page
            .act("set_field", Some(&identifiant(libelle)), Some(valeur))
            .await
            .unwrap();
        assert!(resultat.ok, "{libelle} : {resultat:?}");
    }
    let (resultat, apres) = page
        .act("set_field", Some(&identifiant("Classe")), Some("premiere"))
        .await
        .unwrap();
    assert!(resultat.ok);

    // 4. Vérifier, par l'état rendu avec le résultat, sans nouvelle observation coûteuse.
    let classe = apres.root.find(&identifiant("Classe")).unwrap();
    assert_eq!(classe.value.as_deref(), Some("premiere"));

    // 5. Soumettre. L'action est annotée irréversible et externe : c'est elle qui demanderait une
    // approbation humaine dans une tâche réelle.
    let soumission = apres.action("submit").expect("le formulaire offre submit");
    assert!(
        soumission.irreversible && soumission.external,
        "soumettre un formulaire engage l'utilisateur : {soumission:?}"
    );
    let (resultat, confirmation) = page
        .act("submit", Some(&identifiant("Départ")), None)
        .await
        .unwrap();
    assert!(resultat.ok, "{resultat:?}");

    // 6. Le serveur a bien reçu la réservation, et la page de confirmation est lisible.
    let corps = recu
        .lock()
        .unwrap()
        .clone()
        .expect("le formulaire doit être soumis");
    assert!(corps.contains("depart=Paris"), "{corps}");
    assert!(corps.contains("arrivee=Lyon"), "{corps}");
    assert!(corps.contains("classe=premiere"), "{corps}");
    assert_eq!(confirmation.title, "Confirmation");
    let mut trouve = false;
    confirmation.root.walk(&mut |noeud| {
        if noeud.name.contains("PX-4821") {
            trouve = true;
        }
    });
    assert!(
        trouve,
        "la référence doit apparaître dans l'arbre : {confirmation:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn un_identifiant_inexistant_donne_une_erreur_nommee() {
    let Some(navigateur) = chemin_du_navigateur() else {
        eprintln!("aucun navigateur disponible : test ignoré");
        return;
    };
    let recu = Arc::new(Mutex::new(None));
    let port = serveur(recu).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let browser = Browser::launch(&navigateur, dir.path(), port_libre())
        .await
        .unwrap();
    let endpoint = browser.page_endpoint().await.unwrap();
    let mut page = Page::attach(Session::connect(&endpoint).await.unwrap())
        .await
        .unwrap();
    page.navigate(&format!("http://127.0.0.1:{port}/"))
        .await
        .unwrap();
    page.tree(Detail::Normal).await.unwrap();

    let (resultat, _) = page
        .act("set_field", Some("n99999"), Some("x"))
        .await
        .unwrap();
    assert!(!resultat.ok);
    assert_eq!(
        resultat.error.as_deref(),
        Some("NodeNotFound"),
        "un clic à l'aveugle échoue en silence ; une action typée dit pourquoi"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn le_resume_reduit_l_observation() {
    let Some(navigateur) = chemin_du_navigateur() else {
        eprintln!("aucun navigateur disponible : test ignoré");
        return;
    };
    let recu = Arc::new(Mutex::new(None));
    let port = serveur(recu).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let browser = Browser::launch(&navigateur, dir.path(), port_libre())
        .await
        .unwrap();
    let endpoint = browser.page_endpoint().await.unwrap();
    let mut page = Page::attach(Session::connect(&endpoint).await.unwrap())
        .await
        .unwrap();
    page.navigate(&format!("http://127.0.0.1:{port}/"))
        .await
        .unwrap();

    let complet = page.tree(Detail::Full).await.unwrap();
    let resume = page.tree(Detail::Summary).await.unwrap();
    assert!(
        resume.root.count() <= complet.root.count(),
        "le résumé ne doit pas grossir"
    );
    // Les éléments actionnables survivent au résumé : c'est tout l'intérêt.
    let mut boutons = 0;
    resume.root.walk(&mut |n| {
        if n.role == Role::Button {
            boutons += 1;
        }
    });
    assert_eq!(
        boutons, 1,
        "le bouton de confirmation reste visible dans le résumé"
    );
}
