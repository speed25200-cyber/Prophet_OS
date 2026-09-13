//! Le binaire `prophet`.
//!
//! C'est par lui qu'un humain voit ce que ses agents font, et décide. Trois principes tiennent la
//! conception de cette interface :
//!
//! 1. **Rien ne démarre sans un plan lisible.** Avant d'exécuter, `prophet task new` montre le
//!    pilote, le périmètre, les capacités et le budget.
//! 2. **Tout est révocable.** Une tâche validée reste annulable, une règle d'approbation se
//!    retire, un jeton se révoque.
//! 3. **Les limites sont annoncées.** `prophet status` dit ce que la machine ne sait pas faire,
//!    plutôt que de laisser croire à une protection absente.

#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};

/// Gouverne les agents de Prophet OS.
#[derive(Debug, Parser)]
#[command(name = "prophet", version, about, long_about = None)]
struct Cli {
    /// Rend la sortie en JSON, pour un usage par un programme.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// État général du système et de ses limites.
    Status,
    /// Gestion des tâches.
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },
    /// Journal d'audit.
    Log {
        #[command(subcommand)]
        action: LogAction,
    },
    /// Capacités et approbations.
    Cap {
        #[command(subcommand)]
        action: CapAction,
    },
    /// Pilotes et fournisseurs de modèles.
    Provider {
        #[command(subcommand)]
        action: ProviderAction,
    },
    /// Mémoire.
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },
    /// Gel d'urgence de toutes les tâches.
    Freeze,
}

#[derive(Debug, Subcommand)]
enum TaskAction {
    /// Planifie une mission depuis une requête JSON, sans la démarrer.
    #[command(alias = "plan")]
    New {
        /// Fichier contenant intent, manifest, requested, scopes et availability.
        request: std::path::PathBuf,
    },
    /// Lance une mission déjà planifiée par agentd.
    Start {
        /// Identifiant de la mission.
        id: String,
    },
    /// Relit le résultat conservé par agentd et les changements à examiner.
    Result {
        /// Identifiant de la mission.
        id: String,
    },
    /// Liste les tâches.
    Ls,
    /// Détaille une tâche.
    Show {
        /// Identifiant.
        id: String,
    },
    /// Montre les changements en attente de validation.
    Diff {
        /// Identifiant.
        id: String,
    },
    /// Liste les contextes du service, leurs modèles disponibles et l'état du navigateur piloté.
    Options,
    /// Prépare une mission depuis un contexte du service, sans la lancer.
    Prepare {
        /// Contexte du catalogue du service (voir `prophet task options`).
        #[arg(long)]
        profile: String,
        /// Modèle admis par ce contexte ; avec `--client`, le premier modèle du contexte sinon.
        #[arg(long)]
        model: Option<String>,
        /// La mission accueillera un client MCP (Claude Code, Codex) : le moteur local n'est
        /// pas requis.
        #[arg(long)]
        client: bool,
        /// Référence à conserver ; générée sinon.
        #[arg(long)]
        id: Option<String>,
        /// Objectif de la mission.
        intent: String,
    },
    /// Rend la configuration MCP qui donne à un client (Claude Code, Codex) les outils d'une
    /// mission préparée, par le pont `prophet-mcp`.
    McpConfig {
        /// Identifiant de la mission préparée.
        id: String,
    },
    /// Annule une tâche en cours.
    Cancel {
        /// Identifiant.
        id: String,
    },
    /// Publie dans vos documents les versions examinées d'une mission terminée.
    Apply {
        /// Identifiant.
        id: String,
    },
    /// Annule une publication effectuée, si vos documents n'ont pas changé depuis.
    Undo {
        /// Identifiant.
        id: String,
    },
}

#[derive(Debug, Subcommand)]
enum LogAction {
    /// Affiche les derniers événements.
    Tail {
        /// Nombre d'événements.
        #[arg(short, long, default_value_t = 20)]
        number: usize,
    },
    /// Rejoue une tâche, étape par étape.
    Replay {
        /// Identifiant de la tâche.
        task: String,
    },
    /// Vérifie l'intégrité du journal.
    Verify,
}

#[derive(Debug, Subcommand)]
enum CapAction {
    /// Approbations en attente.
    Approvals,
    /// Accorde une approbation.
    Approve {
        /// Identifiant de la demande.
        id: String,
        /// Portée : once, task ou agent.
        #[arg(long, default_value = "once")]
        scope: String,
    },
    /// Refuse une approbation.
    Deny {
        /// Identifiant de la demande.
        id: String,
    },
    /// Règles d'approbation permanentes.
    Rules,
    /// Révoque une tâche et tous ses descendants.
    Revoke {
        /// Identifiant de la tâche.
        task: String,
    },
}

#[derive(Debug, Subcommand)]
enum ProviderAction {
    /// Liste les pilotes et leur état.
    Ls,
    /// Vérifie la version et la connexion auprès du client officiel lui-même.
    Doctor {
        /// Nom du pilote (codex, claude-code ou gemini).
        driver: String,
    },
    /// Modèles réellement chargés par un moteur local.
    Models {
        /// Base d'API du moteur local.
        #[arg(long, default_value = "http://127.0.0.1:8080/v1")]
        endpoint: String,
    },
    /// Adresse un message à un LLM local, sans lui donner d'outils système.
    Chat {
        /// Identifiant annoncé par le moteur.
        #[arg(long)]
        model: String,
        /// Message envoyé au modèle.
        prompt: String,
        /// Base d'API du moteur local.
        #[arg(long, default_value = "http://127.0.0.1:8080/v1")]
        endpoint: String,
        /// Durée maximale de la requête, en secondes.
        #[arg(long, default_value_t = 120)]
        timeout: u64,
        /// Nombre maximal de tokens générés.
        #[arg(long, default_value_t = 2048)]
        max_tokens: u32,
    },
    /// Explique comment connecter un pilote.
    Login {
        /// Nom du pilote.
        driver: String,
    },
}

