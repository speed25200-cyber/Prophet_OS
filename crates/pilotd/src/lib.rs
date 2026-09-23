//! `pilotd` : les clients officiels de l'humain (Claude Code, Codex, Gemini) comme rôles d'un
//! relais de modèles, lancés dans sa session (ADR 0035).
//!
//! Le service `agentd` ne peut pas lancer ces clients : leurs identifiants appartiennent à
//! l'humain, dans le répertoire privé de chaque client, et l'OS ne les lit jamais. Ce lanceur
//! tourne donc **dans la session de l'humain**, sous son identité, comme le lanceur du bureau
//! qui ouvre « Claude Code · mission ». Il n'admet qu'`agentd` (`SO_PEERCRED`), ne décide
//! d'aucun droit, et fait une seule chose : lancer un client officiel, **sans modification**,
//! en mode non interactif, avec la configuration MCP qui le raccorde à la séance d'outils de
//! la mission (ADR 0026), puis rendre ce que le client a répondu.
//!
//! Ce que le client fait dans la mission passe par le pont `prophet-mcp`, donc par le jeton
//! délégué par capd et par le journal. Le client tourne dans une cage (ADR 0056, [`cage`]) : il
//! ne voit ni la maison de l'humain, ni `/run/prophet`, ni sa session, et ne joint agentd que
//! pour la séance de sa mission. Le réseau de l'hôte lui reste, en attendant qu'egress le porte.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use providers::official::{ClientProfile, ConnectionState, OfficialDriver};
use serde::{Deserialize, Serialize};

pub mod cage;

/// Socket par défaut, dans le répertoire des services : agentd en est membre par le groupe
/// système, le reste de la machine non.
pub const DEFAULT_SOCKET: &str = "/run/prophet/pilot.sock";
/// Méthode : l'état des clients (installés, connectés).
pub const METHOD_STATUS: &str = "pilot.status";
/// Méthode : lancer un client dans une mission et attendre sa fin.
pub const METHOD_RUN: &str = "pilot.run";
/// Méthode : tuer sur-le-champ le client lancé pour une mission (`{task}`), parce que l'humain
/// l'annule. Rend `{task, stopped}` ; `stopped` dit si un client tournait pour cette mission.
pub const METHOD_STOP: &str = "pilot.stop";
/// Ce qu'on garde de la sortie du client, au plus : le reste n'est ni lu ni journalisé.
const MAX_OUTPUT_BYTES: usize = 1 << 20;
/// Ce qu'on rend au parent, au plus.
const MAX_TEXT_CHARS: usize = 16_384;

/// Erreurs du lanceur.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Pilote inconnu du lanceur.
    #[error("pilote inconnu : {0}")]
    UnknownDriver(String),
    /// Le client n'est pas installé ou pas connecté.
    #[error("{driver} : {state}")]
    NotReady {
        /// Pilote visé.
        driver: String,
        /// État constaté.
        state: String,
    },
    /// Le client n'a pas pu être lancé ou lu.
    #[error("lancement de {driver} : {source}")]
    Launch {
        /// Pilote visé.
        driver: String,
        /// Cause.
        source: std::io::Error,
    },
    /// Le client a dépassé la durée accordée.
    #[error("{driver} interrompu après {seconds} s")]
    Timeout {
        /// Pilote visé.
        driver: String,
        /// Durée accordée.
        seconds: u64,
    },
    /// Le client a été tué à la demande (`pilot.stop`).
    #[error("{driver} arrêté à la demande")]
    Stopped {
        /// Pilote visé.
        driver: String,
    },
    /// Requête invalide.
    #[error("{0}")]
    Invalid(String),
    /// La cage du client n'a pas pu se poser : le client n'a pas été lancé (ADR 0056).
    #[error("cage de {driver} impossible : {detail} ; le client n'est pas lancé sans elle")]
    Cage {
        /// Pilote visé.
        driver: String,
        /// Ce qui manque.
        detail: String,
    },
}

