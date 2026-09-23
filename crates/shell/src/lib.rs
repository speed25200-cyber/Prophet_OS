//! Vues destinées à l'humain.
//!
//! Toutes sont des fonctions pures d'un état vers du texte : elles se testent sans lancer quoi que
//! ce soit, et elles servent aussi bien au binaire `prophet` qu'à une interface graphique.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod intent;

pub use intent::{Proposal, propose, render_proposal};

use agentd::{State, Task};
use prophet_types::ledger::{Event, EventKind};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;

/// Marque d'un état, lisible sans couleur.
#[must_use]
pub const fn state_mark(state: State) -> &'static str {
    match state {
        State::Pending => "·",
        State::Planned => "○",
        State::Running => "▸",
        State::WaitingApproval => "?",
        State::Paused => "‖",
        State::Done => "✓",
        State::Failed => "✗",
        State::Cancelled => "⊘",
        State::RolledBack => "↩",
    }
}

/// Nom français d'un état.
#[must_use]
pub const fn state_name(state: State) -> &'static str {
    match state {
        State::Pending => "en attente",
        State::Planned => "planifiée",
        State::Running => "en cours",
        State::WaitingApproval => "approbation",
        State::Paused => "suspendue",
        State::Done => "terminée",
        State::Failed => "échouée",
        State::Cancelled => "annulée",
        State::RolledBack => "annulée après coup",
    }
}

/// Liste des tâches.
#[must_use]
pub fn task_list(tasks: &[&Task]) -> String {
    if tasks.is_empty() {
        return "aucune tâche\n".to_owned();
    }
    let mut out = format!(
        "{:<3} {:<26} {:<20} {:>6} {:>9}  {}\n",
        "", "tâche", "état", "étapes", "tokens", "intention"
    );
    for task in tasks {
        out.push_str(&format!(
            "{:<3} {:<26} {:<20} {:>6} {:>9}  {}\n",
            state_mark(task.state),
            format!("{}{}", "  ".repeat(task.depth as usize), task.id),
            state_name(task.state),
            task.budget.spent.steps,
            task.budget.spent.tokens,
            truncate(&task.intent, 46)
        ));
    }
    out
}

/// Timeline d'une tâche, à partir de ses événements.
#[must_use]
pub fn timeline(task: &str, events: &[Event]) -> String {
    let concernes: Vec<&Event> = events
        .iter()
        .filter(|e| e.task.as_deref() == Some(task))
        .collect();
    if concernes.is_empty() {
        return format!("aucun événement pour {task}\n");
    }
    let mut out = format!("Timeline de {task}\n\n");
    let mut etape_courante = None;
    for event in concernes {
        if event.step != etape_courante && event.step.is_some() {
            etape_courante = event.step;
            out.push_str(&format!("  étape {}\n", event.step.unwrap_or(0)));
        }
        let heure = event
            .ts
            .format(&Rfc3339)
            .unwrap_or_default()
            .chars()
            .skip(11)
            .take(8)
            .collect::<String>();
        out.push_str(&format!(
            "    {heure}  {:<22} {}\n",
            kind_label(event.kind),
            summarize(event)
        ));
    }
    out
}