#[derive(Debug, Subcommand)]
enum MemoryAction {
    /// Liste les entrées d'un espace.
    Ls {
        /// Espace.
        #[arg(default_value = "work")]
        space: String,
    },
    /// Cherche dans un espace.
    Search {
        /// Texte recherché.
        query: String,
        /// Espace.
        #[arg(long, default_value = "work")]
        space: String,
    },
    /// Oublie une entrée.
    Forget {
        /// Identifiant.
        id: String,
    },
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(sortie) => {
            print!("{sortie}");
            std::process::ExitCode::SUCCESS
        }
        Err(erreur) => {
            eprintln!("prophet : {erreur}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> anyhow::Result<String> {
    match &cli.command {
        Command::Status => {
            let taches = taches_en_cours(&socket_agentd()).unwrap_or_default();
            let refs: Vec<&agentd::Task> = taches.iter().collect();
            let navigateur = navigateur_pilote(&socket_agentd());
            Ok(shell::status_complet(
                &sandboxd::Capabilities::probe(),
                &sfs::detect_backend(&home()),
                &refs,
                0,
                &services(),
                &navigateur
                    .as_ref()
                    .map_or(shell::Navigateur::Inconnu, |etat| {
                        etat.as_ref()
                            .map_or(shell::Navigateur::Aucun, shell::Navigateur::Sonde)
                    }),
            ))
        }
        Command::Freeze => {
            let socket = std::env::var("PROPHET_SANDBOXD_SOCKET").map_or_else(
                |_| prophet_ipc::socket_path("sandboxd"),
                std::path::PathBuf::from,
            );
            freeze(&socket, cli.json)
        }
        Command::Provider { action } => provider(action, cli.json),
        Command::Memory { action } => memory(action),
        Command::Log { action } => log(action),
        Command::Task { action } => task(action, cli.json),
        // Les approbations vivent dans un daemon en service : sans lui, la commande le dit au
        // lieu de faire semblant.
        Command::Cap { .. } => anyhow::bail!(
            "les approbations exigent capd en service. \
             Lancez `prophet status` pour voir ce qui est disponible sur cette machine."
        ),
    }
}

fn freeze(socket: &std::path::Path, as_json: bool) -> anyhow::Result<String> {
    let result = sous_delai(async {
        let client = prophet_ipc::Client::connect(socket)
            .await
            .map_err(|e| format!("gel non effectué : sandboxd injoignable ({e})"))?;
        client
            .call("sandbox.freeze_all", serde_json::json!({}))
            .await
            .map_err(|e| format!("gel non confirmé : {}", e.message))
    })
    .map_err(anyhow::Error::msg)?;
    let frozen = result["frozen"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("réponse de gel invalide"))?;
    let errors = result["errors"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("réponse de gel invalide"))?;
    if !errors.is_empty() {
        anyhow::bail!(
            "gel partiel : {} sandbox(es) gelée(s), {} échec(s) : {errors:?}",
            frozen.len(),
            errors.len()
        );
    }
    if as_json {
        Ok(format!("{result}\n"))
    } else {
        Ok(format!("{} sandbox(es) gelée(s)\n", frozen.len()))
    }
}

/// Journal d'audit.
///
/// Deux journaux existent sur une machine Prophet OS, et les confondre est pire que de n'en avoir
/// aucun :
///
/// - celui du **service**, dans `/var/lib/prophet/ledger`, écrit par `prophet-ledger`. C'est le
///   seul où passent les tâches, les capacités et les sandboxes d'une machine en service ;
/// - celui d'un **développement sans daemon**, dans `~/.prophet/ledger`.
///
/// Cette commande ne connaissait que le second. Sur la machine installée, où le service enregistre
/// tout depuis le démarrage, elle répondait « aucun journal sur cette machine » — et le test en
/// machine virtuelle l'a dit en ces termes, juste après avoir vu une sandbox démarrer et le
/// service l'inscrire. C'est la pire des trois réponses possibles pour un journal d'audit : non
/// pas « je ne sais pas », mais « il n'y a rien ».
///
/// L'ordre est donc : le **service** d'abord. Lui seul connaît l'état courant, et lui seul sert
/// les membres de `prophet-system` — qui ne peuvent pas lire son état, fermé en 0700 pour que
/// personne ne puisse réécrire l'histoire par le fichier. Les **fichiers** ensuite, parce qu'un
/// journal d'audit doit rester lisible quand plus rien ne tourne : c'est précisément après un
/// incident qu'on en a besoin.
fn log(action: &LogAction) -> anyhow::Result<String> {
    match ou_est_le_journal() {
        Journal::Service(socket) => log_par_le_service(&socket, action),
        Journal::Fichiers(racine) => log_par_les_fichiers(&racine, action),
        Journal::Aucun(essais) => Ok(format!(
            "aucun journal lisible depuis ici.\n\nCe qui a été tenté :\n{essais}\n\n\
             Si les services tournent, `prophet status` le dira. Le journal du service se lit par \
             lui, pas par son répertoire : il est fermé en 0700 pour que personne ne puisse \
             réécrire l'histoire en écrivant dans le fichier.\n"
        )),
    }
}

/// D'où cette commande lit le journal, et pourquoi.
enum Journal {
    /// Le service répond : on lui demande.
    Service(std::path::PathBuf),
    /// Pas de service, mais un répertoire lisible.
    Fichiers(std::path::PathBuf),
    /// Ni l'un ni l'autre, avec le détail de chaque tentative.
    Aucun(String),
}

/// Cherche le journal, dans l'ordre où il a le plus de chances d'être à jour.
///
/// Chaque échec est **conservé**, pas avalé. « Aucun journal » sans dire où l'on a regardé oblige
/// celui qui lit à deviner entre un service arrêté, un répertoire absent et une permission
/// refusée — trois situations qui n'appellent pas du tout la même chose.
fn ou_est_le_journal() -> Journal {
    let socket = std::env::var("PROPHET_LEDGER_SOCKET").map_or_else(
        |_| prophet_ipc::socket_path("ledger"),
        std::path::PathBuf::from,
    );
    let mut essais = Vec::new();
    match repond("ledger", &socket) {
        Ok(()) => return Journal::Service(socket),
        Err(raison) => essais.push(format!("  le service, sur {} : {raison}", socket.display())),
    }

    let force = std::env::var("PROPHET_LEDGER_DIR")
        .ok()
        .map(std::path::PathBuf::from);
    for racine in candidats_de_journal(force.as_deref(), &home()) {
        // `read_dir` plutôt que `exists` : un répertoire présent mais fermé n'est pas un journal
        // qu'on peut lire, et le dire ici évite une erreur plus loin, sans contexte.
        match std::fs::read_dir(&racine) {
            Ok(_) => return Journal::Fichiers(racine),
            Err(e) => essais.push(format!("  {} : {e}", racine.display())),
        }
    }
    Journal::Aucun(essais.join("\n"))
}

/// Les répertoires où un journal peut vivre, dans l'ordre où on les essaie.
///
/// Séparée de la recherche elle-même pour être vérifiable : la liste est ce qui a manqué, et une
/// liste se teste sans machine, sans daemon et sans `/var`.
fn candidats_de_journal(
    force: Option<&std::path::Path>,
    home: &std::path::Path,
) -> Vec<std::path::PathBuf> {
    let mut candidats = Vec::new();
    if let Some(chemin) = force {
        candidats.push(chemin.to_path_buf());
    }
    // Celui du service d'abord : sur une machine en service, c'est le seul qui dise la vérité.
    candidats.push(std::path::PathBuf::from("/var/lib/prophet/ledger"));
    candidats.push(home.join(".prophet/ledger"));
    candidats
}

/// Le journal tel que le service le tient.
fn log_par_le_service(socket: &std::path::Path, action: &LogAction) -> anyhow::Result<String> {
    match action {
        LogAction::Tail { number } => {
            let events = evenements(socket, serde_json::json!({}))?;
            Ok(rendre_tail(&events, *number))
        }
        LogAction::Replay { task } => {
            let events = evenements(socket, serde_json::json!({ "task": task }))?;
            Ok(shell::timeline(task, &events))
        }
        LogAction::Verify => {
            // La vérification est faite par celui qui détient la chaîne : il a les sceaux et la
            // clé publique. La refaire ici sur une copie partielle dirait moins.
            let brut = appel_au_journal(socket, "ledger.verify", serde_json::json!({}))?;
            let rapport: ledger::VerifyReport = serde_json::from_value(brut)
                .map_err(|e| anyhow::anyhow!("rapport de vérification illisible : {e}"))?;
            rendre_verification(&rapport)
        }
    }
}

/// Le journal tel qu'il est sur le disque, sans daemon.
fn log_par_les_fichiers(racine: &std::path::Path, action: &LogAction) -> anyhow::Result<String> {
    let store = ledger::Store::open(racine)?;
    match action {
        LogAction::Tail { number } => Ok(rendre_tail(&store.read_all()?, *number)),
        LogAction::Replay { task } => Ok(shell::timeline(task, &store.read_all()?)),
        LogAction::Verify => rendre_verification(&store.verify()?),
    }
}

/// Les événements que le service veut bien rendre, selon un filtre.
fn evenements(
    socket: &std::path::Path,
    filtre: serde_json::Value,
) -> anyhow::Result<Vec<prophet_types::ledger::Event>> {
    let brut = appel_au_journal(socket, "ledger.query", filtre)?;
    serde_json::from_value(brut).map_err(|e| anyhow::anyhow!("réponse du journal illisible : {e}"))
}

/// Combien de temps on laisse au journal pour répondre.
///
/// Plus que pour une sonde : `ledger.query` relit des fichiers, et un journal de plusieurs mois
/// n'est pas un « pong ». Moins que l'infini : une commande d'audit qui ne rend pas la main est
/// une commande d'audit qu'on n'utilisera pas au moment où elle compte.
const DELAI_DU_JOURNAL: std::time::Duration = std::time::Duration::from_secs(30);

fn appel_au_journal(
    socket: &std::path::Path,
    methode: &str,
    params: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let execution = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    execution.block_on(async {
        let travail = async {
            let client = prophet_ipc::Client::connect(socket).await.map_err(|e| {
                anyhow::anyhow!("journal injoignable sur {} : {e}", socket.display())
            })?;
            client
                .call(methode, params)
                .await
                .map_err(|e| anyhow::anyhow!("{} : {}", methode, e.message))
        };
        tokio::time::timeout(DELAI_DU_JOURNAL, travail)
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "le journal n'a pas répondu en {} s",
                    DELAI_DU_JOURNAL.as_secs()
                )
            })?
    })
}

