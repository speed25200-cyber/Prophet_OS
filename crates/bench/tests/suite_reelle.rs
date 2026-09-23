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
//! `PROPHET_BENCH_RESULTS` reçoit les résultats en JSON.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
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
#[derive(Debug, Clone, serde::Serialize)]
struct Issue {
    reussie: bool,
    motif: Option<String>,
    secondes: f64,
    tokens: u64,
    etapes: u64,
}

/// capd, ledger et agentd pour un répertoire personnel neuf, reliés au moteur local.
struct Chaine {
    _dir: tempfile::TempDir,
    home: PathBuf,
    _capd: Daemon,
    _ledger: Daemon,
    _agentd: Daemon,
    agents: Client,
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
        let home = home.canonicalize().unwrap();
        Self {
            _dir: dir,
            home,
            _capd: capd,
            _ledger: ledger,
            _agentd: agentd,
            agents,
        }
    }
}

/// Le manifeste d'une tâche : lire et écrire dans son dossier, avec les outils fichiers.
fn mission(tache: &Task, modele: &str) -> Value {
    let racine = format!("~/{}", tache.root);
    let motifs = json!([racine.clone(), format!("{racine}/**")]);
    let mut demandes: Vec<Value> = Vec::new();
    for motif in [racine.clone(), format!("{racine}/**")] {
        demandes.push(json!({"res":"fs","act":"read","match":motif}));
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
            "capabilities": {"max": {"fs.read": motifs, "fs.write": motifs, "tool.call": OUTILS}},
            "budget": {"default": {"tokens": 80_000, "wall_time": "600s", "approvals": 3}}
        },
        "requested": demandes,
        "scopes": [racine],
        "availability": {"local_models": [modele]}
    })
}

/// La tâche à travers Prophet : planifiée, lancée, attendue, publiée, vérifiée.
async fn par_prophet(tache: &Task, endpoint: &str, modele: &str) -> Issue {
    let chaine = Chaine::new(endpoint, &[]).await;
    (tache.setup)(&chaine.home).unwrap();
    let id = format!("banc-{}", tache.id);
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
        reussie: false,
        motif: None,
        secondes: debut.elapsed().as_secs_f64(),
        tokens: depense["tokens"].as_u64().unwrap_or(0),
        etapes: depense["steps"].as_u64().unwrap_or(0),
    };
    if statut["state"] != "done" {
        issue.motif = Some(format!(
            "mission {} : {}",
            statut["state"].as_str().unwrap_or("?"),
            statut["reason"].as_str().unwrap_or("sans motif")
        ));
        return issue;
    }
    // L'humain publie ce que la mission a préparé ; le vérificateur lit le répertoire personnel.
    if let Err(e) = chaine.agents.call("task.apply", json!({"id": id})).await {
        issue.motif = Some(format!("publication : {}", e.message));
        return issue;
    }
    match (tache.verify)(&chaine.home) {
        Ok(()) => issue.reussie = true,
        Err(motif) => issue.motif = Some(motif),
    }
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
        }
    }

    fn definitions(&self) -> Vec<LocalTool> {
        self.outils
            .iter()
            .map(|o| {
                let spec = o.spec();
                LocalTool {
                    name: spec.name,
                    description: spec.description,
                    parameters: spec.input_schema,
                }
            })
            .collect()
    }
}

