//! La suite de conformité appliquée à chaque pilote, plus les propriétés propres à la boucle
//! native (points de reprise, fork, rejeu).

use prophet_types::driver::{DriverEvent, RunStatus};
use providers::conformance::{all_passed, render, run_suite, sample_request};
use providers::mock::MockDriver;
use providers::native::{ModelTurn, NativeDriver, ScriptedModel, ToolExecutor, Usage};
use providers::official::{ClientProfile, OfficialDriver};
use providers::{Driver, DriverError};
use serde_json::{Value, json};

struct ExecuteurDeTest;

impl ToolExecutor for ExecuteurDeTest {
    fn call(&self, tool: &str, _arguments: &Value) -> (bool, Value) {
        match tool {
            "fs.list" => (true, json!({"entries": ["q3.csv"]})),
            "fs.read" => (true, json!({"content": "produit,montant\nA,100\n"})),
            "fs.write" => (true, json!({"staged": true})),
            autre => (false, json!({"code": "NotFound", "detail": autre})),
        }
    }
}

fn script_nominal() -> Vec<(ModelTurn, Usage)> {
    vec![
        (
            ModelTurn::ToolCall {
                tool: "fs.list".into(),
                arguments: json!({"path": "~/ventes"}),
            },
            Usage {
                tokens_in: 1200,
                tokens_out: 40,
            },
        ),
        (
            ModelTurn::ToolCall {
                tool: "fs.read".into(),
                arguments: json!({"path": "~/ventes/q3.csv"}),
            },
            Usage {
                tokens_in: 1400,
                tokens_out: 60,
            },
        ),
        (
            ModelTurn::ToolCall {
                tool: "fs.write".into(),
                arguments: json!({"path": "~/ventes/out/rapport.md", "content": "# Q3"}),
            },
            Usage {
                tokens_in: 1800,
                tokens_out: 120,
            },
        ),
        (
            ModelTurn::Final {
                text: "Rapport écrit dans ~/ventes/out/rapport.md.".into(),
            },
            Usage {
                tokens_in: 1900,
                tokens_out: 30,
            },
        ),
    ]
}

fn boucle_native() -> NativeDriver {
    NativeDriver::new(
        Box::new(ScriptedModel::new("local:qwen3-8b", script_nominal())),
        Box::new(ExecuteurDeTest),
    )
}

#[test]
fn le_simulacre_est_conforme() {
    let mut driver = MockDriver::new(vec![
        DriverEvent::Step {
            n: 1,
            summary: None,
        },
        DriverEvent::Done {
            status: RunStatus::Ok,
            reason: None,
            session_ref: "s".into(),
        },
    ]);
    let outcomes = run_suite(&mut driver);
    assert!(all_passed(&outcomes), "{}", render(&outcomes));
}

#[test]
fn la_boucle_native_est_conforme() {
    let mut driver = boucle_native();
    let outcomes = run_suite(&mut driver);
    assert!(all_passed(&outcomes), "{}", render(&outcomes));
}

#[test]
fn un_pilote_officiel_sans_session_echoue_proprement() {
    let dir = tempfile::tempdir().unwrap();
    let mut driver = OfficialDriver::new(ClientProfile::claude_code(), dir.path(), "u");
    // Sans session, le démarrage échoue, mais les capacités et l'erreur restent exploitables.
    let err = driver.start(&sample_request("claude-code")).unwrap_err();
    assert!(
        matches!(
            err,
            DriverError::NotLoggedIn(_) | DriverError::ClientMissing { .. }
        ),
        "{err}"
    );
    assert!(err.to_string().contains("claude"), "{err}");
}

