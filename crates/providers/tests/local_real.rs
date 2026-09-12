//! Essai d'acceptation avec un moteur et des poids réels, lancé explicitement.
//! Aucun serveur simulé ici : les variables nomment le moteur à vérifier.

use std::sync::Arc;
use std::time::Duration;

use prophet_types::driver::{DriverEvent, RunStatus};
use providers::Driver;
use providers::conformance::sample_request;
use providers::local::{LocalModel, LocalTool};
use providers::native::{NativeDriver, ToolExecutor};
use serde_json::{Value, json};

struct FileTool {
    directory: Arc<tempfile::TempDir>,
}

impl ToolExecutor for FileTool {
    fn call(&self, tool: &str, arguments: &Value) -> (bool, Value) {
        if tool != "write_note" {
            return (false, json!({"code":"UnknownTool"}));
        }
        let Some(text) = arguments["text"].as_str() else {
            return (false, json!({"code":"InvalidArguments"}));
        };
        // Un seul fichier choisi par l'essai. Le modèle ne choisit aucun chemin hôte.
        match std::fs::write(self.directory.path().join("note.txt"), text) {
            Ok(()) => (true, json!({"saved":true})),
            Err(_) => (false, json!({"code":"WriteFailed"})),
        }
    }
}

#[test]
#[ignore = "needs_local_model: PROPHET_TEST_MODEL et PROPHET_TEST_ENDPOINT"]
fn un_modele_reel_choisit_un_outil_ecrit_et_termine() {
    let endpoint = std::env::var("PROPHET_TEST_ENDPOINT").expect("adresse du vrai moteur requise");
    let model = std::env::var("PROPHET_TEST_MODEL").expect("identifiant du vrai modèle requis");
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let nonce = format!("prophet-{}", std::process::id());
    let client = LocalModel::new(&endpoint, &model, Duration::from_secs(120)).unwrap()
        .with_max_tokens(1024).unwrap()
        .with_tools(vec![LocalTool { name:"write_note".into(),
            description:"Save the exact text into the note file. Call once, then answer Done.".into(),
            parameters:json!({"type":"object", "properties":{"text":{"type":"string"}}, "required":["text"], "additionalProperties":false}),
        }]).unwrap();
    let available = client
        .models()
        .expect("le moteur doit exposer son catalogue");
    assert!(
        available.contains(&model),
        "modèle absent du moteur : {available:?}"
    );
    let mut driver = NativeDriver::new(
        Box::new(client),
        Box::new(FileTool {
            directory: directory.clone(),
        }),
    );
    let mut request = sample_request("prophet-agent");
    request.intent = format!(
        "Use write_note to save exactly this text: {nonce}. When the tool reports saved, answer Done. /no_think"
    );
    request.limits.max_steps = 5;
    let run = driver.start(&request).unwrap().run;
    let mut events = Vec::new();
    for _ in 0..6 {
        let next = driver.poll(&run).unwrap();
        let terminal = next.iter().any(DriverEvent::is_terminal);
        events.extend(next);
        if terminal {
            break;
        }
    }
    assert!(
        events
            .iter()
            .any(|e| matches!(e, DriverEvent::ToolResult { ok: true, .. })),
        "{events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            DriverEvent::Done {
                status: RunStatus::Ok,
                ..
            }
        )),
        "{events:?}"
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("note.txt"))
            .unwrap()
            .trim(),
        nonce
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, DriverEvent::Usage {tokens_out:Some(n), ..} if *n > 0))
    );
    println!("moteur={model}; note vérifiée; événements={}", events.len());
}
