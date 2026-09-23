//! La suite de tâches (M13) jouée par un vrai modèle local, deux fois : à travers Prophet et à
//! travers une boucle nue sur Linux. Même modèle, même moteur, mêmes outils fichiers — leurs
//! implémentations mêmes —, mêmes vérificateurs. Ce qui diffère est exactement ce que Prophet
//! ajoute : le jeton de capd et le contrôle de chaque accès, le journal, l'espace de travail SFS
//! scellé puis publié, la condensation des anciens résultats d'outils (sans rôle ni relais, une
//! mission du banc ne reçoit pas de consigne).
//!
//! La boucle nue appelle les outils de `mcp-system` sans registre (un contrôleur qui autorise
//! tout), écrit dans le même espace de travail, puis le recopie dans le répertoire personnel :
//! ce que ferait un agent qu'aucun système ne borne.
//!
//! Lancé par `just bench` et par le travail « Poids du catalogue servis (réels) » de la CI :
//! `PROPHET_TEST_LLAMA_SERVER` nomme le llama-server épinglé ; le modèle (`PROPHET_BENCH_MODEL`,
//! `qwen3-1.7b-q8` par défaut, celui de l'image) se tire du catalogue du système par egress ;
//! `PROPHET_BENCH_REPETITIONS` (1 par défaut) rejoue chaque tâche, le modèle échantillonnant
//! comme l'image le règle ; `PROPHET_BENCH_RESULTS` reçoit les résultats en JSON, avec, pour
//! chaque exécution, les outils appelés et la réponse finale du modèle.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bench::tasks::{Requires, Task, suite};
use mcp_system::registry::{ResourceAccess, Tool, ToolContext};
use prophet_daemon::essai::{Daemon, binaire_voisin};
use prophet_ipc::Client;
use prophet_types::cap::Act;
use prophet_types::driver::{DriverEvent, Limits, RunStatus, SandboxRequest, StartRequest};
use providers::Driver as _;
use providers::local::{AsyncLocalModel, LocalTool};
use providers::native::{ModelClient, ModelTurn, NativeDriver, ToolExecutor, Usage};
use serde_json::{Value, json};

/// Les outils que les deux côtés offrent au modèle.
const OUTILS: [&str; 5] = ["fs.read", "fs.write", "fs.list", "fs.stat", "fs.search"];

/// Ce qu'une exécution a donné.
#[derive(Debug, Clone, Default, serde::Serialize)]
struct Issue {
    reussie: bool,
    motif: Option<String>,
    secondes: f64,
    tokens: u64,
    etapes: u64,
    /// Les outils appelés, dans l'ordre, marqués `✗` quand l'appel a échoué.
    outils: Vec<String>,
    /// La réponse finale du modèle, tronquée.
    reponse: String,
    /// Livrables rappelés par Prophet (ADR 0049) ; toujours vide pour la boucle nue.
    rappels: Vec<String>,
    /// Temps processeur du moteur pendant l'exécution, en secondes.
    moteur_cpu_s: f64,
    /// Temps processeur de capd, du journal et d'agentd pendant l'exécution (Prophet seul).
    services_cpu_s: Option<f64>,
    /// Somme des pics de mémoire résidente de capd, du journal et d'agentd (Prophet seul).
    services_pic_octets: Option<u64>,
}

/// Temps processeur (utilisateur et système) d'un processus, en secondes ; zéro s'il n'est pas
/// lisible. `/proc/<pid>/stat` compte en `USER_HZ`, 100 par seconde sous Linux.
fn cpu_secondes(pid: u32) -> f64 {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return 0.0;
    };
    // Le nom du programme, entre parenthèses, peut contenir des espaces : on lit après lui.
    let Some((_, reste)) = stat.rsplit_once(')') else {
        return 0.0;
    };
    let champs: Vec<&str> = reste.split_whitespace().collect();
    let tics = |i: usize| {
        champs
            .get(i)
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
    };
    // Après le nom : état (0), …, utime (11), stime (12).
    (tics(11) + tics(12)) as f64 / 100.0
}

/// Pic de mémoire résidente d'un processus (`VmHWM`), en octets.
fn pic_memoire(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|status| {
            status.lines().find_map(|ligne| {
                ligne
                    .strip_prefix("VmHWM:")
                    .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
            })
        })
        .map_or(0, |ko| ko * 1024)
}

/// La réponse finale, bornée pour le rapport.
fn tronquer(texte: &str) -> String {
    let texte = texte.trim();
    match texte.char_indices().nth(240) {
        Some((i, _)) => format!("{}…", &texte[..i]),
        None => texte.to_owned(),
    }
}

