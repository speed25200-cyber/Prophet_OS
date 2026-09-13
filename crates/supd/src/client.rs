//! Lancer un client officiel de l'humain sur une mission, depuis sa session (ADR 0034).
//!
//! Le client (Claude Code, Codex) est celui de l'humain : connecté par son abonnement, avec ses
//! propres identifiants dans son répertoire privé, que rien ici ne lit. Le lanceur lui donne une
//! seule chose, le pont `prophet-mcp` vers la séance d'outils de la mission, et lui retire tout
//! le reste : aucun outil intégré, aucune question de permission, un nombre de tours et un délai
//! bornés. Ce que le client fait dans la mission, agentd le voit et capd le tranche ; ce que le
//! lanceur rend, c'est la sortie du client, son code de retour et son texte final.

use std::ffi::OsString;
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use providers::official::{ClientProfile, ConnectionState, McpBridge, OfficialDriver};
use serde_json::{Value, json};
use sup::session::{ClientRunOutcome, ClientRunRequest, ClientStatus};
use tokio::io::AsyncReadExt as _;

/// Sortie standard conservée au plus (le flux d'événements du client peut être long).
const MAX_STDOUT: usize = 4 * 1024 * 1024;
/// Fin de la sortie d'erreur conservée.
const MAX_STDERR: usize = 8 * 1024;
/// Texte final rendu au plus : la limite qu'agentd accepte pour un résultat de séance.
const MAX_TEXT: usize = 16_000;

/// Ce qui empêche de lancer un client.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Aucun profil de ce nom.
    #[error("pilote inconnu : {0}")]
    UnknownDriver(String),
    /// Ce client ne sait pas être délégué.
    #[error("le client {0} ne sait pas être lancé sur une mission sans ses propres outils")]
    Unsupported(String),
    /// Le client n'est pas installé ou pas connecté.
    #[error("{0}")]
    NotConnected(String),
    /// Mission ou intention mal formées.
    #[error("{0}")]
    Invalid(String),
    /// Le lancement lui-même a échoué.
    #[error("{0}")]
    Io(String),
}

/// Le lanceur, configuré par l'environnement de la session.
#[derive(Debug, Clone)]
pub struct Launcher {
    /// Racine de l'état privé des clients (`~/.local/state/prophet`), la même que la CLI.
    root: PathBuf,
    /// Compte de l'humain.
    user: String,
    /// Où chercher les clients ; le PATH de la session par défaut.
    search_path: Option<OsString>,
    /// Le pont `prophet-mcp`.
    bridge: PathBuf,
    /// Socket d'agentd à donner au pont, s'il n'est pas celui du système.
    agentd_socket: Option<String>,
    /// Où poser l'espace de travail éphémère de chaque lancement.
    workspaces: PathBuf,
}