impl ToolExecutor for OutilsNus {
    fn call(&self, tool: &str, arguments: &Value) -> (bool, Value) {
        let Some(outil) = self.outils.iter().find(|o| o.spec().name == tool) else {
            return (false, json!({"code":"NotFound","detail":"outil inconnu"}));
        };
        let resultat = outil.call_checked(arguments, &self.contexte, &ToutPermis);
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
fn par_la_boucle_nue(tache: &Task, endpoint: &str, modele: &str) -> Issue {
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
    let mut boucle = NativeDriver::new(Box::new(modele_nu), Box::new(outils));
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
    let fin = loop {
        let evenements = boucle.poll(&run).unwrap();
        if let Some(fin) = evenements.into_iter().find_map(|e| match e {
            DriverEvent::Done { status, reason, .. } => Some((status, reason)),
            _ => None,
        }) {
            break fin;
        }
        assert!(
            debut.elapsed() < Duration::from_secs(900),
            "{id} : la boucle ne finit pas"
        );
    };
    let mut issue = Issue {
        reussie: false,
        motif: None,
        secondes: debut.elapsed().as_secs_f64(),
        tokens: tokens.load(Ordering::Relaxed),
        etapes: tours.load(Ordering::Relaxed),
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
    let _moteur = moteur(&serveur, &fichier, &alias, port).await;
    let endpoint = format!("http://127.0.0.1:{port}/v1");

    let taches: Vec<Task> = suite()
        .into_iter()
        .filter(|t| t.requires == Requires::Nothing)
        .collect();
    let mut lignes = Vec::new();
    for tache in &taches {
        let prophet = par_prophet(tache, &endpoint, &alias).await;
        let (endpoint_nu, alias_nu, id_tache) = (endpoint.clone(), alias.clone(), tache.id);
        let nue = std::thread::spawn(move || {
            let tache = suite().into_iter().find(|t| t.id == id_tache).unwrap();
            par_la_boucle_nue(&tache, &endpoint_nu, &alias_nu)
        })
        .join()
        .unwrap();
        eprintln!(
            "mesure : banc {} | Prophet {} {:.1} s, {} tokens, {} étapes{} | nue {} {:.1} s, {} tokens, {} tours{}",
            tache.id,
            if prophet.reussie { "✓" } else { "✗" },
            prophet.secondes,
            prophet.tokens,
            prophet.etapes,
            prophet
                .motif
                .as_deref()
                .map_or_else(String::new, |m| format!(" ({m})")),
            if nue.reussie { "✓" } else { "✗" },
            nue.secondes,
            nue.tokens,
            nue.etapes,
            nue.motif
                .as_deref()
                .map_or_else(String::new, |m| format!(" ({m})")),
        );
        lignes.push(
            json!({"task": tache.id, "family": tache.family, "prophet": prophet, "bare": nue}),
        );
    }

    let resume = |cote: &str| {
        let issues: Vec<Issue> = lignes
            .iter()
            .map(|l| serde_json::from_value::<Value>(l[cote].clone()).unwrap())
            .map(|v| Issue {
                reussie: v["reussie"] == true,
                motif: None,
                secondes: v["secondes"].as_f64().unwrap_or(0.0),
                tokens: v["tokens"].as_u64().unwrap_or(0),
                etapes: v["etapes"].as_u64().unwrap_or(0),
            })
            .collect();
        let durees: Vec<f64> = issues.iter().map(|i| i.secondes).collect();
        json!({
            "success": issues.iter().filter(|i| i.reussie).count(),
            "tasks": issues.len(),
            "median_seconds": centile(&durees, 0.5),
            "p95_seconds": centile(&durees, 0.95),
            "mean_tokens": issues.iter().map(|i| i.tokens).sum::<u64>() / issues.len().max(1) as u64,
        })
    };
    let bilan =
        json!({"model": id, "prophet": resume("prophet"), "bare": resume("bare"), "tasks": lignes});
    eprintln!(
        "mesure : banc {id} — Prophet {}/{} réussies, médiane {:.1} s, p95 {:.1} s, {} tokens en moyenne ; nue {}/{}, médiane {:.1} s, p95 {:.1} s, {} tokens",
        bilan["prophet"]["success"],
        bilan["prophet"]["tasks"],
        bilan["prophet"]["median_seconds"].as_f64().unwrap_or(0.0),
        bilan["prophet"]["p95_seconds"].as_f64().unwrap_or(0.0),
        bilan["prophet"]["mean_tokens"],
        bilan["bare"]["success"],
        bilan["bare"]["tasks"],
        bilan["bare"]["median_seconds"].as_f64().unwrap_or(0.0),
        bilan["bare"]["p95_seconds"].as_f64().unwrap_or(0.0),
        bilan["bare"]["mean_tokens"],
    );
    if let Ok(chemin) = std::env::var("PROPHET_BENCH_RESULTS") {
        std::fs::write(&chemin, serde_json::to_string_pretty(&bilan).unwrap()).unwrap();
    }
    // Le banc mesure ; il n'échoue que si rien n'a pu être joué.
    assert!(
        bilan["prophet"]["tasks"].as_u64().unwrap_or(0) > 0,
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
    let prophet = par_prophet(&tache, &endpoint, "faux").await;
    assert!(prophet.reussie, "{prophet:?}");
    assert!(prophet.tokens > 0 && prophet.etapes > 0, "{prophet:?}");
    let e = endpoint.clone();
    let nue = std::thread::spawn(move || {
        let tache = suite()
            .into_iter()
            .find(|t| t.id == "compter-lignes")
            .unwrap();
        par_la_boucle_nue(&tache, &e, "faux")
    })
    .join()
    .unwrap();
    assert!(nue.reussie, "{nue:?}");
    assert_eq!(nue.tokens, 30 + 12 + 40 + 5, "{nue:?}");
    assert_eq!(nue.etapes, 2);
    // Une tâche que le faux moteur ne sait pas faire échoue au vérificateur, des deux côtés.
    let autre = suite()
        .into_iter()
        .find(|t| t.id == "total-des-ventes")
        .unwrap();
    let echec = par_prophet(&autre, &endpoint, "faux").await;
    assert!(!echec.reussie, "{echec:?}");
}