/// Les `n` derniers événements — les derniers, pas les premiers.
///
/// La troncature se fait ici et non dans le filtre envoyé au service : `ledger.query` coupe par le
/// début. Lui demander « vingt » rendrait les vingt **premiers** événements de la machine, ce qui
/// ressemble à une réponse et n'en est pas une.
fn rendre_tail(events: &[prophet_types::ledger::Event], number: usize) -> String {
    let depart = events.len().saturating_sub(number);
    let derniers = &events[depart..];
    if derniers.is_empty() {
        return "journal vide\n".to_owned();
    }
    let mut out = String::new();
    for event in derniers {
        out.push_str(&format!(
            "{:>8}  {:<22} {}\n",
            event.seq,
            shell::kind_label(event.kind),
            event.task.clone().unwrap_or_default()
        ));
    }
    out
}

fn rendre_verification(rapport: &ledger::VerifyReport) -> anyhow::Result<String> {
    if rapport.ok {
        Ok(format!(
            "journal intact : {} événements, {} sceaux vérifiés\n",
            rapport.checked, rapport.seals
        ))
    } else {
        anyhow::bail!(
            "journal altéré à la séquence {} : {}",
            rapport.first_bad_seq.unwrap_or(0),
            rapport.reason.clone().unwrap_or_default()
        )
    }
}

