//! Les outils web : une page ouverte, lue et manipulée par son arbre, sous les mêmes contrôles
//! que les autres outils. Le navigateur est réel ; sans navigateur, le test se tait, sauf si
//! `PROPHET_EXIGER_NAVIGATEUR=1` transforme cette absence en échec.

use std::sync::{Arc, Mutex};

use capd::Broker;
use mcp_system::registry::{MemoryJournal, Registry, ToolContext};
use mcp_system::tools::Browsing;
use prophet_types::cap::{Act, Grant, Res};
use prophet_types::ledger::EventKind;
use prophet_types::manifest::Manifest;
use serde_json::{Value, json};
use time::OffsetDateTime;

const MANIFESTE: &str = r#"
[agent]
id = "org.exemple.navigateur"
version = "1.0.0"
name = "Navigateur"
publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="

[model]
preferred = ["local:qwen3-8b"]

[capabilities.max]
"net.egress" = ["127.0.0.1"]
"ui.read" = ["browser"]
"ui.act" = ["browser"]
"tool.call" = ["web.*"]
"#;

const PAGE: &str = r#"<!doctype html>
<html lang="fr"><head><meta charset="utf-8"><title>Réservation</title></head>
<body>
  <h1>Réserver un billet</h1>
  <form method="POST" action="/reserver">
    <label for="depart">Départ</label>
    <input id="depart" name="depart" type="text" placeholder="Ville de départ">
    <button type="submit" id="valider">Confirmer la réservation</button>
  </form>
  <p id="note">Les places sont limitées.</p>
</body></html>"#;

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
    let depuis_environnement = std::env::var("PROPHET_BROWSER").ok();
    assert!(
        !(depuis_environnement.is_none()
            && std::env::var("PROPHET_EXIGER_NAVIGATEUR").as_deref() == Ok("1")),
        "aucun navigateur trouvé alors que PROPHET_EXIGER_NAVIGATEUR=1"
    );
    depuis_environnement
}

fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

/// Un serveur HTTP minimal sur un thread, qui sert la page et accepte le formulaire.
fn serveur() -> u16 {
    use std::io::{Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buffer = [0u8; 8192];
            let n = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..n]);
            let body = if request.starts_with("POST") {
                "<html><body><h1>Réservation confirmée</h1></body></html>"
            } else {
                PAGE
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    port
}

struct Monde {
    _dir: tempfile::TempDir,
    registry: Arc<Registry>,
    journal: Arc<MemoryJournal>,
    context: ToolContext,
}

fn monde(program: &str) -> Monde {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let mut broker = Broker::new(
        ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng),
        "capd@test",
        home.display().to_string(),
    )
    .unwrap();
    let manifest = Manifest::from_toml(MANIFESTE).unwrap();
    let token = broker
        .mint(
            &manifest,
            "task:web",
            "u",
            &[
                Grant::new(Res::Net, Act::Egress, "127.0.0.1"),
                Grant::new(Res::Ui, Act::Read, "browser"),
                Grant::new(Res::Ui, Act::Act, "browser"),
                Grant::new(Res::Tool, Act::Call, "web.*"),
            ],
            1800,
            now(),
        )
        .unwrap();
    let journal = Arc::new(MemoryJournal::new());
    let mut registry = Registry::new(Arc::new(Mutex::new(broker)), journal.clone());
    let browsing = Browsing::new(program.into(), dir.path().join("navigateurs"));
    for tool in browsing.tools() {
        registry.register(tool);
    }
    let context = ToolContext {
        token,
        task: "task:web".into(),
        home: home.display().to_string(),
        workdir: home.display().to_string(),
        sandbox_level: 0,
        step: 1,
    };
    Monde {
        _dir: dir,
        registry: Arc::new(registry),
        journal,
        context,
    }
}

fn trouver<'a>(node: &'a Value, role: &str, name: &str) -> Option<&'a Value> {
    if node["role"] == role && node["name"].as_str().is_some_and(|n| n.contains(name)) {
        return Some(node);
    }
    node["children"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|child| trouver(child, role, name))
}

