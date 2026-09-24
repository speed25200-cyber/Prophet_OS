//! Commandes humaines rendues et services réels. Le modèle HTTP est contrôlé par le test.
use std::io::{BufRead as _, Read as _, Write as _};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use agentd::State;
use egui::{Event, Modifiers, PointerButton};
use prophet_daemon::essai::{Daemon, binaire_voisin};
use prophet_ipc::Client;
use serde_json::{Value, json};
use surface::bureau::Bureau;
use surface::fenetre::Source as _;
use surface::gpu::{Cible, Contexte};
use surface::missions::{Action, Missions};
use surface::reel::{Reel, Sockets};

const ID: &str = "mission-supervision";
const TEXT: &str = "La note est prête à être examinée.\n\nElle résume le rôle de l'humain : définir l'objectif, examiner les accès, suivre l'exécution et relire le travail produit avant son application.\n\nUn fichier a été préparé dans le périmètre de la mission.";

struct Model {
    endpoint: String,
    received: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Model {
    fn new() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let (sent, received) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(40);
            let mut turn = 0;
            while turn < 2 {
                let stream = loop {
                    if let Ok((stream, _)) = listener.accept() {
                        break stream;
                    }
                    if Instant::now() > deadline {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut stream = std::io::BufReader::new(stream);
                let mut first = String::new();
                if stream.read_line(&mut first).unwrap() == 0 {
                    return;
                }
                let mut size = 0;
                loop {
                    let mut line = String::new();
                    if stream.read_line(&mut line).unwrap() == 0 {
                        return;
                    }
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        size = value.trim().parse::<usize>().unwrap();
                    }
                }
                if first.starts_with("GET /v1/models ") {
                    let body = json!({"data":[{"id":"modele-controle"}]}).to_string();
                    write!(stream.get_mut(), "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                    continue;
                }
                assert!(size < 128 * 1024);
                let mut request = vec![0; size];
                stream.read_exact(&mut request).unwrap();
                if turn == 0 {
                    sent.send(()).unwrap();
                    if gate.recv_timeout(Duration::from_secs(30)).is_err() {
                        return;
                    }
                }
                let reply = if turn == 0 {
                    json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"tool_calls":[{"type":"function","id":"write-note","function":{"name":"fs.write","arguments":json!({"path":"~/docs/note.txt","content":"L'humain définit, supervise et examine le travail des agents."}).to_string()}}]}}],"usage":{"prompt_tokens":30,"completion_tokens":20}})
                } else {
                    json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":TEXT}}],"usage":{"prompt_tokens":50,"completion_tokens":35}})
                };
                let body = reply.to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                if stream.get_mut().write_all(response.as_bytes()).is_err() {
                    return;
                }
                turn += 1;
            }
        });
        Self {
            endpoint,
            received,
            release,
            worker: Some(worker),
        }
    }
}

struct Chain {
    dir: tempfile::TempDir,
    sockets: Sockets,
    _daemons: Vec<Daemon>,
    runtime: tokio::runtime::Runtime,
    agents: Client,
}

impl Chain {
    fn new(endpoint: &str) -> Self {
        let chain = Self::with_model(endpoint, "modele-controle");
        chain.seed();
        chain
    }

    fn with_model(endpoint: &str, model: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join("docs")).unwrap();
        let sockets = Sockets {
            agentd: dir.path().join("agent.sock"),
            capd: dir.path().join("cap.sock"),
            sandboxd: dir.path().join("sandbox-absent.sock"),
        };
        let journal = dir.path().join("ledger.sock");
        let profile = dir.path().join("profiles.json");
        let mut example: Value =
            serde_json::from_str(include_str!("../../../examples/missions/note-locale.json"))
                .unwrap();
        example["manifest"]["model"]["preferred"] = json!([format!("local:{model}")]);
        example["manifest"]["capabilities"]["max"]["fs.read"] = json!(["~/docs/**"]);
        example["manifest"]["capabilities"]["max"]["fs.write"] = json!(["~/docs/**"]);
        std::fs::write(&profile,json!([{"id":"documents","name":"Documents de travail","description":"Rédiger et préparer des fichiers dans votre espace documentaire.","manifest":example["manifest"],"scopes":["~/docs"]}]).to_string()).unwrap();
        let capd = Daemon::lancer_avec(
            binaire_voisin("prophet-capd").to_str().unwrap(),
            &sockets.capd,
            &dir.path().join("cap-state"),
            &[("PROPHET_HOME", home.to_str().unwrap())],
        );
        let ledger = Daemon::lancer(
            binaire_voisin("prophet-ledger").to_str().unwrap(),
            &journal,
            &dir.path().join("ledger-state"),
        );
        let agentd = Daemon::lancer_avec(
            binaire_voisin("prophet-agentd").to_str().unwrap(),
            &sockets.agentd,
            &dir.path().join("agent-state"),
            &[
                ("PROPHET_HOME", home.to_str().unwrap()),
                ("PROPHET_CAPD_SOCKET", sockets.capd.to_str().unwrap()),
                ("PROPHET_LEDGER_SOCKET", journal.to_str().unwrap()),
                ("PROPHET_LOCAL_ENDPOINT", endpoint),
                ("PROPHET_MISSION_PROFILES", profile.to_str().unwrap()),
            ],
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let agents = runtime.block_on(async {
            drop(capd.joindre().await);
            drop(ledger.joindre().await);
            agentd.joindre().await
        });
        Self {
            dir,
            sockets,
            _daemons: vec![agentd, ledger, capd],
            runtime,
            agents,
        }
    }

