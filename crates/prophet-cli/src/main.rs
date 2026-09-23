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
    /// Poids des modèles locaux installés sur cette machine.
    Model {
        #[command(subcommand)]
        action: ModelAction,
    },
    /// Mémoire.
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },
    /// Jev, le décideur rapide : routage des demandes et état de sa configuration.
    Jev {
        #[command(subcommand)]
        action: JevAction,
    },
    /// Secrets du coffre : des références pour les agents, jamais des valeurs.
    Secret {
        #[command(subcommand)]
        action: SecretAction,
    },
    /// Gel d'urgence de toutes les tâches.
    Freeze,
    /// Parler à Prophet OS : enregistrer le micro ou lire un fichier audio, transcrire en
    /// local par whisper.cpp, et au choix en faire une mission à examiner (ADR 0036).
    Voice {
        /// Fichier audio à transcrire ; sans lui, le micro est enregistré.
        #[arg(long)]
        file: Option<std::path::PathBuf>,
        /// Durée d'enregistrement du micro, en secondes.
        #[arg(long, default_value_t = 6)]
        seconds: u32,
        /// Langue (code à deux lettres) ; détection automatique sinon.
        #[arg(long)]
        language: Option<String>,
        /// Préparer une mission dans ce contexte du catalogue avec le texte transcrit comme
        /// objectif ; le plan est rendu, l'humain le lance séparément.
        #[arg(long)]
        prepare: Option<String>,
        /// Modèle admis par le contexte, avec `--prepare` ; le premier modèle admis sinon.
        #[arg(long)]
        model: Option<String>,
        /// Faire parler l'OS : synthétiser ce texte en local (Piper) et le jouer, au lieu
        /// d'écouter.
        #[arg(long, conflicts_with_all = ["file", "prepare"])]
        say: Option<String>,
        /// Répondre à voix haute : ce qui a été compris et, avec `--prepare`, la mission
        /// préparée ; ou pourquoi rien n'a été préparé.
        #[arg(long, conflicts_with = "say")]
        reply: bool,
        /// Écouter en continu, par tranches de `--seconds`, et n'agir que sur une phrase qui
        /// commence par le mot d'activation (`--wake`) : le reste de la phrase est l'intention.
        #[arg(long, conflicts_with_all = ["say", "file"])]
        listen: bool,
        /// Mot d'activation de l'écoute continue.
        #[arg(long, default_value = "prophète")]
        wake: String,
        /// Avec `--listen` : nombre de tranches à écouter, 0 pour ne jamais s'arrêter.
        #[arg(long, default_value_t = 0)]
        rounds: u32,
        /// Avec `--say` ou `--reply` : écrire la réponse dans ce fichier WAV au lieu de la jouer.
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum ModelAction {
    /// Le catalogue des poids : ce que chaque fichier GGUF dit de lui-même (architecture,
    /// quantification, fenêtre de contexte), sans charger les poids.
    Ls {
        /// Dossier à lire, seul. Sans lui : `PROPHET_MODELS_DIR` (sinon `/var/lib/prophet/models`)
        /// et les fichiers que `PROPHET_WEIGHTS` nomme.
        #[arg(long)]
        dir: Option<std::path::PathBuf>,
        /// Base d'API du moteur local, interrogé pour savoir quel poids il sert et avec quelle
        /// fenêtre. Sans elle : `PROPHET_MODEL_ENDPOINT`, sinon `http://127.0.0.1:8080/v1`.
        #[arg(long)]
        endpoint: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum JevAction {
    /// Demande à agentd quel modèle servirait une requête de mission, sans la planifier.
    Route {
        /// Fichier JSON contenant intent, manifest et availability (le format de `task new`).
        request: std::path::PathBuf,
    },
    /// Dit si le service a un décideur configuré et si son secret est dans le coffre.
    Status,
}

#[derive(Debug, Subcommand)]
enum SecretAction {
    /// Dépose un secret dans le coffre. La valeur est lue sur l'entrée standard, jamais en argument.
    Put {
        /// Nom du secret, tel que les agents le référencent (`prophet-secret:<nom>`).
        name: String,
        /// Hôtes auxquels ce secret peut être présenté ; répétable.
        #[arg(long = "host", required = true)]
        hosts: Vec<String>,
        /// En-tête dans lequel il est substitué.
        #[arg(long, default_value = "Authorization")]
        header: String,
        /// Description libre.
        #[arg(long, default_value = "")]
        description: String,
    },
    /// Liste les secrets du coffre : noms, hôtes, en-têtes. Jamais de valeur.
    Ls,
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
        /// Dire le résultat à voix haute (Piper, en local) : l'état de la mission, le début de
        /// son texte, et le nombre de changements à examiner.
        #[arg(long)]
        say: bool,
        /// Avec `--say` : écrire la parole dans ce fichier WAV au lieu de la jouer.
        #[arg(long, requires = "say")]
        out: Option<std::path::PathBuf>,
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
    /// Prépare à nouveau une mission échouée ou arrêtée, dans le même contexte et avec le même
    /// modèle, sans la lancer.
    Retry {
        /// Identifiant de la mission à reprendre.
        mission: String,
        /// Référence de la nouvelle mission ; générée sinon.
        #[arg(long)]
        id: Option<String>,
    },
    /// Rend la configuration MCP qui donne à un client (Claude Code, Codex) les outils d'une
    /// mission préparée, par le pont `prophet-mcp`.
    McpConfig {
        /// Identifiant de la mission préparée.
        id: String,
    },
    /// Ouvre une séance d'outils sur une mission préparée, pour la piloter à la main depuis le
    /// terminal : la mission passe en cours, ses outils répondent à `task call`.
    Attach {
        /// Identifiant de la mission préparée.
        id: String,
        /// Nom du client, inscrit au journal.
        #[arg(long, default_value = "terminal")]
        client: String,
    },
    /// Appelle un outil dans une séance ouverte ; les arguments sont un objet JSON.
    Call {
        /// Identifiant de la mission.
        id: String,
        /// Nom de l'outil (`ui.tree`, `fs.write`…).
        tool: String,
        /// Arguments, en JSON.
        #[arg(default_value = "{}")]
        args: String,
    },
    /// Ferme la séance : les versions sont scellées et la mission passe à l'examen.
    Detach {
        /// Identifiant de la mission.
        id: String,
        /// Un mot de conclusion, inscrit au résultat.
        #[arg(long)]
        text: Option<String>,
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
            let options = options_du_service(&socket_agentd());
            let navigateur = options.as_ref().map(|o| o.browser.clone());
            let mut out = shell::status_complet(
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
            );
            out.push_str(&status_reserve(
                capacites_du_bac(&socket_sandboxd()).as_ref(),
            ));
            out.push_str(&status_parole_et_pilotes(options.as_ref()));
            out.push_str(&status_machine(&chemin_de_l_inventaire()));
            Ok(out)
        }
        Command::Freeze => {
            let socket = std::env::var("PROPHET_SANDBOXD_SOCKET").map_or_else(
                |_| prophet_ipc::socket_path("sandboxd"),
                std::path::PathBuf::from,
            );
            freeze(&socket, cli.json)
        }
        Command::Provider { action } => provider(action, cli.json),
        Command::Model { action } => model(action, cli.json),
        Command::Voice {
            file,
            seconds,
            language,
            prepare,
            model,
            say,
            reply,
            listen: en_ecoute,
            wake,
            rounds,
            out,
        } => {
            let reponse = if *reply { Some(out.as_deref()) } else { None };
            match say {
                Some(text) => speak(text, out.as_deref(), cli.json),
                None if *en_ecoute => listen(
                    wake,
                    *seconds,
                    *rounds,
                    language.as_deref(),
                    prepare.as_deref(),
                    model.clone(),
                    reponse,
                    cli.json,
                ),
                None => voice(
                    file.as_deref(),
                    *seconds,
                    language.as_deref(),
                    prepare.as_deref(),
                    model.clone(),
                    reponse,
                    cli.json,
                ),
            }
        }
        Command::Memory { action } => memory(action),
        Command::Jev { action } => jev(action, cli.json),
        Command::Secret { action } => secret(action, cli.json),
        Command::Log { action } => log(action),
        Command::Task { action } => task(action, cli.json),
        Command::Cap { action } => cap(action, cli.json),
    }
}

/// Les approbations, depuis le terminal (ADR 0041) : voir ce qui attend une décision, la rendre,
/// lire les règles qu'elle a laissées, révoquer une tâche. Tout passe par capd ; sans lui, la
/// commande le dit au lieu de faire semblant.
fn cap(action: &CapAction, as_json: bool) -> anyhow::Result<String> {
    let socket = socket_capd();
    match action {
        CapAction::Approvals => {
            let demandes = capd_rpc(&socket, "approval.pending", serde_json::json!({}))?;
            if as_json {
                return Ok(format!("{}\n", serde_json::to_string_pretty(&demandes)?));
            }
            let liste = demandes.as_array().cloned().unwrap_or_default();
            if liste.is_empty() {
                return Ok("Aucune décision en attente.\n".to_owned());
            }
            let mut out = String::from("Décisions en attente\n");
            for d in &liste {
                out.push_str(&format!(
                    "  {}\n      {} — {} sur {} (mission {}){}\n{}      accorder : prophet cap approve {} [--scope task] · refuser : prophet cap deny {}\n",
                    d["id"].as_str().unwrap_or("?"),
                    d["summary"].as_str().unwrap_or(""),
                    d["action"].as_str().unwrap_or("?"),
                    d["target"].as_str().unwrap_or("?"),
                    d["task"].as_str().unwrap_or("?"),
                    match (
                        d["irreversible"].as_bool().unwrap_or(false),
                        d["external"].as_bool().unwrap_or(false)
                    ) {
                        (true, true) => " · irréversible, hors de la machine",
                        (true, false) => " · irréversible",
                        (false, true) => " · hors de la machine",
                        (false, false) => "",
                    },
                    d["reason"]
                        .as_str()
                        .map(|m| format!("      le modèle dit : « {m} »\n"))
                        .unwrap_or_default(),
                    d["id"].as_str().unwrap_or("?"),
                    d["id"].as_str().unwrap_or("?"),
                ));
            }
            Ok(out)
        }
        CapAction::Approve { id, scope } => {
            anyhow::ensure!(
                matches!(scope.as_str(), "once" | "task" | "agent"),
                "portée inconnue : {scope} (attendu once, task ou agent)"
            );
            let tranchee = capd_rpc(
                &socket,
                "approval.resolve",
                serde_json::json!({"id": id, "decision": "allow", "scope": scope}),
            )?;
            if as_json {
                return Ok(format!("{}\n", serde_json::to_string_pretty(&tranchee)?));
            }
            Ok(format!(
                "Accordé : {} ({}).\n",
                tranchee["summary"].as_str().unwrap_or(id),
                match scope.as_str() {
                    "task" => "pour toute la mission",
                    "agent" => "pour cet agent, un temps",
                    _ => "cette fois seulement",
                }
            ))
        }
        CapAction::Deny { id } => {
            let tranchee = capd_rpc(
                &socket,
                "approval.resolve",
                serde_json::json!({"id": id, "decision": "deny", "scope": "once"}),
            )?;
            if as_json {
                return Ok(format!("{}\n", serde_json::to_string_pretty(&tranchee)?));
            }
            Ok(format!(
                "Refusé : {}.\n",
                tranchee["summary"].as_str().unwrap_or(id)
            ))
        }
        CapAction::Rules => {
            let regles = capd_rpc(&socket, "approval.rules", serde_json::json!({}))?;
            if as_json {
                return Ok(format!("{}\n", serde_json::to_string_pretty(&regles)?));
            }
            let liste = regles.as_array().cloned().unwrap_or_default();
            if liste.is_empty() {
                return Ok("Aucune règle permanente.\n".to_owned());
            }
            let mut out = String::from("Règles issues de décisions humaines\n");
            for r in &liste {
                out.push_str(&format!(
                    "  {} : {} — {} sur {}{}{}\n",
                    r["id"].as_str().unwrap_or("?"),
                    r["decision"].as_str().unwrap_or("?"),
                    r["action"].as_str().unwrap_or("?"),
                    r["target"].as_str().unwrap_or("?"),
                    r["task"]
                        .as_str()
                        .map(|t| format!(" (mission {t})"))
                        .unwrap_or_default(),
                    r["agent"]
                        .as_str()
                        .map(|a| format!(" (agent {a})"))
                        .unwrap_or_default(),
                ));
            }
            Ok(out)
        }
        CapAction::Revoke { task } => {
            let r = capd_rpc(&socket, "cap.revoke", serde_json::json!({"subject": task}))?;
            if as_json {
                return Ok(format!("{}\n", serde_json::to_string_pretty(&r)?));
            }
            Ok(format!(
                "Révoqué : {} et ses descendants.\n",
                r["revoked"].as_str().unwrap_or(task)
            ))
        }
    }
}

/// Un appel à capd ; sans lui, la commande le dit au lieu de faire semblant.
fn capd_rpc(
    socket: &std::path::Path,
    method: &str,
    params: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    task_rpc(socket, method, params).map_err(|e| {
        anyhow::anyhow!(
            "les approbations exigent capd en service ({e}).              Lancez `prophet status` pour voir ce qui est disponible sur cette machine."
        )
    })
}

fn socket_capd() -> std::path::PathBuf {
    std::env::var("PROPHET_CAPD_SOCKET").map_or_else(
        |_| prophet_ipc::socket_path("capd"),
        std::path::PathBuf::from,
    )
}

/// L'OS parle : le texte est synthétisé en local par Piper, puis joué sur la sortie audio de
/// la session, ou écrit dans un fichier (ADR 0036). Rien ne quitte la machine.
fn speak(text: &str, out: Option<&std::path::Path>, as_json: bool) -> anyhow::Result<String> {
    let tools = voice::Tools::from_env()?;
    let temporary;
    let wav = match out {
        Some(path) => path,
        None => {
            let dir = std::env::var_os("XDG_RUNTIME_DIR")
                .map_or_else(std::env::temp_dir, std::path::PathBuf::from);
            temporary = dir.join(format!("prophet-parole-{}.wav", std::process::id()));
            &temporary
        }
    };
    let speech = tools.speak(text, wav)?;
    let played = if out.is_none() {
        let result = tools.play(wav);
        let _ = std::fs::remove_file(wav);
        result?;
        true
    } else {
        false
    };
    if as_json {
        return Ok(format!(
            "{}\n",
            serde_json::to_string_pretty(&serde_json::json!({
                "speech": speech, "played": played, "kept": out.is_some()
            }))?
        ));
    }
    Ok(if played {
        format!("Dit en {} ms.\n", speech.duration_ms)
    } else {
        format!(
            "Écrit dans {} ({} octets, {} ms).\n",
            speech.wav.display(),
            speech.bytes,
            speech.duration_ms
        )
    })
}

/// La parole : un fichier ou le micro, transcrit en local, et au choix une mission préparée
/// avec ce texte pour objectif (ADR 0036). Le son ne quitte pas la machine.
#[allow(clippy::too_many_arguments)]
fn voice(
    file: Option<&std::path::Path>,
    seconds: u32,
    language: Option<&str>,
    prepare: Option<&str>,
    model: Option<String>,
    reply: Option<Option<&std::path::Path>>,
    as_json: bool,
) -> anyhow::Result<String> {
    let tools = voice::Tools::from_env()?;
    let recorded;
    let audio = match file {
        Some(path) => path,
        None => {
            recorded = chemin_temporaire("prophet-voix");
            eprintln!("prophet : enregistrement du micro pendant {seconds} s…");
            tools.record(seconds, &recorded)?;
            &recorded
        }
    };
    let transcript = tools.transcribe(audio, language);
    if file.is_none() {
        let _ = std::fs::remove_file(audio);
    }
    let transcript = transcript?;
    if transcript.text.is_empty() {
        if let Some(out) = reply {
            reply_aloud(&tools, "Je n'ai rien compris. Rien n'est préparé.", out)?;
        }
        anyhow::bail!("rien n'a été compris ; rien n'est préparé");
    }
    agir(&tools, &transcript, prepare, model, None, reply, as_json)
}

/// Écoute en continu, par tranches, et n'agit que sur une phrase qui commence par le mot
/// d'activation : le reste est l'intention, traitée comme une dictée (ADR 0036). Le son de
/// chaque tranche est effacé après transcription ; rien n'est conservé ni envoyé.
#[allow(clippy::too_many_arguments)]
fn listen(
    wake: &str,
    seconds: u32,
    rounds: u32,
    language: Option<&str>,
    prepare: Option<&str>,
    model: Option<String>,
    reply: Option<Option<&std::path::Path>>,
    as_json: bool,
) -> anyhow::Result<String> {
    let tools = voice::Tools::from_env()?;
    if wake.trim().is_empty() {
        anyhow::bail!("le mot d'activation ne peut pas être vide");
    }
    eprintln!(
        "prophet : à l'écoute, dites « {wake} » puis votre demande ({seconds} s par tranche{})",
        if rounds == 0 {
            ", Ctrl+C pour arrêter".to_owned()
        } else {
            format!(", {rounds} tranche(s)")
        }
    );
    let mut out = String::new();
    let mut round = 0u32;
    // La dernière mission préparée dans cette écoute : « lance la mission » et « résultat »
    // parlent d'elle.
    let mut derniere: Option<String> = None;
    while rounds == 0 || round < rounds {
        round += 1;
        let wav = chemin_temporaire("prophet-ecoute");
        let heard = tools
            .record(seconds, &wav)
            .and_then(|()| tools.transcribe(&wav, language));
        let _ = std::fs::remove_file(&wav);
        let transcript = match heard {
            Ok(t) => t,
            Err(e) => {
                eprintln!("prophet : tranche {round} : {e}");
                continue;
            }
        };
        let Some(intent) = voice::after_wake_word(&transcript.text, wake) else {
            if !as_json && !transcript.text.is_empty() {
                eprintln!("prophet : (entendu sans « {wake} ») {}", transcript.text);
            }
            continue;
        };
        let transcript = voice::Transcript {
            text: intent,
            ..transcript
        };
        let resultat = match voice::ordre_vocal(&transcript.text) {
            voice::Ordre::Intention => {
                let id = prepare.map(|_| format!("mission-{}", ulid::Ulid::new()));
                let fait = agir(
                    &tools,
                    &transcript,
                    prepare,
                    model.clone(),
                    id.clone(),
                    reply,
                    as_json,
                );
                if fait.is_ok() && id.is_some() {
                    derniere = id;
                }
                fait
            }
            // En ligne de commande, une intention est préparée aussitôt : « prépare » seul
            // n'a rien à préparer.
            voice::Ordre::Preparer => dire_si_demande(
                &tools,
                reply,
                "Dites votre demande : je la prépare aussitôt.",
            )
            .map(|_| "« prépare » : dites votre demande, elle est préparée aussitôt.\n".to_owned()),
            voice::Ordre::Lancer => lancer_par_la_voix(&tools, derniere.as_deref(), reply, as_json),
            voice::Ordre::Resultat => dire_le_resultat(&tools, derniere.as_deref(), reply, as_json),
            voice::Ordre::Accorder => trancher_par_la_voix(&tools, true, reply, as_json),
            voice::Ordre::Refuser => trancher_par_la_voix(&tools, false, reply, as_json),
            voice::Ordre::Ouvrir(cible) => ouvrir_par_la_voix(&tools, &cible, reply),
        };
        match resultat {
            Ok(text) => out.push_str(&text),
            Err(e) => {
                eprintln!("prophet : {e}");
                out.push_str(&format!("« {} » : {e}\n", transcript.text));
            }
        }
    }
    Ok(out)
}

/// Dit `texte` si une réponse parlée est demandée.
fn dire_si_demande(
    tools: &voice::Tools,
    reply: Option<Option<&std::path::Path>>,
    texte: &str,
) -> anyhow::Result<Option<voice::Speech>> {
    match reply {
        None => Ok(None),
        Some(out) => reply_aloud(tools, texte, out).map(Some),
    }
}

/// « Prophète, lance la mission » : la dernière mission préparée dans cette écoute est lancée ;
/// c'est la décision de l'humain, dite, qui vaut approbation du plan.
/// « Ouvre … » : une application du bureau ou un outil publié, par le lanceur de la session
/// (`prophet-ouvrir`) ; hors du bureau, l'OS dit qu'il n'a rien à ouvrir.
fn ouvrir_par_la_voix(
    tools: &voice::Tools,
    cible: &str,
    reply: Option<Option<&std::path::Path>>,
) -> anyhow::Result<String> {
    let lanceur = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("prophet-ouvrir"))
            .find(|candidat| candidat.is_file())
    });
    let Some(lanceur) = lanceur else {
        dire_si_demande(tools, reply, "Pas de bureau ici : rien à ouvrir.")?;
        return Ok("« ouvre » : pas de lanceur du bureau sur cette machine.\n".to_owned());
    };
    let arguments = voice::arguments_du_lanceur(cible);
    std::process::Command::new(lanceur)
        .args(&arguments)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("lanceur du bureau : {e}"))?;
    let phrase = if arguments.is_empty() {
        "J'ouvre le lanceur.".to_owned()
    } else {
        format!("J'ouvre {}.", arguments.join(" "))
    };
    dire_si_demande(tools, reply, &phrase)?;
    Ok(format!("{phrase}\n"))
}