#[test]
fn la_boucle_native_execute_la_tache_de_bout_en_bout() {
    let mut driver = boucle_native();
    let response = driver.start(&sample_request("prophet-agent")).unwrap();

    let mut tous = Vec::new();
    for _ in 0..20 {
        let events = driver.poll(&response.run).unwrap();
        let fini = events.iter().any(DriverEvent::is_terminal);
        tous.extend(events);
        if fini {
            break;
        }
    }

    let appels: Vec<&str> = tous
        .iter()
        .filter_map(|e| match e {
            DriverEvent::ToolCall { tool, .. } => Some(tool.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(appels, vec!["fs.list", "fs.read", "fs.write"]);

    let fin = tous.iter().rev().find(|e| e.is_terminal()).unwrap();
    assert!(matches!(
        fin,
        DriverEvent::Done {
            status: RunStatus::Ok,
            ..
        }
    ));

    let tokens: u64 = tous
        .iter()
        .filter_map(|e| match e {
            DriverEvent::Usage { tokens_out, .. } => *tokens_out,
            _ => None,
        })
        .sum();
    assert_eq!(tokens, 250);
}

#[test]
fn le_plafond_d_etapes_arrete_la_boucle() {
    let mut driver = NativeDriver::new(
        // Un modèle qui boucle sans jamais conclure : exactement ce que le plafond doit arrêter.
        Box::new(ScriptedModel::new(
            "local:test",
            std::iter::repeat_n(
                (
                    ModelTurn::ToolCall {
                        tool: "fs.list".into(),
                        arguments: json!({"path": "~/x"}),
                    },
                    Usage::default(),
                ),
                100,
            )
            .collect(),
        )),
        Box::new(ExecuteurDeTest),
    );
    let mut request = sample_request("prophet-agent");
    request.limits.max_steps = 3;
    let response = driver.start(&request).unwrap();

    let mut fin = None;
    for _ in 0..10 {
        for event in driver.poll(&response.run).unwrap() {
            if let DriverEvent::Done { status, reason, .. } = event {
                fin = Some((status, reason));
            }
        }
        if fin.is_some() {
            break;
        }
    }
    let (status, reason) = fin.expect("la boucle doit s'arrêter d'elle-même");
    assert_eq!(status, RunStatus::Failed);
    assert!(reason.unwrap().contains("3 étapes"));
}

#[test]
fn un_modele_injoignable_termine_proprement() {
    let mut driver = NativeDriver::new(
        Box::new(ScriptedModel::new("local:test", Vec::new())),
        Box::new(ExecuteurDeTest),
    );
    let response = driver.start(&sample_request("prophet-agent")).unwrap();
    let events = driver.poll(&response.run).unwrap();
    let fin = events.iter().find(|e| e.is_terminal()).unwrap();
    assert!(
        matches!(
            fin,
            DriverEvent::Done {
                status: RunStatus::Failed,
                ..
            }
        ),
        "{fin:?}"
    );
}

#[test]
fn un_echec_d_outil_ne_termine_pas_la_boucle() {
    let mut driver = NativeDriver::new(
        Box::new(ScriptedModel::new(
            "local:test",
            vec![
                (
                    ModelTurn::ToolCall {
                        tool: "outil.inexistant".into(),
                        arguments: json!({}),
                    },
                    Usage::default(),
                ),
                (
                    ModelTurn::Final {
                        text: "j'ai compris l'erreur et je m'arrête".into(),
                    },
                    Usage::default(),
                ),
            ],
        )),
        Box::new(ExecuteurDeTest),
    );
    let response = driver.start(&sample_request("prophet-agent")).unwrap();
    let premier = driver.poll(&response.run).unwrap();
    assert!(
        premier
            .iter()
            .any(|e| matches!(e, DriverEvent::ToolResult { ok: false, .. })),
        "l'échec doit être rendu au modèle, pas masqué"
    );
    assert!(!premier.iter().any(DriverEvent::is_terminal));

    let second = driver.poll(&response.run).unwrap();
    assert!(second.iter().any(DriverEvent::is_terminal));
}

#[test]
fn points_de_reprise_et_fork() {
    let mut driver = NativeDriver::new(
        Box::new(ScriptedModel::new(
            "local:test",
            std::iter::repeat_n(
                (
                    ModelTurn::ToolCall {
                        tool: "fs.list".into(),
                        arguments: json!({"path": "~/x"}),
                    },
                    Usage {
                        tokens_in: 10,
                        tokens_out: 5,
                    },
                ),
                20,
            )
            .collect(),
        )),
        Box::new(ExecuteurDeTest),
    )
    .checkpoint_every(2);

    let response = driver.start(&sample_request("prophet-agent")).unwrap();
    for _ in 0..4 {
        driver.poll(&response.run).unwrap();
    }
    let checkpoints = driver.checkpoints(&response.run);
    assert_eq!(
        checkpoints.len(),
        2,
        "un point de reprise toutes les deux étapes"
    );
    assert_eq!(checkpoints[1].step, 4);
    assert_eq!(checkpoints[1].tokens_out, 20);

    // Le fork rend une exécution indépendante repartant du même état.
    let fork = driver.fork(&checkpoints[0]);
    assert_ne!(fork, response.run);
    let events = driver.poll(&fork).unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, DriverEvent::Step { n: 3, .. })),
        "le fork reprend à l'étape suivante du point de reprise : {events:?}"
    );
}

#[test]
fn seule_la_boucle_native_annonce_le_fork() {
    let native = boucle_native();
    assert!(native.capabilities().supports.fork);
    assert!(native.capabilities().supports.checkpoint);

    let dir = tempfile::tempdir().unwrap();
    let officiel = OfficialDriver::new(ClientProfile::claude_code(), dir.path(), "u");
    assert!(
        !officiel.capabilities().supports.fork,
        "un client hébergé ne peut pas offrir ce qu'il n'expose pas"
    );
}

#[test]
fn le_simulacre_attend_une_decision_de_permission() {
    let mut driver = MockDriver::new(vec![
        DriverEvent::Step {
            n: 1,
            summary: None,
        },
        DriverEvent::PermissionRequest {
            id: "p1".into(),
            tool: "mail.send".into(),
            args_digest: "blake3:aa".into(),
            reason: Some("envoi à Marie".into()),
        },
        DriverEvent::Done {
            status: RunStatus::Ok,
            reason: None,
            session_ref: "s".into(),
        },
    ]);
    let response = driver.start(&sample_request("mock")).unwrap();

    let premier = driver.poll(&response.run).unwrap();
    assert!(
        premier
            .iter()
            .any(|e| matches!(e, DriverEvent::PermissionRequest { .. }))
    );
    // Tant que l'humain n'a pas tranché, la même demande revient : rien n'avance en douce.
    let second = driver.poll(&response.run).unwrap();
    assert!(
        second
            .iter()
            .any(|e| matches!(e, DriverEvent::PermissionRequest { .. }))
    );

    driver
        .resolve_permission(&response.run, "p1", true)
        .unwrap();
    let apres = driver.poll(&response.run).unwrap();
    assert!(apres.iter().any(DriverEvent::is_terminal));
    assert_eq!(driver.permissions(&response.run).get("p1"), Some(&true));
}
