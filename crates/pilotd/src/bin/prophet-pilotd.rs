//! `prophet-pilotd` : le lanceur de pilotes officiels, dans la session de l'humain.
//!
//! Il écoute sur un socket que seul `agentd` est admis à appeler (attesté par `SO_PEERCRED`),
//! dit quels clients sont installés et connectés, et lance un client non modifié dans une
//! mission préparée, sous l'identité de l'humain, avec le profil privé du client : l'OS ne lit
//! jamais ses identifiants (ADR 0035). Il ne décide d'aucun droit : la séance que le client
//! rejoint par le pont fait trancher capd à chaque appel.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use pilotd::{DEFAULT_SOCKET, Launcher, METHOD_RUN, METHOD_STATUS, RunRequest};
use prophet_ipc::{Error, ErrorCode, Handler, PeerIdentity, Server};
use serde_json::Value;

struct Pilot {
    /// Identifiants d'utilisateur admis à appeler : `agentd`, et le propriétaire de la
    /// session s'il l'a demandé explicitement (essais).
    allowed: Vec<u32>,
    launcher: Launcher,
}

impl Handler for Pilot {
    async fn call(
        &self,
        peer: PeerIdentity,
        _auth: Option<String>,
        method: String,
        params: Value,
    ) -> Result<Value, Error> {
        if method == "ping" {
            return Ok(Value::String("pong".into()));
        }
        if !self.allowed.contains(&peer.uid) {
            return Err(Error::new(
                ErrorCode::Unauthorized,
                "seul le service agentd lance les pilotes de cette session",
            ));
        }
        match method.as_str() {
            METHOD_STATUS => serde_json::to_value(self.launcher.status())
                .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string())),
            METHOD_RUN => {
                let request: RunRequest = serde_json::from_value(params)
                    .map_err(|e| Error::new(ErrorCode::InvalidParams, e.to_string()))?;
                let launcher = self.launcher.clone();
                tracing::info!(
                    tache = %request.task,
                    pilote = %request.driver,
                    "lancement d'un client dans une mission"
                );
                let result = tokio::task::spawn_blocking(move || launcher.run(&request))
                    .await
                    .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))?
                    .map_err(|e| {
                        let code = match e {
                            pilotd::Error::UnknownDriver(_) | pilotd::Error::Invalid(_) => {
                                ErrorCode::InvalidParams
                            }
                            pilotd::Error::NotReady { .. } => ErrorCode::Conflict,
                            pilotd::Error::Launch { .. } | pilotd::Error::Timeout { .. } => {
                                ErrorCode::InternalError
                            }
                        };
                        Error::new(code, e.to_string())
                    })?;
                serde_json::to_value(result)
                    .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))
            }
            _ => Err(Error::new(
                ErrorCode::MethodNotFound,
                format!("méthode inconnue : {method}"),
            )),
        }
    }
}

/// Identifiant d'un compte, par `/etc/passwd`, ou d'un groupe, par `/etc/group`.
fn id_in(file: &str, name: &str) -> Option<u32> {
    std::fs::read_to_string(file)
        .ok()?
        .lines()
        .find_map(|line| {
            let mut fields = line.split(':');
            (fields.next()? == name).then(|| fields.nth(1)?.parse().ok())?
        })
}

fn own_uid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::metadata("/proc/self").ok().map(|m| m.uid())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let socket =
        std::env::var("PROPHET_PILOT_SOCKET").unwrap_or_else(|_| DEFAULT_SOCKET.to_owned());
    let client = std::env::var("PROPHET_PILOT_CLIENT").unwrap_or_else(|_| "agentd".to_owned());
    let mut allowed = Vec::new();
    match id_in("/etc/passwd", &client) {
        Some(uid) => allowed.push(uid),
        None => tracing::warn!(compte = %client, "compte client inconnu : personne ne sera admis"),
    }
    if std::env::var("PROPHET_PILOT_ALLOW_OWNER").as_deref() == Ok("1") {
        match own_uid() {
            Some(uid) => allowed.push(uid),
            None => tracing::warn!("identifiant du propriétaire illisible"),
        }
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
    let user = std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty())
        .or_else(|| own_uid().map(|uid| format!("uid-{uid}")))
        .unwrap_or_else(|| "session".to_owned());
    let root = std::env::var_os("PROPHET_PILOT_STATE")
        .map_or_else(|| home.join(".local/state/prophet"), PathBuf::from);
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map_or_else(std::env::temp_dir, PathBuf::from)
        .join("prophet-pilot");
    let bridge = std::env::var_os("PROPHET_MCP_BRIDGE")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|d| d.join("prophet-mcp")))
                .filter(|p| p.is_file())
        })
        .unwrap_or_else(|| PathBuf::from("prophet-mcp"));
    let documents = home.join("Documents/Prophet");
    let overrides = match std::env::var("PROPHET_PILOT_CLIENTS") {
        Ok(json) => match Launcher::parse_overrides(&json) {
            Ok(map) => {
                tracing::warn!(
                    pilotes = ?map.keys().collect::<Vec<_>>(),
                    "clients de remplacement actifs : aucun client officiel ne sera lancé pour eux"
                );
                map
            }
            Err(e) => {
                tracing::error!(erreur = %e, "clients de remplacement illisibles");
                std::process::exit(1);
            }
        },
        Err(_) => BTreeMap::new(),
    };
    let launcher = Launcher {
        root,
        user,
        agentd_socket: std::env::var_os("PROPHET_AGENTD_SOCKET")
            .map_or_else(|| prophet_ipc::socket_path("agentd"), PathBuf::from),
        bridge,
        runtime_dir,
        workdir: if documents.is_dir() { documents } else { home },
        overrides,
    };
    let server = match Server::bind(&socket) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(socket, erreur = %e, "socket impossible à ouvrir");
            std::process::exit(1);
        }
    };
    // Le socket appartient à la session ; le groupe système de Prophet doit pouvoir s'y
    // connecter : `agentd` en est membre, le reste de la machine non.
    let group =
        std::env::var("PROPHET_PILOT_GROUP").unwrap_or_else(|_| "prophet-system".to_owned());
    match id_in("/etc/group", &group) {
        Some(gid) => {
            if let Err(e) = std::os::unix::fs::chown(&socket, None, Some(gid)) {
                tracing::warn!(socket, groupe = %group, erreur = %e, "groupe du socket non posé");
            }
        }
        None => {
            tracing::warn!(groupe = %group, "groupe inconnu : le socket garde celui de la session")
        }
    }
    tracing::info!(socket, admis = ?allowed, pont = %launcher.bridge.display(), "lanceur de pilotes prêt");
    let pilot = Arc::new(Pilot { allowed, launcher });
    if let Err(e) = server.serve(pilot).await {
        tracing::error!(erreur = %e, "service interrompu");
        std::process::exit(1);
    }
}
