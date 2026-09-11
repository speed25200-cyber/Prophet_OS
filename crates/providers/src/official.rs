//! Pilotes des clients officiels d'éditeurs, connectés par abonnement.
//!
//! Règles non négociables, appliquées ici et vérifiées par les tests :
//!
//! 1. Le client tourne **sans modification**, par ses mécanismes documentés : mode non
//!    interactif, sortie structurée, configuration MCP, délégation des permissions.
//! 2. L'OS ne lit, ne copie, ni ne réutilise **jamais** les fichiers d'identifiants du client.
//!    Ils vivent dans un répertoire privé monté dans sa sandbox, et rien d'autre n'y accède.
//! 3. Aucune clé d'API n'est requise : la session de l'abonnement suffit, et c'est le client qui
//!    la détient.
//! 4. Aucune automatisation des applications grand public par capture d'écran ou clics.
//!
//! Les noms exacts des options varient d'une version de client à l'autre. Ils sont rassemblés ici,
//! dans [`ClientProfile`], pour qu'une mise à jour d'un éditeur se traite en un seul endroit.

use std::path::{Path, PathBuf};

use prophet_types::driver::{
    AuthMode, DriverCapabilities, DriverEvent, DriverKind, StartRequest, StartResponse, Supports,
};
use serde::{Deserialize, Serialize};

use crate::{Driver, DriverError};

/// Ce qu'il faut savoir d'un client officiel pour le piloter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientProfile {
    /// Nom du pilote.
    pub driver: String,
    /// Exécutable à lancer.
    pub program: String,
    /// Arguments du mode non interactif, `{intent}` étant remplacé par l'intention.
    pub headless_args: Vec<String>,
    /// Argument désignant le fichier de configuration MCP, s'il en prend un.
    pub mcp_config_arg: Option<String>,
    /// Variable d'environnement désignant le répertoire de configuration privé du client.
    pub config_home_env: String,
    /// Le client sait-il déléguer ses demandes de permission à un programme externe ?
    pub permission_delegation: bool,
    /// Le client sait-il reprendre une session ?
    pub resume_arg: Option<String>,
}

impl ClientProfile {
    /// Profil de Claude Code, connecté par un abonnement Claude.
    #[must_use]
    pub fn claude_code() -> Self {
        Self {
            driver: "claude-code".into(),
            program: "claude".into(),
            headless_args: vec![
                "-p".into(),
                "{intent}".into(),
                "--output-format".into(),
                "stream-json".into(),
            ],
            mcp_config_arg: Some("--mcp-config".into()),
            config_home_env: "CLAUDE_CONFIG_DIR".into(),
            permission_delegation: true,
            resume_arg: Some("--resume".into()),
        }
    }

    /// Profil de Codex CLI, connecté par un compte ChatGPT.
    #[must_use]
    pub fn codex() -> Self {
        Self {
            driver: "codex".into(),
            program: "codex".into(),
            headless_args: vec!["exec".into(), "{intent}".into(), "--json".into()],
            mcp_config_arg: None,
            config_home_env: "CODEX_HOME".into(),
            permission_delegation: true,
            resume_arg: Some("resume".into()),
        }
    }

    /// Profil de Gemini CLI.
    #[must_use]
    pub fn gemini() -> Self {
        Self {
            driver: "gemini".into(),
            program: "gemini".into(),
            headless_args: vec!["-p".into(), "{intent}".into()],
            mcp_config_arg: None,
            config_home_env: "GEMINI_CONFIG_DIR".into(),
            permission_delegation: false,
            resume_arg: None,
        }
    }

    /// Tous les profils connus.
    #[must_use]
    pub fn all() -> Vec<Self> {
        vec![Self::claude_code(), Self::codex(), Self::gemini()]
    }

    /// Construit la ligne de commande du mode non interactif.
    #[must_use]
    pub fn command_line(
        &self,
        intent: &str,
        mcp_config: &str,
        resume: Option<&str>,
    ) -> Vec<String> {
        let mut args: Vec<String> = self
            .headless_args
            .iter()
            .map(|arg| arg.replace("{intent}", intent))
            .collect();
        if let Some(flag) = &self.mcp_config_arg {
            args.push(flag.clone());
            args.push(mcp_config.to_owned());
        }
        if let (Some(flag), Some(session)) = (&self.resume_arg, resume) {
            args.push(flag.clone());
            args.push(session.to_owned());
        }
        args
    }
}

/// Répertoire de configuration privé d'un client, pour un utilisateur.
///
/// Il contient la session de l'abonnement. Le pilote le **monte** dans la sandbox du client et
/// n'en lit jamais le contenu.
#[must_use]
pub fn private_config_dir(root: &Path, driver: &str, user: &str) -> PathBuf {
    root.join("providers").join(driver).join(user)
}