/// Les appels d'outils d'une mission, relus dans son journal.
fn appels_du_journal(evenements: &Value) -> Vec<String> {
    evenements
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|e| e["kind"] == "tool.result")
        .map(|e| {
            let outil = e["payload"]["tool"].as_str().unwrap_or("?");
            if e["payload"]["ok"] == true {
                outil.to_owned()
            } else {
                format!("{outil} ✗")
            }
        })
        .collect()
}

/// capd, ledger et agentd pour un répertoire personnel neuf, reliés au moteur local.
struct Chaine {
    _dir: tempfile::TempDir,
    home: PathBuf,
    _capd: Daemon,
    _ledger: Daemon,
    _agentd: Daemon,
    agents: Client,
    journal: Client,
}

impl Chaine {
    /// Les processus des services de Prophet.
    fn services(&self) -> [u32; 3] {
        [self._capd.pid(), self._ledger.pid(), self._agentd.pid()]
    }
}

impl Chaine {
    async fn new(endpoint: &str, extra: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let cap = dir.path().join("cap.sock");
        let ledger_socket = dir.path().join("ledger.sock");
        let capd = Daemon::lancer_avec(
            binaire_voisin("prophet-capd").to_str().unwrap(),
            &cap,
            &dir.path().join("cap-state"),
            &[("PROPHET_HOME", home.to_str().unwrap())],
        );
        drop(capd.joindre().await);
        let ledger = Daemon::lancer(
            binaire_voisin("prophet-ledger").to_str().unwrap(),
            &ledger_socket,
            &dir.path().join("ledger-state"),
        );
        drop(ledger.joindre().await);
        let egress = dir.path().join("egress.sock");
        let mut env = vec![
            ("PROPHET_HOME", home.to_str().unwrap()),
            ("PROPHET_CAPD_SOCKET", cap.to_str().unwrap()),
            ("PROPHET_LEDGER_SOCKET", ledger_socket.to_str().unwrap()),
            ("PROPHET_EGRESS_SOCKET", egress.to_str().unwrap()),
            ("PROPHET_LOCAL_ENDPOINT", endpoint),
        ];
        env.extend_from_slice(extra);
        let agentd = Daemon::lancer_avec(
            binaire_voisin("prophet-agentd").to_str().unwrap(),
            &dir.path().join("agents.sock"),
            &dir.path().join("agent-state"),
            &env,
        );
        let agents = agentd.joindre().await;
        let journal = ledger.joindre().await;
        let home = home.canonicalize().unwrap();
        Self {
            _dir: dir,
            home,
            _capd: capd,
            _ledger: ledger,
            _agentd: agentd,
            agents,
            journal,
        }
    }
}

/// Le manifeste d'une tâche : lire, lister et écrire dans son dossier, avec les outils
/// fichiers — ce que les profils locaux du système accordent (`examples/missions`).
fn mission(tache: &Task, modele: &str) -> Value {
    let racine = format!("~/{}", tache.root);
    let motifs = json!([racine.clone(), format!("{racine}/**")]);
    let mut demandes: Vec<Value> = Vec::new();
    for motif in [racine.clone(), format!("{racine}/**")] {
        demandes.push(json!({"res":"fs","act":"read","match":motif}));
        demandes.push(json!({"res":"fs","act":"list","match":motif}));
        demandes.push(json!({"res":"fs","act":"write","match":motif}));
    }
    for outil in OUTILS {
        demandes.push(json!({"res":"tool","act":"call","match":outil}));
    }
    json!({
        "id": format!("banc-{}", tache.id),
        "intent": tache.intent,
        "user": "prophet",
        "manifest": {
            "agent": {"id":"org.prophet.banc","version":"1.0.0","name":"Banc M13","publisher_key":"ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="},
            "model": {"preferred": [format!("local:{modele}")]},
            "sandbox": {"min_level": 0},
            "capabilities": {"max": {"fs.read": motifs, "fs.list": motifs, "fs.write": motifs, "tool.call": OUTILS}},
            "budget": {"default": {"tokens": 80_000, "wall_time": "600s", "approvals": 3}}
        },
        "requested": demandes,
        "scopes": [racine],
        "availability": {"local_models": [modele]}
    })
}