/// « Accorde » / « refuse » : la plus ancienne décision en attente est tranchée, cette fois
/// seulement, et dite (ADR 0041). Sans décision en attente, l'OS le dit.
fn trancher_par_la_voix(
    tools: &voice::Tools,
    accorder: bool,
    reply: Option<Option<&std::path::Path>>,
    as_json: bool,
) -> anyhow::Result<String> {
    let socket = socket_capd();
    let demandes = capd_rpc(&socket, "approval.pending", serde_json::json!({}))?;
    let Some(demande) = demandes.as_array().and_then(|l| l.first()).cloned() else {
        dire_si_demande(tools, reply, "Aucune décision n'attend.")?;
        return Ok("aucune décision en attente.\n".to_owned());
    };
    let id = demande["id"].as_str().unwrap_or_default().to_owned();
    let resume = demande["summary"]
        .as_str()
        .unwrap_or("cette action")
        .to_owned();
    let tranchee = capd_rpc(
        &socket,
        "approval.resolve",
        serde_json::json!({
            "id": id,
            "decision": if accorder { "allow" } else { "deny" },
            "scope": "once"
        }),
    )?;
    let phrase = if accorder {
        format!("Accordé : {resume}.")
    } else {
        format!("Refusé : {resume}.")
    };
    dire_si_demande(tools, reply, &phrase)?;
    if as_json {
        return Ok(format!("{}\n", serde_json::to_string_pretty(&tranchee)?));
    }
    Ok(format!("{phrase}\n"))
}

