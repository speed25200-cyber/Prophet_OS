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
        example["manifest"]["model"]["preferred"] = json!(["local:modele-controle"]);
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
        let chain = Self {
            dir,
            sockets,
            _daemons: vec![agentd, ledger, capd],
            runtime,
            agents,
        };
        chain.call("task.spawn", json!({"id":ID,"intent":"Essai d'intégration — Préparer une note sur la supervision humaine","user":"prophet",
            "manifest":{"agent":{"id":"org.prophet.surface-test","version":"1.0.0","name":"Essai de supervision","publisher_key":"ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="},"model":{"preferred":["local:modele-controle"]},"sandbox":{"min_level":0},"capabilities":{"max":{"fs.read":["~/docs/**"],"fs.write":["~/docs/**"],"tool.call":["fs.write"]}},"budget":{"default":{"tokens":2000,"wall_time":"60s","approvals":3}}},
            "requested":[{"res":"fs","act":"read","match":"~/docs/**"},{"res":"fs","act":"write","match":"~/docs/**"},{"res":"tool","act":"call","match":"fs.write"}],"scopes":["~/docs"],"availability":{"local_models":["modele-controle"]}}));
        chain
    }
    fn call(&self, method: &str, params: Value) -> Value {
        self.runtime
            .block_on(self.agents.call(method, params))
            .unwrap()
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
        assert!(
            bureau
                .ctx
                .read_response(egui::Id::new(format!("mission-{ID}")))
                .is_some()
        );
        assert!(!chain.dir.path().join("home/docs/note.txt").exists());
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