/// La tâche à travers Prophet : planifiée, lancée, attendue, publiée, vérifiée.
async fn par_prophet(tache: &Task, endpoint: &str, modele: &str, moteur: Option<u32>) -> Issue {
    let chaine = Chaine::new(endpoint, &[]).await;
    (tache.setup)(&chaine.home).unwrap();
    let id = format!("banc-{}", tache.id);
    let services_avant: f64 = chaine.services().iter().map(|p| cpu_secondes(*p)).sum();
    let moteur_avant = moteur.map_or(0.0, cpu_secondes);
    let debut = Instant::now();
    chaine
        .agents
        .call("task.spawn", mission(tache, modele))
        .await
        .unwrap_or_else(|e| panic!("{id} : planification refusée : {}", e.message));
    chaine
        .agents
        .call("task.start", json!({"id": id}))
        .await
        .unwrap_or_else(|e| panic!("{id} : lancement refusé : {}", e.message));
    let statut = tokio::time::timeout(Duration::from_secs(900), async {
        loop {
            let statut = chaine
                .agents
                .call("task.status", json!({"id": id}))
                .await
                .unwrap();
            if matches!(
                statut["state"].as_str(),
                Some("done" | "failed" | "cancelled")
            ) {
                return statut;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{id} : la mission ne finit pas"));
    let depense = &statut["budget"]["spent"];
    let mut issue = Issue {
        secondes: debut.elapsed().as_secs_f64(),
        tokens: depense["tokens"].as_u64().unwrap_or(0),
        etapes: depense["steps"].as_u64().unwrap_or(0),
        moteur_cpu_s: moteur.map_or(0.0, cpu_secondes) - moteur_avant,
        ..Issue::default()
    };
    if let Ok(evenements) = chaine
        .journal
        .call("ledger.query", json!({"task": id}))
        .await
    {
        issue.outils = appels_du_journal(&evenements);
    }
    if let Ok(resultat) = chaine.agents.call("task.result", json!({"id": id})).await {
        issue.reponse = tronquer(resultat["text"].as_str().unwrap_or(""));
        issue.rappels = resultat["reminded"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|r| r.as_str().map(ToOwned::to_owned))
            .collect();
    }
    if statut["state"] != "done" {
        issue.motif = Some(format!(
            "mission {} : {}",
            statut["state"].as_str().unwrap_or("?"),
            statut["reason"].as_str().unwrap_or("sans motif")
        ));
    } else if let Err(e) = chaine.agents.call("task.apply", json!({"id": id})).await {
        // L'humain publie ce que la mission a préparé ; le vérificateur lit le répertoire
        // personnel.
        issue.motif = Some(format!("publication : {}", e.message));
    } else {
        match (tache.verify)(&chaine.home) {
            Ok(()) => issue.reussie = true,
            Err(motif) => issue.motif = Some(motif),
        }
    }
    let services = chaine.services();
    issue.services_cpu_s =
        Some(services.iter().map(|p| cpu_secondes(*p)).sum::<f64>() - services_avant);
    issue.services_pic_octets = Some(services.iter().map(|p| pic_memoire(*p)).sum());
    issue
}

/// Un contrôleur qui autorise tout : la boucle nue n'a personne pour dire non.
struct ToutPermis;

impl ResourceAccess for ToutPermis {
    fn permits(&self, _act: Act, _path: &str) -> bool {
        true
    }
}

/// Les outils fichiers de `mcp-system`, appelés sans registre.
struct OutilsNus {
    outils: Vec<Box<dyn Tool>>,
    contexte: ToolContext,
    appels: Arc<Mutex<Vec<String>>>,
}

impl OutilsNus {
    fn new(home: &Path, tache: &str) -> Self {
        let workdir = home.join(".prophet/tasks").join(tache).join("work");
        std::fs::create_dir_all(&workdir).unwrap();
        let maintenant = time::OffsetDateTime::now_utc();
        let jeton = prophet_types::cap::Token {
            v: 1,
            iss: "aucun".into(),
            sub: tache.into(),
            agent: "boucle-nue".into(),
            user: "prophet".into(),
            parent: None,
            grants: Vec::new(),
            iat: maintenant,
            exp: maintenant + time::Duration::hours(1),
            nonce: "AAAAAAAAAAAAAAAAAAAAAA==".into(),
            sig: None,
        };
        Self {
            outils: vec![
                Box::new(mcp_system::tools::Read),
                Box::new(mcp_system::tools::Write),
                Box::new(mcp_system::tools::List),
                Box::new(mcp_system::tools::Stat),
                Box::new(mcp_system::tools::Search),
            ],
            contexte: ToolContext {
                token: jeton,
                task: tache.into(),
                home: home.display().to_string(),
                workdir: workdir.display().to_string(),
                sandbox_level: 0,
                step: 1,
            },
            appels: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Les définitions offertes au modèle, dans l'ordre où le registre de Prophet les offre
    /// (par nom) : un petit modèle est sensible à l'ordre de la liste, qui ne doit pas séparer
    /// les deux côtés du banc.
    fn definitions(&self) -> Vec<LocalTool> {
        let mut definitions: Vec<LocalTool> = self
            .outils
            .iter()
            .map(|o| {
                let spec = o.spec();
                LocalTool {
                    name: spec.name,
                    description: spec.description,
                    parameters: spec.input_schema,
                }
            })
            .collect();
        definitions.sort_by(|a, b| a.name.cmp(&b.name));
        definitions
    }
}

impl ToolExecutor for OutilsNus {
    fn call(&self, tool: &str, arguments: &Value) -> (bool, Value) {
        let Some(outil) = self.outils.iter().find(|o| o.spec().name == tool) else {
            self.appels.lock().unwrap().push(format!("{tool} ✗"));
            return (false, json!({"code":"NotFound","detail":"outil inconnu"}));
        };
        let resultat = outil.call_checked(arguments, &self.contexte, &ToutPermis);
        self.appels.lock().unwrap().push(if resultat.is_error {
            format!("{tool} ✗")
        } else {
            tool.to_owned()
        });
        (
            !resultat.is_error,
            resultat
                .structured
                .unwrap_or_else(|| json!({"content": resultat.content})),
        )
    }
}

/// Le modèle local, appelé sans condensation, ses tokens comptés.
struct ModeleNu {
    client: AsyncLocalModel,
    runtime: tokio::runtime::Runtime,
    tokens: Arc<AtomicU64>,
    tours: Arc<AtomicU64>,
    nom: String,
}

impl ModelClient for ModeleNu {
    fn next_turn(
        &mut self,
        history: &[Value],
    ) -> Result<(ModelTurn, Usage), providers::DriverError> {
        let reponse = self.runtime.block_on(self.client.next_turn(history))?;
        self.tokens.fetch_add(
            reponse.usage.tokens_in + reponse.usage.tokens_out,
            Ordering::Relaxed,
        );
        self.tours.fetch_add(1, Ordering::Relaxed);
        reponse.turn.map(|t| (t, reponse.usage))
    }

    fn model_name(&self) -> String {
        self.nom.clone()
    }
}

/// Recopie l'espace de travail dans le répertoire personnel : la « publication » d'un agent
/// que rien ne borne.
fn recopier(depuis: &Path, vers: &Path) {
    let Ok(entrees) = std::fs::read_dir(depuis) else {
        return;
    };
    for entree in entrees.flatten() {
        let cible = vers.join(entree.file_name());
        if entree.path().is_dir() {
            std::fs::create_dir_all(&cible).unwrap();
            recopier(&entree.path(), &cible);
        } else {
            std::fs::copy(entree.path(), &cible).unwrap();
        }
    }
}

/// La tâche à travers la boucle nue, dans un fil à elle.
fn par_la_boucle_nue(tache: &Task, endpoint: &str, modele: &str, moteur: Option<u32>) -> Issue {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let home = home.canonicalize().unwrap();
    (tache.setup)(&home).unwrap();
    let id = format!("nue-{}", tache.id);
    let outils = OutilsNus::new(&home, &id);
    let tokens = Arc::new(AtomicU64::new(0));
    let tours = Arc::new(AtomicU64::new(0));
    let client = AsyncLocalModel::new(
        endpoint,
        modele,
        outils.definitions(),
        Duration::from_secs(300),
        2048,
    )
    .unwrap();
    let modele_nu = ModeleNu {
        client,
        runtime: tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap(),
        tokens: tokens.clone(),
        tours: tours.clone(),
        nom: modele.into(),
    };
    let workdir = outils.contexte.workdir.clone();
    let appels = outils.appels.clone();
    let mut boucle = NativeDriver::new(Box::new(modele_nu), Box::new(outils));
    let moteur_avant = moteur.map_or(0.0, cpu_secondes);
    let debut = Instant::now();
    let run = boucle
        .start(&StartRequest {
            driver: "boucle-nue".into(),
            task: id.clone(),
            intent: tache.intent.into(),
            workdir: workdir.clone(),
            mcp_config: String::new(),
            token: String::new(),
            sandbox: SandboxRequest {
                level: 0,
                profile: "aucun".into(),
            },
            limits: Limits {
                wall_time_s: 600,
                max_steps: 50,
            },
            resume: None,
        })
        .unwrap()
        .run;
    let mut reponse = String::new();
    let fin = loop {
        let mut fin = None;
        for evenement in boucle.poll(&run).unwrap() {
            match evenement {
                DriverEvent::Text { text, .. } => reponse = text,
                DriverEvent::Done { status, reason, .. } => fin = Some((status, reason)),
                _ => {}
            }
        }
        if let Some(fin) = fin {
            break fin;
        }
        assert!(
            debut.elapsed() < Duration::from_secs(900),
            "{id} : la boucle ne finit pas"
        );
    };
    let mut issue = Issue {
        secondes: debut.elapsed().as_secs_f64(),
        tokens: tokens.load(Ordering::Relaxed),
        etapes: tours.load(Ordering::Relaxed),
        outils: appels.lock().unwrap().clone(),
        reponse: tronquer(&reponse),
        moteur_cpu_s: moteur.map_or(0.0, cpu_secondes) - moteur_avant,
        ..Issue::default()
    };
    if fin.0 != RunStatus::Ok {
        issue.motif = Some(format!(
            "boucle interrompue : {}",
            fin.1.unwrap_or_default()
        ));
        return issue;
    }
    recopier(Path::new(&workdir), &home);
    match (tache.verify)(&home) {
        Ok(()) => issue.reussie = true,
        Err(motif) => issue.motif = Some(motif),
    }
    issue
}

/// Le moteur tel que l'image le lance en mode simple, sur un poids du catalogue.
async fn moteur(serveur: &str, poids: &Path, alias: &str, port: u16) -> tokio::process::Child {
    let enfant = tokio::process::Command::new(serveur)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .arg("--model")
        .arg(poids)
        .args(["--alias", alias])
        .args([
            "--load-mode",
            "none",
            "--jinja",
            "--reasoning",
            "off",
            "--ctx-size",
            "4096",
            "--threads",
            "4",
            "--parallel",
            "1",
            "--temp",
            "0.7",
            "--top-p",
            "0.8",
            "--top-k",
            "20",
            "--min-p",
            "0",
            "--presence-penalty",
            "1.5",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let limite = Instant::now() + Duration::from_secs(300);
    loop {
        let pret = std::net::TcpStream::connect(("127.0.0.1", port)).is_ok_and(|mut flux| {
            use std::io::{Read as _, Write as _};
            let _ = flux
                .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
            let mut reponse = String::new();
            let _ = flux.read_to_string(&mut reponse);
            reponse.starts_with("HTTP/1.1 200")
        });
        if pret {
            return enfant;
        }
        assert!(Instant::now() < limite, "le moteur ne démarre pas");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Tire un poids du catalogue du système par une chaîne qui a egress.
async fn tirer(id: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let poids = dir.path().join("poids");
    let cap = dir.path().join("cap.sock");
    let ledger_socket = dir.path().join("ledger.sock");
    let egress_socket = dir.path().join("egress.sock");
    let capd = Daemon::lancer_avec(
        binaire_voisin("prophet-capd").to_str().unwrap(),
        &cap,
        &dir.path().join("cap-state"),
        &[("PROPHET_HOME", home.to_str().unwrap())],
    );
    drop(capd.joindre().await);
    let ledger = Daemon::lancer(
        binaire_voisin("prophet-ledger").to_str().unwrap(),
        &ledger_socket,
        &dir.path().join("ledger-state"),
    );
    drop(ledger.joindre().await);
    let egress = Daemon::lancer_avec(
        binaire_voisin("prophet-egress").to_str().unwrap(),
        &egress_socket,
        &dir.path().join("egress-state"),
        &[("PROPHET_CAPD_SOCKET", cap.to_str().unwrap())],
    );
    egress
        .attendre_reponse(b"GET http://sonde.invalide/ HTTP/1.1\r\nHost: sonde.invalide\r\n\r\n")
        .await;
    let agentd = Daemon::lancer_avec(
        binaire_voisin("prophet-agentd").to_str().unwrap(),
        &dir.path().join("agents.sock"),
        &dir.path().join("agent-state"),
        &[
            ("PROPHET_HOME", home.to_str().unwrap()),
            ("PROPHET_CAPD_SOCKET", cap.to_str().unwrap()),
            ("PROPHET_LEDGER_SOCKET", ledger_socket.to_str().unwrap()),
            ("PROPHET_EGRESS_SOCKET", egress_socket.to_str().unwrap()),
            ("PROPHET_PULL_DIR", poids.to_str().unwrap()),
        ],
    );
    let agents = agentd.joindre().await;
    agents
        .call("model.pull", json!({"id": id}))
        .await
        .unwrap_or_else(|e| panic!("{id} : {}", e.message));
    let suivi = tokio::time::timeout(Duration::from_secs(1200), async {
        loop {
            let suivis = agents.call("model.pulls", json!({})).await.unwrap();
            if let Some(s) = suivis.as_array().unwrap().iter().find(|s| s["id"] == id)
                && s["state"] != "running"
            {
                return s.clone();
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(suivi["state"], "done", "{suivi}");
    let entree = providers::catalogue::Catalogue::builtin()
        .get(id)
        .cloned()
        .unwrap();
    let chemin = poids.join(&entree.file);
    drop((agents, agentd, egress, ledger, capd));
    (dir, chemin)
}

/// Réussites, durées et tokens d'un côté du banc, sur toutes ses exécutions.
fn resumer(issues: &[&Issue]) -> Value {
    let durees: Vec<f64> = issues.iter().map(|i| i.secondes).collect();
    json!({
        "success": issues.iter().filter(|i| i.reussie).count(),
        "runs": issues.len(),
        "median_seconds": centile(&durees, 0.5),
        "p95_seconds": centile(&durees, 0.95),
        "mean_tokens": issues.iter().map(|i| i.tokens).sum::<u64>() / issues.len().max(1) as u64,
        "reminded_runs": issues.iter().filter(|i| !i.rappels.is_empty()).count(),
        "mean_engine_cpu_seconds": issues.iter().map(|i| i.moteur_cpu_s).sum::<f64>() / issues.len().max(1) as f64,
        "mean_services_cpu_seconds": issues.iter().filter_map(|i| i.services_cpu_s).sum::<f64>() / issues.len().max(1) as f64,
        "max_services_peak_bytes": issues.iter().filter_map(|i| i.services_pic_octets).max(),
    })
}

fn centile(valeurs: &[f64], p: f64) -> f64 {
    let mut v = valeurs.to_vec();
    v.sort_by(f64::total_cmp);
    if v.is_empty() {
        return 0.0;
    }
    v[((v.len() - 1) as f64 * p).round() as usize]
}

/// M13-T1 et FRONTIER (« comparaison avec une base Linux utilisant les mêmes modèles et les
/// mêmes tâches ») : la suite sans navigateur, jouée par le vrai modèle des deux côtés.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs_llama_server: PROPHET_TEST_LLAMA_SERVER (llama-server épinglé), Hugging Face joignable"]
async fn la_suite_se_joue_par_prophet_et_par_une_boucle_nue() {
    let serveur = std::env::var("PROPHET_TEST_LLAMA_SERVER")
        .expect("PROPHET_TEST_LLAMA_SERVER : le llama-server épinglé (nix build .#llama-cpp)");
    let id = std::env::var("PROPHET_BENCH_MODEL").unwrap_or_else(|_| "qwen3-1.7b-q8".into());
    let (_poids, fichier) = tirer(&id).await;
    let alias = id
        .trim_end_matches("-q8")
        .trim_end_matches("-q4")
        .to_owned();
    let port = 18_140;
    let moteur_servi = moteur(&serveur, &fichier, &alias, port).await;
    let pid_moteur = moteur_servi.id();
    let endpoint = format!("http://127.0.0.1:{port}/v1");

    let taches: Vec<Task> = suite()
        .into_iter()
        .filter(|t| t.requires == Requires::Nothing)
        .collect();
    let repetitions: u32 = std::env::var("PROPHET_BENCH_REPETITIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(1);
    let mut executions: Vec<(&Task, u32, Issue, Issue)> = Vec::new();
    for passage in 1..=repetitions {
        for tache in &taches {
            let prophet = par_prophet(tache, &endpoint, &alias, pid_moteur).await;
            let (endpoint_nu, alias_nu, id_tache) = (endpoint.clone(), alias.clone(), tache.id);
            let nue = std::thread::spawn(move || {
                let tache = suite().into_iter().find(|t| t.id == id_tache).unwrap();
                par_la_boucle_nue(&tache, &endpoint_nu, &alias_nu, pid_moteur)
            })
            .join()
            .unwrap();
            let decrire = |issue: &Issue, tours: &str| {
                format!(
                    "{} {:.1} s, {} tokens, {} {tours}, moteur {:.1} s CPU{}{}{} [{}] « {} »",
                    if issue.reussie { "✓" } else { "✗" },
                    issue.secondes,
                    issue.tokens,
                    issue.etapes,
                    issue.moteur_cpu_s,
                    match (issue.services_cpu_s, issue.services_pic_octets) {
                        (Some(cpu), Some(pic)) => format!(
                            ", services {cpu:.2} s CPU et {} Mo au plus",
                            pic / 1_000_000
                        ),
                        _ => String::new(),
                    },
                    issue
                        .motif
                        .as_deref()
                        .map_or_else(String::new, |m| format!(" ({m})")),
                    if issue.rappels.is_empty() {
                        String::new()
                    } else {
                        format!(", rappel : {}", issue.rappels.join(", "))
                    },
                    issue.outils.join(" → "),
                    issue.reponse.replace('\n', " "),
                )
            };
            eprintln!(
                "mesure : banc {} #{passage} | Prophet {} | nue {}",
                tache.id,
                decrire(&prophet, "étapes"),
                decrire(&nue, "tours"),
            );
            executions.push((tache, passage, prophet, nue));
        }
    }

    let par_tache: Vec<Value> = taches
        .iter()
        .map(|tache| {
            let siennes: Vec<_> = executions
                .iter()
                .filter(|(t, ..)| t.id == tache.id)
                .collect();
            json!({
                "task": tache.id,
                "family": tache.family,
                "prophet_success": siennes.iter().filter(|(_, _, p, _)| p.reussie).count(),
                "bare_success": siennes.iter().filter(|(_, _, _, n)| n.reussie).count(),
                "runs": siennes.len(),
            })
        })
        .collect();
    let lignes: Vec<Value> = executions
        .iter()
        .map(|(tache, passage, prophet, nue)| {
            json!({"task": tache.id, "family": tache.family, "repetition": passage, "prophet": prophet, "bare": nue})
        })
        .collect();
    let bilan = json!({
        "model": id,
        "repetitions": repetitions,
        "prophet": resumer(&executions.iter().map(|e| &e.2).collect::<Vec<_>>()),
        "bare": resumer(&executions.iter().map(|e| &e.3).collect::<Vec<_>>()),
        "by_task": par_tache,
        "runs": lignes,
    });
    for ligne in &par_tache {
        eprintln!(
            "mesure : banc {} — Prophet {}/{}, nue {}/{}",
            ligne["task"].as_str().unwrap_or("?"),
            ligne["prophet_success"],
            ligne["runs"],
            ligne["bare_success"],
            ligne["runs"],
        );
    }
    eprintln!(
        "mesure : banc {id} — Prophet {}/{} réussies ({} rappelées), médiane {:.1} s, p95 {:.1} s, {} tokens et {:.1} s CPU de moteur en moyenne, services {:.2} s CPU en moyenne et {} Mo au plus ; nue {}/{}, médiane {:.1} s, p95 {:.1} s, {} tokens et {:.1} s CPU de moteur",
        bilan["prophet"]["success"],
        bilan["prophet"]["runs"],
        bilan["prophet"]["reminded_runs"],
        bilan["prophet"]["median_seconds"].as_f64().unwrap_or(0.0),
        bilan["prophet"]["p95_seconds"].as_f64().unwrap_or(0.0),
        bilan["prophet"]["mean_tokens"],
        bilan["prophet"]["mean_engine_cpu_seconds"]
            .as_f64()
            .unwrap_or(0.0),
        bilan["prophet"]["mean_services_cpu_seconds"]
            .as_f64()
            .unwrap_or(0.0),
        bilan["prophet"]["max_services_peak_bytes"]
            .as_u64()
            .unwrap_or(0)
            / 1_000_000,
        bilan["bare"]["success"],
        bilan["bare"]["runs"],
        bilan["bare"]["median_seconds"].as_f64().unwrap_or(0.0),
        bilan["bare"]["p95_seconds"].as_f64().unwrap_or(0.0),
        bilan["bare"]["mean_tokens"],
        bilan["bare"]["mean_engine_cpu_seconds"]
            .as_f64()
            .unwrap_or(0.0),
    );
    if let Ok(chemin) = std::env::var("PROPHET_BENCH_RESULTS") {
        std::fs::write(&chemin, serde_json::to_string_pretty(&bilan).unwrap()).unwrap();
    }
    // Le banc mesure ; il n'échoue que si rien n'a pu être joué.
    assert!(
        bilan["prophet"]["runs"].as_u64().unwrap_or(0) > 0,
        "aucune tâche jouée"
    );
}

/// Un moteur scripté, compatible avec l'API de llama-server : à la première demande d'une
/// conversation, il écrit `5` dans `~/notes/out/total.txt` ; une fois le résultat de l'outil
/// reçu, il conclut. Assez pour éprouver le banc sans modèle.
async fn faux_moteur() -> String {
    use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _};
    let ecoute = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", ecoute.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let Ok((flux, _)) = ecoute.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut lecteur = tokio::io::BufReader::new(flux);
                let mut longueur = 0;
                loop {
                    let mut ligne = String::new();
                    if lecteur.read_line(&mut ligne).await.unwrap_or(0) == 0 {
                        return;
                    }
                    if ligne == "\r\n" {
                        break;
                    }
                    if let Some(v) = ligne.to_ascii_lowercase().strip_prefix("content-length:") {
                        longueur = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut corps = vec![0; longueur];
                let _ = lecteur.read_exact(&mut corps).await;
                let corps = String::from_utf8_lossy(&corps);
                let reponse = if corps.contains("\"role\":\"tool\"") {
                    json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"Le total est écrit."}}],"usage":{"prompt_tokens":40,"completion_tokens":5}})
                } else {
                    json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"tool_calls":[{"type":"function","id":"appel_1","function":{"name":"fs.write","arguments":"{\"path\":\"~/notes/out/total.txt\",\"content\":\"5\"}"}}]}}],"usage":{"prompt_tokens":30,"completion_tokens":12}})
                }
                .to_string();
                let _ = lecteur
                    .get_mut()
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reponse}",
                            reponse.len()
                        )
                        .as_bytes(),
                    )
                    .await;
            });
        }
    });
    endpoint
}

