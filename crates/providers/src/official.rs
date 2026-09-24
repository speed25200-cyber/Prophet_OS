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

/// État rapporté par la commande de diagnostic du client, sans inspecter ses identifiants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    /// Le client rapporte une connexion active.
    Connected,
    /// Le client doit être connecté par son flux officiel.
    LoginRequired,
    /// Aucun exécutable utilisable n'a été trouvé.
    ClientMissing,
    /// Le client n'offre pas encore de sonde documentée dans ce pilote.
    Unknown,
    /// La sonde a échoué ou dépassé son délai.
    ProbeFailed,
}

impl ConnectionState {
    /// Libellé destiné à l'humain.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Connected => "connectée (client)",
            Self::LoginRequired => "connexion requise",
            Self::ClientMissing => "client absent",
            Self::Unknown => "non vérifiée",
            Self::ProbeFailed => "diagnostic en échec",
        }
    }
}

/// Diagnostic public, sans contenu des fichiers de connexion ni sortie brute d'authentification.
#[derive(Debug, Serialize)]
pub struct ClientDiagnostic {
    /// Pilote concerné.
    pub driver: String,
    /// Exécutable réellement résolu.
    pub executable: Option<PathBuf>,
    /// Version annoncée par le client.
    pub version: Option<String>,
    /// État rapporté par le client.
    pub connection: ConnectionState,
    /// Le raccordement à l'exécution agentique est-il opérationnel ?
    pub agent_execution_ready: bool,
}

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
    /// Hôtes de son éditeur que le client doit joindre pour travailler (API, renouvellement de
    /// sa connexion) : en mission, sa sortie réseau passe par egress sous un jeton borné à ces
    /// hôtes (ADR 0056). Relevés dans la documentation de chaque client ; l'administrateur les
    /// complète par `PROPHET_CLIENT_HOSTS` sans reconstruire le système.
    #[serde(default)]
    pub hosts: Vec<String>,
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
                "--verbose".into(),
                "--include-partial-messages".into(),
            ],
            mcp_config_arg: Some("--mcp-config".into()),
            config_home_env: "CLAUDE_CONFIG_DIR".into(),
            permission_delegation: true,
            resume_arg: Some("--resume".into()),
            hosts: vec![
                "api.anthropic.com".into(),
                "console.anthropic.com".into(),
                "platform.claude.com".into(),
                "claude.ai".into(),
            ],
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
            hosts: vec![
                "chatgpt.com".into(),
                "auth.openai.com".into(),
                "api.openai.com".into(),
            ],
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
            hosts: vec![
                "cloudcode-pa.googleapis.com".into(),
                "oauth2.googleapis.com".into(),
                "generativelanguage.googleapis.com".into(),
            ],
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
        if self.driver == "codex" {
            let mut args = vec!["exec".into()];
            if let Some(session) = resume {
                args.extend([
                    "resume".into(),
                    "--json".into(),
                    "--".into(),
                    session.into(),
                ]);
            } else {
                args.extend(["--json".into(), "--".into()]);
            }
            args.push(intent.into());
            return args;
        }
        if self.driver == "gemini" {
            return self
                .headless_args
                .iter()
                .map(|arg| arg.replace("{intent}", intent))
                .collect();
        }
        let mut args: Vec<String> = self
            .headless_args
            .iter()
            .filter(|arg| arg.as_str() != "{intent}")
            .cloned()
            .collect();
        if let Some(flag) = &self.mcp_config_arg {
            args.push(format!("{flag}={mcp_config}"));
        }
        if let (Some(flag), Some(session)) = (&self.resume_arg, resume) {
            args.push(format!("{flag}={session}"));
        }
        args.extend(["--".into(), intent.into()]);
        args
    }

    /// Commande interactive de connexion du client.
    #[must_use]
    pub fn login_args(&self) -> Vec<&str> {
        match self.driver.as_str() {
            "claude-code" => vec!["auth", "login"],
            "codex" => vec!["login"],
            _ => Vec::new(),
        }
    }

    fn status_args(&self) -> Option<&[&str]> {
        match self.driver.as_str() {
            "claude-code" => Some(&["auth", "status"]),
            "codex" => Some(&["login", "status"]),
            _ => None,
        }
    }
}

