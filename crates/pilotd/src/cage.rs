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

/// Variables de la session transmises au client : langue, fuseau, identité et autorités de
/// certification. Aucune autre ne passe — ni `DBUS_SESSION_BUS_ADDRESS`, ni `SWAYSOCK`, ni
/// `WAYLAND_DISPLAY`, ni `XDG_RUNTIME_DIR`, ni les proxys de la session : dans la cage, le seul
/// proxy est le relais vers egress.
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
];

/// Port du relais vers egress dans la cage, sur sa boucle locale : l'espace réseau est propre à
/// la cage, rien d'autre n'y écoute.
pub const PORT_DU_RELAIS: u16 = 3128;

/// Variables qui désignent le proxy au client, dans les deux casses que les clients lisent.
const PROXYS: &[&str] = &[
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "ALL_PROXY",
    "https_proxy",
    "http_proxy",
    "all_proxy",
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
    /// Socket du relais vers egress, s'il y a un jeton de sortie : la seule issue réseau de la
    /// cage. Sans lui, la cage n'a aucun réseau.
    pub sortie: Option<&'a Path>,
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
    if let Some(sortie) = plan.sortie {
        let proxy = format!("http://127.0.0.1:{PORT_DU_RELAIS}");
        for cle in PROXYS {
            spec = spec.env(*cle, proxy.clone());
        }
        spec.egress_socket = Some(sortie.display().to_string());
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

/// Un socket du lanceur pour une mission : il écoute pour le client, traite chaque connexion
/// dans son fil, et se ferme avec la mission.
pub struct Relais {
    chemin: PathBuf,
    arret: Arc<AtomicBool>,
    fil: Option<std::thread::JoinHandle<()>>,
}

impl std::fmt::Debug for Relais {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Relais")
            .field("chemin", &self.chemin)
            .finish_non_exhaustive()
    }
}

impl Relais {
    /// Le socket de séance (en 0600) : relié à agentd par `amont`, il ne laisse passer que la
    /// séance d'outils de la mission `task`.
    ///
    /// # Errors
    /// Socket impossible à ouvrir.
    pub fn ouvrir(chemin: &Path, amont: &Path, task: &str) -> std::io::Result<Self> {
        let (amont, task) = (amont.to_path_buf(), task.to_owned());
        Self::servir(chemin, move |flux| relayer(flux, &amont, &task))
    }

    /// Le socket de sortie (en 0600) : chaque requête de proxy du client reçoit l'en-tête du
    /// jeton de la mission, puis part vers egress par `egress`, qui demande à capd. Le jeton ne
    /// franchit jamais la cage.
    ///
    /// # Errors
    /// Socket impossible à ouvrir.
    pub fn sortie(chemin: &Path, egress: &Path, jeton: &str) -> std::io::Result<Self> {
        let (egress, jeton) = (egress.to_path_buf(), jeton.to_owned());
        Self::servir(chemin, move |flux| sortir(flux, &egress, &jeton))
    }

    fn servir(
        chemin: &Path,
        gestionnaire: impl Fn(UnixStream) + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        use std::os::unix::fs::PermissionsExt as _;
        if let Some(parent) = chemin.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _ = std::fs::remove_file(chemin);
        let ecoute = UnixListener::bind(chemin)?;
        std::fs::set_permissions(chemin, std::fs::Permissions::from_mode(0o600))?;
        ecoute.set_nonblocking(true)?;
        let arret = Arc::new(AtomicBool::new(false));
        let fin = arret.clone();
        let gestionnaire = Arc::new(gestionnaire);
        let fil = std::thread::Builder::new()
            .name("relais".into())
            .spawn(move || {
                while !fin.load(Ordering::Acquire) {
                    match ecoute.accept() {
                        Ok((flux, _)) => {
                            let gestionnaire = gestionnaire.clone();
                            let _ = std::thread::Builder::new()
                                .name("relais-connexion".into())
                                .spawn(move || gestionnaire(flux));
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

/// Taille maximale de la tête d'une requête de proxy, comme egress l'accepte.
const TETE_MAX: usize = 64 * 1024;

/// Relaie une connexion de proxy vers egress : la tête reçoit l'en-tête du jeton (celui qu'aurait
/// posé le client est retiré), puis les deux sens sont recopiés tels quels — un tunnel `CONNECT`
/// reste chiffré de bout en bout.
fn sortir(client: UnixStream, egress: &Path, jeton: &str) {
    let _ = client.set_nonblocking(false);
    let Ok(lecture) = client.try_clone() else {
        return;
    };
    let mut lecteur = BufReader::new(lecture);
    let mut tete = Vec::new();
    let mut premiere = true;
    loop {
        let mut ligne = Vec::new();
        match (&mut lecteur)
            .take((TETE_MAX + 1) as u64)
            .read_until(b'\n', &mut ligne)
        {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if tete.len() + ligne.len() > TETE_MAX {
            return;
        }
        let fin = ligne == b"\r\n" || ligne == b"\n";
        // Le nom d'en-tête, sans espaces ni casse : `Proxy-Authorization :` ou une ligne de
        // continuation ne font pas passer un jeton du client.
        let texte = String::from_utf8_lossy(&ligne);
        let jeton_du_client = texte
            .split_once(':')
            .is_some_and(|(nom, _)| nom.trim().eq_ignore_ascii_case("proxy-authorization"));
        if !jeton_du_client {
            tete.extend_from_slice(&ligne);
        }
        if std::mem::take(&mut premiere) {
            tete.extend_from_slice(format!("Proxy-Authorization: Prophet {jeton}\r\n").as_bytes());
        }
        if fin {
            break;
        }
    }
    let Ok(mut amont) = UnixStream::connect(egress) else {
        let mut client = client;
        let _ = client
            .write_all(b"HTTP/1.1 503 Prophet\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return;
    };
    if amont.write_all(&tete).is_err() {
        return;
    }
    let (Ok(mut amont_lecture), Ok(mut client_ecriture)) = (amont.try_clone(), client.try_clone())
    else {
        return;
    };
    let descendant = std::thread::spawn(move || {
        let _ = std::io::copy(&mut amont_lecture, &mut client_ecriture);
        let _ = client_ecriture.shutdown(std::net::Shutdown::Write);
    });
    // Ce que le lecteur a déjà lu au-delà de la tête part d'abord, puis le reste du flux.
    let _ = std::io::copy(&mut lecteur, &mut amont);
    let _ = amont.shutdown(std::net::Shutdown::Write);
    let _ = descendant.join();
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
    fn la_sortie_porte_le_jeton_de_la_mission_et_jamais_celui_du_client() {
        use std::io::{BufRead as _, Read as _, Write as _};
        let dir = tempfile::tempdir().unwrap();
        let egress = dir.path().join("egress.sock");
        let ecoute = UnixListener::bind(&egress).unwrap();
        let serveur = std::thread::spawn(move || {
            let (flux, _) = ecoute.accept().unwrap();
            let mut lecteur = BufReader::new(flux.try_clone().unwrap());
            let mut tete = String::new();
            loop {
                let mut ligne = String::new();
                if lecteur.read_line(&mut ligne).unwrap() == 0 || ligne == "\r\n" {
                    break;
                }
                tete.push_str(&ligne);
            }
            let mut flux = flux;
            flux.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .unwrap();
            tete
        });
        let sortie = dir.path().join("sortie.sock");
        let _relais = Relais::sortie(&sortie, &egress, "JETON").unwrap();
        let mut client = UnixStream::connect(&sortie).unwrap();
        client
            .write_all(
                b"CONNECT api.exemple.fr:443 HTTP/1.1\r\nHost: api.exemple.fr\r\n\
                  Proxy-Authorization: Prophet VOLE\r\nproxy-authorization : Prophet VOLE2\r\n\r\n",
            )
            .unwrap();
        let mut reponse = String::new();
        client.read_to_string(&mut reponse).unwrap();
        assert!(reponse.ends_with("ok"), "{reponse}");
        let tete = serveur.join().unwrap();
        assert!(
            tete.starts_with("CONNECT api.exemple.fr:443 HTTP/1.1\r\n"),
            "{tete}"
        );
        assert!(
            tete.contains("Proxy-Authorization: Prophet JETON\r\n"),
            "{tete}"
        );
        assert!(!tete.contains("VOLE"), "{tete}");
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
            sortie: None,
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