/// Les deux côtés offrent au modèle les mêmes outils, décrits de même, dans le même ordre.
#[test]
fn les_deux_cotes_offrent_les_memes_outils_dans_le_meme_ordre() {
    let dir = tempfile::tempdir().unwrap();
    let nus = OutilsNus::new(dir.path(), "ordre").definitions();
    let mut registre = mcp_system::registry::Registry::new(
        Arc::new(Mutex::new(
            capd::Broker::new(
                ed25519_dalek::SigningKey::from_bytes(&[7; 32]),
                "capd@banc",
                dir.path().display().to_string(),
            )
            .unwrap(),
        )),
        Arc::new(mcp_system::registry::MemoryJournal::new()),
    );
    registre.register(Arc::new(mcp_system::tools::Read));
    registre.register(Arc::new(mcp_system::tools::Write));
    registre.register(Arc::new(mcp_system::tools::List));
    registre.register(Arc::new(mcp_system::tools::Stat));
    registre.register(Arc::new(mcp_system::tools::Search));
    let prophet: Vec<(String, String)> = registre
        .all()
        .into_iter()
        .map(|s| (s.name, s.description))
        .collect();
    let nue: Vec<(String, String)> = nus.into_iter().map(|d| (d.name, d.description)).collect();
    assert_eq!(prophet, nue);
}