fn lancer_par_la_voix(
    tools: &voice::Tools,
    derniere: Option<&str>,
    reply: Option<Option<&std::path::Path>>,
    as_json: bool,
) -> anyhow::Result<String> {
    let Some(id) = derniere else {
        dire_si_demande(
            tools,
            reply,
            "Aucune mission n'est préparée dans cette écoute.",
        )?;
        anyhow::bail!("aucune mission préparée dans cette écoute : rien à lancer");
    };
    let lancement = task_rpc(
        &socket_agentd(),
        "task.start",
        serde_json::json!({"id": id}),
    );
    match lancement {
        Ok(result) => {
            let spoken = dire_si_demande(
                tools,
                reply,
                "Mission lancée. Dites « résultat » quand vous voudrez l'entendre.",
            )?;
            if as_json {
                return Ok(format!(
                    "{}\n",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "order": "start", "task": id, "result": result, "reply": spoken
                    }))?
                ));
            }
            Ok(format!(
                "Mission {id} lancée par la voix.{}\n",
                spoken.map_or(String::new(), |s| format!(
                    " Réponse dite ({} ms).",
                    s.duration_ms
                ))
            ))
        }
        Err(error) => {
            dire_si_demande(tools, reply, "Je n'ai pas pu lancer la mission.")?;
            Err(error)
        }
    }
}