/// Pilote d'un client officiel.
#[derive(Debug)]
pub struct OfficialDriver {
    profile: ClientProfile,
    config_root: PathBuf,
    user: String,
}

impl OfficialDriver {
    /// Construit un pilote pour un profil donné.
    #[must_use]
    pub fn new(
        profile: ClientProfile,
        config_root: impl Into<PathBuf>,
        user: impl Into<String>,
    ) -> Self {
        Self {
            profile,
            config_root: config_root.into(),
            user: user.into(),
        }
    }

    /// Profil piloté.
    #[must_use]
    pub const fn profile(&self) -> &ClientProfile {
        &self.profile
    }

    /// Répertoire privé du client.
    #[must_use]
    pub fn config_dir(&self) -> PathBuf {
        private_config_dir(&self.config_root, &self.profile.driver, &self.user)
    }

    /// Vrai si une session d'abonnement existe.
    ///
    /// La présence du répertoire suffit : le pilote n'ouvre aucun fichier pour le vérifier, car
    /// cela reviendrait à lire des identifiants.
    #[must_use]
    pub fn logged_in(&self) -> bool {
        let dir = self.config_dir();
        dir.exists()
            && std::fs::read_dir(&dir)
                .map(|mut entries| entries.next().is_some())
                .unwrap_or(false)
    }

    /// Vrai si l'exécutable du client est présent.
    #[must_use]
    pub fn client_available(&self) -> bool {
        which(&self.profile.program).is_some()
    }

    /// Variables d'environnement transmises au client. Rien d'autre ne passe.
    ///
    /// En particulier, aucune variable portant une clé d'API n'est propagée : ce pilote ne
    /// fonctionne que par abonnement, et laisser passer une clé brouillerait cette garantie.
    #[must_use]
    pub fn environment(&self, workdir: &str, mcp_config: &str) -> Vec<(String, String)> {
        vec![
            (
                self.profile.config_home_env.clone(),
                self.config_dir().display().to_string(),
            ),
            ("HOME".to_owned(), workdir.to_owned()),
            ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
            ("PROPHET_MCP_CONFIG".to_owned(), mcp_config.to_owned()),
            (
                "PROPHET_PERMISSION_HELPER".to_owned(),
                "/run/current-system/sw/bin/prophet-permission".to_owned(),
            ),
        ]
    }

    /// Instructions de connexion, à afficher à l'humain.
    ///
    /// L'OS ne conduit pas le flux de connexion : il lance celui du client, qui écrit ses propres
    /// identifiants dans son répertoire privé.
    #[must_use]
    pub fn login_instructions(&self) -> String {
        format!(
            "Connexion à {} : lancez `{} login` dans une session interactive. \
             Le client écrira sa session dans {}. Prophet OS ne lit jamais ce répertoire ; \
             il se contente de le monter dans la sandbox du client.",
            self.profile.driver,
            self.profile.program,
            self.config_dir().display()
        )
    }
}

fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