/// Répertoire de configuration privé d'un client, pour un utilisateur.
///
/// Il est réservé au client. La cage du lanceur de pilotes le lui monte en écriture (ADR
/// 0056) ; ni le lanceur ni le pilote n'en lisent jamais le contenu.
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
    /// Où chercher l'exécutable du client ; le PATH du processus par défaut.
    search_path: Option<std::ffi::OsString>,
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
            search_path: None,
        }
    }

    /// Cherche le client dans ces répertoires plutôt que dans le PATH du processus. Les tests
    /// s'en servent pour rester hermétiques : un client réellement installé et connecté sur la
    /// machine de développement ne doit pas changer ce qu'ils vérifient.
    #[must_use]
    pub fn with_search_path(mut self, path: impl Into<std::ffi::OsString>) -> Self {
        self.search_path = Some(path.into());
        self
    }

    fn locate(&self) -> Option<PathBuf> {
        let path = match &self.search_path {
            Some(path) => path.clone(),
            None => std::env::var_os("PATH")?,
        };
        which(&self.profile.program, &path)
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

    /// Vrai seulement si le client rapporte une connexion. Ne lit jamais ses fichiers privés.
    #[must_use]
    pub fn logged_in(&self) -> bool {
        self.connection_state() == ConnectionState::Connected
    }

    /// Interroge le client pendant au plus cinq secondes ; aucune sortie d'authentification
    /// n'est capturée ni publiée.
    #[must_use]
    pub fn connection_state(&self) -> ConnectionState {
        let Some(program) = self.locate() else {
            return ConnectionState::ClientMissing;
        };
        let Some(args) = self.profile.status_args() else {
            return ConnectionState::Unknown;
        };
        if !self.config_dir().is_dir() {
            return ConnectionState::LoginRequired;
        }
        match self.probe_command(&program, args, false) {
            Ok((Some(0), _)) => ConnectionState::Connected,
            Ok((Some(1), _)) => ConnectionState::LoginRequired,
            _ => ConnectionState::ProbeFailed,
        }
    }

    /// Version et connexion réellement sondées, en distinguant le client du pilote agentique.
    #[must_use]
    pub fn diagnostic(&self) -> ClientDiagnostic {
        let executable = self.locate();
        let version = executable
            .as_ref()
            .and_then(|program| {
                let (code, output) = self.probe_command(program, &["--version"], true).ok()?;
                (code == Some(0)).then(|| {
                    output
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(128)
                        .collect::<String>()
                })
            })
            .filter(|text| !text.is_empty());
        ClientDiagnostic {
            driver: self.profile.driver.clone(),
            executable,
            version,
            connection: self.connection_state(),
            agent_execution_ready: false,
        }
    }

    fn probe_command(
        &self,
        program: &Path,
        args: &[&str],
        capture: bool,
    ) -> std::io::Result<(Option<i32>, String)> {
        use std::process::Stdio;
        use std::time::Duration;
        use tokio::io::AsyncReadExt as _;
        let program = program.to_owned();
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let environment = self.environment(&self.config_dir().display().to_string(), "");
        // Une boucle dédiée évite d'imbriquer un runtime Tokio chez un appelant asynchrone.
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async move {
                let mut child = tokio::process::Command::new(program)
                    .args(args)
                    .env_clear()
                    .envs(environment)
                    .current_dir(std::env::temp_dir())
                    .stdin(Stdio::null())
                    .stderr(Stdio::null())
                    .stdout(if capture {
                        Stdio::piped()
                    } else {
                        Stdio::null()
                    })
                    .kill_on_drop(true)
                    .spawn()?;
                let result = tokio::time::timeout(Duration::from_secs(5), async {
                    let mut bytes = Vec::new();
                    if let Some(stdout) = child.stdout.take() {
                        stdout.take(4097).read_to_end(&mut bytes).await?;
                        if bytes.len() > 4096 {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "diagnostic trop grand",
                            ));
                        }
                    }
                    let status = child.wait().await?;
                    Ok((status.code(), String::from_utf8_lossy(&bytes).into_owned()))
                })
                .await
                .map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "le client n'a pas répondu en cinq secondes",
                    )
                })
                .and_then(std::convert::identity);
                if result.is_err() {
                    // kill().await attend aussi la terminaison : pas de processus laissé en vie
                    // à la destruction du runtime après un délai ou une sortie trop volumineuse.
                    let _ = child.kill().await;
                }
                result
            })
        })
        .join()
        .map_err(|_| std::io::Error::other("diagnostic interrompu"))?
    }

    /// Vrai si l'exécutable du client est présent.
    #[must_use]
    pub fn client_available(&self) -> bool {
        self.locate().is_some()
    }

    /// Variables d'environnement transmises au client. Rien d'autre ne passe.
    ///
    /// En particulier, aucune variable portant une clé d'API n'est propagée. Le parcours cible
    /// utilise la connexion par abonnement ; le type effectif de session reste géré par le client.
    #[must_use]
    pub fn environment(&self, workdir: &str, mcp_config: &str) -> Vec<(String, String)> {
        vec![
            (
                self.profile.config_home_env.clone(),
                self.config_dir().display().to_string(),
            ),
            ("HOME".to_owned(), workdir.to_owned()),
            (
                "PATH".to_owned(),
                "/run/current-system/sw/bin:/usr/bin:/bin".to_owned(),
            ),
            ("PROPHET_MCP_CONFIG".to_owned(), mcp_config.to_owned()),
        ]
    }

    /// Instructions de connexion, à afficher à l'humain.
    ///
    /// L'OS ne conduit pas le flux de connexion : il lance celui du client, qui écrit ses propres
    /// identifiants dans son répertoire privé.
    #[must_use]
    pub fn login_instructions(&self) -> String {
        format!(
            "Connexion à {} : préparez le répertoire privé puis lancez le client dans votre terminal :\n\
             install -d -m 700 -- {}\n\
             env {}={} {} {}\n\
             Prophet OS ne lit jamais les fichiers d'identifiants ; le client gère sa connexion. \
             L'exécution agentique via sandboxd reste à raccorder.",
            self.profile.driver,
            shell_quote(&self.config_dir().display().to_string()),
            self.profile.config_home_env,
            shell_quote(&self.config_dir().display().to_string()),
            self.profile.program,
            self.profile.login_args().join(" ")
        )
    }
}