/// Libellé français d'un type d'événement.
#[must_use]
pub const fn kind_label(kind: EventKind) -> &'static str {
    match kind {
        EventKind::TaskCreated => "tâche créée",
        EventKind::TaskPlanned => "tâche planifiée",
        EventKind::TaskStarted => "tâche démarrée",
        EventKind::TaskWaiting => "en attente",
        EventKind::TaskDone => "tâche terminée",
        EventKind::TaskFailed => "tâche échouée",
        EventKind::TaskCancelled => "tâche annulée",
        EventKind::TaskRolledBack => "tâche annulée après",
        EventKind::ToolCall => "appel d'outil",
        EventKind::ToolResult => "résultat d'outil",
        EventKind::PolicyAllow => "politique : accord",
        EventKind::PolicyDeny => "politique : refus",
        EventKind::PolicyRevoked => "jeton révoqué",
        EventKind::ApprovalRequested => "approbation demandée",
        EventKind::ApprovalResolved => "approbation tranchée",
        EventKind::FsBegin => "espace de travail",
        EventKind::FsCommit => "changements validés",
        EventKind::FsUndo => "changements annulés",
        EventKind::FsAbandon => "travail abandonné",
        EventKind::NetRequest => "requête réseau",
        EventKind::NetDeny => "réseau refusé",
        EventKind::NetExfilSuspected => "exfiltration suspectée",
        EventKind::ProviderStarted => "pilote démarré",
        EventKind::ProviderQuota => "quota",
        EventKind::ProviderStopped => "pilote arrêté",
        EventKind::SandboxStarted => "sandbox démarrée",
        EventKind::SandboxFrozen => "sandbox gelée",
        EventKind::SandboxKilled => "sandbox arrêtée",
        EventKind::UiTree => "lecture d'interface",
        EventKind::UiAct => "action d'interface",
        EventKind::MemoryWrite => "mémoire écrite",
        EventKind::ModelPulled => "poids téléchargé",
        EventKind::ModelRemoved => "poids retiré",
        EventKind::LedgerSeal => "journal scellé",
    }
}

