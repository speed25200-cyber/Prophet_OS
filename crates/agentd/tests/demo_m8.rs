//! Démonstration du jalon M8.
//!
//! **La même tâche, les mêmes outils, les mêmes permissions, sur trois pilotes différents, sans
//! changer une ligne et sans clé d'API.**
//!
//! C'est la propriété qui justifie tout le reste : l'agent n'est pas lié à un fournisseur, et
//! l'utilisateur n'a pas à choisir entre la commodité d'un abonnement et le contrôle de sa machine.

use agentd::runtime::PlanRequest;
use agentd::{Runtime, State};
use capd::Broker;
use prophet_types::cap::{Act, Grant, Res};
use prophet_types::driver::{DriverEvent, RunStatus};
use prophet_types::ledger::EventKind;
use prophet_types::manifest::Manifest;
use providers::Driver;
use providers::mock::MockDriver;
use providers::native::{ModelTurn, NativeDriver, ScriptedModel, ToolExecutor, Usage};
use providers::official::{ClientProfile, OfficialDriver};
use providers::selection::Availability;
use serde_json::{Value, json};
use time::OffsetDateTime;

const MANIFESTE: &str = r#"
[agent]
id = "org.exemple.analyste-ventes"
version = "1.0.0"
name = "Analyste ventes"
publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="

[model]
preferred = ["local:qwen3-8b", "driver:claude-code", "driver:codex"]
privacy = "local-preferred"

[capabilities.max]
"fs.read" = ["~/ventes/**"]
"fs.write" = ["~/ventes/out/**"]
"fs.list" = ["~/ventes/**"]
"tool.call" = ["fs.*", "task.*"]

[budget.default]
tokens = 50000
wall_time = "10m"
approvals = 3
"#;

const INTENTION: &str =
    "lis les ventes du trimestre et écris un résumé dans ~/ventes/out/resume.md";

fn now() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_789_000_000).unwrap()
}

/// Les capacités demandées, identiques quel que soit le pilote.
fn grants_demandes() -> Vec<Grant> {
    vec![
        Grant::new(Res::Fs, Act::Read, "~/ventes/**"),
        Grant::new(Res::Fs, Act::Write, "~/ventes/out/**"),
        Grant::new(Res::Fs, Act::List, "~/ventes/**"),
        Grant::new(Res::Tool, Act::Call, "fs.*"),
        Grant::new(Res::Tool, Act::Call, "task.*"),
    ]
}

struct Outils;

impl ToolExecutor for Outils {
    fn call(&self, tool: &str, _arguments: &Value) -> (bool, Value) {
        match tool {
            "fs.list" => (true, json!({"entries": ["q3.csv"]})),
            "fs.read" => (true, json!({"content": "produit,montant\nA,100\nB,250\n"})),
            "fs.write" => (true, json!({"staged": true, "bytes": 42})),
            autre => (false, json!({"code": "NotFound", "detail": autre})),
        }
    }
}

/// Le même déroulé, exprimé une fois, joué par chaque pilote.
fn deroule_attendu() -> Vec<(&'static str, Value)> {
    vec![
        ("fs.list", json!({"path": "~/ventes"})),
        ("fs.read", json!({"path": "~/ventes/q3.csv"})),
        (
            "fs.write",
            json!({"path": "~/ventes/out/resume.md", "content": "# Ventes Q3\n\nTotal : 350.\n"}),
        ),
    ]
}

fn pilote_natif() -> NativeDriver {
    let mut turns: Vec<(ModelTurn, Usage)> = deroule_attendu()
        .into_iter()
        .map(|(tool, arguments)| {
            (
                ModelTurn::ToolCall {
                    tool: tool.to_owned(),
                    arguments,
                },
                Usage {
                    tokens_in: 1000,
                    tokens_out: 50,
                },
            )
        })
        .collect();
    turns.push((
        ModelTurn::Final {
            text: "Résumé écrit dans ~/ventes/out/resume.md.".into(),
        },
        Usage {
            tokens_in: 1200,
            tokens_out: 30,
        },
    ));
    NativeDriver::new(
        Box::new(ScriptedModel::new("local:qwen3-8b", turns)),
        Box::new(Outils),
    )
}

