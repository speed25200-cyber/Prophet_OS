//! Pilote scripté.
//!
//! Il ne simule pas un modèle : il rejoue une suite d'événements décidée par le test. C'est ce qui
//! permet de vérifier le runtime, les budgets et les approbations sans compte ni GPU, et de
//! reproduire exactement un scénario d'échec.

use std::collections::HashMap;

use prophet_types::driver::{
    AuthMode, DriverCapabilities, DriverEvent, DriverKind, RunStatus, StartRequest, StartResponse,
    Supports,
};

use crate::{Driver, DriverError};

/// Pilote rejouant un script d'événements.
#[derive(Debug, Default)]
pub struct MockDriver {
    script: Vec<DriverEvent>,
    runs: HashMap<String, RunState>,
    next_run: u64,
    logged_in: bool,
    fail_start: Option<String>,
}

#[derive(Debug, Default)]
struct RunState {
    position: usize,
    cancelled: bool,
    pending_permissions: HashMap<String, bool>,
}

impl MockDriver {
    /// Pilote connecté, rejouant le script donné.
    #[must_use]
    pub fn new(script: Vec<DriverEvent>) -> Self {
        Self {
            script,
            runs: HashMap::new(),
            next_run: 1,
            logged_in: true,
            fail_start: None,
        }
    }

    /// Pilote non connecté : tout démarrage échoue, comme un client sans session.
    #[must_use]
    pub fn logged_out() -> Self {
        Self {
            logged_in: false,
            ..Self::default()
        }
    }

    /// Pilote dont le démarrage échoue pour la raison donnée.
    #[must_use]
    pub fn failing(reason: impl Into<String>) -> Self {
        Self {
            logged_in: true,
            fail_start: Some(reason.into()),
            ..Self::default()
        }
    }

    /// Décisions de permission enregistrées pour une exécution.
    #[must_use]
    pub fn permissions(&self, run: &str) -> HashMap<String, bool> {
        self.runs
            .get(run)
            .map(|state| state.pending_permissions.clone())
            .unwrap_or_default()
    }
}

impl Driver for MockDriver {
    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            driver: "mock".into(),
            kind: DriverKind::Native,
            auth: AuthMode::None,
            supports: Supports {
                resume: true,
                checkpoint: true,
                fork: true,
                token_usage: true,
                quota_estimate: false,
                cost: false,
                streaming_events: true,
                permission_delegation: true,
            },
            logged_in: self.logged_in,
            models: vec!["script".into()],
        }
    }

    fn start(&mut self, _request: &StartRequest) -> Result<StartResponse, DriverError> {
        if !self.logged_in {
            return Err(DriverError::NotLoggedIn("mock".into()));
        }
        if let Some(reason) = &self.fail_start {
            return Err(DriverError::Io(reason.clone()));
        }
        let run = format!("run:mock-{}", self.next_run);
        self.next_run += 1;
        self.runs.insert(run.clone(), RunState::default());
        Ok(StartResponse {
            run: run.clone(),
            session_ref: format!("session-{run}"),
        })
    }

    fn poll(&mut self, run: &str) -> Result<Vec<DriverEvent>, DriverError> {
        let script = self.script.clone();
        let state = self
            .runs
            .get_mut(run)
            .ok_or_else(|| DriverError::UnknownRun(run.to_owned()))?;
        if state.cancelled {
            return Ok(vec![DriverEvent::Done {
                status: RunStatus::Cancelled,
                reason: Some("annulée".into()),
                session_ref: format!("session-{run}"),
            }]);
        }
        // Le script s'arrête à une demande de permission tant qu'elle n'est pas tranchée.
        let mut out = Vec::new();
        while state.position < script.len() {
            let event = script[state.position].clone();
            if let DriverEvent::PermissionRequest { id, .. } = &event
                && !state.pending_permissions.contains_key(id)
            {
                out.push(event);
                return Ok(out);
            }
            state.position += 1;
            let terminal = event.is_terminal();
            out.push(event);
            if terminal {
                break;
            }
        }
        Ok(out)
    }

    fn resolve_permission(
        &mut self,
        run: &str,
        id: &str,
        allowed: bool,
    ) -> Result<(), DriverError> {
        let state = self
            .runs
            .get_mut(run)
            .ok_or_else(|| DriverError::UnknownRun(run.to_owned()))?;
        state.pending_permissions.insert(id.to_owned(), allowed);
        Ok(())
    }

    fn cancel(&mut self, run: &str) -> Result<(), DriverError> {
        let state = self
            .runs
            .get_mut(run)
            .ok_or_else(|| DriverError::UnknownRun(run.to_owned()))?;
        state.cancelled = true;
        Ok(())
    }
}