/// Ce que le lanceur sait d'un client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverState {
    /// Nom du pilote (`claude-code`, `codex`, `gemini`).
    pub driver: String,
    /// `connected`, `login_required`, `missing`, `probe_failed`, `unknown` ou `simulated`.
    pub connection: String,
    /// Exécutable résolu, s'il y en a un.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    /// Version annoncée par le client, s'il répond.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl DriverState {
    /// Le pilote peut être lancé dans une mission.
    #[must_use]
    pub fn ready(&self) -> bool {
        matches!(self.connection.as_str(), "connected" | "simulated")
    }
}

/// Requête de `pilot.stop`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopRequest {
    /// Mission dont le client doit être tué.
    pub task: String,
}

/// Les arrêts demandés et les clients en cours, partagés entre les appels du lanceur : un
/// `pilot.stop` reçu pendant un `pilot.run` fait tuer le client de cette mission.
#[derive(Debug, Clone, Default)]
pub struct Arrets {
    demandes: Arc<Mutex<BTreeSet<String>>>,
    en_cours: Arc<Mutex<BTreeSet<String>>>,
}

impl Arrets {
    fn demander(&self, task: &str) -> bool {
        let en_cours = self
            .en_cours
            .lock()
            .map(|e| e.contains(task))
            .unwrap_or(false);
        if let Ok(mut demandes) = self.demandes.lock() {
            demandes.insert(task.to_owned());
        }
        en_cours
    }

    fn demande(&self, task: &str) -> bool {
        self.demandes
            .lock()
            .map(|mut d| d.remove(task))
            .unwrap_or(false)
    }

    fn commence(&self, task: &str) {
        if let Ok(mut e) = self.en_cours.lock() {
            e.insert(task.to_owned());
        }
    }

    fn finit(&self, task: &str) {
        if let Ok(mut e) = self.en_cours.lock() {
            e.remove(task);
        }
        if let Ok(mut d) = self.demandes.lock() {
            d.remove(task);
        }
    }
}

/// Réponse de `pilot.status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Status {
    /// Un état par pilote connu.
    pub drivers: Vec<DriverState>,
}

/// Requête de `pilot.run`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRequest {
    /// Mission préparée pour ce client (séance à rejoindre par le pont).
    pub task: String,
    /// Pilote à lancer.
    pub driver: String,
    /// Objectif remis au client.
    pub intent: String,
    /// Durée accordée, en secondes ; au-delà, le client est tué.
    pub wall_time_s: u64,
    /// Palier de modèle demandé au client (`opus`, `haiku`, un identifiant que son option
    /// `--model` accepte), passé tel quel ; sans lui, le client prend son modèle par défaut
    /// (ADR 0040).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Réponse de `pilot.run`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunResult {
    /// Code de sortie du client, `None` s'il a été tué.
    pub exit_code: Option<i32>,
    /// Réponse finale du client, bornée ; pour Claude Code, le champ `result` de sa sortie.
    pub text: String,
    /// Durée réelle.
    pub duration_ms: u64,
    /// Octets lus sur la sortie standard, avant troncature.
    pub output_bytes: usize,
}

/// Un client de remplacement, pour les essais : programme et arguments (`{intent}` remplacé),
/// reçoit le même environnement qu'un vrai client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Override {
    /// Programme à lancer.
    pub program: String,
    /// Arguments, `{intent}` étant remplacé par l'objectif.
    #[serde(default)]
    pub args: Vec<String>,
}

/// La commande qui lance un client dans une mission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCommand {
    /// Programme.
    pub program: String,
    /// Arguments.
    pub args: Vec<String>,
    /// Variables d'environnement ajoutées à celles de la session.
    pub env: Vec<(String, String)>,
}