/// Un pilote de client officiel est représenté ici par le simulacre, qui rejoue exactement les
/// événements qu'un client produit dans son mode non interactif. Le pilote réel est couvert par
/// ses propres tests ; ce qui est démontré ici, c'est que le runtime ne fait aucune différence.
fn pilote_abonnement(nom: &str) -> MockDriver {
    let mut events = vec![DriverEvent::Step {
        n: 1,
        summary: None,
    }];
    for (index, (tool, arguments)) in deroule_attendu().into_iter().enumerate() {
        events.push(DriverEvent::Step {
            n: u32::try_from(index).unwrap_or(0) + 1,
            summary: None,
        });
        events.push(DriverEvent::ToolCall {
            tool: tool.to_owned(),
            args_digest: format!(
                "blake3:{}",
                blake3::hash(arguments.to_string().as_bytes()).to_hex()
            ),
            via: prophet_types::driver::ToolVia::Mcp,
        });
        events.push(DriverEvent::ToolResult {
            tool: tool.to_owned(),
            ok: true,
            error: None,
        });
        events.push(DriverEvent::Usage {
            tokens_in: Some(1000),
            tokens_out: Some(50),
            cost_eur: None,
            quota_pct: Some(f64::from(u32::try_from(index).unwrap_or(0) + 1) * 4.0),
        });
    }
    events.push(DriverEvent::Text {
        role: "assistant".into(),
        text: "Résumé écrit dans ~/ventes/out/resume.md.".into(),
    });
    events.push(DriverEvent::Done {
        status: RunStatus::Ok,
        reason: None,
        session_ref: format!("session-{nom}"),
    });
    MockDriver::new(events)
}

struct Monde {
    _dir: tempfile::TempDir,
    runtime: Runtime,
    manifest: Manifest,
}

fn monde() -> Monde {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().to_path_buf();
    std::fs::create_dir_all(home.join("ventes")).unwrap();
    let broker = Broker::new(
        ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng),
        "capd@demo",
        home.display().to_string(),
    )
    .unwrap();
    Monde {
        _dir: dir,
        runtime: Runtime::new(broker, home),
        manifest: Manifest::from_toml(MANIFESTE).unwrap(),
    }
}

fn disponibilite(local: bool, connectes: &[&str]) -> Availability {
    Availability {
        local_models: if local {
            vec!["qwen3-8b".into()]
        } else {
            Vec::new()
        },
        logged_in_drivers: connectes.iter().map(|d| (*d).to_owned()).collect(),
        ..Availability::default()
    }
}

/// Exécute la tâche de référence avec un pilote donné et rend le rapport.
fn executer(
    monde: &mut Monde,
    id: &str,
    driver: &mut dyn Driver,
    availability: &Availability,
) -> agentd::runtime::Report {
    let plan = monde
        .runtime
        .plan(
            &PlanRequest {
                id,
                intent: INTENTION,
                manifest: &monde.manifest,
                user: "u",
                requested: &grants_demandes(),
                scopes: &["~/ventes"],
                availability,
            },
            now(),
        )
        .unwrap();
    // Le plan est le même quel que soit le pilote, sauf le pilote lui-même.
    assert_eq!(plan.grants.len(), 5);
    assert_eq!(plan.sandbox_level, 1);
    monde.runtime.run(id, driver, now()).unwrap()
}

