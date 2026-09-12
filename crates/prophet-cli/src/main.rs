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
    /// Annule une tâche en cours.
    Cancel {
        /// Identifiant.
        id: String,
    },
    /// Annule les changements d'une tâche déjà validée.
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
            Ok(shell::status_avec_services(
                &sandboxd::Capabilities::probe(),
                &sfs::detect_backend(&home()),
                &refs,
                0,
                &services(),
            ))
        }
        Command::Freeze => {
            let manager = sandboxd::Manager::new("prophet-sandbox-helper");
            let gelees = manager.freeze_all();
            Ok(format!("{gelees} sandbox(es) gelée(s)\n"))
        }
        Command::Provider { action } => provider(action),
        Command::Memory { action } => memory(action),
        Command::Log { action } => log(action),
        Command::Task { action } => task(action),
        // Les approbations vivent dans un daemon en service : sans lui, la commande le dit au
        // lieu de faire semblant.
        Command::Cap { .. } => anyhow::bail!(
            "les approbations exigent capd en service. \
             Lancez `prophet status` pour voir ce qui est disponible sur cette machine."
        ),
    }
}

/// Journal d'audit. Il vit dans des fichiers : ces commandes fonctionnent sans daemon, ce qui est
/// exactement ce qu'on attend d'un journal d'audit, y compris après un incident.
fn log(action: &LogAction) -> anyhow::Result<String> {
    let racine = home().join(".prophet/ledger");
    if !racine.exists() {
        return Ok("aucun journal sur cette machine\n".to_owned());
    }
    let store = ledger::Store::open(&racine)?;
    match action {
        LogAction::Tail { number } => {
            let mut events = store.read_all()?;
            let depart = events.len().saturating_sub(*number);
            events.drain(..depart);
            if events.is_empty() {
                return Ok("journal vide\n".to_owned());
            }
            let mut out = String::new();
            for event in events {
                out.push_str(&format!(
                    "{:>8}  {:<22} {}\n",
                    event.seq,
                    shell::kind_label(event.kind),
                    event.task.unwrap_or_default()
                ));
            }
            Ok(out)
        }
        LogAction::Replay { task } => {
            let events = store.read_all()?;
            Ok(shell::timeline(task, &events))
        }
        LogAction::Verify => {
            let rapport = store.verify()?;
            if rapport.ok {
                Ok(format!(
                    "journal intact : {} événements, {} sceaux vérifiés\n",
                    rapport.checked, rapport.seals
                ))
            } else {
                anyhow::bail!(
                    "journal altéré à la séquence {} : {}",
                    rapport.first_bad_seq.unwrap_or(0),
                    rapport.reason.unwrap_or_default()
                )
            }
        }
    }
}