/// Le lanceur : où sont les profils privés des clients, le socket d'agentd que le pont doit
/// joindre, le pont lui-même, et d'éventuels clients de remplacement pour les essais.
#[derive(Debug, Clone)]
pub struct Launcher {
    /// Racine des profils privés (`<root>/providers/<pilote>/<utilisateur>`).
    pub root: PathBuf,
    /// Utilisateur de la session, nom du sous-répertoire de profil.
    pub user: String,
    /// Socket d'agentd que le pont joindra.
    pub agentd_socket: PathBuf,
    /// Le pont `prophet-mcp`.
    pub bridge: PathBuf,
    /// Où écrire les configurations MCP, une par mission, en 0600.
    pub runtime_dir: PathBuf,
    /// Clients de remplacement, par pilote (essais).
    pub overrides: BTreeMap<String, Override>,
    /// Arrêts demandés et clients en cours, partagés entre les appels.
    pub arrets: Arrets,
    /// La cage (`prophet-pilot-cage`) où chaque client est lancé (ADR 0056).
    pub cage: PathBuf,
    /// Chemins supplémentaires visibles, en lecture seule, dans la cage : un client installé
    /// hors du système, ou les binaires d'un essai.
    pub lecture_seule: Vec<PathBuf>,
}

impl Launcher {
    /// Demande l'arrêt du client lancé pour `task` : le `pilot.run` en cours le tue avec tout
    /// son groupe de processus et rend `Error::Stopped`. Vrai si un client tournait pour cette
    /// mission ; sinon la demande est retenue jusqu'à ce qu'un lancement la consomme.
    pub fn stop(&self, task: &str) -> bool {
        self.arrets.demander(task)
    }

    /// Lit les clients de remplacement d'une variable JSON (`{"codex": {"program": …}}`).
    ///
    /// # Errors
    /// JSON illisible.
    pub fn parse_overrides(json: &str) -> Result<BTreeMap<String, Override>, Error> {
        serde_json::from_str(json)
            .map_err(|e| Error::Invalid(format!("clients de remplacement : {e}")))
    }

    /// L'état d'un pilote, sondé par la commande du client (jusqu'à cinq secondes par sonde),
    /// sans lire ses fichiers ; `None` pour un pilote inconnu.
    #[must_use]
    pub fn driver_state(&self, driver: &str) -> Option<DriverState> {
        let profile = ClientProfile::all()
            .into_iter()
            .find(|p| p.driver == driver)?;
        if let Some(over) = self.overrides.get(&profile.driver) {
            return Some(DriverState {
                driver: profile.driver,
                connection: "simulated".into(),
                executable: Some(over.program.clone()),
                version: None,
            });
        }
        let diagnostic = OfficialDriver::new(profile.clone(), &self.root, &self.user).diagnostic();
        Some(DriverState {
            driver: profile.driver,
            connection: connection_name(diagnostic.connection).into(),
            executable: diagnostic.executable.map(|p| p.display().to_string()),
            version: diagnostic.version,
        })
    }

    /// L'état de chaque pilote connu, sondé par sa propre commande, sans lire ses fichiers.
    /// Les sondes coûtent jusqu'à quinze secondes en tout : le service les garde en cache.
    #[must_use]
    pub fn status(&self) -> Status {
        let drivers = ClientProfile::all()
            .into_iter()
            .filter_map(|profile| self.driver_state(&profile.driver))
            .collect();
        Status { drivers }
    }

    /// La configuration MCP qui raccorde un client à la séance de `task` par `socket` (le
    /// socket filtré de la mission), écrite en 0600.
    ///
    /// # Errors
    /// Répertoire ou fichier impossible à écrire.
    pub fn write_mcp_config(&self, task: &str, socket: &Path) -> Result<PathBuf, std::io::Error> {
        std::fs::create_dir_all(&self.runtime_dir)?;
        let path = self.runtime_dir.join(format!("{task}.json"));
        let config = serde_json::json!({
            "mcpServers": {
                "prophet": {
                    "command": self.bridge.display().to_string(),
                    "args": [],
                    "env": {
                        "PROPHET_TASK": task,
                        "PROPHET_AGENTD_SOCKET": socket.display().to_string()
                    }
                }
            }
        });
        write_private(&path, serde_json::to_string_pretty(&config)?.as_bytes())?;
        Ok(path)
    }