#[test]
fn la_meme_tache_sur_trois_pilotes() {
    let mut rapports = Vec::new();

    // 1. Modèle local, par la boucle native. Aucun compte, aucune clé.
    {
        let mut m = monde();
        let mut driver = pilote_natif();
        let rapport = executer(&mut m, "task:local", &mut driver, &disponibilite(true, &[]));
        assert_eq!(
            m.runtime.task("task:local").unwrap().driver.as_deref(),
            Some("local:qwen3-8b")
        );
        rapports.push(("local:qwen3-8b", rapport));
    }

    // 2. Abonnement Claude, par le client officiel. Aucune clé d'API.
    {
        let mut m = monde();
        let mut driver = pilote_abonnement("claude-code");
        let rapport = executer(
            &mut m,
            "task:claude",
            &mut driver,
            &disponibilite(false, &["claude-code"]),
        );
        assert_eq!(
            m.runtime.task("task:claude").unwrap().driver.as_deref(),
            Some("driver:claude-code")
        );
        rapports.push(("driver:claude-code", rapport));
    }

    // 3. Abonnement ChatGPT, par Codex CLI. Aucune clé d'API.
    {
        let mut m = monde();
        let mut driver = pilote_abonnement("codex");
        let rapport = executer(
            &mut m,
            "task:codex",
            &mut driver,
            &disponibilite(false, &["codex"]),
        );
        assert_eq!(
            m.runtime.task("task:codex").unwrap().driver.as_deref(),
            Some("driver:codex")
        );
        rapports.push(("driver:codex", rapport));
    }

    // Les trois aboutissent, avec les mêmes appels d'outils.
    for (pilote, rapport) in &rapports {
        assert_eq!(rapport.state, State::Done, "{pilote} n'a pas abouti");
        assert_eq!(
            rapport.tool_calls, 3,
            "{pilote} : appels d'outils différents"
        );
        assert_eq!(rapport.approvals, 0, "{pilote} : approbation inattendue");
        assert!(
            rapport.final_text.as_ref().unwrap().contains("resume.md"),
            "{pilote} : réponse finale inattendue"
        );
    }

    // Tableau de la démonstration, tel que `just demo M8` l'imprime.
    eprintln!(
        "\n{:<22} {:>7} {:>7} {:>9} {:>8}",
        "pilote", "étapes", "outils", "tokens", "état"
    );
    for (pilote, rapport) in &rapports {
        eprintln!(
            "{pilote:<22} {:>7} {:>7} {:>9} {:>8?}",
            rapport.steps, rapport.tool_calls, rapport.tokens, rapport.state
        );
    }
}

#[test]
fn le_choix_du_pilote_est_explicable() {
    let mut m = monde();
    let plan = m
        .runtime
        .plan(
            &PlanRequest {
                id: "task:01",
                intent: INTENTION,
                manifest: &m.manifest,
                user: "u",
                requested: &grants_demandes(),
                scopes: &["~/ventes"],
                availability: &disponibilite(false, &["claude-code"]),
            },
            now(),
        )
        .unwrap();
    assert_eq!(plan.choice.reference, "driver:claude-code");
    assert!(
        plan.choice.reason.contains("abonnement"),
        "{}",
        plan.choice.reason
    );

    let rendu = plan.render();
    assert!(rendu.contains("Pilote"), "{rendu}");
    assert!(rendu.contains("Capacités"), "{rendu}");
    assert!(rendu.contains("fs.read"), "{rendu}");
    assert!(rendu.contains("50000 tokens"), "{rendu}");
}

#[test]
fn sans_aucun_pilote_la_tache_ne_demarre_pas() {
    let mut m = monde();
    let err = m
        .runtime
        .plan(
            &PlanRequest {
                id: "task:01",
                intent: INTENTION,
                manifest: &m.manifest,
                user: "u",
                requested: &grants_demandes(),
                scopes: &["~/ventes"],
                availability: &Availability::default(),
            },
            now(),
        )
        .unwrap_err();
    assert!(err.to_string().contains("aucun pilote"), "{err}");
}

#[test]
fn le_jeton_est_borne_par_le_manifeste_quel_que_soit_le_pilote() {
    let mut m = monde();
    // L'agent demande tout le répertoire personnel ; le manifeste ne couvre que ~/ventes.
    let trop_large = vec![
        Grant::new(Res::Fs, Act::Read, "~/**"),
        Grant::new(Res::Fs, Act::Read, "~/ventes/**"),
    ];
    m.runtime
        .plan(
            &PlanRequest {
                id: "task:01",
                intent: INTENTION,
                manifest: &m.manifest,
                user: "u",
                requested: &trop_large,
                scopes: &["~/ventes"],
                availability: &disponibilite(true, &[]),
            },
            now(),
        )
        .unwrap();
    let token = m.runtime.token("task:01").unwrap();
    assert_eq!(token.grants.len(), 1);
    assert_eq!(token.grants[0].pattern, "~/ventes/**");
}

