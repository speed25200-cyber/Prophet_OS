//! Pilotes d'agents : trois façons d'obtenir une boucle agentique, une seule interface.
//!
//! - **Client officiel d'éditeur** ([`official`]) : Claude Code, Codex CLI, Gemini CLI, lancés
//!   sans modification, connectés par l'abonnement grand public de l'utilisateur. Aucune clé
//!   d'API. Le pilote n'utilise que des mécanismes documentés de ces clients et ne lit jamais
//!   leurs fichiers d'identifiants.
//! - **Boucle native** ([`native`]) : la boucle agentique de Prophet OS, pour les modèles locaux.
//!   C'est elle qui donne les points de reprise, le fork et le rejeu exact.
//! - **Simulacre** ([`mock`]) : un pilote scripté, pour tester tout ce qui est au-dessus sans
//!   dépendre d'un compte ni d'un modèle.
//!
//! [`conformance`] vérifie que chaque pilote respecte le contrat, quelle que soit sa nature.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod conformance;
pub mod jev;
pub mod local;
pub mod mock;
pub mod native;
pub mod official;
pub mod selection;
pub mod stream;

use prophet_types::driver::{DriverCapabilities, DriverEvent, StartRequest, StartResponse};

/// Erreur d'un pilote.
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    /// Le pilote n'est pas connecté.
    #[error("{0} n'est pas connecté : lancez `prophet provider login {0}`")]
    NotLoggedIn(String),
    /// Le client officiel est introuvable.
    #[error("client {client} introuvable ; installez-le ou choisissez un autre pilote")]
    ClientMissing {
        /// Nom du client.
        client: String,
    },
    /// Exécution inconnue.
    #[error("exécution inconnue : {0}")]
    UnknownRun(String),
    /// Erreur d'entrée-sortie.
    #[error("erreur d'entrée-sortie : {0}")]
    Io(String),
    /// Le modèle a produit une réponse inexploitable.
    #[error("réponse du modèle inexploitable : {0}")]
    BadModelOutput(String),
    /// Budget épuisé.
    #[error("budget épuisé : {0}")]
    BudgetExceeded(String),
    /// Un décideur rapide rend la main à un modèle génératif : ce n'est pas une panne, c'est la
    /// limite qu'il s'est fixée. Une cascade reprend la même histoire avec le modèle suivant.
    #[error("main rendue au modèle génératif : {0}")]
    HandOver(String),
}

/// Contrat commun à tous les pilotes.
///
/// Volontairement synchrone dans sa forme de test : un pilote produit une suite d'événements, que
/// l'appelant consomme. Cela rend la conformité vérifiable sans horloge ni réseau.
pub trait Driver: Send + Sync {
    /// Ce que le pilote sait faire.
    fn capabilities(&self) -> DriverCapabilities;

    /// Démarre une exécution.
    ///
    /// # Errors
    /// Pilote non connecté, client absent, ou erreur de lancement.
    fn start(&mut self, request: &StartRequest) -> Result<StartResponse, DriverError>;

    /// Récupère les événements disponibles pour une exécution.
    ///
    /// # Errors
    /// Exécution inconnue.
    fn poll(&mut self, run: &str) -> Result<Vec<DriverEvent>, DriverError>;

    /// Répond à une demande de permission.
    ///
    /// # Errors
    /// Exécution ou demande inconnue.
    fn resolve_permission(&mut self, run: &str, id: &str, allowed: bool)
    -> Result<(), DriverError>;

    /// Annule une exécution.
    ///
    /// # Errors
    /// Exécution inconnue.
    fn cancel(&mut self, run: &str) -> Result<(), DriverError>;
}