    /// La commande d'un pilote pour une mission : programme, arguments, environnement.
    ///
    /// # Errors
    /// Pilote inconnu.
    pub fn command(
        &self,
        request: &RunRequest,
        mcp_config: &Path,
        socket: &Path,
    ) -> Result<ClientCommand, Error> {
        let profile = ClientProfile::all()
            .into_iter()
            .find(|p| p.driver == request.driver)
            .ok_or_else(|| Error::UnknownDriver(request.driver.clone()))?;
        let config = mcp_config.display().to_string();
        let env = vec![
            (
                profile.config_home_env.clone(),
                providers::official::private_config_dir(&self.root, &profile.driver, &self.user)
                    .display()
                    .to_string(),
            ),
            ("PROPHET_TASK".to_owned(), request.task.clone()),
            (
                "PROPHET_AGENTD_SOCKET".to_owned(),
                socket.display().to_string(),
            ),
            ("PROPHET_MCP_CONFIG".to_owned(), config.clone()),
        ];
        if let Some(over) = self.overrides.get(&request.driver) {
            let mut args: Vec<String> = over
                .args
                .iter()
                .map(|a| a.replace("{intent}", &request.intent))
                .collect();
            // Le client de remplacement reçoit le palier comme le vrai, pour que les essais
            // le voient.
            if let Some(model) = &request.model {
                args.extend(["--model".to_owned(), model.clone()]);
            }
            return Ok(ClientCommand {
                program: over.program.clone(),
                args,
                env,
            });
        }
        let mut args = profile.command_line(&request.intent, &config, None);
        // Le palier de modèle, par l'option de chaque client : `--model` pour Claude Code,
        // avant le séparateur ; `-m` pour Codex, après `exec` ; `-m` pour Gemini (ADR 0040).
        if let Some(model) = &request.model {
            match profile.driver.as_str() {
                "claude-code" => {
                    let pos = args.iter().position(|a| a == "--").unwrap_or(args.len());
                    args.splice(pos..pos, ["--model".to_owned(), model.clone()]);
                }
                "codex" => {
                    args.splice(1..1, ["-m".to_owned(), model.clone()]);
                }
                _ => {
                    args.splice(0..0, ["-m".to_owned(), model.clone()]);
                }
            }
        }
        if profile.driver == "codex" {
            // Codex ne prend pas de fichier MCP en argument : ses serveurs viennent de sa
            // configuration, que `-c` sait surcharger pour cette seule exécution.
            args.splice(
                1..1,
                [
                    "-c".to_owned(),
                    format!(
                        "mcp_servers.prophet.command={}",
                        toml_string(&self.bridge.display().to_string())
                    ),
                    "-c".to_owned(),
                    format!(
                        "mcp_servers.prophet.env={{PROPHET_TASK={},PROPHET_AGENTD_SOCKET={}}}",
                        toml_string(&request.task),
                        toml_string(&socket.display().to_string())
                    ),
                ],
            );
        }
        Ok(ClientCommand {
            program: profile.program,
            args,
            env,
        })
    }