/// Le banc lui-même, éprouvé sans modèle : les deux chemins jouent une tâche jusqu'au
/// vérificateur — planification, lancement, publication par agentd d'un côté ; outils nus,
/// recopie de l'espace de travail de l'autre.
#[tokio::test(flavor = "multi_thread")]
async fn le_banc_joue_une_tache_des_deux_cotes_avec_un_faux_moteur() {
    let endpoint = faux_moteur().await;
    let tache = suite()
        .into_iter()
        .find(|t| t.id == "compter-lignes")
        .unwrap();
    // Le test lui-même tient lieu de moteur : son temps processeur se lit comme celui d'un autre.
    let soi = Some(std::process::id());
    let prophet = par_prophet(&tache, &endpoint, "faux", soi).await;
    assert!(prophet.reussie, "{prophet:?}");
    assert!(prophet.tokens > 0 && prophet.etapes > 0, "{prophet:?}");
    assert_eq!(prophet.outils, ["fs.write"], "{prophet:?}");
    assert_eq!(prophet.reponse, "Le total est écrit.");
    assert!(prophet.rappels.is_empty(), "{prophet:?}");
    // Ce que coûtent les services : du temps processeur et une mémoire résidente mesurés.
    assert!(prophet.moteur_cpu_s >= 0.0, "{prophet:?}");
    assert!(
        prophet.services_cpu_s.is_some_and(|s| s > 0.0),
        "{prophet:?}"
    );
    assert!(
        prophet
            .services_pic_octets
            .is_some_and(|o| o > 3 * 1024 * 1024),
        "{prophet:?}"
    );
    let e = endpoint.clone();
    let nue = std::thread::spawn(move || {
        let tache = suite()
            .into_iter()
            .find(|t| t.id == "compter-lignes")
            .unwrap();
        par_la_boucle_nue(&tache, &e, "faux", None)
    })
    .join()
    .unwrap();
    assert!(nue.reussie, "{nue:?}");
    assert_eq!(nue.tokens, 30 + 12 + 40 + 5, "{nue:?}");
    assert_eq!(nue.etapes, 2);
    assert_eq!(nue.outils, ["fs.write"], "{nue:?}");
    assert_eq!(nue.reponse, "Le total est écrit.");
    assert!(nue.services_cpu_s.is_none() && nue.services_pic_octets.is_none());
    // Une tâche que le faux moteur ne sait pas faire échoue au vérificateur, des deux côtés.
    let autre = suite()
        .into_iter()
        .find(|t| t.id == "total-des-ventes")
        .unwrap();
    // Son écriture sort de la portée de la mission : capd la refuse, le refus revient au
    // modèle (ADR 0050), qui conclut ; Prophet lui rappelle le fichier demandé, en vain.
    let echec = par_prophet(&autre, &endpoint, "faux", None).await;
    assert!(!echec.reussie, "{echec:?}");
    assert_eq!(echec.outils, ["fs.write ✗"], "{echec:?}");
    assert_eq!(echec.rappels, ["~/ventes/out/total.txt"], "{echec:?}");
}