    fn seed(&self) {
        self.call("task.spawn", json!({"id":ID,"intent":"Essai d'intégration — Préparer une note sur la supervision humaine","user":"prophet",
            "manifest":{"agent":{"id":"org.prophet.surface-test","version":"1.0.0","name":"Essai de supervision","publisher_key":"ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="},"model":{"preferred":["local:modele-controle"]},"sandbox":{"min_level":0},"capabilities":{"max":{"fs.read":["~/docs/**"],"fs.write":["~/docs/**"],"tool.call":["fs.write"]}},"budget":{"default":{"tokens":2000,"wall_time":"60s","approvals":3}}},
            "requested":[{"res":"fs","act":"read","match":"~/docs/**"},{"res":"fs","act":"write","match":"~/docs/**"},{"res":"tool","act":"call","match":"fs.write"}],"scopes":["~/docs"],"availability":{"local_models":["modele-controle"]}}));
    }
    fn call(&self, method: &str, params: Value) -> Value {
        self.runtime
            .block_on(self.agents.call(method, params))
            .unwrap()
    }
    /// Écrit au journal un événement de la mission, comme egress le fait pour ses sorties.
    fn journaliser(&self, kind: &str, payload: Value) {
        let journal = self.dir.path().join("ledger.sock");
        self.runtime
            .block_on(async {
                prophet_ipc::Client::connect(journal)
                    .await
                    .unwrap()
                    .call(
                        "ledger.append",
                        json!({"kind": kind, "task": ID, "actor": "egress", "payload": payload}),
                    )
                    .await
            })
            .unwrap();
    }
    fn wait(&self, missions: &mut Missions, state: State) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            missions.update();
            if missions.snapshot().is_some_and(|s| s.task.state == state) && !missions.busy() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "état {state:?} absent : lecture={:?}, commande={:?}, état reçu={:?}",
                missions.error(),
                missions.notice(),
                missions.snapshot().map(|s| s.task.state)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn le_controleur_lance_une_fois_et_confirme_l_arret_du_vrai_travailleur() {
    let model = Model::new();
    let chain = Chain::new(&model.endpoint);
    let mut missions = Missions::connect(chain.sockets.agentd.clone());
    missions.select(Some(ID));
    chain.wait(&mut missions, State::Planned);
    missions.command(Action::Start).unwrap();
    assert!(
        missions.command(Action::Start).is_err(),
        "pas de double lancement"
    );
    model
        .received
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    chain.wait(&mut missions, State::Running);
    missions.command(Action::Cancel).unwrap();
    assert!(missions.command(Action::Cancel).is_err());
    chain.wait(&mut missions, State::Cancelled);
    assert_eq!(
        chain.call("task.status", json!({"id":ID}))["state"],
        "cancelled"
    );
    assert!(!chain.dir.path().join("home/docs/note.txt").exists());
    drop(model.release); // Libère le serveur de test sans produire un second tour.
    model.worker.unwrap().join().unwrap();
}

fn frame(
    bureau: &mut Bureau,
    source: &mut Reel,
    context: &Contexte,
    target: &Cible,
    events: Vec<Event>,
) -> egui::PlatformOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(target.largeur as f32, target.hauteur as f32),
        )),
        time: Some(8.0),
        events,
        focused: true,
        ..Default::default()
    };
    let (mut output, decision) = bureau.composer(input, &source.scene());
    assert!(decision.is_none());
    bureau.rendre(context, target, &mut output);
    output.platform_output
}

fn click(bureau: &Bureau, target: &Cible, id: &str) -> Vec<Event> {
    let response = bureau
        .ctx
        .read_response(egui::Id::new(id))
        .unwrap_or_else(|| panic!("contrôle absent : {id}"));
    let bounds = egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(target.largeur as f32, target.hauteur as f32),
    );
    assert!(response.enabled(), "{id} désactivé");
    assert!(
        response.interact_rect.contains_rect(response.rect),
        "{id} coupé par le défilement : {:?} dans {:?}",
        response.rect,
        response.interact_rect
    );
    assert!(
        bounds.contains_rect(response.rect),
        "{id} hors écran : {:?}",
        response.rect
    );
    let pos = response.rect.center();
    vec![
        Event::PointerMoved(pos),
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::default(),
        },
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::default(),
        },
    ]
}

fn capture(context: &Contexte, target: &Cible, name: &str) {
    if let Ok(dir) = std::env::var("PROPHET_CAPTURE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(
            std::path::Path::new(&dir)
                .join(format!("surface-mission-{name}-{}.png", target.largeur)),
        )
        .unwrap();
        let mut encoder = png::Encoder::new(file, target.largeur, target.hauteur);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&target.pixels(context).unwrap())
            .unwrap();
    }
}