/// Missions du service. Les captures d'agentd restent privées ; seule la liste historique
/// hors service consulte encore le disque directement. Publier et annuler passent par agentd,
/// qui exige le créateur de la mission et l'index exact qu'il a examiné.
fn task(action: &TaskAction, as_json: bool) -> anyhow::Result<String> {
    let maison = home();
    match action {
        TaskAction::New { request } => {
            use std::io::Read as _;
            let mut bytes = Vec::new();
            std::fs::File::open(request)?
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            anyhow::ensure!(
                bytes.len() <= 1024 * 1024,
                "requête de mission limitée à 1 Mio"
            );
            let params = serde_json::from_slice(&bytes)?;
            let result = task_rpc(&socket_agentd(), "task.spawn", params)?;
            if as_json {
                return Ok(format!("{}\n", serde_json::to_string_pretty(&result)?));
            }
            let plan: agentd::TaskPlan = serde_json::from_value(result)?;
            Ok(format!(
                "{}\nDémarrer : prophet task start {}\n",
                plan.render(),
                plan.task
            ))
        }
        TaskAction::Start { id } => {
            let result = task_rpc(&socket_agentd(), "task.start", serde_json::json!({"id":id}))?;
            if as_json {
                Ok(format!("{}\n", serde_json::to_string_pretty(&result)?))
            } else {
                Ok(format!(
                    "Mission {id} lancée. Suivi : prophet task ls ; résultat : prophet task result {id}\n"
                ))
            }
        }
        TaskAction::Result { id } => {
            let result = task_rpc(
                &socket_agentd(),
                "task.result",
                serde_json::json!({"id":id}),
            )?;
            if as_json {
                return Ok(format!("{}\n", serde_json::to_string_pretty(&result)?));
            }
            let mut out = format!(
                "Mission {id} · {}\n",
                result["state"].as_str().unwrap_or("inconnu")
            );
            if let Some(text) = result["text"].as_str() {
                out.push_str(text);
                out.push('\n');
            }
            if let Some(reason) = result["reason"].as_str() {
                out.push_str(reason);
                out.push('\n');
            }
            if let Some(diff) = result.get("diff") {
                let diff: sfs::Diff = serde_json::from_value(diff.clone())?;
                out.push_str(&diff.render());
                out.push_str("Changements conservés dans le travail ; validation non appliquée.\n");
            }
            Ok(out)
        }
        TaskAction::Ls => {
            if as_json {
                return Ok(format!(
                    "{}\n",
                    serde_json::to_string_pretty(&task_rpc(
                        &socket_agentd(),
                        "task.list",
                        serde_json::json!({})
                    )?)?
                ));
            }
            // Le service conserve aussi les missions terminées. Ne pas tenter ensuite de lire
            // ses captures privées : leur refus d'accès annulait une liste pourtant reçue.
            let mut out = String::new();
            match taches_en_cours(&socket_agentd()) {
                Ok(taches) if taches.is_empty() => {
                    return Ok("aucune tâche connue du service\n".into());
                }
                Ok(taches) => {
                    out.push_str(&format!(
                        "{:<30} {:<12} {:<14} {}\n",
                        "tâche", "état", "pilote", "intention"
                    ));
                    for tache in &taches {
                        out.push_str(&format!(
                            "{:<30} {:<12} {:<14} {}\n",
                            tache.id,
                            format!("{:?}", tache.state).to_lowercase(),
                            tache.driver.as_deref().unwrap_or("—"),
                            tache.intent,
                        ));
                    }
                    return Ok(out);
                }
                Err(raison) => {
                    // Le dire, plutôt que d'afficher les seuls espaces de travail comme si c'était
                    // toute la vérité.
                    out.push_str(&format!("tâches en cours : indisponibles ({raison})\n\n"));
                }
            }

            let espaces = sfs::Workspace::list(&maison)?;
            if espaces.is_empty() {
                out.push_str(
                    "aucun espace de travail : aucune tâche n'a encore modifié de fichier\n",
                );
                return Ok(out);
            }
            out.push_str(&format!(
                "{:<30} {:<22} {}\n",
                "tâche", "espace de travail", "changements"
            ));
            for (id, state) in espaces {
                let changements = sfs::Workspace::open(&maison, &id)
                    .and_then(|w| w.diff())
                    .map(|d| {
                        let (a, m, s) = d.counts();
                        format!("{a} ajouté(s), {m} modifié(s), {s} supprimé(s)")
                    })
                    .unwrap_or_else(|_| "illisible".to_owned());
                out.push_str(&format!(
                    "{id:<30} {:<22} {changements}\n",
                    format!("{state:?}")
                ));
            }
            Ok(out)
        }
        TaskAction::Show { id } | TaskAction::Diff { id } => {
            let inspection: agentd::Inspection = serde_json::from_value(task_rpc(
                &socket_agentd(),
                "task.inspect",
                serde_json::json!({"id":id}),
            )?)?;
            anyhow::ensure!(inspection.task.id == *id, "réponse pour une autre mission");
            if matches!(action, TaskAction::Show { .. }) && as_json {
                return Ok(format!("{}\n", serde_json::to_string_pretty(&inspection)?));
            }
            let diff = inspection
                .result
                .as_ref()
                .and_then(|result| result.get("diff"))
                .cloned()
                .map(serde_json::from_value::<sfs::Diff>)
                .transpose()?;
            if matches!(action, TaskAction::Diff { .. }) {
                let diff = diff.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Changements non disponibles pour {id}. Consultez `prophet task show {id}`."
                    )
                })?;
                return if as_json {
                    Ok(format!("{}\n", serde_json::to_string_pretty(&diff)?))
                } else {
                    Ok(format!("{}Changements non appliqués.\n", diff.render()))
                };
            }
            let mut out = format!(
                "Mission {id} : {}\nÉtat : {:?}\n",
                inspection.task.intent, inspection.task.state
            );
            if let Some(publication) = inspection.publication {
                out.push_str(&format!(
                    "Publication : {}\n",
                    publication_lisible(publication)
                ));
            }
            if inspection.can_apply {
                out.push_str(&format!(
                    "Pour publier ces versions dans vos documents : prophet task apply {id}\n"
                ));
            }
            if inspection.can_undo {
                out.push_str(&format!(
                    "Pour annuler cette publication : prophet task undo {id}\n"
                ));
            }
            if let Some(plan) = inspection.plan {
                out.push_str(&plan.render());
            }
            if let Some(reason) = inspection.task.reason {
                out.push_str(&format!("{reason}\n"));
            }
            if let Some(result) = inspection.result
                && let Some(text) = result["text"].as_str()
            {
                out.push_str(&format!("{text}\n"));
            }
            if let Some(diff) = diff {
                out.push_str(&diff.render());
                out.push_str("Changements non appliqués.\n");
            } else {
                out.push_str("Changements non disponibles.\n");
            }
            Ok(out)
        }
        // Les deux commandes passent par agentd : lui seul connaît le créateur de la mission
        // et l'index exact qu'il a examiné. La bibliothèque refuse un document retouché depuis.
        TaskAction::Apply { id } | TaskAction::Undo { id } => {
            let apply = matches!(action, TaskAction::Apply { .. });
            let result = task_rpc(
                &socket_agentd(),
                if apply { "task.apply" } else { "task.undo" },
                serde_json::json!({"id":id}),
            )?;
            if as_json {
                return Ok(format!("{}\n", serde_json::to_string_pretty(&result)?));
            }
            let key = if apply { "applied" } else { "undone" };
            anyhow::ensure!(
                result[key].as_str() == Some(id.as_str()),
                "réponse pour une autre mission"
            );
            let changes = &result["changes"];
            let (a, m, s) = (
                changes["added"].as_u64().unwrap_or(0),
                changes["modified"].as_u64().unwrap_or(0),
                changes["deleted"].as_u64().unwrap_or(0),
            );
            Ok(if apply {
                format!(
                    "Mission {id} : versions publiées dans vos documents : {a} ajout(s), {m} modification(s), {s} suppression(s)\n"
                )
            } else {
                format!(
                    "Mission {id} : publication annulée : {a} ajout(s) retiré(s), {m} modification(s) rétablie(s), {s} suppression(s) rétablie(s)\n"
                )
            })
        }
        TaskAction::Options => {
            let options: agentd::preparation::Options = serde_json::from_value(task_rpc(
                &socket_agentd(),
                "task.options",
                serde_json::json!({}),
            )?)?;
            if as_json {
                return Ok(format!("{}\n", serde_json::to_string_pretty(&options)?));
            }
            let mut out = String::from("Contextes de mission\n");
            if options.profiles.is_empty() {
                out.push_str(
                    "  aucun : le service ne charge aucun catalogue (PROPHET_MISSION_PROFILES)\n",
                );
            }
            for profile in &options.profiles {
                let models = if profile.models.is_empty() {
                    "aucun modèle disponible".to_owned()
                } else {
                    profile.models.join(", ")
                };
                out.push_str(&format!(
                    "  {} — {}{}\n      {}\n      modèles : {} · périmètres : {}\n",
                    profile.id,
                    profile.name,
                    if profile.web {
                        " (consulte le web)"
                    } else {
                        ""
                    },
                    profile.description,
                    models,
                    profile.scopes.join(", ")
                ));
            }
            if let Some(error) = &options.model_error {
                out.push_str(&format!("  moteur indisponible : {error}\n"));
            }
            match &options.browser {
                None => out.push_str("Navigateur piloté : aucun (PROPHET_BROWSER absent)\n"),
                Some(state) => out.push_str(&format!(
                    "Navigateur piloté : {} {} — {}\n",
                    if state.ready { "✓" } else { "✗" },
                    state.program,
                    state.detail
                )),
            }
            out.push_str("Préparer : prophet task prepare --profile <contexte> --model <modèle> \"<objectif>\"\n");
            Ok(out)
        }
        TaskAction::Prepare {
            profile,
            model,
            client,
            id,
            intent,
        } => {
            let id = id
                .clone()
                .unwrap_or_else(|| format!("mission-{}", ulid::Ulid::new()));
            let model = match model {
                Some(model) => model.clone(),
                None if *client => {
                    // Le client apporte son modèle ; le contexte doit seulement en admettre un.
                    let options: agentd::preparation::Options = serde_json::from_value(task_rpc(
                        &socket_agentd(),
                        "task.options",
                        serde_json::json!({}),
                    )?)?;
                    options
                        .profiles
                        .iter()
                        .find(|p| p.id == *profile)
                        .and_then(|p| p.preferred.first().cloned())
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "contexte {profile} inconnu ou sans modèle admis ; voir `prophet task options`"
                            )
                        })?
                }
                None => anyhow::bail!(
                    "--model est requis sans --client ; les modèles disponibles sont dans `prophet task options`"
                ),
            };
            let mut params =
                serde_json::json!({"id":id, "intent":intent, "profile":profile, "model":model});
            if *client {
                params["client"] = serde_json::json!(true);
            }
            let result = task_rpc(&socket_agentd(), "task.prepare", params)?;
            if as_json {
                return Ok(format!("{}\n", serde_json::to_string_pretty(&result)?));
            }
            let plan: agentd::TaskPlan = serde_json::from_value(result)?;
            Ok(format!(
                "{}\nDémarrer : prophet task start {id}\nClient MCP : prophet task mcp-config {id}\n",
                plan.render()
            ))
        }
        TaskAction::McpConfig { id } => {
            // La mission doit exister pour ce créateur ; le pont ne fait que la servir.
            let etat = task_rpc(
                &socket_agentd(),
                "task.inspect",
                serde_json::json!({"id":id}),
            )?;
            if etat["task"]["state"] != "planned" {
                anyhow::bail!(
                    "la mission {id} est {} ; seule une mission préparée et non lancée accueille un client",
                    etat["task"]["state"]
                        .as_str()
                        .unwrap_or("dans un état inconnu")
                );
            }
            let pont = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|d| d.join("prophet-mcp")))
                .filter(|p| p.is_file())
                .map_or_else(|| "prophet-mcp".to_owned(), |p| p.display().to_string());
            let config = serde_json::json!({
                "mcpServers": {
                    "prophet": {"command": pont, "args": [], "env": {"PROPHET_TASK": id}}
                }
            });
            Ok(format!("{}\n", serde_json::to_string_pretty(&config)?))
        }
        TaskAction::Cancel { id } => {
            let result = task_rpc(
                &socket_agentd(),
                "task.cancel",
                serde_json::json!({"id":id}),
            )
            .map_err(|e| {
                anyhow::anyhow!(
                    "{e} ; pour défaire une tâche déjà validée, utilisez `prophet task undo`"
                )
            })?;
            if as_json {
                Ok(format!("{}\n", serde_json::to_string_pretty(&result)?))
            } else if result.get("cancel_requested").is_some() {
                Ok(format!(
                    "Annulation demandée pour {id}. L'état final confirmera l'arrêt.\n"
                ))
            } else {
                Ok(format!("Mission {id} annulée.\n"))
            }
        }
    }
}

