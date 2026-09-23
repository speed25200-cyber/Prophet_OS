//! La cage d'un client officiel lancé en mission (ADR 0056, M8-T4).
//!
//! Un client lancé par le lanceur ne voit plus la session de l'humain : ni sa maison, ni ses
//! documents, ni `/run/prophet` (capd, le journal, les autres services), ni son bus de session,
//! ni sway. Il voit le système en lecture seule, son profil privé (ses identifiants, que l'OS ne
//! lit jamais : il les lui monte), une maison, un temporaire et un répertoire de travail propres
//! à la mission, et un seul socket : celui du lanceur, qui ne laisse passer vers agentd que la
//! séance d'outils de **cette** mission. Ce qu'il fait dans la mission passe par cette séance,
//! donc par capd et par le journal ; ce qu'il écrirait ailleurs reste dans la cage et disparaît
//! avec elle.
//!
//! Le réseau, lui, reste celui de l'hôte : le client joint son éditeur directement. Le faire
//! passer par egress demande un relais et une politique des hôtes de chaque éditeur ; c'est la
//! phase suivante de l'ADR 0056.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use prophet_ipc::ErrorCode;
use sandboxd::spec::SandboxSpec;
use serde_json::{Value, json};

/// Variables de la session transmises au client : langue, fuseau, identité, autorités de
/// certification et proxy choisis par l'humain. Aucune autre ne passe — en particulier ni
/// `DBUS_SESSION_BUS_ADDRESS`, ni `SWAYSOCK`, ni `WAYLAND_DISPLAY`, ni `XDG_RUNTIME_DIR`.
pub const ENV_TRANSMIS: &[&str] = &[
    "PATH",
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_CTYPE",
    "LC_MESSAGES",
    "TZ",
    "USER",
    "LOGNAME",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "NIX_SSL_CERT_FILE",
    "NODE_EXTRA_CA_CERTS",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "NO_PROXY",
    "https_proxy",
    "http_proxy",
    "no_proxy",
];

/// Les méthodes d'agentd qu'un client en mission peut appeler : sa séance d'outils, rien d'autre.
pub const METHODES_DE_SEANCE: &[&str] = &["task.attach", "task.tools", "task.call", "task.detach"];

/// Variable qui désigne à la cage l'emplacement de sa racine minimale, pour que le lanceur la
/// retire après elle.
pub const RACINE_ENV: &str = "PROPHET_CAGE_RACINE";

/// Système visible en lecture seule, au-delà des montages par défaut des sandboxes : `/etc`
/// (résolveur, utilisateurs, profils du système) et le système courant de NixOS.
const SYSTEME: &[&str] = &["/etc", "/run/current-system"];

/// Les lieux d'une mission dans la cage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lieux {
    /// Racine des lieux de la mission, retirée après elle.
    pub base: PathBuf,
    /// Maison du client pour la mission (`HOME`).
    pub maison: PathBuf,
    /// Temporaire du client (`TMPDIR`).
    pub temporaire: PathBuf,
    /// Répertoire de travail du client.
    pub travail: PathBuf,
}

impl Lieux {
    /// Les lieux de `task` sous `racine` (`<racine>/missions/<task>`), sans les créer.
    ///
    /// # Errors
    /// Un identifiant de mission qui sortirait de la racine.
    pub fn de(racine: &Path, task: &str) -> Result<Self, String> {
        if task.is_empty()
            || task.contains('/')
            || task.contains('\0')
            || task == "."
            || task == ".."
        {
            return Err(format!("identifiant de mission invalide : {task:?}"));
        }
        let base = racine.join("missions").join(task);
        Ok(Self {
            maison: base.join("maison"),
            temporaire: base.join("tmp"),
            travail: base.join("travail"),
            base,
        })
    }

    /// Crée les lieux, en 0700, vides.
    ///
    /// # Errors
    /// Répertoire impossible à créer.
    pub fn creer(&self) -> std::io::Result<()> {
        use std::os::unix::fs::DirBuilderExt as _;
        let _ = std::fs::remove_dir_all(&self.base);
        for dir in [&self.maison, &self.temporaire, &self.travail] {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
        }
        Ok(())
    }