#[test]
#[ignore = "needs_gpu: services réels et modèle HTTP contrôlé"]
fn les_widgets_lancent_la_mission_et_permettent_de_lire_son_resultat() {
    let mut model = Model::new();
    let chain = Chain::new(&model.endpoint);
    let context = Contexte::hors_ecran().unwrap();
    let mut source = Reel::demarrer(chain.sockets.clone());
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.brancher_missions(chain.sockets.agentd.clone());
    bureau.brancher_journal(chain.dir.path().join("ledger.sock"));
    bureau.figer_transitions();
    let target = Cible::nouvelle(&context, 1440, 1000);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
        if bureau.missions().snapshot().is_some() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    capture(&context, &target, "plan");
    let events = click(&bureau, &target, "mission-start");
    frame(&mut bureau, &mut source, &context, &target, events);
    model
        .received
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    chain.wait(bureau.missions(), State::Running);
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    capture(&context, &target, "execution");
    model.release.send(()).unwrap();
    chain.wait(bureau.missions(), State::Done);
    model.worker.take().unwrap().join().unwrap();
    assert_eq!(
        bureau
            .missions()
            .snapshot()
            .unwrap()
            .result
            .as_ref()
            .unwrap()["diff"]["changes"][0]["path"],
        "docs/note.txt"
    );
    assert!(!chain.dir.path().join("home/docs/note.txt").exists());
    assert!(
        chain
            .dir
            .path()
            .join("home/.prophet/tasks/mission-supervision/work/docs/note.txt")
            .exists()
    );
    // Le parcours relu dans le journal dit quel fichier l'agent a touché, et que l'appel a
    // réussi ; il ne dit pas ce qu'il y a écrit.
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
        let trail = bureau.missions().trail();
        if trail.iter().any(|e| {
            e.tool == "fs.write"
                && e.target
                    .as_deref()
                    .is_some_and(|t| t.ends_with("/docs/note.txt"))
                && e.outcome == surface::missions::Outcome::Ok
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "parcours sans l'appel fs.write : {trail:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    // Le dernier geste se lit dans l'en-tête de la mission, sans ouvrir le parcours, et il
    // est offert à l'accessibilité.
    frame(&mut bureau, &mut source, &context, &target, vec![]);
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("mission-dernier-geste"))
            .is_some(),
        "le dernier geste de l'agent n'est pas affiché"
    );
    for (width, height) in [(1440, 1000), (1920, 1080), (1280, 800), (640, 900)] {
        let target = Cible::nouvelle(&context, width, height);
        for _ in 0..3 {
            frame(&mut bureau, &mut source, &context, &target, vec![]);
        }
        if width == 640 {
            let events = click(&bureau, &target, &format!("mission-{ID}"));
            frame(&mut bureau, &mut source, &context, &target, events);
            for _ in 0..3 {
                frame(&mut bureau, &mut source, &context, &target, vec![]);
            }
        }
        capture(&context, &target, "resultat");
        let events = click(&bureau, &target, "mission-copy-result");
        let output = frame(&mut bureau, &mut source, &context, &target, events);
        assert!(
            output
                .commands
                .iter()
                .any(|c| matches!(c,egui::OutputCommand::CopyText(text) if text == TEXT)),
            "le résultat reçu doit être copiable sans modification"
        );
        let events = click(&bureau, &target, "mission-history-tab");
        frame(&mut bureau, &mut source, &context, &target, events);
        for _ in 0..3 {
            frame(&mut bureau, &mut source, &context, &target, vec![]);
        }
        capture(&context, &target, "parcours");
        let events = click(&bureau, &target, "mission-result-tab");
        frame(&mut bureau, &mut source, &context, &target, events);
    }
}

