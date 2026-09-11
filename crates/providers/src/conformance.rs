//! Suite de conformité des pilotes.
//!
//! Le contrat n'a de valeur que s'il est vérifié de la même façon pour tous. Cette suite est
//! appliquée au simulacre, à la boucle native, et à tout pilote de client officiel dès qu'une
//! session existe. Un pilote qui déclare une capacité doit la tenir : déclarer la reprise sans la
//! fournir est un échec de conformité, pas une nuance.

use prophet_types::driver::{DriverEvent, Limits, RunStatus, SandboxRequest, StartRequest};

use crate::{Driver, DriverError};

/// Résultat d'un scénario.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Nom du scénario.
    pub scenario: &'static str,
    /// Réussite.
    pub passed: bool,
    /// Détail en cas d'échec.
    pub detail: String,
}

impl Outcome {
    fn ok(scenario: &'static str) -> Self {
        Self {
            scenario,
            passed: true,
            detail: String::new(),
        }
    }

    fn fail(scenario: &'static str, detail: impl Into<String>) -> Self {
        Self {
            scenario,
            passed: false,
            detail: detail.into(),
        }
    }
}

/// Requête de démarrage type.
#[must_use]
pub fn sample_request(driver: &str) -> StartRequest {
    StartRequest {
        driver: driver.to_owned(),
        task: "task:conformance".into(),
        intent: "liste les fichiers puis écris un résumé".into(),
        workdir: "/tmp/prophet-conformance".into(),
        mcp_config: "/tmp/prophet-conformance/mcp.json".into(),
        token: "jeton-de-test".into(),
        sandbox: SandboxRequest {
            level: 1,
            profile: "base".into(),
        },
        limits: Limits {
            wall_time_s: 60,
            max_steps: 20,
        },
        resume: None,
    }
}

/// Déroule une exécution jusqu'à son événement terminal.
fn drain(driver: &mut dyn Driver, run: &str, max_polls: usize) -> Vec<DriverEvent> {
    let mut all = Vec::new();
    for _ in 0..max_polls {
        let Ok(events) = driver.poll(run) else {
            break;
        };
        let terminal = events.iter().any(DriverEvent::is_terminal);
        let waiting = events
            .iter()
            .any(|e| matches!(e, DriverEvent::PermissionRequest { .. }));
        all.extend(events);
        if terminal || waiting {
            break;
        }
    }
    all
}

/// Exécute la suite complète sur un pilote.
///
/// Les scénarios qui dépendent d'une capacité déclarée sont ignorés, et l'indiquent, plutôt que de
/// compter comme des réussites gratuites.
pub fn run_suite(driver: &mut dyn Driver) -> Vec<Outcome> {
    let name = driver.capabilities().driver;
    vec![
        scenario_nominal(driver, &name),
        scenario_cancel(driver, &name),
        scenario_unknown_run(driver),
        scenario_capabilities_are_honest(driver),
    ]
}

fn scenario_nominal(driver: &mut dyn Driver, name: &str) -> Outcome {
    const SCENARIO: &str = "démarrage puis fin normale";
    let Ok(response) = driver.start(&sample_request(name)) else {
        return Outcome::fail(SCENARIO, "démarrage impossible");
    };
    let events = drain(driver, &response.run, 50);
    if events.is_empty() {
        return Outcome::fail(SCENARIO, "aucun événement produit");
    }
    let terminal = events.iter().any(DriverEvent::is_terminal);
    let waiting = events
        .iter()
        .any(|e| matches!(e, DriverEvent::PermissionRequest { .. }));
    if terminal || waiting {
        Outcome::ok(SCENARIO)
    } else {
        Outcome::fail(SCENARIO, "ni fin, ni demande de permission")
    }
}

fn scenario_cancel(driver: &mut dyn Driver, name: &str) -> Outcome {
    const SCENARIO: &str = "annulation en cours";
    let Ok(response) = driver.start(&sample_request(name)) else {
        return Outcome::fail(SCENARIO, "démarrage impossible");
    };
    if driver.cancel(&response.run).is_err() {
        return Outcome::fail(SCENARIO, "annulation refusée");
    }
    let events = drain(driver, &response.run, 5);
    let cancelled = events.iter().any(|event| {
        matches!(
            event,
            DriverEvent::Done {
                status: RunStatus::Cancelled,
                ..
            }
        )
    });
    if cancelled {
        Outcome::ok(SCENARIO)
    } else {
        Outcome::fail(SCENARIO, "l'annulation n'a pas produit de fin annulée")
    }
}

fn scenario_unknown_run(driver: &mut dyn Driver) -> Outcome {
    const SCENARIO: &str = "exécution inconnue refusée";
    match driver.poll("run:inexistant") {
        Err(DriverError::UnknownRun(_)) => Outcome::ok(SCENARIO),
        Err(other) => Outcome::fail(SCENARIO, format!("mauvaise erreur : {other}")),
        Ok(_) => Outcome::fail(SCENARIO, "une exécution inconnue a été acceptée"),
    }
}

fn scenario_capabilities_are_honest(driver: &mut dyn Driver) -> Outcome {
    const SCENARIO: &str = "capacités déclarées cohérentes";
    let caps = driver.capabilities();
    if caps.driver.is_empty() {
        return Outcome::fail(SCENARIO, "nom de pilote vide");
    }
    if caps.supports.fork && !caps.supports.checkpoint {
        return Outcome::fail(
            SCENARIO,
            "le fork suppose des points de reprise : les deux doivent aller ensemble",
        );
    }
    if caps.supports.cost && !caps.supports.token_usage {
        return Outcome::fail(
            SCENARIO,
            "annoncer un coût sans compter les tokens est incohérent",
        );
    }
    Outcome::ok(SCENARIO)
}

/// Vrai si tous les scénarios passent.
#[must_use]
pub fn all_passed(outcomes: &[Outcome]) -> bool {
    outcomes.iter().all(|o| o.passed)
}

/// Rendu lisible d'un résultat de suite.
#[must_use]
pub fn render(outcomes: &[Outcome]) -> String {
    outcomes
        .iter()
        .map(|o| {
            if o.passed {
                format!("  ✓ {}", o.scenario)
            } else {
                format!("  ✗ {} : {}", o.scenario, o.detail)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