    /// Lance le client dans la mission et attend sa fin, au plus `wall_time_s` secondes.
    ///
    /// # Errors
    /// Pilote inconnu, client absent ou non connecté, lancement impossible, délai dépassé.
    pub fn run(&self, request: &RunRequest) -> Result<RunResult, Error> {
        if request.task.trim().is_empty() || request.intent.trim().is_empty() {
            return Err(Error::Invalid("task et intent sont requis".into()));
        }
        if request.wall_time_s == 0 {
            return Err(Error::Invalid("wall_time_s doit être positif".into()));
        }
        let state = self
            .driver_state(&request.driver)
            .ok_or_else(|| Error::UnknownDriver(request.driver.clone()))?;
        if !state.ready() {
            return Err(Error::NotReady {
                driver: request.driver.clone(),
                state: state.connection,
            });
        }
        let cage_impossible = |detail: String| Error::Cage {
            driver: request.driver.clone(),
            detail,
        };
        if !self.cage.is_file() {
            return Err(cage_impossible(format!(
                "{} introuvable",
                self.cage.display()
            )));
        }
        let lieux = cage::Lieux::de(&self.root, &request.task).map_err(Error::Invalid)?;
        let programme = state
            .executable
            .as_deref()
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or_else(|| Error::NotReady {
                driver: request.driver.clone(),
                state: "exécutable introuvable".into(),
            })?;
        let lancement = |source| Error::Launch {
            driver: request.driver.clone(),
            source,
        };
        // Le socket filtré de la mission : la seule porte vers agentd que le client verra.
        let socket = self.runtime_dir.join(format!("{}.sock", request.task));
        let mcp_config = self
            .write_mcp_config(&request.task, &socket)
            .map_err(lancement)?;
        let racine = self.runtime_dir.join(format!("{}.racine", request.task));
        let mut menage = Menage {
            configuration: mcp_config.clone(),
            relais: None,
            lieux: lieux.clone(),
            racine: racine.clone(),
        };
        let ClientCommand { args, env, .. } = self.command(request, &mcp_config, &socket)?;
        lieux.creer().map_err(lancement)?;
        menage.relais = Some(
            cage::Relais::ouvrir(&socket, &self.agentd_socket, &request.task).map_err(lancement)?,
        );
        let profile = ClientProfile::all()
            .into_iter()
            .find(|p| p.driver == request.driver)
            .ok_or_else(|| Error::UnknownDriver(request.driver.clone()))?;
        let profil =
            providers::official::private_config_dir(&self.root, &profile.driver, &self.user);
        let spec = cage::description(
            &cage::Plan {
                programme: &programme,
                args: &args,
                env: &env,
                profil: &profil,
                lieux: &lieux,
                configuration: &mcp_config,
                socket: &socket,
                pont: &self.bridge,
                lecture_seule: &self.lecture_seule,
            },
            |cle| std::env::var(cle).ok(),
        );
        let spec = serde_json::to_string(&spec)
            .map_err(|e| cage_impossible(format!("description illisible : {e}")))?;
        let started = Instant::now();
        // La cage mène son propre groupe de processus : la tuer au délai ou à la demande tue
        // aussi le client et ce qu'il a lancé, sans quoi un sous-processus garderait la sortie
        // ouverte. Rien de la session ne passe : la description dit tout ce que le client reçoit.
        let mut child = Command::new(&self.cage)
            .env_clear()
            .env(sandboxd::spec::SPEC_ENV, spec)
            .env(cage::RACINE_ENV, &racine)
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .process_group(0)
            .spawn()
            .map_err(lancement)?;
        self.arrets.commence(&request.task);
        let mut stdout = child.stdout.take().expect("sortie standard demandée");
        let reader = std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let mut total = 0usize;
            let mut chunk = [0u8; 8192];
            loop {
                match stdout.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        total += n;
                        if buffer.len() < MAX_OUTPUT_BYTES {
                            let keep = n.min(MAX_OUTPUT_BYTES - buffer.len());
                            buffer.extend_from_slice(&chunk[..keep]);
                        }
                    }
                }
            }
            (buffer, total)
        });
        let deadline = started + Duration::from_secs(request.wall_time_s);
        let tuer = |child: &mut std::process::Child| {
            tuer_le_groupe(child);
            let _ = child.wait();
        };
        let exit_code = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.code(),
                Ok(None) if self.arrets.demande(&request.task) => {
                    tuer(&mut child);
                    let _ = reader.join();
                    let _ = std::fs::remove_file(&mcp_config);
                    self.arrets.finit(&request.task);
                    return Err(Error::Stopped {
                        driver: request.driver.clone(),
                    });
                }
                Ok(None) if Instant::now() >= deadline => {
                    tuer(&mut child);
                    let _ = reader.join();
                    let _ = std::fs::remove_file(&mcp_config);
                    self.arrets.finit(&request.task);
                    return Err(Error::Timeout {
                        driver: request.driver.clone(),
                        seconds: request.wall_time_s,
                    });
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(source) => {
                    tuer(&mut child);
                    let _ = reader.join();
                    let _ = std::fs::remove_file(&mcp_config);
                    self.arrets.finit(&request.task);
                    return Err(Error::Launch {
                        driver: request.driver.clone(),
                        source,
                    });
                }
            }
        };
        let (output, output_bytes) = reader.join().unwrap_or_default();
        let _ = std::fs::remove_file(&mcp_config);
        self.arrets.finit(&request.task);
        drop(menage);
        if exit_code == Some(CAGE_IMPOSSIBLE) && output_bytes == 0 {
            return Err(cage_impossible(
                "la cage n'a pas pu se poser (espaces de noms ou montages refusés, voir le journal)"
                    .into(),
            ));
        }
        Ok(RunResult {
            exit_code,
            text: final_text(&request.driver, &output),
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            output_bytes,
        })
    }
}

