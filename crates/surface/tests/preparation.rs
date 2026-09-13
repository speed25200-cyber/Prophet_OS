//! Une réponse de création perdue ne doit pas provoquer une seconde création.
use std::io::{BufRead as _, Write as _};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use surface::preparation::Preparation;

fn wait(preparation: &mut Preparation, condition: impl Fn(&Preparation) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        preparation.update();
        if condition(preparation) {
            return;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn une_reponse_perdue_se_recupere_par_lecture_sans_recreer_la_mission() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("agent.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let server = std::thread::spawn(move || {
        let mut plan = Value::Null;
        let mut methods = Vec::new();
        for turn in 0..3 {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut stream = std::io::BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            methods.push(request["method"].as_str().unwrap().to_owned());
            let result = match turn {
                0 => {
                    json!({"profiles":[{"id":"docs","name":"Documents","description":"Contexte local","models":["local"],"scopes":["~/docs"],"grants":[],"limits":agentd::Limits::default()}],"model_error":null})
                }
                1 => {
                    plan = json!({"task":request["params"]["id"],"intent":request["params"]["intent"],"choice":{"reference":"local:local","reason":"modèle présent"},"sandbox_level":0,"grants":[],"scopes":["~/docs"],"limits":agentd::Limits::default()});
                    continue; // La mission est conservée, mais la connexion est coupée avant réponse.
                }
                _ => {
                    assert_eq!(request["params"]["id"], plan["task"]);
                    let mut task = agentd::Task::new(
                        plan["task"].as_str().unwrap(),
                        plan["intent"].as_str().unwrap(),
                        "org.prophet.test",
                        "uid:1000",
                        agentd::Budget::new(Default::default()),
                        time::OffsetDateTime::now_utc(),
                    );
                    task.state = agentd::State::Planned;
                    json!({"task":task,"plan":plan,"result":null,"can_start":true,"can_cancel":true,"start_reason":null})
                }
            };
            writeln!(
                stream.get_mut(),
                "{}",
                json!({"jsonrpc":"2.0","id":request["id"],"result":result})
            )
            .unwrap();
        }
        methods
    });
    let ctx = egui::Context::default();
    let mut preparation = Preparation::connect(socket);
    preparation.discover(&ctx);
    wait(&mut preparation, |p| p.options().is_some());
    preparation.intent = "Une note à préparer".into();
    preparation.submit(&ctx).unwrap();
    assert!(preparation.submit(&ctx).is_err());
    wait(&mut preparation, |p| !p.pending());
    assert!(preparation.error().is_some());
    let id = preparation.attempted_id().unwrap().to_owned();
    assert!(preparation.submit(&ctx).is_err());
    preparation.recover(&ctx);
    wait(&mut preparation, |p| !p.pending());
    let plan = preparation.take_prepared().unwrap();
    assert_eq!(plan.task, id);
    assert_eq!(plan.intent, "Une note à préparer");
    assert_eq!(
        server.join().unwrap(),
        vec!["task.options", "task.prepare", "task.inspect"]
    );
}