/// « Prophète, résultat » : le résultat de la dernière mission préparée, dit comme
/// `task result --say`.
fn dire_le_resultat(
    tools: &voice::Tools,
    derniere: Option<&str>,
    reply: Option<Option<&std::path::Path>>,
    as_json: bool,
) -> anyhow::Result<String> {
    let Some(id) = derniere else {
        dire_si_demande(
            tools,
            reply,
            "Aucune mission n'est préparée dans cette écoute.",
        )?;
        anyhow::bail!("aucune mission préparée dans cette écoute : rien à lire");
    };
    match task_rpc(
        &socket_agentd(),
        "task.result",
        serde_json::json!({"id": id}),
    ) {
        Ok(result) => {
            let phrase = voice::resume_du_resultat(&result);
            let spoken = dire_si_demande(tools, reply, &phrase)?;
            if as_json {
                return Ok(format!(
                    "{}\n",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "order": "result", "task": id, "result": result,
                        "text": phrase, "reply": spoken
                    }))?
                ));
            }
            Ok(format!(
                "Mission {id} : « {phrase} »{}\n",
                spoken.map_or(String::new(), |s| format!(
                    " Réponse dite ({} ms).",
                    s.duration_ms
                ))
            ))
        }
        Err(error) => {
            dire_si_demande(tools, reply, "Le résultat n'est pas encore disponible.")?;
            Err(error)
        }
    }
}

/// Ce qu'on fait d'une phrase comprise : rien de plus que la rendre, ou préparer une mission
/// avec elle pour objectif, et répondre à voix haute si on l'a demandé.
#[allow(clippy::too_many_arguments)]
fn agir(
    tools: &voice::Tools,
    transcript: &voice::Transcript,
    prepare: Option<&str>,
    model: Option<String>,
    id: Option<String>,
    reply: Option<Option<&std::path::Path>>,
    as_json: bool,
) -> anyhow::Result<String> {
    let repondre = |texte: String| -> anyhow::Result<Option<voice::Speech>> {
        match reply {
            None => Ok(None),
            Some(out) => reply_aloud(tools, &texte, out).map(Some),
        }
    };
    let (plan, spoken) = match prepare {
        Some(profile) => {
            let id = id.unwrap_or_else(|| format!("mission-{}", ulid::Ulid::new()));
            let prepared = task(
                &TaskAction::Prepare {
                    profile: profile.to_owned(),
                    model,
                    client: false,
                    id: Some(id.clone()),
                    intent: transcript.text.clone(),
                },
                as_json,
            );
            match prepared {
                Ok(plan) => {
                    // L'identifiant n'est pas dit : épelé, un ULID n'aide personne ; il est
                    // écrit dans la réponse et dans l'atelier.
                    let spoken = repondre(format!(
                        "J'ai compris : {}. La mission est préparée dans le contexte {}. Examinez son plan dans l'atelier, puis lancez-la.",
                        transcript.text, profile
                    ))?;
                    (Some(plan), spoken)
                }
                Err(error) => {
                    repondre(format!(
                        "J'ai compris : {}. Mais je n'ai pas pu préparer la mission.",
                        transcript.text
                    ))?;
                    return Err(error);
                }
            }
        }
        None => (
            None,
            repondre(format!("J'ai compris : {}", transcript.text))?,
        ),
    };
    if as_json {
        let plan: Option<serde_json::Value> =
            plan.as_deref().map(serde_json::from_str).transpose()?;
        return Ok(format!(
            "{}\n",
            serde_json::to_string_pretty(
                &serde_json::json!({"transcript": transcript, "plan": plan, "reply": spoken})
            )?
        ));
    }
    let mut out = format!(
        "« {} »\n({}, {} segment(s), transcrit en {} ms)\n",
        transcript.text,
        transcript.language.as_deref().unwrap_or("langue inconnue"),
        transcript.segments,
        transcript.duration_ms
    );
    if let Some(plan) = plan {
        out.push_str(&plan);
    }
    if let Some(spoken) = spoken {
        out.push_str(&format!("Réponse dite ({} ms).\n", spoken.duration_ms));
    }
    Ok(out)
}

/// Un fichier temporaire de la session, dans son répertoire d'exécution s'il existe.
fn chemin_temporaire(prefixe: &str) -> std::path::PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map_or_else(std::env::temp_dir, std::path::PathBuf::from);
    dir.join(format!(
        "{prefixe}-{}-{}.wav",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis())
    ))
}