/// Code de sortie de la cage quand elle n'a pas pu se poser (`prophet-pilot-cage`).
const CAGE_IMPOSSIBLE: i32 = 125;

/// Ce qu'une mission laisse derrière elle, retiré quoi qu'il arrive : la configuration MCP, le
/// socket filtré, les lieux de la cage.
struct Menage {
    configuration: PathBuf,
    relais: Option<cage::Relais>,
    lieux: cage::Lieux,
    /// Point de montage de la racine minimale, vide une fois la cage partie.
    racine: PathBuf,
}

impl Drop for Menage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.configuration);
        self.relais.take();
        self.lieux.retirer();
        let _ = std::fs::remove_dir(&self.racine);
    }
}

/// Tue le client et tout son groupe de processus (il en est le meneur, voir `process_group`),
/// puis le client seul si le groupe n'a pas pu l'être.
fn tuer_le_groupe(child: &mut std::process::Child) {
    if let Ok(pid) = libc::pid_t::try_from(child.id()) {
        // SAFETY: `killpg` n'a pas d'effet mémoire ; le groupe visé est celui que ce processus
        // a créé pour son enfant (`process_group(0)`), identifié par l'identifiant de l'enfant
        // que `Child` tient encore : aucun autre groupe ne peut porter ce numéro tant que
        // l'enfant n'a pas été attendu.
        let _ = unsafe { libc::killpg(pid, libc::SIGKILL) };
    }
    let _ = child.kill();
}

/// Le dernier état sondé des clients, partagé entre le service et le fil qui le rafraîchit :
/// `pilot.status` répond sans attendre les sondes, qui coûtent jusqu'à cinq secondes par client
/// quand le réseau est coupé.
#[derive(Debug, Clone, Default)]
pub struct StatusCache {
    inner: std::sync::Arc<std::sync::Mutex<Option<(Instant, Status)>>>,
}

impl StatusCache {
    /// Le dernier état connu, s'il y en a un, et son âge.
    #[must_use]
    pub fn get(&self) -> Option<(Status, Duration)> {
        self.inner
            .lock()
            .ok()?
            .as_ref()
            .map(|(at, status)| (status.clone(), at.elapsed()))
    }

    /// Sonde maintenant, mémorise, et rend l'état.
    pub fn refresh(&self, launcher: &Launcher) -> Status {
        let status = launcher.status();
        if let Ok(mut guard) = self.inner.lock() {
            *guard = Some((Instant::now(), status.clone()));
        }
        status
    }

    /// L'état connu s'il a moins de `ttl`, sinon une sonde neuve.
    pub fn get_or_refresh(&self, launcher: &Launcher, ttl: Duration) -> Status {
        match self.get() {
            Some((status, age)) if age <= ttl => status,
            _ => self.refresh(launcher),
        }
    }
}