#[test]
#[ignore = "needs_gpu: arrêt et échec avec les services réels"]
fn les_widgets_permettent_l_arret_et_montrent_un_echec_de_moteur() {
    let model = Model::new();
    let context = Contexte::hors_ecran().unwrap();
    for (endpoint, expected, name) in [
        (&model.endpoint[..], State::Cancelled, "arret"),
        ("http://127.0.0.1:1/v1", State::Failed, "echec"),
    ] {
        let chain = Chain::new(endpoint);
        let mut source = Reel::demarrer(chain.sockets.clone());
        let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
        bureau.brancher_missions(chain.sockets.agentd.clone());
        bureau.figer_transitions();
        let target = Cible::nouvelle(&context, 1440, 1000);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            frame(&mut bureau, &mut source, &context, &target, vec![]);
            if bureau.missions().snapshot().is_some() {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        for _ in 0..3 {
            frame(&mut bureau, &mut source, &context, &target, vec![]);
        }
        let events = click(&bureau, &target, "mission-start");
        frame(&mut bureau, &mut source, &context, &target, events);
        if expected == State::Cancelled {
            model
                .received
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            chain.wait(bureau.missions(), State::Running);
            for _ in 0..3 {
                frame(&mut bureau, &mut source, &context, &target, vec![]);
            }
            let events = click(&bureau, &target, "mission-cancel");
            frame(&mut bureau, &mut source, &context, &target, events);
        }
        chain.wait(bureau.missions(), expected);
        assert!(bureau.missions().snapshot().unwrap().task.reason.is_some());
        for _ in 0..3 {
            frame(&mut bureau, &mut source, &context, &target, vec![]);
        }
        capture(&context, &target, name);
        // La mission reste dans la vue terminée, y compris en cas d'échec ou d'annulation.
        let events = click(&bureau, &target, "filter-done");
        frame(&mut bureau, &mut source, &context, &target, events);
        for _ in 0..3 {
            frame(&mut bureau, &mut source, &context, &target, vec![]);
        }
        // Une mission seule est directement ouverte dans l'inspecteur.
        // Vérifier l'identité réellement consultable, pas la présence d'une carte de galerie.
        let events = click(&bureau, &target, "copier-reference");
        let output = frame(&mut bureau, &mut source, &context, &target, events);
        assert!(
            output
                .commands
                .iter()
                .any(|c| matches!(c, egui::OutputCommand::CopyText(value) if value == ID))
        );
        assert_eq!(bureau.missions().snapshot().unwrap().task.state, expected);
        assert!(!chain.dir.path().join("home/docs/note.txt").exists());
        // Échouée ou arrêtée, la mission se relance : la préparation s'ouvre avec la même
        // intention et le même modèle, et rien ne part avant que l'humain confirme le plan.
        let intention = bureau.missions().snapshot().unwrap().task.intent.clone();
        let events = click(&bureau, &target, "mission-relancer");
        frame(&mut bureau, &mut source, &context, &target, events);
        for _ in 0..3 {
            frame(&mut bureau, &mut source, &context, &target, vec![]);
        }
        // Le modèle est repris lui aussi, puis confronté au catalogue : un moteur injoignable
        // ne le propose plus, et la préparation ne l'invente pas (test unitaire de `relance`).
        assert_eq!(bureau.preparation().intent, intention);
        assert!(bureau.preparation().attempted_id().is_none());
        assert!(
            bureau
                .ctx
                .read_response(egui::Id::new("mission-prepare-submit"))
                .is_some(),
            "la préparation est ouverte"
        );
        capture(&context, &target, &format!("{name}-relance"));
        assert_eq!(
            chain.call("task.list", json!({})).as_array().unwrap().len(),
            1,
            "relancer ne crée rien avant la confirmation"
        );
    }
    drop(model.release);
    model.worker.unwrap().join().unwrap();
}

#[test]
#[ignore = "needs_gpu: intention saisie, préparation, lancement et résultat avec services réels"]
fn une_intention_saisie_dans_la_surface_devient_une_mission_et_un_fichier_prepare() {
    let mut model = Model::new();
    let chain = Chain::new(&model.endpoint);
    let context = Contexte::hors_ecran().unwrap();
    let mut source = Reel::demarrer(chain.sockets.clone());
    // Le catalogue doit provenir du moteur agentd malgré le dialogue non connecté.
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.brancher_missions(chain.sockets.agentd.clone());
    bureau.figer_transitions();
    let target = Cible::nouvelle(&context, 1440, 1000);
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    // Une recherche antérieure ne doit pas masquer le nouveau plan confirmé.
    let events = click(&bureau, &target, "mission-search");
    frame(&mut bureau, &mut source, &context, &target, events);
    frame(
        &mut bureau,
        &mut source,
        &context,
        &target,
        vec![Event::Paste("aucune-mission-ne-correspond".into())],
    );
    let events = click(&bureau, &target, "preparer-mission");
    frame(&mut bureau, &mut source, &context, &target, events);
    let deadline = Instant::now() + Duration::from_secs(10);
    while bureau.preparation().options().is_none() {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
        assert!(
            Instant::now() < deadline,
            "catalogue absent : {:?}",
            bureau.preparation().error()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let events = click(&bureau, &target, "mission-intent");
    frame(&mut bureau, &mut source, &context, &target, events);
    let intent = "Prépare une note sur la supervision humaine dans ~/docs/note.txt, puis résume le travail réalisé.";
    frame(
        &mut bureau,
        &mut source,
        &context,
        &target,
        vec![Event::Paste(intent.into())],
    );
    assert_eq!(bureau.preparation().intent, intent);
    for (width, height) in [(1440, 1000), (1280, 800), (640, 900)] {
        let size = Cible::nouvelle(&context, width, height);
        for _ in 0..3 {
            frame(&mut bureau, &mut source, &context, &size, vec![]);
        }
        let _ = click(&bureau, &size, "mission-prepare-submit"); // Le bouton entier reste accessible.
        capture(&context, &size, "objectif");
    }
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    let events = click(&bureau, &target, "mission-prepare-submit");
    frame(&mut bureau, &mut source, &context, &target, events);
    let id = bureau.preparation().attempted_id().unwrap().to_owned();
    assert!(
        model.received.try_recv().is_err(),
        "aucune inférence avant lancement"
    );
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
        if bureau
            .missions()
            .snapshot()
            .is_some_and(|s| s.task.id == id && s.task.state == State::Planned)
            && bureau
                .ctx
                .read_response(egui::Id::new("mission-start"))
                .is_some_and(|r| r.enabled())
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "plan absent : {:?}",
            bureau.preparation().error()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(bureau.missions().snapshot().unwrap().task.intent, intent);
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    capture(&context, &target, "objectif-plan");
    let events = click(&bureau, &target, "mission-start");
    frame(&mut bureau, &mut source, &context, &target, events);
    model
        .received
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    model.release.send(()).unwrap();
    chain.wait(bureau.missions(), State::Done);
    model.worker.take().unwrap().join().unwrap();
    assert_eq!(
        std::fs::read_to_string(
            chain
                .dir
                .path()
                .join(format!("home/.prophet/tasks/{id}/work/docs/note.txt"))
        )
        .unwrap(),
        "L'humain définit, supervise et examine le travail des agents."
    );
    assert!(!chain.dir.path().join("home/docs/note.txt").exists());
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    capture(&context, &target, "objectif-resultat");
    let events = click(&bureau, &target, "nav-conversation");
    frame(&mut bureau, &mut source, &context, &target, events);
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    let events = click(&bureau, &target, "intention");
    frame(&mut bureau, &mut source, &context, &target, events);
    let next = "Relis maintenant cette note et propose une version plus courte.";
    frame(
        &mut bureau,
        &mut source,
        &context,
        &target,
        vec![Event::Paste(next.into())],
    );
    frame(&mut bureau, &mut source, &context, &target, vec![]);
    let events = click(&bureau, &target, "conversation-vers-mission");
    frame(&mut bureau, &mut source, &context, &target, events);
    assert_eq!(bureau.preparation().intent, next);
    assert!(
        bureau.preparation().attempted_id().is_none(),
        "la nouvelle demande ne reprend pas la mission précédente"
    );
}

/// Même interface et mêmes services que le parcours contrôlé, avec un moteur externe réel.
/// La feature évite de confondre cet essai avec les captures sans poids de la CI.
#[cfg(feature = "real-model-tests")]
#[test]
#[ignore = "needs_gpu, needs_local_model: moteur et modèle explicitement requis"]
fn une_intention_graphique_est_executee_par_un_modele_reel() {
    let endpoint = std::env::var("PROPHET_TEST_ENDPOINT").expect("PROPHET_TEST_ENDPOINT requis");
    let model = std::env::var("PROPHET_TEST_MODEL").expect("PROPHET_TEST_MODEL requis");
    let chain = Chain::with_model(&endpoint, &model);
    let context = Contexte::hors_ecran().unwrap();
    let mut source = Reel::demarrer(chain.sockets.clone());
    let mut bureau = Bureau::nouveau(&context, endpoint, false);
    bureau.brancher_missions(chain.sockets.agentd.clone());
    bureau.atelier.decouvrir(&bureau.ctx);
    bureau.figer_transitions();
    let target = Cible::nouvelle(&context, 1440, 1000);
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    let events = click(&bureau, &target, "preparer-mission");
    frame(&mut bureau, &mut source, &context, &target, events);
    let deadline = Instant::now() + Duration::from_secs(10);
    while bureau.preparation().options().is_none() {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
        assert!(
            Instant::now() < deadline,
            "catalogue absent : {:?}",
            bureau.preparation().error()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        bureau.preparation().options().unwrap().profiles[0].models,
        vec![model.clone()]
    );
    assert_eq!(bureau.atelier.modeles, vec![model.clone()]);
    let expected = format!("mission-{}", ulid::Ulid::new().random() as u32);
    let intent = format!(
        "Use fs.write to save exactly {expected} into ~/docs/note.txt. After staged=true, answer Done. /no_think"
    );
    let events = click(&bureau, &target, "mission-intent");
    frame(&mut bureau, &mut source, &context, &target, events);
    frame(
        &mut bureau,
        &mut source,
        &context,
        &target,
        vec![Event::Paste(intent.clone())],
    );
    assert_eq!(bureau.preparation().intent, intent);
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    capture(&context, &target, "reel-objectif");
    let events = click(&bureau, &target, "mission-prepare-submit");
    frame(&mut bureau, &mut source, &context, &target, events);
    let id = bureau.preparation().attempted_id().unwrap().to_owned();
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
        if bureau
            .missions()
            .snapshot()
            .is_some_and(|s| s.task.id == id && s.task.state == State::Planned)
            && bureau
                .ctx
                .read_response(egui::Id::new("mission-start"))
                .is_some_and(|r| r.enabled())
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "plan absent : {:?}",
            bureau.preparation().error()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !chain
            .dir
            .path()
            .join(format!("home/.prophet/tasks/{id}"))
            .exists(),
        "la préparation ne doit pas créer le travail"
    );
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    capture(&context, &target, "reel-plan");
    let events = click(&bureau, &target, "mission-start");
    let started = Instant::now();
    frame(&mut bureau, &mut source, &context, &target, events);
    let mut running_frames = 0;
    loop {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
        if let Some(snapshot) = bureau.missions().snapshot() {
            if snapshot.task.state.is_terminal() {
                assert_eq!(
                    snapshot.task.state,
                    State::Done,
                    "échec du modèle : {:?}",
                    snapshot.task.reason
                );
                break;
            }
            if snapshot.task.state == State::Running {
                running_frames += 1;
                if running_frames == 3 {
                    capture(&context, &target, "reel-execution");
                }
            }
        }
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "résultat absent"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let snapshot = bureau.missions().snapshot().unwrap();
    let result = snapshot
        .result
        .as_ref()
        .expect("résultat reçu par l'interface");
    assert_eq!(snapshot.task.intent, intent);
    assert_eq!(result["diff"]["changes"][0]["path"], "docs/note.txt");
    assert!(snapshot.task.budget.spent.tokens > 0);
    assert!(
        running_frames > 0,
        "l'interface doit composer pendant l'inférence"
    );
    assert_eq!(
        std::fs::read_to_string(
            chain
                .dir
                .path()
                .join(format!("home/.prophet/tasks/{id}/work/docs/note.txt"))
        )
        .unwrap(),
        expected
    );
    assert!(!chain.dir.path().join("home/docs/note.txt").exists());
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    capture(&context, &target, "reel-resultat");
    let events = click(&bureau, &target, "mission-file-docs/note.txt");
    frame(&mut bureau, &mut source, &context, &target, events);
    let deadline = Instant::now() + Duration::from_secs(10);
    while bureau.missions().file_review().is_none() {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
        assert!(
            Instant::now() < deadline,
            "aperçu absent : {:?}",
            bureau.missions().file_error()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let review = bureau.missions().file_review().unwrap();
    assert_eq!(review.task, id);
    assert_eq!(
        review.file.after.as_ref().unwrap().content,
        agentd::PreviewContent::Text {
            text: expected.clone()
        }
    );
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    capture(&context, &target, "reel-fichier");
    println!(
        "modèle={model}, mission={id}, contenu={expected}, durée={:?}, images pendant exécution={running_frames}",
        started.elapsed()
    );
}

#[test]
#[ignore = "needs_gpu: examen des deux versions avec vrais services et modèle HTTP contrôlé"]
fn les_widgets_comparent_les_versions_et_refusent_un_travail_altere() {
    let mut model = Model::new();
    let chain = Chain::new(&model.endpoint);
    let before = "La supervision humaine\n\nDéfinir un objectif.\nRelire le travail proposé.\n";
    let after = "L'humain définit, supervise et examine le travail des agents.";
    std::fs::write(chain.dir.path().join("home/docs/note.txt"), before).unwrap();
    let context = Contexte::hors_ecran().unwrap();
    let mut source = Reel::demarrer(chain.sockets.clone());
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.brancher_missions(chain.sockets.agentd.clone());
    bureau.figer_transitions();
    let target = Cible::nouvelle(&context, 1440, 1000);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
        if bureau
            .ctx
            .read_response(egui::Id::new("mission-start"))
            .is_some_and(|r| r.enabled())
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    let events = click(&bureau, &target, "mission-start");
    frame(&mut bureau, &mut source, &context, &target, events);
    model
        .received
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    model.release.send(()).unwrap();
    chain.wait(bureau.missions(), State::Done);
    model.worker.take().unwrap().join().unwrap();
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    let events = click(&bureau, &target, "mission-file-docs/note.txt");
    frame(&mut bureau, &mut source, &context, &target, events);
    let deadline = Instant::now() + Duration::from_secs(10);
    while bureau.missions().file_review().is_none() {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
        assert!(
            Instant::now() < deadline,
            "lecture absente : {:?}",
            bureau.missions().file_error()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let review = bureau.missions().file_review().unwrap();
    assert_eq!(
        review.file.before.as_ref().unwrap().content,
        agentd::PreviewContent::Text {
            text: before.into()
        }
    );
    assert_eq!(
        review.file.after.as_ref().unwrap().content,
        agentd::PreviewContent::Text { text: after.into() }
    );
    for (width, height) in [(1440, 1000), (1280, 800), (640, 900)] {
        let size = Cible::nouvelle(&context, width, height);
        for _ in 0..3 {
            frame(&mut bureau, &mut source, &context, &size, vec![]);
        }
        if width == 640 {
            let events = click(&bureau, &size, &format!("mission-{ID}"));
            frame(&mut bureau, &mut source, &context, &size, events);
            for _ in 0..3 {
                frame(&mut bureau, &mut source, &context, &size, vec![]);
            }
        }
        capture(&context, &size, "fichier");
        let events = click(&bureau, &size, "review-copy-after");
        let output = frame(&mut bureau, &mut source, &context, &size, events);
        assert!(
            output
                .commands
                .iter()
                .any(|c| matches!(c, egui::OutputCommand::CopyText(text) if text == after))
        );
    }
    std::fs::write(
        chain
            .dir
            .path()
            .join(format!("home/.prophet/tasks/{ID}/work/docs/note.txt")),
        "Travail altéré après la mission",
    )
    .unwrap();
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    let events = click(&bureau, &target, "review-refresh");
    frame(&mut bureau, &mut source, &context, &target, events);
    assert!(bureau.missions().file_review().is_none());
    let deadline = Instant::now() + Duration::from_secs(10);
    while bureau.missions().file_error().is_none() {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(bureau.missions().file_review().is_none());
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    capture(&context, &target, "fichier-altere");
    assert_eq!(
        std::fs::read_to_string(chain.dir.path().join("home/docs/note.txt")).unwrap(),
        before
    );
}

/// L'arrêt d'urgence avec les vrais services : la mission est en pleine inférence, l'humain
/// demande « Tout arrêter » puis confirme ; agentd arrête le travailleur, la mission finit
/// annulée, rien n'est écrit, et la surface dit l'issue.
#[test]
#[ignore = "needs_gpu: arrêt d'urgence avec les services réels"]
fn l_arret_d_urgence_arrete_la_mission_en_cours_par_le_service() {
    let model = Model::new();
    let context = Contexte::hors_ecran().unwrap();
    let chain = Chain::new(&model.endpoint);
    let mut source = Reel::demarrer(chain.sockets.clone());
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.brancher_missions(chain.sockets.agentd.clone());
    bureau.figer_transitions();
    let target = Cible::nouvelle(&context, 1440, 1000);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
        if bureau.missions().snapshot().is_some() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    let events = click(&bureau, &target, "mission-start");
    frame(&mut bureau, &mut source, &context, &target, events);
    model
        .received
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    chain.wait(bureau.missions(), State::Running);
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    let events = click(&bureau, &target, "arret-tout");
    frame(&mut bureau, &mut source, &context, &target, events);
    for _ in 0..2 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    let events = click(&bureau, &target, "arret-confirmer");
    frame(&mut bureau, &mut source, &context, &target, events);
    chain.wait(bureau.missions(), State::Cancelled);
    let deadline = Instant::now() + Duration::from_secs(10);
    while bureau
        .ctx
        .read_response(egui::Id::new("arret-ecarter"))
        .is_none()
    {
        assert!(Instant::now() < deadline, "l'issue de l'arrêt se lit");
        frame(&mut bureau, &mut source, &context, &target, vec![]);
        std::thread::sleep(Duration::from_millis(20));
    }
    capture(&context, &target, "arret-urgence");
    assert!(!chain.dir.path().join("home/docs/note.txt").exists());
    drop(model.release);
    model.worker.unwrap().join().unwrap();
}

/// Les sorties réseau d'une mission prennent place dans sa frise (ADR 0056) : egress les écrit
/// au journal de la mission — une requête relayée, un hôte refusé —, et le parcours les montre
/// avec l'hôte, les octets et l'issue, à côté des appels d'outils.
#[test]
#[ignore = "needs_gpu: parcours des sorties réseau avec les services réels"]
fn le_parcours_montre_les_sorties_reseau_de_la_mission() {
    let (endpoint, worker, _liberer) = moteur_scripte(
        vec![
            appel(
                1,
                "fs.write",
                json!({"path":"~/docs/note.txt","content":TEXT}),
            ),
            conclusion(TEXT),
        ],
        None,
    );
    let chain = Chain::with_model(&endpoint, "modele-controle");
    chain.seed();
    let context = Contexte::hors_ecran().unwrap();
    let mut source = Reel::demarrer(chain.sockets.clone());
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.brancher_missions(chain.sockets.agentd.clone());
    bureau.brancher_journal(chain.dir.path().join("ledger.sock"));
    bureau.figer_transitions();
    let target = Cible::nouvelle(&context, 1440, 1000);
    let deadline = Instant::now() + Duration::from_secs(10);
    while bureau.missions().snapshot().is_none() {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    chain.call("task.start", json!({"id":ID}));
    chain.wait(bureau.missions(), State::Done);
    worker.join().unwrap();
    // Un client ouvre plusieurs connexions de suite vers son éditeur, puis bute deux fois sur
    // un hôte que son jeton ne porte pas : deux lignes, qui comptent chacune les leurs.
    for _ in 0..3 {
        chain.journaliser(
            "net.request",
            json!({"host":"api.anthropic.com","port":443,"method":"CONNECT","bytes_out":12_400,"bytes_in":48_000,"status":200}),
        );
    }
    for _ in 0..2 {
        chain.journaliser(
            "net.deny",
            json!({"host":"collecte.exemple","reason":"no_grant"}),
        );
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
        let sorties = bureau
            .missions()
            .trail()
            .iter()
            .filter(|e| e.tool == "sortie")
            .map(|e| e.fois)
            .sum::<u32>();
        if sorties == 5 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "sorties absentes du parcours : {:?}",
            bureau.missions().trail()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    // Le dernier geste vient de paraître et décale la page : le clic vise l'onglet une fois la
    // mise en page posée.
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    let events = click(&bureau, &target, "mission-history-tab");
    frame(&mut bureau, &mut source, &context, &target, events);
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    capture(&context, &target, "sorties-reseau");
    let trail = bureau.missions().trail();
    assert!(trail.iter().any(|e| {
        e.tool == "sortie"
            && e.outcome == surface::missions::Outcome::Ok
            && e.fois == 3
            && e.target
                .as_deref()
                .is_some_and(|t| t.starts_with("api.anthropic.com — 3 × CONNECT"))
    }));
    assert!(trail.iter().any(|e| e.tool == "sortie"
        && e.fois == 2
        && matches!(&e.outcome, surface::missions::Outcome::Denied(m) if m == "no_grant")));
    // En largeur étroite, la frise passe sous les états et chaque sortie tient dans la colonne.
    let etroite = Cible::nouvelle(&context, 640, 1000);
    for id in [format!("mission-{ID}"), "mission-history-tab".to_owned()] {
        for _ in 0..3 {
            frame(&mut bureau, &mut source, &context, &etroite, vec![]);
        }
        let events = click(&bureau, &etroite, &id);
        frame(&mut bureau, &mut source, &context, &etroite, events);
    }
    // La frise des gestes est sous les états : la page défile jusqu'à elle, d'un geste de pavé
    // tactile, qu'egui applique sans lissage (le lissage de la molette suit une horloge que
    // l'essai tient figée).
    let geste = |phase, dy| Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, dy),
        phase,
        modifiers: Modifiers::default(),
    };
    // Le pointeur survole la page une image avant le geste : egui choisit la zone qui défile
    // d'après ce qu'il survolait.
    frame(
        &mut bureau,
        &mut source,
        &context,
        &etroite,
        vec![Event::PointerMoved(egui::pos2(320.0, 700.0))],
    );
    let molette = vec![
        geste(egui::TouchPhase::Start, 0.0),
        geste(egui::TouchPhase::Move, -420.0),
    ];
    frame(&mut bureau, &mut source, &context, &etroite, molette);
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &etroite, vec![]);
    }
    capture(&context, &etroite, "sorties-reseau");
}

/// Un moteur scripté : il rend ses réponses dans l'ordre, une par requête de complétion, et le
/// catalogue de modèles à qui le demande ; la réponse de rang `retenue` attend que le test la
/// libère, pour qu'il observe la mission en cours.
fn moteur_scripte(
    reponses: Vec<Value>,
    retenue: Option<usize>,
) -> (String, std::thread::JoinHandle<()>, mpsc::Sender<()>) {
    let (liberer, porte) = mpsc::channel::<()>();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let worker = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut rang = 0;
        let mut reponses = reponses.into_iter();
        loop {
            let stream = loop {
                if let Ok((stream, _)) = listener.accept() {
                    break stream;
                }
                if Instant::now() > deadline {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut stream = std::io::BufReader::new(stream);
            let mut first = String::new();
            if stream.read_line(&mut first).unwrap_or(0) == 0 {
                continue;
            }
            let mut size = 0;
            loop {
                let mut line = String::new();
                if stream.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    size = value.trim().parse::<usize>().unwrap();
                }
            }
            let completion = !first.starts_with("GET /v1/models ");
            let body = if completion {
                let mut request = vec![0; size];
                let _ = stream.read_exact(&mut request);
                if retenue == Some(rang) && porte.recv_timeout(Duration::from_secs(30)).is_err() {
                    return;
                }
                rang += 1;
                let Some(reponse) = reponses.next() else {
                    return;
                };
                reponse.to_string()
            } else {
                json!({"data":[{"id":"modele-controle"}]}).to_string()
            };
            let _ = write!(
                stream.get_mut(),
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            if completion && reponses.len() == 0 {
                return;
            }
        }
    });
    (endpoint, worker, liberer)
}

fn appel(n: u32, outil: &str, arguments: Value) -> Value {
    json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"tool_calls":[{"type":"function","id":format!("appel-{n}"),"function":{"name":outil,"arguments":arguments.to_string()}}]}}],"usage":{"prompt_tokens":40,"completion_tokens":12}})
}

fn conclusion(texte: &str) -> Value {
    json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":texte}}],"usage":{"prompt_tokens":60,"completion_tokens":8}})
}

/// Le parcours d'une mission qui se trompe puis se corrige : une recherche, une écriture hors
/// de la portée que capd refuse et que le modèle reçoit (ADR 0050), une conclusion sans le
/// fichier demandé que le service rappelle (ADR 0049), puis l'écriture juste. La frise dit
/// chacun de ces gestes à sa place.
#[test]
#[ignore = "needs_gpu: services réels et modèle HTTP scripté"]
fn le_parcours_montre_le_refus_rendu_au_modele_et_le_rappel_du_livrable() {
    // La réponse de rang 3 (l'écriture juste) attend : la mission est alors en cours, après la
    // recherche, le refus et le rappel.
    let (endpoint, worker, liberer) = moteur_scripte(
        vec![
            appel(1, "fs.search", json!({"root":"~/docs"})),
            appel(
                2,
                "fs.write",
                json!({"path":"~/ailleurs/note.txt","content":"hors de la portée"}),
            ),
            conclusion("C'est fait."),
            appel(
                3,
                "fs.write",
                json!({"path":"~/docs/note.txt","content":"L'humain définit, supervise et examine."}),
            ),
            conclusion(TEXT),
        ],
        Some(3),
    );
    let chain = Chain::with_model(&endpoint, "modele-controle");
    chain.call("task.spawn", json!({"id":ID,"intent":"Écris une note sur la supervision humaine dans ~/docs/note.txt","user":"prophet",
        "manifest":{"agent":{"id":"org.prophet.surface-test","version":"1.0.0","name":"Essai de supervision","publisher_key":"ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="},"model":{"preferred":["local:modele-controle"]},"sandbox":{"min_level":0},"capabilities":{"max":{"fs.read":["~/docs/**"],"fs.write":["~/docs/**"],"tool.call":["fs.write","fs.search"]}},"budget":{"default":{"tokens":4000,"wall_time":"60s","approvals":3}}},
        "requested":[{"res":"fs","act":"read","match":"~/docs/**"},{"res":"fs","act":"write","match":"~/docs/**"},{"res":"tool","act":"call","match":"fs.write"},{"res":"tool","act":"call","match":"fs.search"}],"scopes":["~/docs"],"availability":{"local_models":["modele-controle"]}}));
    let context = Contexte::hors_ecran().unwrap();
    let mut source = Reel::demarrer(chain.sockets.clone());
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.brancher_missions(chain.sockets.agentd.clone());
    bureau.brancher_journal(chain.dir.path().join("ledger.sock"));
    bureau.figer_transitions();
    let target = Cible::nouvelle(&context, 1440, 1000);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
        if bureau.missions().snapshot().is_some() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    chain.call("task.start", json!({"id":ID}));
    // En cours : les derniers gestes se lisent en direct dans l'onglet Proposition.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
        let rappele = bureau.missions().trail().iter().any(|e| e.tool == "rappel");
        if rappele
            && bureau
                .missions()
                .snapshot()
                .is_some_and(|s| s.task.state == State::Running)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "la mission n'atteint pas le rappel"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    for _ in 0..3 {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
    }
    capture(&context, &target, "direct");
    liberer.send(()).unwrap();
    chain.wait(bureau.missions(), State::Done);
    worker.join().unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        frame(&mut bureau, &mut source, &context, &target, vec![]);
        let trail = bureau.missions().trail();
        let refuse = trail.iter().any(|e| {
            e.tool == "fs.write" && matches!(e.outcome, surface::missions::Outcome::Denied(_))
        });
        let rappele = trail
            .iter()
            .any(|e| e.tool == "rappel" && e.target.as_deref() == Some("~/docs/note.txt"));
        let ecrit = trail
            .iter()
            .any(|e| e.tool == "fs.write" && e.outcome == surface::missions::Outcome::Ok);
        if refuse && rappele && ecrit {
            break;
        }
        assert!(Instant::now() < deadline, "parcours incomplet : {trail:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
    for (width, height) in [(1440, 1000), (1920, 1080), (640, 900)] {
        let target = Cible::nouvelle(&context, width, height);
        for _ in 0..3 {
            frame(&mut bureau, &mut source, &context, &target, vec![]);
        }
        if width == 640 {
            let events = click(&bureau, &target, &format!("mission-{ID}"));
            frame(&mut bureau, &mut source, &context, &target, events);
            for _ in 0..3 {
                frame(&mut bureau, &mut source, &context, &target, vec![]);
            }
        }
        let events = click(&bureau, &target, "mission-history-tab");
        frame(&mut bureau, &mut source, &context, &target, events);
        for _ in 0..3 {
            frame(&mut bureau, &mut source, &context, &target, vec![]);
        }
        capture(&context, &target, "parcours-corrige");
        let events = click(&bureau, &target, "mission-result-tab");
        frame(&mut bureau, &mut source, &context, &target, events);
    }
}
