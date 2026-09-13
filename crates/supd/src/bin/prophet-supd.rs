//! `prophet-supd` : l'adaptateur d'accessibilité, dans la session de l'humain.
//!
//! Il écoute sur un socket que seul `agentd` est admis à appeler (attesté par `SO_PEERCRED`),
//! joint le bus d'accessibilité de la session à la demande, et rend les arbres SUP des
//! applications ou y exécute une action. Il ne décide d'aucun droit : c'est `agentd` qui fait
//! trancher capd avant de l'appeler ; lui vérifie seulement qui l'appelle.

use std::sync::Arc;

use prophet_ipc::{Error, ErrorCode};
use prophet_ipc::{Handler, PeerIdentity, Server};
use serde_json::Value;
use sup::session::{
    ActRequest, DEFAULT_SOCKET, METHOD_ACT, METHOD_APPS, METHOD_STATUS, METHOD_TREE, Status,
    TreeRequest,
};
use supd::Desktop;
use tokio::sync::Mutex;

struct Adapter {
    /// Identifiants d'utilisateur admis à appeler : `agentd`, et le propriétaire de la session
    /// s'il l'a demandé explicitement (essais).
    allowed: Vec<u32>,
    desktop: Mutex<Option<Desktop>>,
}

impl Adapter {
    async fn desktop(&self) -> Result<Desktop, Error> {
        let mut guard = self.desktop.lock().await;
        if let Some(d) = guard.as_ref() {
            // Une connexion morte (bus relancé) se voit au premier appel ; on la refait alors.
            if d.count().await.is_ok() {
                return Ok(d.clone());
            }
        }
        let desktop = Desktop::connect()
            .await
            .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))?;
        *guard = Some(desktop.clone());
        Ok(desktop)
    }
}

fn code(e: &supd::Error) -> ErrorCode {
    match e {
        supd::Error::Bus(_) => ErrorCode::InternalError,
        supd::Error::UnknownApp(_)
        | supd::Error::UnknownWindow(_)
        | supd::Error::UnknownNode(_) => ErrorCode::NotFound,
        supd::Error::Unsupported(_) | supd::Error::Invalid(_) => ErrorCode::InvalidParams,
        supd::Error::Timeout => ErrorCode::InternalError,
    }
}

impl Handler for Adapter {
    async fn call(
        &self,
        peer: PeerIdentity,
        _auth: Option<String>,
        method: String,
        params: Value,
    ) -> Result<Value, Error> {
        if !self.allowed.contains(&peer.uid) {
            return Err(Error::new(
                ErrorCode::Unauthorized,
                "seul le service agentd pilote les applications de cette session",
            ));
        }
        match method.as_str() {
            METHOD_STATUS => {
                let status = match self.desktop().await {
                    Ok(d) => match d.count().await {
                        Ok(n) => Status {
                            ready: true,
                            detail: format!("{n} application(s) sur le bus d'accessibilité"),
                        },
                        Err(e) => Status {
                            ready: false,
                            detail: e.to_string(),
                        },
                    },
                    Err(e) => Status {
                        ready: false,
                        detail: e.message,
                    },
                };
                Ok(serde_json::to_value(status).unwrap_or(Value::Null))
            }
            METHOD_APPS => {
                let apps = self
                    .desktop()
                    .await?
                    .applications()
                    .await
                    .map_err(|e| Error::new(code(&e), e.to_string()))?;
                Ok(serde_json::to_value(apps).unwrap_or(Value::Null))
            }
            METHOD_TREE => {
                let request: TreeRequest = serde_json::from_value(params)
                    .map_err(|e| Error::new(ErrorCode::InvalidParams, e.to_string()))?;
                let obs = self
                    .desktop()
                    .await?
                    .tree(&request.app, request.window.as_deref())
                    .await
                    .map_err(|e| Error::new(code(&e), e.to_string()))?;
                Ok(serde_json::to_value(obs).unwrap_or(Value::Null))
            }
            METHOD_ACT => {
                let request: ActRequest = serde_json::from_value(params)
                    .map_err(|e| Error::new(ErrorCode::InvalidParams, e.to_string()))?;
                let outcome = self
                    .desktop()
                    .await?
                    .act(&request)
                    .await
                    .map_err(|e| Error::new(code(&e), e.to_string()))?;
                Ok(serde_json::to_value(outcome).unwrap_or(Value::Null))
            }
            _ => Err(Error::new(ErrorCode::MethodNotFound, "méthode inconnue")),
        }
    }
}

/// Résout un nom de compte en identifiant, par `/etc/passwd` : pas de dépendance à libc pour
/// une seule lecture, et un fichier que la session peut lire.
fn uid_of(user: &str) -> Option<u32> {
    id_in("/etc/passwd", user)
}

/// Résout un nom de groupe en identifiant, par `/etc/group`.
fn gid_of(group: &str) -> Option<u32> {
    id_in("/etc/group", group)
}

fn id_in(fichier: &str, nom: &str) -> Option<u32> {
    let contenu = std::fs::read_to_string(fichier).ok()?;
    contenu.lines().find_map(|line| {
        let mut champs = line.split(':');
        (champs.next()? == nom).then(|| champs.nth(1)?.parse().ok())?
    })
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let socket = std::env::var("PROPHET_SUP_SOCKET").unwrap_or_else(|_| DEFAULT_SOCKET.to_owned());
    let client = std::env::var("PROPHET_SUP_CLIENT").unwrap_or_else(|_| "agentd".to_owned());
    let mut allowed = Vec::new();
    match uid_of(&client) {
        Some(uid) => allowed.push(uid),
        None => tracing::warn!(compte = %client, "compte client inconnu : personne ne sera admis"),
    }
    if std::env::var("PROPHET_SUP_ALLOW_OWNER").as_deref() == Ok("1") {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata("/proc/self") {
            Ok(m) => allowed.push(m.uid()),
            Err(e) => tracing::warn!(erreur = %e, "identifiant du propriétaire illisible"),
        }
    }
    let server = match Server::bind(&socket) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(socket, erreur = %e, "socket impossible à ouvrir");
            std::process::exit(1);
        }
    };
    // Le socket appartient à la session, mais c'est le groupe système de Prophet qui doit
    // pouvoir s'y connecter : `agentd` en est membre, le reste de la machine non.
    let group = std::env::var("PROPHET_SUP_GROUP").unwrap_or_else(|_| "prophet-system".to_owned());
    match gid_of(&group) {
        Some(gid) => {
            if let Err(e) = std::os::unix::fs::chown(&socket, None, Some(gid)) {
                tracing::warn!(socket, groupe = %group, erreur = %e, "groupe du socket non posé");
            }
        }
        None => {
            tracing::warn!(groupe = %group, "groupe inconnu : le socket garde celui de la session")
        }
    }
    tracing::info!(socket, admis = ?allowed, "adaptateur d'accessibilité prêt");
    let adapter = Arc::new(Adapter {
        allowed,
        desktop: Mutex::new(None),
    });
    if let Err(e) = server.serve(adapter).await {
        tracing::error!(erreur = %e, "service interrompu");
        std::process::exit(1);
    }
}