/// La réponse finale d'un client, d'après sa sortie : le champ `result` du dernier événement
/// `result` de Claude Code, le dernier `agent_message` de Codex, sinon la dernière ligne.
#[must_use]
pub fn final_text(driver: &str, output: &[u8]) -> String {
    let text = String::from_utf8_lossy(output);
    let mut found: Option<String> = None;
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
            continue;
        };
        let candidate = match driver {
            "claude-code" if value["type"] == "result" => value["result"].as_str(),
            "codex" => value["item"]["text"]
                .as_str()
                .filter(|_| value["item"]["type"] == "agent_message")
                .or_else(|| value["msg"]["message"].as_str()),
            _ => None,
        };
        if let Some(candidate) = candidate.filter(|c| !c.trim().is_empty()) {
            found = Some(candidate.to_owned());
        }
    }
    let chosen = found.unwrap_or_else(|| {
        text.lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or_default()
            .to_owned()
    });
    chosen.chars().take(MAX_TEXT_CHARS).collect()
}

fn connection_name(state: ConnectionState) -> &'static str {
    match state {
        ConnectionState::Connected => "connected",
        ConnectionState::LoginRequired => "login_required",
        ConnectionState::ClientMissing => "missing",
        ConnectionState::ProbeFailed => "probe_failed",
        _ => "unknown",
    }
}