fn which(program: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt as _;
    std::env::split_paths(path)
        .map(|dir| dir.join(program))
        .find(|candidate| {
            candidate
                .metadata()
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\"'\"'"))
}

impl Driver for OfficialDriver {
    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            driver: self.profile.driver.clone(),
            kind: DriverKind::OfficialClient,
            auth: AuthMode::Subscription,
            // Le profil décrit le client amont ; le contrat décrit ce pilote. Tant que start,
            // poll et resolve_permission ne sont pas raccordés, ces fonctions ne sont pas livrées.
            supports: Supports::default(),
            logged_in: self.logged_in(),
            models: Vec::new(),
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
        // du proxy, puis exécute la ligne construite ci-dessous. Le raccordement à `agentd`
        // n'est pas encore implémenté ; la présence du service ne suffit pas.
        let _command = self.profile.command_line(
            &request.intent,
            &request.mcp_config,
            request.resume.as_deref(),
        );
        Err(DriverError::Io(format!(
            "le lancement agentique de {} via sandboxd n'est pas encore implémenté",
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
        assert_eq!(args.last().unwrap(), "prépare le rapport");
        assert!(args.contains(&"--mcp-config=/run/prophet/mcp.json".to_owned()));
    }

    #[test]
    fn reprise_de_session() {
        let profile = ClientProfile::claude_code();
        let args = profile.command_line("suite", "/x.json", Some("sess-42"));
        assert!(args.contains(&"--resume=sess-42".to_owned()));
    }

    #[test]
    fn les_valeurs_de_claude_ne_deviennent_pas_des_options() {
        let flag = "--dangerously-skip-permissions";
        let args = ClientProfile::claude_code().command_line("bonjour", flag, Some(flag));
        assert!(
            !args.iter().any(|arg| arg == flag),
            "une valeur ne doit pas devenir une option autonome : {args:?}"
        );
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

    /// Un faux client sur un chemin de recherche privé : le test ne dépend pas de ce qui est
    /// installé, ni connecté, sur la machine où il tourne.
    fn faux_client(dir: &std::path::Path, code: i32) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let client = bin.join(ClientProfile::claude_code().program);
        std::fs::write(&client, format!("#!/bin/sh\nexit {code}\n")).unwrap();
        std::fs::set_permissions(&client, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin
    }

    #[test]
    fn un_fichier_de_configuration_ne_prouve_pas_une_connexion() {
        let dir = tempfile::tempdir().unwrap();
        // Sans client sur le chemin : il manque, quoi que contienne le répertoire.
        let absent = OfficialDriver::new(ClientProfile::claude_code(), dir.path(), "u")
            .with_search_path(dir.path().join("nulle-part"));
        assert_eq!(absent.connection_state(), ConnectionState::ClientMissing);

        // Un client qui répond « pas de session » : rien dans le répertoire n'y change rien.
        let bin = faux_client(dir.path(), 1);
        let driver = OfficialDriver::new(ClientProfile::claude_code(), dir.path(), "u")
            .with_search_path(&bin);
        assert!(!driver.logged_in());

        let config = driver.config_dir();
        std::fs::create_dir_all(&config).unwrap();
        assert!(
            !driver.logged_in(),
            "un répertoire vide ne vaut pas session"
        );

        std::fs::write(config.join(".credentials.json"), "{}").unwrap();
        assert!(
            !driver.logged_in(),
            "un fichier ne prouve pas une session active"
        );
        assert_eq!(driver.connection_state(), ConnectionState::LoginRequired);

        // Seule la réponse du client fait foi.
        let bin = faux_client(dir.path(), 0);
        let connecte = OfficialDriver::new(ClientProfile::claude_code(), dir.path(), "u")
            .with_search_path(&bin);
        assert!(connecte.logged_in());
    }

    #[test]
    fn claude_utilise_les_options_de_flux_documentees() {
        let args = ClientProfile::claude_code().command_line("bonjour", "/mcp.json", None);
        assert!(args.contains(&"--verbose".to_owned()));
        assert!(args.contains(&"--include-partial-messages".to_owned()));
    }

    #[test]
    fn codex_reprend_avant_de_recevoir_le_prompt() {
        let args = ClientProfile::codex().command_line(
            "--dangerously-bypass-approvals-and-sandbox",
            "/mcp.json",
            Some("session-42"),
        );
        assert_eq!(&args[..4], ["exec", "resume", "--json", "--"]);
        assert_eq!(
            &args[4..],
            ["session-42", "--dangerously-bypass-approvals-and-sandbox"]
        );
    }

    fn fake_client(script: &str) -> (tempfile::TempDir, OfficialDriver) {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("client");
        std::fs::write(&executable, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut profile = ClientProfile::codex();
        profile.program = executable.display().to_string();
        let driver = OfficialDriver::new(profile, dir.path(), "user");
        std::fs::create_dir_all(driver.config_dir()).unwrap();
        (dir, driver)
    }

    #[test]
    fn seule_la_reponse_du_client_atteste_la_connexion() {
        for (code, state) in [
            (0, ConnectionState::Connected),
            (1, ConnectionState::LoginRequired),
            (2, ConnectionState::ProbeFailed),
        ] {
            let (_dir, driver) = fake_client(&format!("exit {code}"));
            assert_eq!(driver.connection_state(), state);
        }
    }

    #[test]
    fn le_diagnostic_ne_publie_pas_la_sortie_authentification() {
        let (_dir, driver) = fake_client(
            "if [ \"$1\" = --version ]; then echo 'client 1.2.3'; else echo 'sortie-auth-privee'; fi",
        );
        let diagnostic = driver.diagnostic();
        assert_eq!(diagnostic.version.as_deref(), Some("client 1.2.3"));
        assert_eq!(diagnostic.connection, ConnectionState::Connected);
        assert!(!diagnostic.agent_execution_ready);
        assert!(
            !serde_json::to_string(&diagnostic)
                .unwrap()
                .contains("sortie-auth-privee")
        );
    }

    #[test]
    fn une_version_trop_grande_est_refusee() {
        let (_dir, driver) =
            fake_client("i=0; while [ $i -lt 4100 ]; do printf x; i=$((i + 1)); done");
        let error = driver
            .probe_command(Path::new(&driver.profile.program), &["--version"], true)
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn un_client_muet_est_arrete_apres_le_delai() {
        // Boucle de shell sans sous-processus : la sonde doit terminer et récolter ce client.
        let (_dir, driver) = fake_client("while :; do :; done");
        let started = std::time::Instant::now();
        let error = driver
            .probe_command(Path::new(&driver.profile.program), &["--version"], true)
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < std::time::Duration::from_secs(8));
    }

    #[test]
    #[ignore = "needs_official_clients : chemins PROPHET_TEST_CODEX et PROPHET_TEST_CLAUDE"]
    fn needs_official_clients_versions_et_sessions_vierges() {
        for (mut profile, variable) in [
            (ClientProfile::codex(), "PROPHET_TEST_CODEX"),
            (ClientProfile::claude_code(), "PROPHET_TEST_CLAUDE"),
        ] {
            profile.program = std::env::var(variable).expect("chemin du vrai client requis");
            let dir = tempfile::tempdir().unwrap();
            let driver = OfficialDriver::new(profile, dir.path(), "test");
            std::fs::create_dir_all(driver.config_dir()).unwrap();
            let diagnostic = driver.diagnostic();
            eprintln!("{}", serde_json::to_string(&diagnostic).unwrap());
            assert!(diagnostic.version.is_some());
            assert_eq!(diagnostic.connection, ConnectionState::LoginRequired);
            assert!(!diagnostic.agent_execution_ready);
        }
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
    fn les_capacites_ne_promettent_pas_un_pilote_non_raccorde() {
        let dir = tempfile::tempdir().unwrap();
        let claude = OfficialDriver::new(ClientProfile::claude_code(), dir.path(), "u");
        assert_eq!(claude.capabilities().auth, AuthMode::Subscription);
        assert_eq!(claude.capabilities().kind, DriverKind::OfficialClient);
        assert_eq!(claude.capabilities().supports, Supports::default());
        assert!(claude.capabilities().models.is_empty());
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