#[test]
fn ouvrir_lire_et_agir_par_l_arbre_sous_le_controle_de_capd() {
    let Some(program) = chemin_du_navigateur() else {
        eprintln!("aucun navigateur : test sans effet");
        return;
    };
    let port = serveur();
    let m = monde(&program);

    // Un hôte hors des droits est refusé avant tout lancement du navigateur.
    let refused = m.registry.call(
        "web.open",
        &json!({"url": "http://exemple.invalide/"}),
        &m.context,
        now(),
    );
    assert!(refused.is_error, "{refused:?}");
    assert_eq!(refused.structured.unwrap()["code"], "PolicyDenied");

    let opened = m.registry.call(
        "web.open",
        &json!({"url": format!("http://127.0.0.1:{port}/")}),
        &m.context,
        now(),
    );
    assert!(!opened.is_error, "{opened:?}");
    let opened = opened.structured.unwrap();
    assert_eq!(opened["title"], "Réservation", "{opened}");
    // La supervision lit où l'agent est, sans l'arbre : adresse, titre, taille.
    let observation: Value = serde_json::from_str(
        &std::fs::read_to_string(Browsing::observation_path(
            &m._dir.path().join("navigateurs"),
            "task:web",
        ))
        .expect("observation déposée"),
    )
    .unwrap();
    assert_eq!(observation["title"], "Réservation");
    assert!(
        observation["url"]
            .as_str()
            .unwrap()
            .starts_with("http://127.0.0.1")
    );
    assert!(observation["nodes"].as_u64().unwrap() > 3);
    assert!(observation.get("tree").is_none(), "jamais l'arbre lui-même");
    assert!(opened["nodes"].as_u64().unwrap() > 3);
    let field = trouver(&opened["tree"]["root"], "field", "Départ").expect("champ Départ");
    let field_id = field["id"].as_str().unwrap().to_owned();
    assert!(
        trouver(&opened["tree"]["root"], "text", "Réserver un billet").is_some(),
        "{opened}"
    );

    let typed = m.registry.call(
        "web.act",
        &json!({"action": "set_field", "node": field_id, "value": "Paris"}),
        &m.context,
        now(),
    );
    assert!(!typed.is_error, "{typed:?}");
    let tree = m
        .registry
        .call("web.tree", &json!({"detail": "full"}), &m.context, now());
    assert!(!tree.is_error, "{tree:?}");
    let tree = tree.structured.unwrap();
    let field = trouver(&tree["tree"]["root"], "field", "Départ").unwrap();
    assert_eq!(field["value"], "Paris", "{field}");

    // Soumettre engage un effet extérieur : sans décision humaine, capd refuse.
    let button = trouver(&tree["tree"]["root"], "button", "Confirmer").unwrap();
    let button_id = button["id"].as_str().unwrap().to_owned();
    let submit = m.registry.call(
        "web.act",
        &json!({"action": "submit", "node": button_id}),
        &m.context,
        now(),
    );
    assert!(submit.is_error, "{submit:?}");
    assert_eq!(submit.structured.unwrap()["code"], "ApprovalRequired");

    let missing = m.registry.call(
        "web.act",
        &json!({"action": "click", "node": "n999"}),
        &m.context,
        now(),
    );
    assert!(missing.is_error);
    assert_eq!(missing.structured.unwrap()["code"], "NotFound");

    // Le journal porte chaque appel et son issue, jamais le contenu de la page.
    let events = m.journal.events();
    assert!(
        events
            .iter()
            .filter(|e| e.kind == EventKind::ToolCall)
            .count()
            >= 5
    );
    assert!(
        events
            .iter()
            .all(|e| !e.payload.to_string().contains("Réserver")),
        "le contenu de la page n'entre pas dans le journal"
    );
    assert!(
        events.iter().any(|e| e.kind == EventKind::ToolCall
            && e.payload["tool"] == "web.open"
            && e.payload["target"] == "127.0.0.1"),
        "le journal dit quel hôte a été ouvert : {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| e.kind == EventKind::PolicyDeny || e.kind == EventKind::ToolResult),
    );
}