    /// Retire les lieux : ce que le client a écrit hors de sa séance disparaît avec la cage.
    pub fn retirer(&self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// Ce qu'il faut pour décrire une cage.
#[derive(Debug, Clone)]
pub struct Plan<'a> {
    /// Programme du client, chemin absolu résolu.
    pub programme: &'a Path,
    /// Arguments.
    pub args: &'a [String],
    /// Environnement propre à la mission (profil privé, mission, configuration MCP, socket).
    pub env: &'a [(String, String)],
    /// Profil privé du client, monté en écriture.
    pub profil: &'a Path,
    /// Lieux de la mission.
    pub lieux: &'a Lieux,
    /// Configuration MCP de la mission, montée en lecture seule.
    pub configuration: &'a Path,
    /// Socket filtré vers agentd.
    pub socket: &'a Path,
    /// Pont `prophet-mcp`, monté en lecture seule.
    pub pont: &'a Path,
    /// Chemins supplémentaires en lecture seule (un client installé hors du système).
    pub lecture_seule: &'a [PathBuf],
}

/// La description que la cage reçoit : ce que le client voit, ce qu'il peut écrire, ce qu'il
/// reçoit de la session.
#[must_use]
pub fn description(plan: &Plan<'_>, session: impl Fn(&str) -> Option<String>) -> SandboxSpec {
    // Le chemin réel du programme : un lien hors du système (`~/.nix-profile/bin/claude`)
    // n'existe pas dans la cage, sa cible si.
    let programme =
        std::fs::canonicalize(plan.programme).unwrap_or_else(|_| plan.programme.to_path_buf());
    let mut spec = SandboxSpec::new(
        0,
        programme.display().to_string(),
        plan.lieux.travail.display().to_string(),
    )
    .args(plan.args.iter().cloned());
    for cle in ENV_TRANSMIS {
        if let Some(valeur) = session(cle) {
            spec = spec.env(*cle, valeur);
        }
    }
    spec = spec
        .env("HOME", plan.lieux.maison.display().to_string())
        .env("TMPDIR", plan.lieux.temporaire.display().to_string())
        .env("TERM", session("TERM").unwrap_or_else(|| "dumb".to_owned()));
    for (cle, valeur) in plan.env {
        spec = spec.env(cle.clone(), valeur.clone());
    }
    let mut lecture: Vec<String> = spec.read_only_mounts.clone();
    for chemin in SYSTEME {
        if Path::new(chemin).exists() {
            lecture.push((*chemin).to_owned());
        }
    }
    let visible = |chemin: &Path, lecture: &[String]| lecture.iter().any(|m| chemin.starts_with(m));
    for chemin in [plan.programme, plan.pont, plan.configuration]
        .into_iter()
        .map(Path::to_path_buf)
        .chain(plan.lecture_seule.iter().cloned())
    {
        let chemin = std::fs::canonicalize(&chemin).unwrap_or(chemin);
        if !visible(&chemin, &lecture) {
            lecture.push(chemin.display().to_string());
        }
    }
    spec.read_only_mounts = lecture;
    spec.rules.paths = [
        plan.profil,
        &plan.lieux.maison,
        &plan.lieux.temporaire,
        &plan.lieux.travail,
    ]
    .into_iter()
    .map(|chemin| capd::enforce::PathRule {
        path: chemin.display().to_string(),
        read: true,
        write: true,
    })
    .collect();
    spec.mcp_sockets = vec![plan.socket.display().to_string()];
    spec
}

/// Vrai si un client en mission peut appeler `methode` avec `params` : sa séance d'outils, et
/// seulement pour la mission `task`. `ping` passe aussi.
///
/// # Errors
/// La raison du refus, telle que le client la lira.
pub fn admise(methode: &str, params: &Value, task: &str) -> Result<(), String> {
    if methode == "ping" {
        return Ok(());
    }
    if !METHODES_DE_SEANCE.contains(&methode) {
        return Err(format!(
            "{methode} n'est pas ouvert à un client en mission : seule sa séance d'outils l'est \
             (task.attach, task.tools, task.call, task.detach)"
        ));
    }
    if params.get("id").and_then(Value::as_str) != Some(task) {
        return Err(format!(
            "{methode} : ce client ne peut agir que dans la mission {task}"
        ));
    }
    Ok(())
}

/// Le socket filtré d'une mission : il écoute pour le client, ne relaie vers agentd que sa
/// séance d'outils, et se ferme avec la mission.
#[derive(Debug)]
pub struct Relais {
    chemin: PathBuf,
    arret: Arc<AtomicBool>,
    fil: Option<std::thread::JoinHandle<()>>,
}

impl Relais {
    /// Ouvre le socket `chemin` (en 0600), relié à `amont` pour la mission `task`.
    ///
    /// # Errors
    /// Socket impossible à ouvrir.
    pub fn ouvrir(chemin: &Path, amont: &Path, task: &str) -> std::io::Result<Self> {
        use std::os::unix::fs::PermissionsExt as _;
        if let Some(parent) = chemin.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _ = std::fs::remove_file(chemin);
        let ecoute = UnixListener::bind(chemin)?;
        std::fs::set_permissions(chemin, std::fs::Permissions::from_mode(0o600))?;
        ecoute.set_nonblocking(true)?;
        let arret = Arc::new(AtomicBool::new(false));
        let (fin, amont, task) = (arret.clone(), amont.to_path_buf(), task.to_owned());
        let fil = std::thread::Builder::new()
            .name(format!("relais-{task}"))
            .spawn(move || {
                while !fin.load(Ordering::Acquire) {
                    match ecoute.accept() {
                        Ok((flux, _)) => {
                            let (amont, task) = (amont.clone(), task.clone());
                            let _ = std::thread::Builder::new()
                                .name("relais-connexion".into())
                                .spawn(move || relayer(flux, &amont, &task));
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(20));
                        }
                        Err(_) => break,
                    }
                }
            })?;
        Ok(Self {
            chemin: chemin.to_path_buf(),
            arret,
            fil: Some(fil),
        })
    }
}