impl Launcher {
    /// Lit sa configuration dans l'environnement de la session : `HOME`, `USER`,
    /// `PROPHET_STATE_ROOT`, `PROPHET_SUP_CLIENT_PATH` (essais), `PROPHET_MCP_BRIDGE`,
    /// `PROPHET_AGENTD_SOCKET`, `XDG_RUNTIME_DIR`.
    #[must_use]
    pub fn from_env() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let root = std::env::var_os("PROPHET_STATE_ROOT")
            .map_or_else(|| home.join(".local/state/prophet"), PathBuf::from);
        let user = std::env::var("USER").unwrap_or_else(|_| {
            use std::os::unix::fs::MetadataExt as _;
            std::fs::metadata("/proc/self")
                .map_or_else(|_| "inconnu".to_owned(), |m| m.uid().to_string())
        });
        let bridge = std::env::var_os("PROPHET_MCP_BRIDGE")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(|d| d.join("prophet-mcp")))
                    .filter(|p| p.is_file())
            })
            .unwrap_or_else(|| PathBuf::from("prophet-mcp"));
        let workspaces = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|d| d.is_dir())
            .unwrap_or_else(std::env::temp_dir)
            .join("prophet-client");
        Self {
            root,
            user,
            search_path: std::env::var_os("PROPHET_SUP_CLIENT_PATH"),
            bridge,
            agentd_socket: std::env::var("PROPHET_AGENTD_SOCKET").ok(),
            workspaces,
        }
    }

    fn driver(&self, name: &str) -> Result<(ClientProfile, OfficialDriver), Error> {
        let profile = ClientProfile::all()
            .into_iter()
            .find(|p| p.driver == name)
            .ok_or_else(|| Error::UnknownDriver(name.to_owned()))?;
        let mut driver = OfficialDriver::new(profile.clone(), &self.root, &self.user);
        if let Some(path) = &self.search_path {
            driver = driver.with_search_path(path.clone());
        }
        Ok((profile, driver))
    }

    /// Le client est-il installé et connecté ? Demandé au client lui-même, jamais lu.
    #[must_use]
    pub fn status(&self, name: &str) -> ClientStatus {
        match self.driver(name) {
            Ok((_, driver)) => {
                let state = driver.connection_state();
                ClientStatus {
                    driver: name.to_owned(),
                    available: state != ConnectionState::ClientMissing,
                    logged_in: state == ConnectionState::Connected,
                    detail: format!("{state:?}"),
                }
            }
            Err(e) => ClientStatus {
                driver: name.to_owned(),
                available: false,
                logged_in: false,
                detail: e.to_string(),
            },
        }
    }

    /// Lance le client sur la mission et attend qu'il ait fini, ou que le délai l'arrête.
    ///
    /// # Errors
    /// Pilote inconnu ou non délégable, client absent ou déconnecté, lancement impossible.
    pub async fn run(&self, request: &ClientRunRequest) -> Result<ClientRunOutcome, Error> {
        if request.task.is_empty()
            || request.task.len() > 160
            || !request
                .task
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':'))
        {
            return Err(Error::Invalid("identifiant de mission invalide".into()));
        }
        if request.intent.trim().is_empty() {
            return Err(Error::Invalid("intention vide".into()));
        }
        let (profile, driver) = self.driver(&request.driver)?;
        let state = driver.connection_state();
        if state != ConnectionState::Connected {
            return Err(Error::NotConnected(format!(
                "{} : {state:?} ; connectez-le avec « prophet provider login {} »",
                profile.driver, profile.driver
            )));
        }
        let program = driver.executable().ok_or_else(|| {
            Error::NotConnected(format!("{} : client introuvable", profile.driver))
        })?;

        // Un espace éphémère par lancement : le home du client, et la configuration du pont.
        let workspace = self.workspaces.join(request.task.replace(':', "_"));
        let _ = std::fs::remove_dir_all(&workspace);
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&workspace)
            .map_err(|e| Error::Io(format!("espace de lancement : {e}")))?;
        let home = workspace.join("home");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&home)
            .map_err(|e| Error::Io(format!("home du client : {e}")))?;
        let mut env = vec![("PROPHET_TASK".to_owned(), request.task.clone())];
        if let Some(socket) = &self.agendd_socket_for_bridge() {
            env.push(("PROPHET_AGENTD_SOCKET".to_owned(), socket.clone()));
        }
        let config_path = workspace.join("mcp.json");
        let config = json!({
            "mcpServers": {
                "prophet": {
                    "command": self.bridge.display().to_string(),
                    "args": [],
                    "env": env
                        .iter()
                        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                        .collect::<serde_json::Map<String, Value>>()
                }
            }
        });
        ecrire_prive(
            &config_path,
            &serde_json::to_vec_pretty(&config).unwrap_or_default(),
        )?;
        let bridge = McpBridge {
            config_path: config_path.display().to_string(),
            command: self.bridge.display().to_string(),
            env,
        };
        let args = profile
            .delegated_command_line(request.intent.trim(), &bridge)
            .ok_or_else(|| Error::Unsupported(profile.driver.clone()))?;
        let environment = driver.environment(&home.display().to_string(), &bridge.config_path);

        let mut child = tokio::process::Command::new(&program)
            .args(&args)
            .env_clear()
            .envs(environment)
            .current_dir(&home)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| Error::Io(format!("lancement de {} : {e}", program.display())))?;
        let pid = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let lecteur_out = tokio::spawn(async move { lire_borne(stdout, MAX_STDOUT).await });
        let lecteur_err = tokio::spawn(async move { lire_borne(stderr, MAX_STDERR).await });
        let delai = Duration::from_secs(request.timeout_s.clamp(1, 3600));
        let (exit_code, timed_out) = match tokio::time::timeout(delai, child.wait()).await {
            Ok(Ok(status)) => (status.code(), false),
            Ok(Err(e)) => {
                tuer(pid);
                let _ = child.kill().await;
                return Err(Error::Io(format!("attente du client : {e}")));
            }
            Err(_) => {
                // Le groupe entier : le client et le pont qu'il a lancé.
                tuer(pid);
                let _ = child.kill().await;
                let _ = child.wait().await;
                (None, true)
            }
        };
        let sortie = tokio::time::timeout(Duration::from_secs(5), lecteur_out)
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
        let erreurs = tokio::time::timeout(Duration::from_secs(5), lecteur_err)
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
        let _ = std::fs::remove_dir_all(&workspace);
        let mut text = texte_final(&profile.driver, &sortie);
        if text.chars().count() > MAX_TEXT {
            text = text.chars().take(MAX_TEXT).collect();
        }
        Ok(ClientRunOutcome {
            exit_code,
            timed_out,
            text,
            stderr: queue(&erreurs, MAX_STDERR),
        })
    }

    fn agendd_socket_for_bridge(&self) -> Option<String> {
        self.agentd_socket.clone()
    }
}