fn toml_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    use std::io::Write as _;
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt as _;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?
    };
    #[cfg(not(unix))]
    let mut file = std::fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launcher(dir: &Path) -> Launcher {
        Launcher {
            root: dir.join("state"),
            user: "humain".into(),
            agentd_socket: dir.join("agent.sock"),
            bridge: dir.join("prophet-mcp"),
            runtime_dir: dir.join("run"),
            overrides: BTreeMap::new(),
            arrets: Arrets::default(),
            cage: dir.join("prophet-pilot-cage"),
            lecture_seule: Vec::new(),
        }
    }

    #[test]
    fn la_configuration_mcp_nomme_le_pont_la_mission_et_le_socket() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("run/m-1.sock");
        let path = launcher(dir.path())
            .write_mcp_config("m-1", &socket)
            .unwrap();
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            config["mcpServers"]["prophet"]["env"]["PROPHET_TASK"],
            "m-1"
        );
        assert!(
            config["mcpServers"]["prophet"]["command"]
                .as_str()
                .unwrap()
                .ends_with("prophet-mcp")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn la_commande_de_chaque_client_porte_la_mission_et_le_profil_prive() {
        let dir = tempfile::tempdir().unwrap();
        let launcher = launcher(dir.path());
        let config = dir.path().join("run/m.json");
        let socket = dir.path().join("run/m.sock");
        let claude = launcher
            .command(
                &RunRequest {
                    task: "m".into(),
                    driver: "claude-code".into(),
                    intent: "Écris".into(),
                    wall_time_s: 10,
                    model: None,
                },
                &config,
                &socket,
            )
            .unwrap();
        assert_eq!(claude.program, "claude");
        assert!(claude.args.iter().any(|a| a.starts_with("--mcp-config=")));
        assert_eq!(claude.args.last().unwrap(), "Écris");
        assert!(
            claude
                .env
                .iter()
                .any(|(k, v)| k == "CLAUDE_CONFIG_DIR"
                    && v.ends_with("providers/claude-code/humain"))
        );
        assert!(
            claude
                .env
                .iter()
                .any(|(k, v)| k == "PROPHET_TASK" && v == "m")
        );
        let codex = launcher
            .command(
                &RunRequest {
                    task: "m".into(),
                    driver: "codex".into(),
                    intent: "Code".into(),
                    wall_time_s: 10,
                    model: None,
                },
                &config,
                &socket,
            )
            .unwrap();
        assert_eq!(codex.program, "codex");
        assert_eq!(codex.args[0], "exec");
        assert!(
            codex
                .args
                .iter()
                .any(|a| a.starts_with("mcp_servers.prophet.command="))
        );
        assert!(codex.env.iter().any(|(k, _)| k == "CODEX_HOME"));
        assert!(matches!(
            launcher.command(
                &RunRequest {
                    task: "m".into(),
                    driver: "muse".into(),
                    intent: "?".into(),
                    wall_time_s: 1,
                    model: None,
                },
                &config,
                &socket
            ),
            Err(Error::UnknownDriver(_))
        ));
    }

    #[test]
    fn un_palier_de_modele_est_passe_au_client() {
        let dir = tempfile::tempdir().unwrap();
        let mut launcher = launcher(dir.path());
        let config = dir.path().join("run/m.json");
        let socket = dir.path().join("run/m.sock");
        let requete = |driver: &str| RunRequest {
            task: "m".into(),
            driver: driver.into(),
            intent: "Écris".into(),
            wall_time_s: 10,
            model: Some("opus".into()),
        };
        let claude = launcher
            .command(&requete("claude-code"), &config, &socket)
            .unwrap();
        let separateur = claude.args.iter().position(|a| a == "--").unwrap();
        let option = claude.args.iter().position(|a| a == "--model").unwrap();
        assert!(
            option < separateur && claude.args[option + 1] == "opus",
            "{:?}",
            claude.args
        );
        let codex = launcher
            .command(&requete("codex"), &config, &socket)
            .unwrap();
        let option = codex.args.iter().position(|a| a == "-m").unwrap();
        assert!(
            option > 0 && codex.args[option + 1] == "opus",
            "{:?}",
            codex.args
        );
        launcher.overrides = Launcher::parse_overrides(
            r#"{"codex": {"program": "/bin/echo", "args": ["{intent}"]}}"#,
        )
        .unwrap();
        let faux = launcher
            .command(&requete("codex"), &config, &socket)
            .unwrap();
        assert_eq!(faux.args, ["Écris", "--model", "opus"]);
    }

    #[test]
    fn le_cache_d_etat_sert_le_dernier_sondage_puis_le_renouvelle() {
        let dir = tempfile::tempdir().unwrap();
        let mut launcher = launcher(dir.path());
        launcher.overrides = Launcher::parse_overrides(
            r#"{"codex": {"program": "/bin/echo", "args": ["{intent}"]}}"#,
        )
        .unwrap();
        let cache = StatusCache::default();
        assert!(
            cache.get().is_none(),
            "rien n'est connu avant la première sonde"
        );
        let premier = cache.get_or_refresh(&launcher, Duration::from_secs(60));
        assert_eq!(premier.drivers[1].connection, "simulated");
        let (connu, age) = cache.get().expect("la sonde est mémorisée");
        assert_eq!(connu, premier);
        assert!(age < Duration::from_secs(60));
        // Tant que l'état est frais, aucune sonde : un remplacement changé n'est pas vu.
        launcher.overrides = BTreeMap::new();
        assert_eq!(
            cache.get_or_refresh(&launcher, Duration::from_secs(60)),
            premier
        );
        // Périmé, il est resondé.
        let renouvele = cache.get_or_refresh(&launcher, Duration::ZERO);
        assert_ne!(renouvele.drivers[1].connection, "simulated");
        assert_eq!(cache.refresh(&launcher), renouvele);
    }

    #[test]
    fn la_reponse_finale_est_extraite_de_la_sortie_structuree() {
        let claude = b"{\"type\":\"system\"}\n{\"type\":\"result\",\"result\":\"Fini.\"}\n";
        assert_eq!(final_text("claude-code", claude), "Fini.");
        let codex = "{\"item\":{\"type\":\"agent_message\",\"text\":\"Code écrit.\"}}\n";
        assert_eq!(final_text("codex", codex.as_bytes()), "Code écrit.");
        assert_eq!(final_text("gemini", b"a\nb\n\n"), "b");
        assert_eq!(final_text("codex", b"pas du json"), "pas du json");
    }
}