/// Dit `texte` sur la sortie audio de la session, ou l'écrit dans `out`.
fn reply_aloud(
    tools: &voice::Tools,
    texte: &str,
    out: Option<&std::path::Path>,
) -> anyhow::Result<voice::Speech> {
    match out {
        Some(path) => Ok(tools.speak(texte, path)?),
        None => Ok(tools.say(texte)?),
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
        TaskAction::Result { id, say, out } => {
            let mut result = task_rpc(
                &socket_agentd(),
                "task.result",
                serde_json::json!({"id":id}),
            )?;
            // L'OS lit le résultat : ce qu'il dit est écrit aussi, pour qu'on puisse le relire.
            let dit = if *say {
                let tools = voice::Tools::from_env()?;
                let phrase = voice::resume_du_resultat(&result);
                let speech = reply_aloud(&tools, &phrase, out.as_deref())?;
                if let Some(object) = result.as_object_mut() {
                    object.insert(
                        "speech".into(),
                        serde_json::json!({"text": phrase, "speech": speech, "played": out.is_none()}),
                    );
                }
                Some(match out.as_deref() {
                    Some(path) => {
                        format!("Résultat écrit dans {} : « {phrase} »\n", path.display())
                    }
                    None => format!("Résultat dit en {} ms : « {phrase} »\n", speech.duration_ms),
                })
            } else {
                None
            };
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
            // Ce que chaque modèle a coûté, et la part prise en charge hors du modèle de la
            // mission : la mesure du relais (ADR 0034).
            if let Ok(usage) = serde_json::from_value::<agentd::UsageByModel>(
                result
                    .get("usage")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            ) && !usage.is_empty()
            {
                let reference = result["driver"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| usage.keys().next().cloned().unwrap_or_default());
                out.push_str(&format!(
                    "Tokens par modèle : {}\n",
                    agentd::relay::render_usage(&usage, &reference)
                ));
            }
            if let Some(diff) = result.get("diff") {
                let diff: sfs::Diff = serde_json::from_value(diff.clone())?;
                out.push_str(&diff.render());
                out.push_str("Changements conservés dans le travail ; validation non appliquée.\n");
            }
            if let Some(dit) = dit {
                out.push_str(&dit);
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
            // Qui la mène, par son nom d'usage — un client officiel ou un modèle local — et,
            // pour une sous-mission, qui la lui a confiée (ADR 0035, 0039).
            if let Some(driver) = &inspection.task.driver {
                out.push_str(&format!(
                    "Menée par : {}\n",
                    agentd::preparation::reference_label(driver)
                ));
            }
            if let Some(parent) = &inspection.task.parent {
                out.push_str(&format!(
                    "Confiée par la mission {parent} : son travail y est revenu et se publie avec elle.\n"
                ));
            }
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
            if let Some(role) = &inspection.task.role {
                out.push_str(&format!("Rôle dans le relais : {role}\n"));
            }
            if !inspection.task.usage.is_empty() {
                let reference = inspection.task.driver.clone().unwrap_or_default();
                out.push_str(&format!(
                    "Tokens par modèle : {}\n",
                    agentd::relay::render_usage(&inspection.task.usage, &reference)
                ));
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
                // Un client officiel se nomme par son identifiant, comme un modèle local, et se
                // lit par son nom d'usage (ADR 0035).
                let models = if profile.models.is_empty() {
                    "aucun modèle disponible".to_owned()
                } else {
                    profile
                        .models
                        .iter()
                        .map(|m| match agentd::preparation::driver_name(m) {
                            Some(_) => format!("{m} — {}", agentd::preparation::model_label(m)),
                            None => m.clone(),
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
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
            // Les clients officiels sont les modèles principaux : leur état dit lesquels une
            // mission peut prendre, et comment connecter les autres.
            if let Some(pilot) = &options.pilot {
                if pilot.drivers.is_empty() {
                    out.push_str("Clients officiels : aucun connu du lanceur de la session\n");
                } else {
                    out.push_str("Clients officiels (modèles principaux) :\n");
                    for state in &pilot.drivers {
                        let suite = if state.ready() {
                            String::new()
                        } else {
                            format!(" — prophet provider login {}", state.driver)
                        };
                        out.push_str(&format!(
                            "  {} {} ({}) : {}{suite}\n",
                            if state.ready() { "✓" } else { "✗" },
                            agentd::preparation::model_label(&state.driver),
                            state.driver,
                            state.connection
                        ));
                    }
                }
            }
            out.push_str("Préparer : prophet task prepare --profile <contexte> --model <modèle ou client : codex, claude-code> \"<objectif>\"\n");
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
        TaskAction::Retry { mission, id } => {
            // La relance repasse par le catalogue du service : même contexte, même modèle, même
            // intention ; jamais un manifeste recopié, que `task.prepare` refuserait d'ailleurs.
            let info: agentd::Inspection = serde_json::from_value(task_rpc(
                &socket_agentd(),
                "task.inspect",
                serde_json::json!({"id":mission}),
            )?)?;
            if !matches!(
                info.task.state,
                agentd::State::Failed | agentd::State::Cancelled
            ) {
                anyhow::bail!(
                    "la mission {mission} n'est ni échouée ni arrêtée ; seule une mission échouée ou arrêtée se relance"
                );
            }
            if info.task.parent.is_some() {
                anyhow::bail!(
                    "la mission {mission} a été confiée par une autre : relancez celle-ci"
                );
            }
            let plan = info.plan.as_ref();
            let Some(profile) = plan.and_then(|p| p.profile.clone()) else {
                anyhow::bail!(
                    "la mission {mission} n'a pas été préparée depuis un contexte du service ; préparez-la avec `prophet task prepare`"
                );
            };
            let reference = plan.map_or("", |p| p.choice.reference.as_str());
            let model = reference
                .strip_prefix("local:")
                .or_else(|| reference.strip_prefix("driver:"))
                .unwrap_or(reference)
                .to_owned();
            task(
                &TaskAction::Prepare {
                    profile,
                    model: Some(model),
                    client: false,
                    id: id.clone(),
                    intent: info.task.intent.clone(),
                },
                as_json,
            )
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
        TaskAction::Attach { id, client } => {
            let result = task_rpc(
                &socket_agentd(),
                "task.attach",
                serde_json::json!({"id":id, "client":client}),
            )?;
            if as_json {
                return Ok(format!("{}\n", serde_json::to_string_pretty(&result)?));
            }
            let outils: Vec<String> = result["tools"]
                .as_array()
                .map(|t| {
                    t.iter()
                        .filter_map(|o| o["name"].as_str().or_else(|| o.as_str()))
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            Ok(format!(
                "Séance ouverte sur {id} pour « {client} ». Outils : {}.\n",
                if outils.is_empty() {
                    "voir `prophet task call`".to_owned()
                } else {
                    outils.join(", ")
                }
            ))
        }
        TaskAction::Call { id, tool, args } => {
            let arguments: serde_json::Value = serde_json::from_str(args)
                .map_err(|e| anyhow::anyhow!("arguments : JSON attendu ({e})"))?;
            if !arguments.is_object() {
                anyhow::bail!("arguments : objet JSON attendu");
            }
            // Un outil travaille pour de vrai : l'arbre d'accessibilité d'une application, une
            // page web, un document. Deux secondes, le délai d'une sonde, ne lui suffisent
            // pas — le scénario du bureau installé l'a montré sur `ui.tree` (« pas de réponse
            // en 2 s »). Une minute : le service et l'adaptateur bornent chacun leur part.
            let result = task_rpc_sous(
                &socket_agentd(),
                "task.call",
                serde_json::json!({"id":id, "name":tool, "arguments":arguments}),
                DELAI_D_OUTIL,
            )?;
            if as_json {
                return Ok(format!("{}\n", serde_json::to_string_pretty(&result)?));
            }
            let erreur = result["isError"].as_bool().unwrap_or(false);
            let texte = result["content"]
                .as_array()
                .map(|c| {
                    c.iter()
                        .filter_map(|m| m["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            if erreur {
                anyhow::bail!("{tool} : {texte}");
            }
            Ok(format!("{texte}\n"))
        }
        TaskAction::Detach { id, text } => {
            let mut params = serde_json::json!({"id":id});
            if let Some(text) = text {
                params["text"] = serde_json::json!(text);
            }
            let result = task_rpc(&socket_agentd(), "task.detach", params)?;
            if as_json {
                return Ok(format!("{}\n", serde_json::to_string_pretty(&result)?));
            }
            Ok(format!(
                "Séance fermée : la mission {id} est {}.\n",
                result["state"].as_str().unwrap_or("à l'examen")
            ))
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

fn model(action: &ModelAction, as_json: bool) -> anyhow::Result<String> {
    let ModelAction::Ls { dir, endpoint } = action;
    // Le moteur dit ce qu'il sert ; injoignable, le catalogue se lit quand même.
    let endpoint = endpoint
        .clone()
        .or_else(|| std::env::var("PROPHET_MODEL_ENDPOINT").ok())
        .unwrap_or_else(|| "http://127.0.0.1:8080/v1".to_owned());
    let served = providers::local::LocalModel::new(
        &endpoint,
        "catalogue",
        std::time::Duration::from_secs(2),
    )
    .and_then(|engine| engine.served());
    let served_path = served
        .as_ref()
        .ok()
        .and_then(|s| s.path.as_ref())
        .and_then(|p| std::fs::canonicalize(p).ok());
    let is_served = |path: &std::path::Path| {
        served_path.is_some() && std::fs::canonicalize(path).ok() == served_path
    };
    // Un dossier nommé se lit seul ; sinon, le dossier des poids et les fichiers que la
    // configuration du système nomme, comme le modèle par défaut dans /nix/store (ADR 0033).
    let (dir, catalog) = match dir {
        Some(dir) => (dir.clone(), providers::weights::catalog(dir)),
        None => {
            let dir = providers::weights::dir();
            let catalog = providers::weights::installed(&dir, &providers::weights::configured());
            (dir, catalog)
        }
    };
    if as_json {
        let (weights, refused): (Vec<_>, Vec<_>) = catalog.into_iter().partition(Result::is_ok);
        return Ok(format!(
            "{}\n",
            serde_json::json!({
                "dir": dir,
                "weights": weights.into_iter().flatten().collect::<Vec<_>>(),
                "refused": refused.into_iter().filter_map(Result::err).collect::<Vec<_>>(),
                "endpoint": endpoint,
                "served": served.as_ref().ok(),
                "engine_error": served.as_ref().err().map(ToString::to_string),
            })
        ));
    }
    if catalog.is_empty() {
        return Ok(format!("Aucun poids GGUF dans {}.\n", dir.display()));
    }
    let mut out = format!(
        "{:<28} {:<10} {:<8} {:<8} {:>9} {:>8}\n",
        "fichier", "archi.", "taille", "quant.", "contexte", "Go"
    );
    let tiret = || "—".to_owned();
    for entry in catalog {
        match entry {
            Ok(w) => {
                let go = w.gigabytes();
                let marque = if is_served(&w.path) {
                    match served.as_ref().ok().and_then(|s| s.n_ctx) {
                        Some(n_ctx) => format!("  ← servi, fenêtre {n_ctx}"),
                        None => "  ← servi".to_owned(),
                    }
                } else {
                    String::new()
                };
                out.push_str(&format!(
                    "{:<28} {:<10} {:<8} {:<8} {:>9} {:>8.1}{marque}\n",
                    w.path
                        .file_name()
                        .map_or_else(tiret, |n| n.to_string_lossy().into_owned()),
                    w.architecture.unwrap_or_else(tiret),
                    w.size_label.unwrap_or_else(tiret),
                    w.quantization.unwrap_or_else(tiret),
                    w.context_length.map_or_else(tiret, |c| c.to_string()),
                    go
                ));
            }
            Err(raison) => out.push_str(&format!("refusé : {raison}\n")),
        }
    }
    match &served {
        Ok(providers::local::Served {
            path: Some(path),
            n_ctx,
        }) if !is_served(path) => {
            out.push_str(&format!(
                "Le moteur sert {}{}, hors de ce catalogue.\n",
                path.display(),
                n_ctx.map_or_else(String::new, |n| format!(" avec une fenêtre de {n} tokens"))
            ));
        }
        Ok(_) => {}
        Err(error) => out.push_str(&format!("{error} : rien n'est servi.\n")),
    }
    Ok(out)
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

fn jev(action: &JevAction, as_json: bool) -> anyhow::Result<String> {
    match action {
        JevAction::Route { request } => {
            use std::io::Read as _;
            let mut bytes = Vec::new();
            std::fs::File::open(request)?
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            anyhow::ensure!(
                bytes.len() <= 1024 * 1024,
                "requête de mission limitée à 1 Mio"
            );
            let demande: serde_json::Value = serde_json::from_slice(&bytes)?;
            let params = serde_json::json!({
                "intent": demande["intent"],
                "manifest": demande["manifest"],
                "availability": demande.get("availability").cloned().unwrap_or(serde_json::Value::Null),
            });
            let result = task_rpc(&socket_agentd(), "task.route", params)?;
            if as_json {
                return Ok(format!("{}\n", serde_json::to_string_pretty(&result)?));
            }
            let route: providers::jev::router::Route = serde_json::from_value(result)?;
            let mut out = format!(
                "Modèle : {}\nRaison : {}\nDécideur : {}\n",
                route.choice.reference,
                route.choice.reason,
                match route.decider {
                    providers::jev::router::Decider::Jev => "Jev",
                    providers::jev::router::Decider::Static => "sélection statique",
                }
            );
            for (reference, p) in &route.probabilities {
                out.push_str(&format!("  {reference:<32} p = {p:.2}\n"));
            }
            if let Some(difficulty) = route.difficulty {
                out.push_str(&format!(
                    "Difficulté : {difficulty:.2} (0 triviale, 1 experte)\n"
                ));
            }
            if let Some(risk) = route.risk {
                out.push_str(&format!("Risque : {risk:.2}\n"));
            }
            if let Some(reason) = &route.fallback_reason {
                out.push_str(&format!("Sans Jev : {reason}\n"));
            }
            Ok(out)
        }
        JevAction::Status => {
            let options = task_rpc(&socket_agentd(), "task.options", serde_json::json!({}))?;
            let configured = options.get("jev").cloned().filter(|j| !j.is_null());
            let secret = configured
                .as_ref()
                .and_then(|j| j["secret"].as_str())
                .map(str::to_owned);
            let dans_le_coffre = match &secret {
                Some(nom) => {
                    let refs =
                        task_rpc(&socket_vault(), "secrets.list_refs", serde_json::json!({})).ok();
                    refs.and_then(|liste| {
                        liste.as_array().map(|l| {
                            l.iter().any(|s| {
                                s["name"] == nom.as_str()
                                    && s["domains"].as_array().is_some_and(|d| {
                                        d.iter().any(|h| h == providers::jev::HOST)
                                    })
                            })
                        })
                    })
                }
                None => None,
            };
            if as_json {
                return Ok(format!(
                    "{}\n",
                    serde_json::json!({
                        "configured": configured.is_some(),
                        "model": configured.as_ref().and_then(|j| j["model"].as_str()),
                        "secret": secret,
                        "host": providers::jev::HOST,
                        "secret_in_vault": dans_le_coffre,
                    })
                ));
            }
            let Some(jev) = configured else {
                return Ok("Jev : non configuré sur ce service (PROPHET_JEV_SECRET absent). Les missions tournent sans décideur rapide.\n".into());
            };
            Ok(format!(
                "Jev : configuré, modèle {}\nSecret : {} ({})\nHôte : {} (sortie par egress, POST traité comme une lecture si l'hôte est déclaré)\n",
                jev["model"].as_str().unwrap_or("?"),
                jev["secret"].as_str().unwrap_or("?"),
                match dans_le_coffre {
                    Some(true) => "présent dans le coffre pour cet hôte",
                    Some(false) =>
                        "ABSENT du coffre ou non destiné à cet hôte : `prophet secret put`",
                    None => "coffre injoignable",
                },
                providers::jev::HOST
            ))
        }
    }
}

fn secret(action: &SecretAction, as_json: bool) -> anyhow::Result<String> {
    match action {
        SecretAction::Put {
            name,
            hosts,
            header,
            description,
        } => {
            use std::io::Read as _;
            let mut valeur = String::new();
            std::io::stdin()
                .take(64 * 1024)
                .read_to_string(&mut valeur)?;
            let valeur = valeur.trim_end_matches(['\n', '\r']).to_owned();
            anyhow::ensure!(
                !valeur.is_empty(),
                "aucune valeur lue sur l'entrée standard"
            );
            let result = task_rpc(
                &socket_vault(),
                "vault.put",
                serde_json::json!({
                    "info": {"name": name, "domains": hosts, "header": header, "description": description},
                    "value": valeur,
                }),
            )?;
            if as_json {
                Ok(format!("{}\n", serde_json::to_string(&result)?))
            } else {
                Ok(format!(
                    "Secret {name} déposé pour {} ; les agents le référencent par prophet-secret:{name}.\n",
                    hosts.join(", ")
                ))
            }
        }
        SecretAction::Ls => {
            let refs = task_rpc(&socket_vault(), "secrets.list_refs", serde_json::json!({}))?;
            if as_json {
                return Ok(format!("{}\n", serde_json::to_string_pretty(&refs)?));
            }
            let liste = refs.as_array().cloned().unwrap_or_default();
            if liste.is_empty() {
                return Ok("aucun secret dans le coffre\n".into());
            }
            let mut out = format!("{:<24} {:<16} {}\n", "nom", "en-tête", "hôtes");
            for s in liste {
                out.push_str(&format!(
                    "{:<24} {:<16} {}\n",
                    s["name"].as_str().unwrap_or("?"),
                    s["header"].as_str().unwrap_or("?"),
                    s["domains"]
                        .as_array()
                        .map(|d| d
                            .iter()
                            .filter_map(|h| h.as_str())
                            .collect::<Vec<_>>()
                            .join(", "))
                        .unwrap_or_default()
                ));
            }
            Ok(out)
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
/// Ce qu'on accorde à un outil appelé dans une séance depuis le terminal.
const DELAI_D_OUTIL: std::time::Duration = std::time::Duration::from_secs(60);

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
    sous_delai_de(travail, DELAI_DE_SONDE)
}

/// Comme [`sous_delai`], avec le délai qu'on veut.
fn sous_delai_de<T>(
    travail: impl std::future::Future<Output = Result<T, String>>,
    delai: std::time::Duration,
) -> Result<T, String> {
    let execution = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    execution.block_on(async {
        tokio::time::timeout(delai, travail)
            .await
            .map_err(|_| format!("pas de réponse en {} s", delai.as_secs()))?
    })
}

/// Où joindre le coffre.
fn socket_vault() -> std::path::PathBuf {
    std::env::var("PROPHET_VAULT_SOCKET").map_or_else(
        |_| prophet_ipc::socket_path("vault"),
        std::path::PathBuf::from,
    )
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
    task_rpc_sous(socket, method, params, DELAI_DE_SONDE)
}

/// Comme [`task_rpc`], avec le délai qu'on veut.
fn task_rpc_sous(
    socket: &std::path::Path,
    method: &str,
    params: serde_json::Value,
    delai: std::time::Duration,
) -> anyhow::Result<serde_json::Value> {
    sous_delai_de(
        async {
            let client = prophet_ipc::Client::connect(socket)
                .await
                .map_err(|e| format!("agentd indisponible : {e}"))?;
            client.call(method, params).await.map_err(|e| e.message)
        },
        delai,
    )
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
/// Le catalogue du service, avec l'état du navigateur piloté et des pilotes de session.
fn options_du_service(socket: &std::path::Path) -> Option<agentd::preparation::Options> {
    sous_delai(async {
        let client = prophet_ipc::Client::connect(socket)
            .await
            .map_err(|e| e.to_string())?;
        let options = client
            .call("task.options", serde_json::json!({}))
            .await
            .map_err(|e| e.message.clone())?;
        serde_json::from_value::<agentd::preparation::Options>(options).map_err(|e| e.to_string())
    })
    .ok()
}

fn socket_sandboxd() -> std::path::PathBuf {
    std::env::var("PROPHET_SANDBOXD_SOCKET").map_or_else(
        |_| prophet_ipc::socket_path("sandboxd"),
        std::path::PathBuf::from,
    )
}

/// Ce que sandboxd dit savoir isoler, réserve de microVM comprise ; `None` s'il se tait.
fn capacites_du_bac(socket: &std::path::Path) -> Option<serde_json::Value> {
    sous_delai(async {
        let client = prophet_ipc::Client::connect(socket)
            .await
            .map_err(|e| e.to_string())?;
        client
            .call("sandbox.capabilities", serde_json::json!({}))
            .await
            .map_err(|e| e.message.clone())
    })
    .ok()
}

/// Les lignes de `prophet status` sur la réserve de microVM du niveau 2 (ADR 0045).
fn status_reserve(capacites: Option<&serde_json::Value>) -> String {
    let mut out = String::from("\n  Réserve de microVM\n");
    let Some(capacites) = capacites else {
        out.push_str("    ? sandboxd ne répond pas\n");
        return out;
    };
    let reserve = &capacites["reserve"];
    if !reserve.is_object() {
        out.push_str(
            "    — aucune : le niveau 2 est inatteignable ici, ou PROPHET_MICROVM_POOL=0\n",
        );
        return out;
    }
    let pretes = reserve["pretes"].as_u64().unwrap_or(0);
    let cible = reserve["cible"].as_u64().unwrap_or(0);
    match reserve["erreur"].as_str() {
        Some(erreur) if pretes == 0 => out.push_str(&format!(
            "    ✗ vide — {erreur} ; le niveau 2 démarre à froid\n"
        )),
        _ => {
            let signe = if pretes >= cible { "✓" } else { "·" };
            out.push_str(&format!(
                "    {signe} {pretes} prête{} sur {cible} — le niveau 2 part sans démarrer de noyau{}{}\n",
                if pretes > 1 { "s" } else { "" },
                reserve["restauration_ms"]
                    .as_u64()
                    .map_or_else(String::new, |ms| format!(" ; restauration en {ms} ms")),
                if reserve["instantane_repris"].as_bool() == Some(true) {
                    " ; instantané repris du démarrage précédent"
                } else {
                    ""
                }
            ));
        }
    }
    out
}

/// Les lignes de `prophet status` sur ce que l'humain peut dire et entendre, et sur les clients
/// officiels que le lanceur de session sait lancer (ADR 0035, 0036). Rien n'est inventé : un
/// service muet donne « inconnu », une chaîne absente le dit.
/// Le relevé que l'installeur a fait de la machine avant d'effacer le disque, gardé avec la
/// source posée sur elle ; `PROPHET_INVENTAIRE` le déplace (essais, machine non installée par
/// l'installeur).
fn chemin_de_l_inventaire() -> std::path::PathBuf {
    std::env::var_os("PROPHET_INVENTAIRE").map_or_else(
        || std::path::PathBuf::from("/etc/prophet/source/image/machine/inventaire.txt"),
        std::path::PathBuf::from,
    )
}

/// Ce que l'installeur a vu de la machine : processeur, mémoire, KVM, carte graphique, réseau,
/// son et micro, Secure Boot, TPM. Une ligne que l'installeur a marquée « ! » est un manque, et
/// se lit comme tel ; sans relevé, la commande le dit au lieu de deviner.
fn status_machine(inventaire: &std::path::Path) -> String {
    let mut out = String::from("\n  Machine, vue par l'installeur\n");
    match std::fs::read_to_string(inventaire) {
        Ok(texte) if !texte.trim().is_empty() => {
            for ligne in texte.lines() {
                let ligne = ligne.trim_end();
                if let Some(manque) = ligne.strip_prefix('!') {
                    out.push_str(&format!("    ✗ {}\n", manque.trim()));
                } else if !ligne.trim().is_empty() {
                    out.push_str(&format!("    · {}\n", ligne.trim()));
                }
            }
        }
        _ => out.push_str(
            "    — aucun relevé : cette machine n'a pas été installée par l'installeur de Prophet OS\n",
        ),
    }
    out
}

fn status_parole_et_pilotes(options: Option<&agentd::preparation::Options>) -> String {
    let mut out = String::from("\n  Parole\n");
    match voice::Tools::from_env() {
        Ok(tools) => {
            out.push_str(&format!(
                "    ✓ écoute — {} ; {}\n",
                tools.model.display(),
                match &tools.recorder {
                    Some(_) => "micro par la session",
                    None => "aucun enregistreur (fichiers seulement)",
                }
            ));
            out.push_str(&if tools.can_speak() {
                "    ✓ parle — voix locale\n".to_owned()
            } else {
                "    ✗ parle — aucune voix (PROPHET_PIPER_VOICE)\n".to_owned()
            });
        }
        Err(e) => out.push_str(&format!("    ✗ {e}\n")),
    }
    out.push_str("\n  Pilotes de session\n");
    match options.and_then(|o| o.pilot.as_ref()) {
        None if options.is_none() => out.push_str("    ? agentd ne répond pas\n"),
        None => out.push_str("    — aucun lanceur de pilotes dans la session\n"),
        Some(status) => {
            for d in &status.drivers {
                let signe = if d.ready() { "✓" } else { "✗" };
                out.push_str(&format!(
                    "    {signe} {} — {}{}\n",
                    d.driver,
                    match d.connection.as_str() {
                        "connected" => "connecté".to_owned(),
                        "simulated" => "simulé (essai)".to_owned(),
                        "login_required" => "installé, connexion requise".to_owned(),
                        "missing" => "absent de cette machine".to_owned(),
                        other => other.to_owned(),
                    },
                    d.version
                        .as_deref()
                        .map(|v| format!(" ({v})"))
                        .unwrap_or_default()
                ));
            }
        }
    }
    out
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
mod machine {
    //! Le relevé de l'installeur, relu par `prophet status`.

    #[test]
    fn le_releve_de_l_installeur_se_relit_avec_ses_manques() {
        let dir = tempfile::tempdir().unwrap();
        let releve = dir.path().join("inventaire.txt");
        std::fs::write(
            &releve,
            "  processeur : 8 cœurs, AMD Ryzen 5\n! son : aucune carte détectée — ni voix ni parole sur cette machine\n  TPM : présent\n",
        )
        .unwrap();
        let rendu = super::status_machine(&releve);
        assert!(rendu.contains("Machine, vue par l'installeur"), "{rendu}");
        assert!(
            rendu.contains("· processeur : 8 cœurs, AMD Ryzen 5"),
            "{rendu}"
        );
        assert!(rendu.contains("✗ son : aucune carte détectée"), "{rendu}");
        assert!(rendu.contains("· TPM : présent"), "{rendu}");

        let rendu = super::status_machine(&dir.path().join("absent.txt"));
        assert!(rendu.contains("aucun relevé"), "{rendu}");
    }
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

    #[test]
    fn la_reserve_de_microvm_se_dit_dans_prophet_status() {
        let pleine = serde_json::json!({"reserve": {"cible": 2, "pretes": 2, "restauration_ms": 41, "erreur": null}});
        let dit = status_reserve(Some(&pleine));
        assert!(
            dit.contains("✓ 2 prêtes sur 2") && dit.contains("41 ms"),
            "{dit}"
        );
        let vide = serde_json::json!({"reserve": {"cible": 2, "pretes": 0, "erreur": "instantané refusé"}});
        let dit = status_reserve(Some(&vide));
        assert!(
            dit.contains("✗ vide") && dit.contains("instantané refusé"),
            "{dit}"
        );
        assert!(!dit.contains("repris"), "{dit}");
        let reprise = serde_json::json!({"reserve": {"cible": 2, "pretes": 1, "restauration_ms": 6, "erreur": null, "instantane_repris": true}});
        let dit = status_reserve(Some(&reprise));
        assert!(
            dit.contains("· 1 prête sur 2") && dit.contains("instantané repris"),
            "{dit}"
        );
        assert!(status_reserve(Some(&serde_json::json!({"reserve": null}))).contains("— aucune"));
        assert!(status_reserve(None).contains("ne répond pas"));
    }
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
            vec!["prophet", "jev", "route", "mission.json"],
            vec!["prophet", "jev", "status"],
            vec![
                "prophet",
                "secret",
                "put",
                "typesafe",
                "--host",
                "api.typesafe.ai",
            ],
            vec!["prophet", "secret", "ls"],
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