fn provider(action: &ProviderAction, as_json: bool) -> anyhow::Result<String> {
    use providers::local::LocalModel;
    use providers::native::{ModelClient as _, ModelTurn};
    use providers::official::{ClientProfile, OfficialDriver};
    let racine = home().join(".local/state/prophet");
    let utilisateur = std::env::var("USER").unwrap_or_else(|_| "inconnu".to_owned());
    match action {
        ProviderAction::Models { endpoint } => {
            let client = LocalModel::new(endpoint, "discovery", std::time::Duration::from_secs(5))?;
            let models = client.models()?;
            if as_json {
                Ok(format!("{}\n", serde_json::to_string(&models)?))
            } else if models.is_empty() {
                Ok("aucun modèle chargé\n".into())
            } else {
                Ok(format!("{}\n", models.join("\n")))
            }
        }
        ProviderAction::Chat {
            model,
            prompt,
            endpoint,
            timeout,
            max_tokens,
        } => {
            let mut client =
                LocalModel::new(endpoint, model, std::time::Duration::from_secs(*timeout))?
                    .with_max_tokens(*max_tokens)?;
            let started = std::time::Instant::now();
            let (turn, usage) =
                client.next_turn(&[serde_json::json!({"role":"user", "content":prompt})])?;
            let ModelTurn::Final { text } = turn else {
                anyhow::bail!("aucun outil n'est disponible dans cette conversation");
            };
            if as_json {
                Ok(format!(
                    "{}\n",
                    serde_json::json!({"model":model, "text":text,
                    "usage":usage, "elapsed_ms":started.elapsed().as_millis()})
                ))
            } else {
                Ok(format!("{text}\n"))
            }
        }
        ProviderAction::Ls => {
            let diagnostics: Vec<_> = ClientProfile::all()
                .into_iter()
                .map(|profile| OfficialDriver::new(profile, &racine, &utilisateur).diagnostic())
                .collect();
            if as_json {
                return Ok(format!(
                    "{}\n",
                    serde_json::json!({
                        "official_clients": diagnostics,
                        "local_runtime": {"driver": "prophet-agent", "integrated": true}
                    })
                ));
            }
            let mut out = format!(
                "{:<16} {:<16} {:<14} {}\n",
                "pilote", "auth souhaitée", "client", "connexion"
            );
            for diagnostic in diagnostics {
                out.push_str(&format!(
                    "{:<16} {:<16} {:<14} {}\n",
                    diagnostic.driver,
                    "abonnement",
                    if diagnostic.executable.is_some() {
                        "présent"
                    } else {
                        "absent"
                    },
                    diagnostic.connection.label()
                ));
            }
            out.push_str(&format!(
                "{:<16} {:<16} {:<14} {}\n",
                "prophet-agent", "aucune", "intégré", "sans objet"
            ));
            out.push_str("Exécution agentique des clients officiels : raccordement à réaliser.\n");
            Ok(out)
        }
        ProviderAction::Doctor { driver } => {
            let profile = ClientProfile::all()
                .into_iter()
                .find(|p| &p.driver == driver)
                .ok_or_else(|| anyhow::anyhow!("pilote inconnu : {driver}"))?;
            let diagnostic = OfficialDriver::new(profile, &racine, &utilisateur).diagnostic();
            if as_json {
                Ok(format!("{}\n", serde_json::to_string(&diagnostic)?))
            } else {
                Ok(format!(
                    "{} : {}\nVersion : {}\nConnexion : {}\nExécution agentique : raccordement à réaliser\n",
                    diagnostic.driver,
                    diagnostic
                        .executable
                        .as_deref()
                        .map_or_else(|| "client absent".into(), |path| path.display().to_string()),
                    diagnostic.version.as_deref().unwrap_or("non vérifiée"),
                    diagnostic.connection.label()
                ))
            }
        }
        ProviderAction::Login { driver } => {
            let profile = ClientProfile::all()
                .into_iter()
                .find(|p| &p.driver == driver)
                .ok_or_else(|| anyhow::anyhow!("pilote inconnu : {driver}"))?;
            let instructions =
                OfficialDriver::new(profile, &racine, &utilisateur).login_instructions();
            if as_json {
                Ok(format!(
                    "{}\n",
                    serde_json::json!({"driver": driver, "instructions": instructions})
                ))
            } else {
                Ok(format!("{instructions}\n"))
            }
        }
    }
}

fn memory(action: &MemoryAction) -> anyhow::Result<String> {
    use memoryd::{HashEmbedder, Query, Space, Store};
    let chemin = home().join(".prophet/memoire.db");
    let store = Store::open(&chemin, Box::new(HashEmbedder::default()))?;
    match action {
        MemoryAction::Ls { space } => {
            let entries = store.list(&Space::new(space))?;
            if entries.is_empty() {
                return Ok(format!("aucune entrée dans l'espace {space}\n"));
            }
            let mut out = String::new();
            for entry in entries {
                out.push_str(&format!("{}  {}\n", entry.id, entry.text));
            }
            Ok(out)
        }
        MemoryAction::Search { query, space } => {
            let resultats = store.search(&Query::in_space(Space::new(space), query))?;
            if resultats.is_empty() {
                return Ok("aucun résultat\n".to_owned());
            }
            let mut out = String::new();
            for entry in resultats {
                out.push_str(&format!(
                    "{:.2}  {}\n",
                    entry.score.unwrap_or(0.0),
                    entry.text
                ));
            }
            Ok(out)
        }
        MemoryAction::Forget { id } => {
            if store.forget(id)? {
                Ok(format!("{id} oublié\n"))
            } else {
                anyhow::bail!("entrée inconnue : {id}")
            }
        }
    }
}