impl Drop for Relais {
    fn drop(&mut self) {
        self.arret.store(true, Ordering::Release);
        if let Some(fil) = self.fil.take() {
            let _ = fil.join();
        }
        let _ = std::fs::remove_file(&self.chemin);
    }
}

/// Relaie une connexion : une requête par ligne, une réponse par ligne, comme le codec d'IPC.
fn relayer(client: UnixStream, amont: &Path, task: &str) {
    let _ = client.set_nonblocking(false);
    let Ok(lecture) = client.try_clone() else {
        return;
    };
    let mut lecteur = BufReader::new(lecture);
    let mut client = client;
    let mut service: Option<(BufReader<UnixStream>, UnixStream)> = None;
    loop {
        let mut ligne = Vec::new();
        match (&mut lecteur)
            .take(prophet_ipc::MAX_MESSAGE_BYTES as u64 + 1)
            .read_until(b'\n', &mut ligne)
        {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if ligne.len() > prophet_ipc::MAX_MESSAGE_BYTES || !ligne.ends_with(b"\n") {
            let _ = repondre_erreur(
                &mut client,
                &Value::Null,
                ErrorCode::InvalidRequest,
                "requête trop grande",
            );
            return;
        }
        let Ok(requete) = serde_json::from_slice::<Value>(&ligne) else {
            let _ = repondre_erreur(
                &mut client,
                &Value::Null,
                ErrorCode::ParseError,
                "JSON illisible",
            );
            continue;
        };
        let id = requete.get("id").cloned().unwrap_or(Value::Null);
        let methode = requete.get("method").and_then(Value::as_str).unwrap_or("");
        let params = requete.get("params").cloned().unwrap_or(Value::Null);
        if let Err(raison) = admise(methode, &params, task) {
            tracing::warn!(mission = %task, %methode, "appel d'un client en mission refusé");
            if repondre_erreur(&mut client, &id, ErrorCode::PolicyDenied, &raison).is_err() {
                return;
            }
            continue;
        }
        if service.is_none() {
            let Ok(flux) = UnixStream::connect(amont) else {
                let _ = repondre_erreur(
                    &mut client,
                    &id,
                    ErrorCode::InternalError,
                    "agentd injoignable",
                );
                return;
            };
            let Ok(lecture) = flux.try_clone() else {
                return;
            };
            service = Some((BufReader::new(lecture), flux));
        }
        let Some((reponses, envoi)) = service.as_mut() else {
            return;
        };
        // agentd reçoit exactement ce qui a été vérifié, réécrit : une ligne qu'un autre
        // analyseur lirait autrement (clés en double) ne passe pas telle quelle.
        let Ok(mut verifiee) = serde_json::to_vec(&requete) else {
            return;
        };
        verifiee.push(b'\n');
        if envoi
            .write_all(&verifiee)
            .and_then(|()| envoi.flush())
            .is_err()
        {
            return;
        }
        let mut reponse = Vec::new();
        match reponses
            .take(prophet_ipc::MAX_MESSAGE_BYTES as u64 + 1)
            .read_until(b'\n', &mut reponse)
        {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if client
            .write_all(&reponse)
            .and_then(|()| client.flush())
            .is_err()
        {
            return;
        }
    }
}

fn repondre_erreur(
    client: &mut UnixStream,
    id: &Value,
    code: ErrorCode,
    message: &str,
) -> std::io::Result<()> {
    let mut ligne = serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": i32::from(code), "message": message},
    }))
    .unwrap_or_default();
    ligne.push(b'\n');
    client.write_all(&ligne)?;
    client.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seule_la_seance_de_la_mission_passe() {
        let seance = json!({"id": "m-1"});
        for methode in METHODES_DE_SEANCE {
            assert!(admise(methode, &seance, "m-1").is_ok(), "{methode}");
        }
        assert!(admise("ping", &Value::Null, "m-1").is_ok());
        assert!(
            admise("task.attach", &json!({"id": "autre"}), "m-1").is_err(),
            "une autre mission"
        );
        assert!(
            admise("task.call", &json!({}), "m-1").is_err(),
            "sans mission"
        );
        for methode in [
            "task.spawn",
            "task.list",
            "task.apply",
            "task.halt",
            "task.start",
            "approval.resolve",
        ] {
            assert!(admise(methode, &seance, "m-1").is_err(), "{methode}");
        }
    }

    #[test]
    fn un_identifiant_de_mission_ne_sort_pas_de_la_racine() {
        let racine = Path::new("/etat");
        assert!(Lieux::de(racine, "../x").is_err());
        assert!(Lieux::de(racine, "..").is_err());
        assert!(Lieux::de(racine, "").is_err());
        let lieux = Lieux::de(racine, "m-1").unwrap();
        assert_eq!(lieux.base, Path::new("/etat/missions/m-1"));
        assert!(lieux.maison.starts_with(&lieux.base));
    }

    #[test]
    fn la_cage_montre_le_systeme_le_profil_et_la_mission_sans_la_session() {
        let lieux = Lieux::de(Path::new("/etat"), "m-1").unwrap();
        let env = vec![(
            "CODEX_HOME".to_owned(),
            "/etat/providers/codex/u".to_owned(),
        )];
        let lecture = vec![PathBuf::from("/opt/clients")];
        let plan = Plan {
            programme: Path::new("/opt/clients/codex"),
            args: &["exec".to_owned()],
            env: &env,
            profil: Path::new("/etat/providers/codex/u"),
            lieux: &lieux,
            configuration: Path::new("/run/u/prophet-pilot/m-1.json"),
            socket: Path::new("/run/u/prophet-pilot/m-1.sock"),
            pont: Path::new("/nix/store/x-prophet/bin/prophet-mcp"),
            lecture_seule: &lecture,
        };
        let session = |cle: &str| match cle {
            "PATH" => Some("/run/current-system/sw/bin".to_owned()),
            "DBUS_SESSION_BUS_ADDRESS" => Some("unix:path=/run/user/1000/bus".to_owned()),
            "SWAYSOCK" => Some("/run/user/1000/sway.sock".to_owned()),
            _ => None,
        };
        let spec = description(&plan, session);
        let env: std::collections::BTreeMap<_, _> = spec.env.iter().cloned().collect();
        assert_eq!(env["HOME"], "/etat/missions/m-1/maison");
        assert_eq!(env["TMPDIR"], "/etat/missions/m-1/tmp");
        assert_eq!(env["CODEX_HOME"], "/etat/providers/codex/u");
        assert_eq!(env["PATH"], "/run/current-system/sw/bin");
        assert!(!env.contains_key("DBUS_SESSION_BUS_ADDRESS"));
        assert!(!env.contains_key("SWAYSOCK"));
        assert!(!env.contains_key("XDG_RUNTIME_DIR"));
        assert_eq!(spec.workdir, "/etat/missions/m-1/travail");
        let ecriture: Vec<_> = spec.rules.paths.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(
            ecriture,
            [
                "/etat/providers/codex/u",
                "/etat/missions/m-1/maison",
                "/etat/missions/m-1/tmp",
                "/etat/missions/m-1/travail"
            ]
        );
        assert!(spec.read_only_mounts.iter().any(|m| m == "/opt/clients"));
        assert!(
            spec.read_only_mounts
                .iter()
                .any(|m| m == "/run/u/prophet-pilot/m-1.json")
        );
        assert!(
            !spec.read_only_mounts.iter().any(|m| m.starts_with("/home")),
            "{:?}",
            spec.read_only_mounts
        );
        assert_eq!(spec.mcp_sockets, ["/run/u/prophet-pilot/m-1.sock"]);
    }
}