impl Driver for OfficialDriver {
    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            driver: self.profile.driver.clone(),
            kind: DriverKind::OfficialClient,
            auth: AuthMode::Subscription,
            supports: Supports {
                resume: self.profile.resume_arg.is_some(),
                checkpoint: false,
                fork: false,
                token_usage: true,
                quota_estimate: true,
                cost: false,
                streaming_events: true,
                permission_delegation: self.profile.permission_delegation,
            },
            logged_in: self.logged_in(),
            models: vec!["default".into()],
        }
    }

    fn start(&mut self, request: &StartRequest) -> Result<StartResponse, DriverError> {
        if !self.client_available() {
            return Err(DriverError::ClientMissing {
                client: self.profile.program.clone(),
            });
        }
        if !self.logged_in() {
            return Err(DriverError::NotLoggedIn(self.profile.driver.clone()));
        }
        // Le lancement effectif passe par `sandboxd`, qui monte le répertoire privé et le socket
        // du proxy, puis exécute la ligne construite ci-dessous. Cette étape est assurée par
        // `agentd` en service ; le pilote en fournit la description exacte.
        let _command = self.profile.command_line(
            &request.intent,
            &request.mcp_config,
            request.resume.as_deref(),
        );
        Err(DriverError::Io(format!(
            "le lancement de {} exige sandboxd en service",
            self.profile.driver
        )))
    }

    fn poll(&mut self, run: &str) -> Result<Vec<DriverEvent>, DriverError> {
        Err(DriverError::UnknownRun(run.to_owned()))
    }

    fn resolve_permission(
        &mut self,
        run: &str,
        _id: &str,
        _allowed: bool,
    ) -> Result<(), DriverError> {
        Err(DriverError::UnknownRun(run.to_owned()))
    }

    fn cancel(&mut self, run: &str) -> Result<(), DriverError> {
        Err(DriverError::UnknownRun(run.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ligne_de_commande_de_claude_code() {
        let profile = ClientProfile::claude_code();
        let args = profile.command_line("prépare le rapport", "/run/prophet/mcp.json", None);
        assert_eq!(args[0], "-p");
        assert_eq!(args[1], "prépare le rapport");
        assert!(args.contains(&"--mcp-config".to_owned()));
        assert!(args.contains(&"/run/prophet/mcp.json".to_owned()));
    }

    #[test]
    fn reprise_de_session() {
        let profile = ClientProfile::claude_code();
        let args = profile.command_line("suite", "/x.json", Some("sess-42"));
        assert!(args.contains(&"--resume".to_owned()));
        assert!(args.contains(&"sess-42".to_owned()));
    }

    #[test]
    fn un_client_sans_reprise_ignore_la_session() {
        let profile = ClientProfile::gemini();
        let args = profile.command_line("x", "/x.json", Some("sess-42"));
        assert!(!args.contains(&"sess-42".to_owned()));
    }

    #[test]
    fn le_repertoire_prive_est_par_pilote_et_par_utilisateur() {
        let a = private_config_dir(Path::new("/var/lib/prophet"), "claude-code", "hakik");
        let b = private_config_dir(Path::new("/var/lib/prophet"), "codex", "hakik");
        let c = private_config_dir(Path::new("/var/lib/prophet"), "claude-code", "autre");
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert!(a.ends_with("providers/claude-code/hakik"));
    }

    #[test]
    fn aucune_cle_d_api_dans_l_environnement() {
        let dir = tempfile::tempdir().unwrap();
        let driver = OfficialDriver::new(ClientProfile::claude_code(), dir.path(), "u");
        let env = driver.environment("/work", "/run/prophet/mcp.json");
        let noms: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        for interdit in ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "GOOGLE_API_KEY"] {
            assert!(
                !noms.contains(&interdit),
                "{interdit} ne doit jamais être transmis : ce pilote fonctionne par abonnement"
            );
        }
        assert!(noms.contains(&"CLAUDE_CONFIG_DIR"));
    }

    #[test]
    fn non_connecte_tant_que_le_repertoire_est_vide() {
        let dir = tempfile::tempdir().unwrap();
        let driver = OfficialDriver::new(ClientProfile::claude_code(), dir.path(), "u");
        assert!(!driver.logged_in());

        let config = driver.config_dir();
        std::fs::create_dir_all(&config).unwrap();
        assert!(
            !driver.logged_in(),
            "un répertoire vide ne vaut pas session"
        );

        std::fs::write(config.join(".credentials.json"), "{}").unwrap();
        assert!(driver.logged_in());
    }

    #[test]
    fn demarrage_refuse_sans_session() {
        let dir = tempfile::tempdir().unwrap();
        let mut driver = OfficialDriver::new(ClientProfile::claude_code(), dir.path(), "u");
        let request = StartRequest {
            driver: "claude-code".into(),
            task: "task:01".into(),
            intent: "x".into(),
            workdir: "/work".into(),
            mcp_config: "/x.json".into(),
            token: "jeton".into(),
            sandbox: prophet_types::driver::SandboxRequest {
                level: 1,
                profile: "base".into(),
            },
            limits: prophet_types::driver::Limits {
                wall_time_s: 60,
                max_steps: 10,
            },
            resume: None,
        };
        let err = driver.start(&request).unwrap_err();
        assert!(
            matches!(
                err,
                DriverError::NotLoggedIn(_) | DriverError::ClientMissing { .. }
            ),
            "{err}"
        );
    }

    #[test]
    fn les_capacites_refletent_le_profil() {
        let dir = tempfile::tempdir().unwrap();
        let claude = OfficialDriver::new(ClientProfile::claude_code(), dir.path(), "u");
        assert_eq!(claude.capabilities().auth, AuthMode::Subscription);
        assert_eq!(claude.capabilities().kind, DriverKind::OfficialClient);
        assert!(claude.capabilities().supports.resume);
        assert!(
            !claude.capabilities().supports.checkpoint,
            "un client officiel n'expose pas de point de reprise complet"
        );

        let gemini = OfficialDriver::new(ClientProfile::gemini(), dir.path(), "u");
        assert!(!gemini.capabilities().supports.permission_delegation);
    }

    #[test]
    fn les_instructions_de_connexion_nomment_le_repertoire_sans_le_lire() {
        let dir = tempfile::tempdir().unwrap();
        let driver = OfficialDriver::new(ClientProfile::codex(), dir.path(), "u");
        let texte = driver.login_instructions();
        assert!(texte.contains("codex login"), "{texte}");
        assert!(texte.contains("ne lit jamais"), "{texte}");
    }

    #[test]
    fn tous_les_profils_sont_distincts() {
        let profils = ClientProfile::all();
        let noms: std::collections::BTreeSet<&str> =
            profils.iter().map(|p| p.driver.as_str()).collect();
        assert_eq!(noms.len(), profils.len());
    }
}