fn home() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/root"))
}

/// Combien de temps on laisse à un service pour répondre à une sonde.
///
/// `prophet status` doit rendre la main. Un service qui met plus de deux secondes à dire qu'il est
/// là est, du point de vue de celui qui regarde son écran, un service en panne — et l'afficher
/// ainsi est plus utile qu'une invite qui ne revient jamais.
const DELAI_DE_SONDE: std::time::Duration = std::time::Duration::from_secs(2);

/// Lesquels des services répondent, et pourquoi les autres ne répondent pas.
///
/// On parle à chacun, on ne regarde pas si son fichier de socket existe — c'est la seule chose qui
/// distingue « le service est déclaré » de « le service sert », et l'histoire de ce dépôt montre
/// que confondre les deux coûte cher.
///
/// Chacun est interrogé **dans sa langue**. Six parlent JSON-RPC ; `egress` est un proxy HTTP.
fn services() -> Vec<(String, Option<String>)> {
    [
        "capd", "ledger", "vault", "egress", "sandboxd", "memoryd", "agentd",
    ]
    .into_iter()
    .map(|nom| {
        let socket = prophet_ipc::socket_path(nom);
        (nom.to_owned(), repond(nom, &socket).err())
    })
    .collect()
}

/// Le service répond-il, dans le délai qu'on lui laisse ?
fn repond(nom: &str, socket: &std::path::Path) -> Result<(), String> {
    let egress = nom == "egress";
    sous_delai(async move {
        if egress {
            sonde_http(socket).await
        } else {
            sonde_jsonrpc(socket).await
        }
    })
}

/// Un `ping` JSON-RPC, la langue des six daemons.
async fn sonde_jsonrpc(socket: &std::path::Path) -> Result<(), String> {
    let client = prophet_ipc::Client::connect(socket)
        .await
        .map_err(|_| "socket injoignable".to_owned())?;
    client
        .call("ping", serde_json::json!({}))
        .await
        .map(|_| ())
        .map_err(|e| e.message.clone())
}

/// `egress` ne parle pas JSON-RPC : c'est un proxy HTTP.
///
/// Lui envoyer un `ping` JSON-RPC revient à lui envoyer une requête HTTP tronquée. Il attend la
/// ligne vide qui termine les en-têtes ; elle ne vient jamais, et personne n'est en faute — le
/// proxy fait exactement son travail. C'est `prophet status` qui ne rendait plus la main, quinze
/// minutes durant, sans rien afficher du tout. Une commande d'état qui se tait est pire qu'une
/// commande d'état qui annonce une panne : elle n'apprend rien et elle bloque le terminal.
///
/// On lui parle donc sa langue. La requête n'a pas de jeton : elle est refusée par un `407` avant
/// que rien ne sorte de la machine. La sonde prouve ainsi davantage qu'un `pong` — que la règle
/// « rien ne sort d'ici sans qu'on sache pour qui » est bien en place.
async fn sonde_http(socket: &std::path::Path) -> Result<(), String> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let mut flux = tokio::net::UnixStream::connect(socket)
        .await
        .map_err(|_| "socket injoignable".to_owned())?;
    flux.write_all(
        b"GET http://sonde.prophet.invalid/ HTTP/1.1\r\nHost: sonde.prophet.invalid\r\n\r\n",
    )
    .await
    .map_err(|e| e.to_string())?;

    let mut tete = [0u8; 64];
    let lus = flux.read(&mut tete).await.map_err(|e| e.to_string())?;
    let tete = String::from_utf8_lossy(&tete[..lus]);
    // Le code exact compte : un proxy qui laisserait passer une requête sans jeton répondrait
    // autre chose, et ce serait une panne bien plus grave qu'un silence.
    if tete.starts_with("HTTP/1.1 407") {
        Ok(())
    } else {
        Err(format!(
            "refus 407 attendu pour une requête sans jeton, obtenu : {}",
            tete.lines().next().unwrap_or("(rien)")
        ))
    }
}

/// Exécute un échange avec un daemon, et abandonne s'il dure trop.
///
/// Le délai est posé ici, une fois, plutôt que dans chaque appel : un seul appel oublié suffirait
/// à rendre `prophet status` inutilisable, et c'est exactement ce qui est arrivé.
fn sous_delai<T>(
    travail: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    let execution = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    execution.block_on(async {
        tokio::time::timeout(DELAI_DE_SONDE, travail)
            .await
            .map_err(|_| format!("pas de réponse en {} s", DELAI_DE_SONDE.as_secs()))?
    })
}

/// Où joindre `agentd`. La variable d'environnement sert aux tests et aux développements ; sur une
/// machine installée, c'est le chemin conventionnel.
fn socket_agentd() -> std::path::PathBuf {
    std::env::var("PROPHET_AGENTD_SOCKET").map_or_else(
        |_| prophet_ipc::socket_path("agentd"),
        std::path::PathBuf::from,
    )
}

/// L'état de publication SFS, dit à l'humain.
fn publication_lisible(state: sfs::WorkspaceState) -> &'static str {
    use sfs::WorkspaceState as W;
    match state {
        W::Open => "versions examinables, non appliquées",
        W::Applying => "publication interrompue ; `prophet task apply` la reprend",
        W::Undoing => "annulation interrompue ; `prophet task undo` la reprend",
        W::Conflict => "interrompue sur un conflit ; les fichiers déplacés sont conservés",
        W::Committed => "versions publiées dans vos documents",
        W::RolledBack => "publication annulée, documents initiaux rétablis",
        W::Abandoned => "travail abandonné sans publication",
    }
}

fn task_rpc(
    socket: &std::path::Path,
    method: &str,
    params: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    sous_delai(async {
        let client = prophet_ipc::Client::connect(socket)
            .await
            .map_err(|e| format!("agentd indisponible : {e}"))?;
        client.call(method, params).await.map_err(|e| e.message)
    })
    .map_err(anyhow::Error::msg)
}

/// Les tâches que `agentd` tient en ce moment.
///
/// La CLI parle au daemon plutôt que de deviner : lui seul sait ce qui est planifié, en cours ou
/// en attente d'approbation. Quand il n'est pas là, on le dit — c'est une information, pas une
/// panne : sur une machine où rien ne tourne, `prophet log` et `prophet task show` restent utiles
/// parce qu'ils lisent des fichiers.
/// Le verdict de la sonde du navigateur piloté ; `None` si agentd ne répond pas, `Some(None)`
/// s'il n'en configure aucun.
fn navigateur_pilote(
    socket: &std::path::Path,
) -> Option<Option<agentd::preparation::BrowserState>> {
    sous_delai(async {
        let client = prophet_ipc::Client::connect(socket)
            .await
            .map_err(|e| e.to_string())?;
        let options = client
            .call("task.options", serde_json::json!({}))
            .await
            .map_err(|e| e.message.clone())?;
        let options: agentd::preparation::Options =
            serde_json::from_value(options).map_err(|e| e.to_string())?;
        Ok(options.browser)
    })
    .ok()
}