/// Tâches. Leur espace de travail vit sur le disque : diff et annulation fonctionnent donc sans
/// daemon, ce qui compte, car c'est précisément quand quelque chose a mal tourné qu'on en a besoin.
fn task(action: &TaskAction) -> anyhow::Result<String> {
    let maison = home();
    match action {
        TaskAction::Ls => {
            // Deux questions différentes, et il vaut mieux les poser toutes les deux. `agentd` sait
            // ce qui *tourne* ; les espaces de travail savent ce qui a *changé des fichiers*. Une
            // tâche fraîchement planifiée n'a encore touché à rien, et n'apparaissait donc nulle
            // part — ce qui donnait « aucune tâche » à quelqu'un qui venait d'en lancer une.
            let mut out = String::new();
            match taches_en_cours(&socket_agentd()) {
                Ok(taches) if taches.is_empty() => {
                    out.push_str("aucune tâche en cours\n\n");
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
                    out.push('\n');
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
            let espace = sfs::Workspace::open(&maison, id)?;
            Ok(format!(
                "Tâche {id}\n  espace de travail : {:?}\n  dorsale : {}\n\n{}",
                espace.state(),
                espace.backend().reason,
                espace.diff()?.render()
            ))
        }
        TaskAction::Undo { id } => {
            let mut espace = sfs::Workspace::open(&maison, id)?;
            let diff = espace.undo()?;
            let (a, m, s) = diff.counts();
            Ok(format!(
                "tâche {id} annulée : {a} création(s) retirée(s), {m} modification(s) rétablie(s), {s} suppression(s) rétablie(s)\n"
            ))
        }
        TaskAction::Cancel { .. } => anyhow::bail!(
            "annuler une tâche en cours exige agentd en service ; \
             pour défaire une tâche déjà validée, utilisez `prophet task undo`"
        ),
    }
}

fn provider(action: &ProviderAction) -> anyhow::Result<String> {
    use providers::official::{ClientProfile, OfficialDriver};
    let racine = std::path::Path::new("/var/lib/prophet");
    let utilisateur = std::env::var("USER").unwrap_or_else(|_| "inconnu".to_owned());
    match action {
        ProviderAction::Ls => {
            let mut out = format!(
                "{:<16} {:<16} {:<14} {}\n",
                "pilote", "authentification", "client", "session"
            );
            for profile in ClientProfile::all() {
                let driver = OfficialDriver::new(profile.clone(), racine, &utilisateur);
                out.push_str(&format!(
                    "{:<16} {:<16} {:<14} {}\n",
                    profile.driver,
                    "abonnement",
                    if driver.client_available() {
                        "présent"
                    } else {
                        "absent"
                    },
                    if driver.logged_in() {
                        "connectée"
                    } else {
                        "aucune"
                    }
                ));
            }
            out.push_str(&format!(
                "{:<16} {:<16} {:<14} {}\n",
                "prophet-agent", "aucune", "intégré", "sans objet"
            ));
            Ok(out)
        }
        ProviderAction::Login { driver } => {
            let profile = ClientProfile::all()
                .into_iter()
                .find(|p| &p.driver == driver)
                .ok_or_else(|| anyhow::anyhow!("pilote inconnu : {driver}"))?;
            Ok(format!(
                "{}\n",
                OfficialDriver::new(profile, racine, &utilisateur).login_instructions()
            ))
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

/// Lesquels des services répondent, et pourquoi les autres ne répondent pas.
///
/// Un `ping` sur chaque socket. C'est peu, mais c'est la seule chose qui distingue « le service
/// est déclaré » de « le service sert » — et l'histoire de ce dépôt montre que confondre les deux
/// coûte cher.
fn services() -> Vec<(String, Option<String>)> {
    [
        "capd", "ledger", "vault", "egress", "sandboxd", "memoryd", "agentd",
    ]
    .into_iter()
    .map(|nom| {
        let socket = prophet_ipc::socket_path(nom);
        (nom.to_owned(), repond(&socket).err())
    })
    .collect()
}

/// Le service répond-il ? On lui parle, on ne regarde pas si son fichier existe.
fn repond(socket: &std::path::Path) -> Result<(), String> {
    let execution = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    execution.block_on(async {
        let client = prophet_ipc::Client::connect(socket)
            .await
            .map_err(|_| "socket injoignable".to_owned())?;
        client
            .call("ping", serde_json::json!({}))
            .await
            .map(|_| ())
            .map_err(|e| e.message.clone())
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

/// Les tâches que `agentd` tient en ce moment.
///
/// La CLI parle au daemon plutôt que de deviner : lui seul sait ce qui est planifié, en cours ou
/// en attente d'approbation. Quand il n'est pas là, on le dit — c'est une information, pas une
/// panne : sur une machine où rien ne tourne, `prophet log` et `prophet task show` restent utiles
/// parce qu'ils lisent des fichiers.
fn taches_en_cours(socket: &std::path::Path) -> Result<Vec<agentd::Task>, String> {
    let execution = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    execution.block_on(async {
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