#[test]
fn tout_est_journalise_du_debut_a_la_fin() {
    let mut m = monde();
    let mut driver = pilote_natif();
    executer(&mut m, "task:01", &mut driver, &disponibilite(true, &[]));

    let kinds: Vec<EventKind> = m.runtime.journal().iter().map(|e| e.kind).collect();
    for attendu in [
        EventKind::TaskCreated,
        EventKind::TaskPlanned,
        EventKind::ProviderStarted,
        EventKind::ToolCall,
        EventKind::ToolResult,
        EventKind::TaskDone,
        EventKind::ProviderStopped,
    ] {
        assert!(kinds.contains(&attendu), "{attendu:?} absent du journal");
    }

    // L'intention et les arguments n'apparaissent qu'en empreinte.
    let rendu = serde_json::to_string(
        &m.runtime
            .journal()
            .iter()
            .map(|e| e.payload.clone())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert!(
        !rendu.contains("resume.md"),
        "le journal ne doit porter que des empreintes"
    );
    assert!(rendu.contains("blake3:"));
}

#[test]
fn un_budget_trop_court_arrete_la_tache_proprement() {
    let mut m = monde();
    let court = MANIFESTE.replace("tokens = 50000", "tokens = 60");
    m.manifest = Manifest::from_toml(&court).unwrap();
    let mut driver = pilote_natif();
    let rapport = executer(&mut m, "task:01", &mut driver, &disponibilite(true, &[]));
    assert_eq!(rapport.state, State::Failed);
    assert!(
        rapport.reason.unwrap().contains("tokens"),
        "l'arrêt doit nommer la dimension épuisée"
    );
}

#[test]
fn un_pilote_annule_produit_une_tache_annulee() {
    // Un client arrêté de l'extérieur rend une fin annulée : le runtime doit la refléter telle
    // quelle, sans la confondre avec un échec.
    let mut m = monde();
    let mut driver = MockDriver::new(vec![
        DriverEvent::Step {
            n: 1,
            summary: None,
        },
        DriverEvent::Done {
            status: RunStatus::Cancelled,
            reason: Some("arrêt demandé".into()),
            session_ref: "s".into(),
        },
    ]);
    let rapport = executer(&mut m, "task:01", &mut driver, &disponibilite(true, &[]));
    assert_eq!(rapport.state, State::Cancelled);
}

#[test]
fn l_humain_peut_annuler_une_tache_planifiee() {
    let mut m = monde();
    let mut driver = pilote_natif();
    m.runtime
        .plan(
            &PlanRequest {
                id: "task:01",
                intent: INTENTION,
                manifest: &m.manifest,
                user: "u",
                requested: &grants_demandes(),
                scopes: &["~/ventes"],
                availability: &disponibilite(true, &[]),
            },
            now(),
        )
        .unwrap();
    m.runtime
        .cancel("task:01", None, &mut driver, now())
        .unwrap();
    assert_eq!(m.runtime.task("task:01").unwrap().state, State::Cancelled);

    // Une tâche annulée ne se relance pas.
    assert!(m.runtime.run("task:01", &mut driver, now()).is_err());
}

#[test]
fn l_annulation_aboutit_meme_si_le_pilote_ne_repond_pas() {
    let mut m = monde();
    // Un pilote dont l'annulation échoue : l'humain ne doit pas rester prisonnier de sa panne.
    let mut driver = MockDriver::failing("pilote muet");
    m.runtime
        .plan(
            &PlanRequest {
                id: "task:01",
                intent: INTENTION,
                manifest: &m.manifest,
                user: "u",
                requested: &grants_demandes(),
                scopes: &["~/ventes"],
                availability: &disponibilite(true, &[]),
            },
            now(),
        )
        .unwrap();
    m.runtime
        .cancel("task:01", Some("run:inexistant"), &mut driver, now())
        .unwrap();
    assert_eq!(m.runtime.task("task:01").unwrap().state, State::Cancelled);
}

#[test]
fn un_client_officiel_reel_sans_session_est_signale_clairement() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = monde();
    let mut driver = OfficialDriver::new(ClientProfile::claude_code(), dir.path(), "u");
    m.runtime
        .plan(
            &PlanRequest {
                id: "task:01",
                intent: INTENTION,
                manifest: &m.manifest,
                user: "u",
                requested: &grants_demandes(),
                scopes: &["~/ventes"],
                availability: &disponibilite(false, &["claude-code"]),
            },
            now(),
        )
        .unwrap();
    let err = m.runtime.run("task:01", &mut driver, now()).unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("connecté") || message.contains("introuvable"),
        "le message doit dire quoi faire : {message}"
    );
}