/// Écrit un fichier lisible par son seul propriétaire, dès sa création.
fn ecrire_prive(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| Error::Io(format!("{} : {e}", path.display())))?;
    f.write_all(bytes)
        .map_err(|e| Error::Io(format!("{} : {e}", path.display())))
}

/// Lit un flux en gardant au plus `limite` octets, sans jamais bloquer l'enfant sur un tube plein.
async fn lire_borne<R: tokio::io::AsyncRead + Unpin>(flux: Option<R>, limite: usize) -> Vec<u8> {
    let Some(mut flux) = flux else {
        return Vec::new();
    };
    let mut garde = Vec::new();
    let mut tampon = [0u8; 8192];
    loop {
        match flux.read(&mut tampon).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if garde.len() < limite {
                    let reste = limite - garde.len();
                    garde.extend_from_slice(&tampon[..n.min(reste)]);
                }
            }
        }
    }
    garde
}

/// Envoie SIGKILL au groupe de processus du client : lui, et le pont qu'il a lancé.
fn tuer(pid: Option<u32>) {
    if let Some(pid) = pid.and_then(|p| i32::try_from(p).ok()) {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
}

/// Le texte final que le client a rendu : l'événement `result` de Claude Code, le dernier
/// message d'agent de Codex, ou, à défaut, la dernière ligne non vide.
fn texte_final(driver: &str, sortie: &[u8]) -> String {
    let texte = String::from_utf8_lossy(sortie);
    let mut dernier = String::new();
    let mut resultat: Option<String> = None;
    for ligne in texte.lines() {
        let ligne = ligne.trim();
        if ligne.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(ligne) {
            Ok(v) => match driver {
                "claude-code" if v["type"] == "result" => {
                    if let Some(r) = v["result"].as_str() {
                        resultat = Some(r.to_owned());
                    }
                }
                "codex"
                    if v["type"] == "item.completed" && v["item"]["type"] == "agent_message" =>
                {
                    if let Some(t) = v["item"]["text"].as_str() {
                        resultat = Some(t.to_owned());
                    }
                }
                _ => {}
            },
            Err(_) => dernier = ligne.to_owned(),
        }
    }
    resultat.unwrap_or(dernier)
}

fn queue(bytes: &[u8], max: usize) -> String {
    let texte = String::from_utf8_lossy(bytes);
    let n = texte.chars().count();
    if n <= max {
        texte.into_owned()
    } else {
        texte.chars().skip(n - max).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_texte_final_vient_de_l_evenement_result() {
        let flux = b"{\"type\":\"system\"}\n{\"type\":\"assistant\"}\n{\"type\":\"result\",\"result\":\"Fini.\"}\n";
        assert_eq!(texte_final("claude-code", flux), "Fini.");
        let codex = "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"Voilà.\"}}\n";
        assert_eq!(texte_final("codex", codex.as_bytes()), "Voilà.");
        assert_eq!(texte_final("claude-code", b"bonjour\n\nmonde\n"), "monde");
    }

    #[test]
    fn une_mission_mal_nommee_est_refusee_avant_tout_lancement() {
        let launcher = Launcher {
            root: std::env::temp_dir(),
            user: "u".into(),
            search_path: Some(std::env::temp_dir().join("nulle-part").into()),
            bridge: PathBuf::from("prophet-mcp"),
            agentd_socket: None,
            workspaces: std::env::temp_dir().join("prophet-client-essai"),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let erreur = rt
            .block_on(launcher.run(&ClientRunRequest {
                task: "../evil".into(),
                driver: "claude-code".into(),
                intent: "x".into(),
                timeout_s: 5,
            }))
            .unwrap_err();
        assert!(matches!(erreur, Error::Invalid(_)), "{erreur}");
        let erreur = rt
            .block_on(launcher.run(&ClientRunRequest {
                task: "t".into(),
                driver: "claude-code".into(),
                intent: "x".into(),
                timeout_s: 5,
            }))
            .unwrap_err();
        assert!(matches!(erreur, Error::NotConnected(_)), "{erreur}");
        assert!(!launcher.status("claude-code").available);
        assert!(!launcher.status("inconnu").available);
    }
}