fn taches_en_cours(socket: &std::path::Path) -> Result<Vec<agentd::Task>, String> {
    sous_delai(async {
        let client = prophet_ipc::Client::connect(socket)
            .await
            .map_err(|_| format!("agentd ne répond pas sur {}", socket.display()))?;
        let brut = client
            .call("task.list", serde_json::json!({}))
            .await
            .map_err(|e| e.message.clone())?;
        serde_json::from_value(brut).map_err(|e| format!("réponse illisible : {e}"))
    })
}

#[cfg(test)]
mod sondes {
    //! Ce que `prophet status` doit à celui qui la tape : revenir.
    //!
    //! La commande a bloqué quinze minutes dans le test en machine virtuelle, sans rien afficher.
    //! La cause n'était pas une panne : `egress` est un proxy HTTP, on lui parlait JSON-RPC, et il
    //! attendait sagement la fin d'en-têtes qui ne viendraient jamais. Les deux fautes sont ici.

    use std::io::{BufRead as _, Read as _, Write as _};
    use std::os::unix::net::UnixListener;
    use std::path::Path;

    /// Lance un faux service qui accepte une connexion, lit ce qu'on lui envoie, et répond ce
    /// qu'on lui a dit de répondre. Rend ce qu'il a reçu.
    fn faux_service(
        socket: &Path,
        reponse: Option<&'static str>,
    ) -> std::sync::mpsc::Receiver<String> {
        let ecoute = UnixListener::bind(socket).expect("socket d'essai");
        let (envoi, reception) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let Ok((mut flux, _)) = ecoute.accept() else {
                return;
            };
            let mut lecteur = std::io::BufReader::new(flux.try_clone().expect("duplication"));
            let mut premiere = String::new();
            let _ = lecteur.read_line(&mut premiere);
            let _ = envoi.send(premiere);
            match reponse {
                Some(texte) => {
                    let _ = flux.write_all(texte.as_bytes());
                }
                // Le silence : exactement ce que faisait `egress` devant un `ping` JSON-RPC.
                None => {
                    let mut poubelle = Vec::new();
                    let _ = flux.read_to_end(&mut poubelle);
                }
            }
        });
        reception
    }

    /// Appelle `repond` sans risquer de suspendre la suite de tests elle-même.
    ///
    /// Le défaut qu'on vérifie ici est un blocage : un test qui l'attendrait sur son propre fil
    /// bloquerait à son tour, et un test bloqué n'est pas un test qui échoue.
    fn repond_en_temps_borne(nom: &'static str, socket: &Path) -> Result<(), String> {
        let socket = socket.to_path_buf();
        let (envoi, reception) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = envoi.send(super::repond(nom, &socket));
        });
        reception
            .recv_timeout(std::time::Duration::from_secs(15))
            .expect("la sonde doit rendre la main bien avant quinze secondes")
    }

    #[test]
    fn un_service_muet_est_declare_muet_au_lieu_de_suspendre_la_commande() {
        let temp = tempfile::tempdir().expect("répertoire temporaire");
        let socket = temp.path().join("muet.sock");
        let _recu = faux_service(&socket, None);

        let erreur = repond_en_temps_borne("capd", &socket)
            .expect_err("un service qui ne répond pas n'est pas un service qui va bien");
        assert!(
            erreur.contains("pas de réponse"),
            "et le motif doit être le délai, pas autre chose : {erreur}"
        );
    }

    #[test]
    fn egress_est_interroge_en_http_et_non_en_json_rpc() {
        // C'est toute la correction : `egress` est un proxy. Le `407` qu'il rend à une requête
        // sans jeton prouve davantage qu'un `pong` — que rien ne sort sans qu'on sache pour qui.
        let temp = tempfile::tempdir().expect("répertoire temporaire");
        let socket = temp.path().join("egress.sock");
        let recu = faux_service(
            &socket,
            Some("HTTP/1.1 407 Prophet\r\nContent-Length: 0\r\n\r\n"),
        );

        repond_en_temps_borne("egress", &socket).expect("un 407 est la bonne réponse");

        let demande = recu.recv().expect("la sonde a parlé");
        assert!(
            demande.starts_with("GET http://"),
            "la sonde doit envoyer une requête HTTP, obtenu : {demande:?}"
        );
        assert!(
            !demande.contains("jsonrpc"),
            "et surtout pas du JSON-RPC, qui est précisément ce qui la faisait bloquer : {demande:?}"
        );
    }

    #[test]
    fn un_proxy_qui_laisse_passer_une_requete_sans_jeton_n_est_pas_sain() {
        // Le piège serait de se contenter d'« une réponse est arrivée ». Un proxy qui répond `200`
        // à une requête sans jeton a laissé sortir quelque chose, et l'annoncer comme sain serait
        // pire que de le dire muet.
        let temp = tempfile::tempdir().expect("répertoire temporaire");
        let socket = temp.path().join("egress.sock");
        let _recu = faux_service(
            &socket,
            Some("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"),
        );

        let erreur = repond_en_temps_borne("egress", &socket).expect_err("200 n'est pas un refus");
        assert!(erreur.contains("407"), "{erreur}");
    }

    #[test]
    fn un_socket_absent_se_dit_tout_de_suite() {
        let erreur = repond_en_temps_borne("capd", Path::new("/nulle/part/capd.sock"))
            .expect_err("rien n'écoute là");
        assert!(erreur.contains("injoignable"), "{erreur}");
    }

    #[test]
    fn le_gel_utilise_le_gestionnaire_du_daemon() {
        let temp = tempfile::tempdir().unwrap();
        let socket = temp.path().join("sandboxd.sock");
        let recu = faux_service(
            &socket,
            Some(
                "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"frozen\":[\"task:1\",\"task:2\"],\"errors\":[]}}\n",
            ),
        );
        let result = super::freeze(&socket, true).unwrap();
        let request: serde_json::Value = serde_json::from_str(&recu.recv().unwrap()).unwrap();
        assert_eq!(request["method"], "sandbox.freeze_all");
        let response: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(response["frozen"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn un_gel_partiel_ou_un_daemon_absent_ne_passe_pas_pour_un_succes() {
        let temp = tempfile::tempdir().unwrap();
        let socket = temp.path().join("sandboxd.sock");
        assert!(
            super::freeze(&socket, false)
                .unwrap_err()
                .to_string()
                .contains("injoignable")
        );
        let _recu = faux_service(
            &socket,
            Some(
                "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"frozen\":[\"task:1\"],\"errors\":[{\"task\":\"task:2\",\"error\":\"refus\"}]}}\n",
            ),
        );
        assert!(
            super::freeze(&socket, false)
                .unwrap_err()
                .to_string()
                .contains("gel partiel")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory as _;

    #[test]
    fn la_ligne_de_commande_est_coherente() {
        Cli::command().debug_assert();
    }

    #[test]
    fn les_verbes_attendus_existent() {
        for arguments in [
            vec!["prophet", "status"],
            vec!["prophet", "freeze"],
            vec!["prophet", "task", "ls"],
            vec!["prophet", "task", "show", "task:01"],
            vec!["prophet", "task", "diff", "task:01"],
            vec!["prophet", "task", "cancel", "task:01"],
            vec!["prophet", "task", "apply", "task:01"],
            vec!["prophet", "task", "undo", "task:01"],
            vec!["prophet", "log", "tail"],
            vec!["prophet", "log", "replay", "task:01"],
            vec!["prophet", "log", "verify"],
            vec!["prophet", "cap", "approvals"],
            vec!["prophet", "cap", "approve", "apr:01", "--scope", "task"],
            vec!["prophet", "cap", "deny", "apr:01"],
            vec!["prophet", "cap", "revoke", "task:01"],
            vec!["prophet", "provider", "ls"],
            vec!["prophet", "provider", "login", "claude-code"],
            vec!["prophet", "memory", "search", "ventes"],
        ] {
            assert!(
                Cli::try_parse_from(&arguments).is_ok(),
                "commande refusée : {arguments:?}"
            );
        }
    }

    #[test]
    fn le_format_json_est_disponible_partout() {
        let cli = Cli::try_parse_from(["prophet", "--json", "task", "ls"]).unwrap();
        assert!(cli.json);
        let cli = Cli::try_parse_from(["prophet", "task", "ls", "--json"]).unwrap();
        assert!(cli.json);
    }

    #[test]
    fn le_statut_fonctionne_sans_daemon() {
        let cli = Cli::try_parse_from(["prophet", "status"]).unwrap();
        let sortie = run(&cli).unwrap();
        assert!(sortie.contains("Prophet OS"), "{sortie}");
        assert!(sortie.contains("Isolation"), "{sortie}");
    }

    #[test]
    fn la_liste_des_pilotes_fonctionne_sans_daemon() {
        let cli = Cli::try_parse_from(["prophet", "provider", "ls"]).unwrap();
        let sortie = run(&cli).unwrap();
        assert!(sortie.contains("claude-code"), "{sortie}");
        assert!(sortie.contains("codex"), "{sortie}");
        assert!(sortie.contains("prophet-agent"), "{sortie}");
        assert!(
            sortie.contains("abonnement"),
            "le mode d'authentification doit être visible : {sortie}"
        );
    }

    #[test]
    fn les_instructions_de_connexion_sont_explicites() {
        let cli = Cli::try_parse_from(["prophet", "provider", "login", "codex"]).unwrap();
        let sortie = run(&cli).unwrap();
        assert!(sortie.contains("codex login"), "{sortie}");
        assert!(sortie.contains("ne lit jamais"), "{sortie}");
    }

    #[test]
    fn un_pilote_inconnu_est_signale() {
        let cli = Cli::try_parse_from(["prophet", "provider", "login", "inexistant"]).unwrap();
        assert!(
            run(&cli)
                .unwrap_err()
                .to_string()
                .contains("pilote inconnu")
        );
    }

    #[test]
    fn une_commande_exigeant_un_daemon_le_dit_clairement() {
        let cli = Cli::try_parse_from(["prophet", "cap", "approvals"]).unwrap();
        let erreur = run(&cli).unwrap_err().to_string();
        assert!(erreur.contains("capd en service"), "{erreur}");
        assert!(
            erreur.contains("prophet status"),
            "le message doit dire quoi faire ensuite : {erreur}"
        );
    }

    #[test]
    fn le_journal_et_les_taches_se_lisent_sans_daemon() {
        // Un journal d'audit qui exigerait un daemon serait inutile après un incident.
        for arguments in [
            vec!["prophet", "log", "tail"],
            vec!["prophet", "task", "ls"],
        ] {
            let cli = Cli::try_parse_from(&arguments).unwrap();
            assert!(
                run(&cli).is_ok(),
                "{arguments:?} doit fonctionner hors service"
            );
        }
    }

    #[test]
    fn annuler_une_tache_en_cours_oriente_vers_undo() {
        let cli = Cli::try_parse_from(["prophet", "task", "cancel", "task:01"]).unwrap();
        let erreur = run(&cli).unwrap_err().to_string();
        assert!(erreur.contains("prophet task undo"), "{erreur}");
    }
    #[test]
    fn sans_agentd_la_liste_des_taches_le_dit_au_lieu_de_mentir() {
        // Le piege serait de n'afficher que les espaces de travail et de conclure « aucune
        // tache ». Quelqu'un qui vient d'en planifier une verrait alors le contraire de la
        // verite, et chercherait le defaut ailleurs.
        let erreur = super::taches_en_cours(std::path::Path::new("/nulle/part/agentd.sock"))
            .expect_err("aucun daemon n'écoute là");
        assert!(
            erreur.contains("agentd"),
            "le motif doit nommer le daemon absent, obtenu : {erreur}"
        );
    }
}

#[cfg(test)]
mod journal {
    //! Ce que `prophet log` doit à celui qui cherche ce qui s'est passé.
    //!
    //! La commande lisait `~/.prophet/ledger` et rien d'autre. Sur la machine installée, où
    //! `prophet-ledger` écrit dans `/var/lib/prophet/ledger` depuis le démarrage, elle répondait
    //! « aucun journal sur cette machine » — juste après que le test en machine virtuelle eut vu
    //! une sandbox démarrer et le service l'inscrire. Les deux fautes sont ici : ne pas connaître
    //! le journal du service, et rendre les premiers événements là où l'on en demandait les
    //! derniers.

    use prophet_types::ledger::{Actor, Event, EventKind};

    fn evenement(seq: u64) -> Event {
        Event {
            v: 1,
            seq,
            ts: time::OffsetDateTime::UNIX_EPOCH,
            actor: Actor("essai".to_owned()),
            kind: EventKind::TaskCreated,
            task: Some(format!("task:{seq}")),
            step: None,
            payload: serde_json::Value::Null,
            prev: String::new(),
            hash: None,
        }
    }

    #[test]
    fn le_journal_du_service_est_cherche_avant_celui_du_compte() {
        let home = std::path::Path::new("/home/quelqu-un");
        let candidats = super::candidats_de_journal(None, home);

        let systeme = candidats
            .iter()
            .position(|c| c == std::path::Path::new("/var/lib/prophet/ledger"))
            .expect(
                "le journal du service doit être cherché : c'est le seul qui existe sur une \
                 machine installée, et l'ignorer faisait dire « aucun journal » à une commande \
                 d'audit devant un journal plein",
            );
        let compte = candidats
            .iter()
            .position(|c| c == &home.join(".prophet/ledger"))
            .expect("celui du compte reste utile sans daemon");
        assert!(
            systeme < compte,
            "le journal du service passe avant celui du compte : {candidats:?}"
        );
    }

    #[test]
    fn un_chemin_force_passe_avant_tout() {
        let force = std::path::Path::new("/ailleurs");
        let candidats = super::candidats_de_journal(Some(force), std::path::Path::new("/home/x"));
        assert_eq!(
            candidats.first().map(std::path::PathBuf::as_path),
            Some(force)
        );
    }

    #[test]
    fn tail_rend_les_derniers_et_non_les_premiers() {
        let events: Vec<Event> = (1..=50).map(evenement).collect();
        let rendu = super::rendre_tail(&events, 3);
        let lignes: Vec<&str> = rendu.lines().collect();
        assert_eq!(lignes.len(), 3, "trois demandés, trois rendus : {rendu}");
        assert!(
            rendu.contains("task:50") && rendu.contains("task:48") && !rendu.contains("task:1 "),
            "« tail -n 3 » doit rendre 48, 49 et 50 — pas 1, 2 et 3 :\n{rendu}"
        );
    }

    #[test]
    fn tail_le_dit_quand_il_n_y_a_rien() {
        assert_eq!(super::rendre_tail(&[], 20), "journal vide\n");
    }
}