fn summarize(event: &Event) -> String {
    let p = &event.payload;
    let texte = |clef: &str| p.get(clef).and_then(Value::as_str).unwrap_or("");
    match event.kind {
        EventKind::ToolCall => texte("tool").to_owned(),
        EventKind::ToolResult => format!(
            "{} {}",
            texte("tool"),
            if p.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                "ok"
            } else {
                "échec"
            }
        ),
        EventKind::PolicyDeny => format!("{} {}", texte("tool"), texte("reason")),
        EventKind::ApprovalRequested => texte("action").to_owned(),
        EventKind::ProviderStarted | EventKind::ProviderStopped => texte("driver").to_owned(),
        EventKind::NetRequest | EventKind::NetDeny | EventKind::NetExfilSuspected => {
            texte("host").to_owned()
        }
        EventKind::TaskDone | EventKind::TaskFailed => p
            .get("stats")
            .map(|s| {
                format!(
                    "{} étapes, {} outils, {} tokens",
                    s.get("steps").and_then(Value::as_u64).unwrap_or(0),
                    s.get("tool_calls").and_then(Value::as_u64).unwrap_or(0),
                    s.get("tokens").and_then(Value::as_u64).unwrap_or(0)
                )
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Centre d'approbations.
///
/// Une demande doit pouvoir être tranchée en cinq secondes : on montre donc ce qui va se passer,
/// pas la mécanique qui y mène.
#[must_use]
pub fn approvals(pending: &[capd::Approval]) -> String {
    if pending.is_empty() {
        return "aucune approbation en attente\n".to_owned();
    }
    let mut out = format!("{} demande(s) en attente\n\n", pending.len());
    for (index, approval) in pending.iter().enumerate() {
        let marques = match (approval.irreversible, approval.external) {
            (true, true) => "irréversible, hors de la machine",
            (true, false) => "irréversible",
            (false, true) => "hors de la machine",
            (false, false) => "réversible",
        };
        out.push_str(&format!(
            "  [{}] {}\n      {} · {} sur {}\n      demandé par {} pour {}\n",
            index + 1,
            approval.summary,
            marques,
            approval.action,
            approval.target,
            approval.agent,
            approval.task
        ));
    }
    out.push_str("\n  o accorder une fois · t pour la tâche · a pour l'agent · n refuser\n");
    out
}

/// Statut du système.
#[must_use]
pub fn status(
    caps: &sandboxd::Capabilities,
    backend: &sfs::Backend,
    tasks: &[&Task],
    pending_approvals: usize,
) -> String {
    status_avec_services(caps, backend, tasks, pending_approvals, &[])
}

/// Comme [`status`], en disant aussi lesquels des services répondent.
///
/// C'est la première chose qu'on veut savoir après une installation, et la seule que les autres
/// lignes ne disent pas : un système dont `capd` est mort affiche une isolation parfaite et un
/// journal intact, et ne peut rien faire.
#[must_use]
pub fn status_avec_services(
    caps: &sandboxd::Capabilities,
    backend: &sfs::Backend,
    tasks: &[&Task],
    pending_approvals: usize,
    services: &[(String, Option<String>)],
) -> String {
    status_complet(
        caps,
        backend,
        tasks,
        pending_approvals,
        services,
        &Navigateur::Inconnu,
    )
}

/// Ce que `agentd` dit de son navigateur piloté, tel que `task.options` le rend.
#[derive(Debug, Clone, Copy)]
pub enum Navigateur<'a> {
    /// Personne n'a demandé, ou `agentd` ne répond pas : ne rien inventer.
    Inconnu,
    /// Le service n'en configure aucun : les contextes web sont refusés à la préparation.
    Aucun,
    /// Le verdict de la sonde du service.
    Sonde(&'a agentd::preparation::BrowserState),
}

/// Comme [`status_avec_services`], en disant aussi si le navigateur piloté répond.
///
/// C'est ce qui sépare « un navigateur est installé » de « une mission peut ouvrir une page » :
/// le service le sonde sous ses propres contraintes, et c'est son verdict qu'on affiche.
#[must_use]
pub fn status_complet(
    caps: &sandboxd::Capabilities,
    backend: &sfs::Backend,
    tasks: &[&Task],
    pending_approvals: usize,
    services: &[(String, Option<String>)],
    navigateur: &Navigateur<'_>,
) -> String {
    let actives = tasks.iter().filter(|t| !t.state.is_terminal()).count();
    let mut out = String::from("Prophet OS\n\n");
    out.push_str(&format!(
        "  Tâches        : {actives} active(s), {} au total\n",
        tasks.len()
    ));
    out.push_str(&format!(
        "  Approbations  : {pending_approvals} en attente\n"
    ));
    out.push_str(&format!("  Fichiers      : {}\n", backend.reason));
    if !backend.has_native_snapshots() {
        out.push_str(&format!(
            "                  limite : {}\n",
            backend.limitations()
        ));
    }
    if !services.is_empty() {
        out.push_str("\n  Services\n");
        let muets = services.iter().filter(|(_, p)| p.is_some()).count();
        for (nom, panne) in services {
            match panne {
                None => out.push_str(&format!("    ✓ {nom}\n")),
                Some(raison) => out.push_str(&format!("    ✗ {nom} — {raison}\n")),
            }
        }
        if muets > 0 {
            // Dire ce que l'absence coûte, plutôt que de laisser une croix sans conséquence.
            out.push_str(&format!(
                "    {muets} service(s) muet(s) : voir `docs/components/` pour ce que chacun\n\
                 \x20   emporte avec lui, et `journalctl -u prophet-<nom>` pour savoir pourquoi\n"
            ));
        }
    }

    match navigateur {
        Navigateur::Inconnu => {}
        Navigateur::Aucun => out.push_str(
            "\n  Navigateur piloté\n    aucun — sans PROPHET_BROWSER, les contextes web sont refusés à la préparation\n",
        ),
        Navigateur::Sonde(etat) => {
            let signe = if etat.ready { '✓' } else { '✗' };
            out.push_str(&format!(
                "\n  Navigateur piloté\n    {signe} {} — {}\n",
                etat.program, etat.detail
            ));
        }
    }

    out.push_str("\n  Isolation\n");
    for ligne in caps.report().lines() {
        out.push_str(&format!("  {ligne}\n"));
    }
    out
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    format!(
        "{}…",
        text.chars().take(max.saturating_sub(1)).collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentd::{Budget, Limits};
    use prophet_types::ledger::{Actor, Draft, GENESIS};
    use serde_json::json;
    use time::OffsetDateTime;

    fn now() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_789_000_000).unwrap()
    }

    fn tache(id: &str, state: State) -> Task {
        let mut t = Task::new(
            id,
            "prépare le rapport de ventes du troisième trimestre",
            "org.test.agent",
            "u",
            Budget::new(Limits::default()),
            now(),
        );
        t.state = state;
        t.budget.spent.steps = 4;
        t.budget.spent.tokens = 1234;
        t
    }

    fn evenement(kind: EventKind, payload: serde_json::Value, step: u32) -> Event {
        Event::seal(
            Draft::new(now(), Actor::daemon("agentd"), kind, payload)
                .task("task:01")
                .step(step),
            0,
            GENESIS,
        )
        .unwrap()
    }

    #[test]
    fn liste_vide() {
        assert_eq!(task_list(&[]), "aucune tâche\n");
    }

    #[test]
    fn liste_avec_etat_lisible() {
        let t = tache("task:01", State::Running);
        let rendu = task_list(&[&t]);
        assert!(rendu.contains("task:01"), "{rendu}");
        assert!(rendu.contains("en cours"), "{rendu}");
        assert!(rendu.contains("1234"), "{rendu}");
    }

    #[test]
    fn les_sous_taches_sont_indentees() {
        let racine = tache("task:01", State::Running);
        let enfant = racine
            .spawn_child("task:02", "sous-tâche", 0.5, now())
            .unwrap();
        let rendu = task_list(&[&racine, &enfant]);
        assert!(rendu.contains("  task:02"), "{rendu}");
    }

    #[test]
    fn intention_longue_tronquee() {
        let mut t = tache("task:01", State::Running);
        t.intent = "x".repeat(200);
        assert!(task_list(&[&t]).contains('…'));
    }

    #[test]
    fn chaque_etat_a_une_marque_et_un_nom() {
        for state in [
            State::Pending,
            State::Planned,
            State::Running,
            State::WaitingApproval,
            State::Paused,
            State::Done,
            State::Failed,
            State::Cancelled,
            State::RolledBack,
        ] {
            assert!(!state_mark(state).is_empty());
            assert!(!state_name(state).is_empty());
        }
    }

    #[test]
    fn timeline_groupee_par_etape() {
        let events = vec![
            evenement(EventKind::ToolCall, json!({"tool": "fs.read"}), 1),
            evenement(
                EventKind::ToolResult,
                json!({"tool": "fs.read", "ok": true}),
                1,
            ),
            evenement(EventKind::ToolCall, json!({"tool": "fs.write"}), 2),
        ];
        let rendu = timeline("task:01", &events);
        assert!(rendu.contains("étape 1"), "{rendu}");
        assert!(rendu.contains("étape 2"), "{rendu}");
        assert!(rendu.contains("appel d'outil"), "{rendu}");
        assert!(rendu.contains("fs.read ok"), "{rendu}");
    }

    #[test]
    fn timeline_d_une_tache_inconnue() {
        assert!(timeline("task:99", &[]).contains("aucun événement"));
    }

    #[test]
    fn le_resume_de_fin_montre_les_chiffres() {
        let events = vec![evenement(
            EventKind::TaskDone,
            json!({"stats": {"steps": 4, "tool_calls": 3, "tokens": 1234}}),
            4,
        )];
        let rendu = timeline("task:01", &events);
        assert!(rendu.contains("4 étapes, 3 outils, 1234 tokens"), "{rendu}");
    }

    #[test]
    fn tous_les_types_d_evenements_ont_un_libelle() {
        for kind in [
            EventKind::TaskCreated,
            EventKind::ToolCall,
            EventKind::PolicyDeny,
            EventKind::NetExfilSuspected,
            EventKind::LedgerSeal,
            EventKind::UiAct,
        ] {
            assert!(!kind_label(kind).is_empty());
        }
    }

    #[test]
    fn centre_d_approbations_vide() {
        assert!(approvals(&[]).contains("aucune approbation"));
    }

    #[test]
    fn une_approbation_dit_ce_qui_va_se_passer() {
        let demande = capd::Approval {
            id: "apr:01".into(),
            task: "task:01".into(),
            agent: "org.test.agent".into(),
            action: "tool.call".into(),
            target: "mail.send".into(),
            summary: "Envoyer le rapport Q3 à marie@exemple.fr".into(),
            reason: None,
            irreversible: true,
            external: true,
            created: now(),
            expires: now() + time::Duration::hours(24),
            state: capd::ApprovalState::Pending,
        };
        let rendu = approvals(&[demande]);
        assert!(rendu.contains("Envoyer le rapport Q3"), "{rendu}");
        assert!(
            rendu.contains("irréversible, hors de la machine"),
            "l'humain doit voir l'enjeu avant la mécanique : {rendu}"
        );
        assert!(rendu.contains("o accorder une fois"), "{rendu}");
    }

    #[test]
    fn le_statut_annonce_les_limites_de_la_machine() {
        let caps = sandboxd::Capabilities::probe();
        let backend = sfs::detect_backend(std::path::Path::new("/tmp"));
        let t = tache("task:01", State::Running);
        let rendu = status(&caps, &backend, &[&t], 2);
        assert!(rendu.contains("1 active"), "{rendu}");
        assert!(rendu.contains("2 en attente"), "{rendu}");
        assert!(rendu.contains("Isolation"), "{rendu}");
        assert!(rendu.contains("Landlock"), "{rendu}");
    }
    #[test]
    fn le_statut_dit_quels_services_repondent_et_ce_que_leur_absence_coute() {
        // Un système dont `capd` est mort affiche une isolation parfaite et un journal intact, et
        // ne peut rien faire. C'est la seule chose que les autres lignes ne disent pas.
        let rendu = status_avec_services(
            &sandboxd::Capabilities::probe(),
            &sfs::Backend {
                kind: sfs::BackendKind::Portable,
                reason: "essai".to_owned(),
            },
            &[],
            0,
            &[
                ("capd".to_owned(), None),
                ("agentd".to_owned(), Some("socket injoignable".to_owned())),
            ],
        );
        assert!(rendu.contains("✓ capd"), "{rendu}");
        assert!(rendu.contains("✗ agentd — socket injoignable"), "{rendu}");
        assert!(
            rendu.contains("1 service(s) muet(s)"),
            "l'absence doit être comptée et sa conséquence dite : {rendu}"
        );
    }

    #[test]
    fn le_statut_dit_si_le_navigateur_pilote_repond_ou_ce_que_son_absence_coute() {
        let backend = sfs::Backend {
            kind: sfs::BackendKind::Portable,
            reason: "essai".to_owned(),
        };
        let caps = sandboxd::Capabilities::probe();
        let pret = agentd::preparation::BrowserState {
            program: "/run/current-system/sw/bin/chromium".to_owned(),
            ready: true,
            detail: "HeadlessChrome/131.0".to_owned(),
        };
        let rendu = status_complet(&caps, &backend, &[], 0, &[], &Navigateur::Sonde(&pret));
        assert!(
            rendu.contains("✓ /run/current-system/sw/bin/chromium — HeadlessChrome/131.0"),
            "{rendu}"
        );
        let panne = agentd::preparation::BrowserState {
            ready: false,
            detail: "lancement : SIGSYS".to_owned(),
            ..pret
        };
        let rendu = status_complet(&caps, &backend, &[], 0, &[], &Navigateur::Sonde(&panne));
        assert!(
            rendu.contains("✗ /run/current-system/sw/bin/chromium — lancement : SIGSYS"),
            "{rendu}"
        );
        let rendu = status_complet(&caps, &backend, &[], 0, &[], &Navigateur::Aucun);
        assert!(rendu.contains("aucun — sans PROPHET_BROWSER"), "{rendu}");
        let rendu = status_complet(&caps, &backend, &[], 0, &[], &Navigateur::Inconnu);
        assert!(!rendu.contains("Navigateur piloté"), "{rendu}");
    }

    #[test]
    fn sans_liste_de_services_le_statut_reste_ce_qu_il_etait() {
        // `status` est appelé ailleurs qu'en ligne de commande ; il ne doit pas se mettre à parler
        // de services quand personne n'a demandé.
        let rendu = status(
            &sandboxd::Capabilities::probe(),
            &sfs::Backend {
                kind: sfs::BackendKind::Portable,
                reason: "essai".to_owned(),
            },
            &[],
            0,
        );
        assert!(!rendu.contains("Services"), "{rendu}");
    }
}
