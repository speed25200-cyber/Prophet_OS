//! Le travail et son plan occupent la surface principale ; les compteurs restent secondaires.
use crate::hud;
use crate::missions::{Action, Missions, Outcome};
use crate::scene::Courant;
use crate::supervision::bouton;
use crate::theme::Accent;
use crate::theme::palette::{ACCOMPLI, ATTENTE, ATTENTE_VOILE, DISCRET, ENCRE, TRAIT, VERRE_HAUT};
use agentd::{Inspection, State, WorkspaceState};
use egui::{Color32, Frame, RichText, Stroke};

const INK: Color32 = ENCRE;
const MUTED: Color32 = DISCRET;
const LINE: Color32 = TRAIT;
const GREEN: Color32 = ACCOMPLI;
const RED: Color32 = ATTENTE;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    Auto,
    Result,
    Plan,
    History,
    Files,
}

pub(crate) fn status(state: State, accent: &Accent) -> (&'static str, Color32) {
    match state {
        State::Pending => ("À préparer", MUTED),
        State::Planned => ("Plan à examiner", accent.sourd),
        State::Running => ("En cours", accent.vif),
        State::WaitingApproval => ("Votre décision", ATTENTE),
        State::Paused => ("En pause", MUTED),
        State::Done => ("Exécution terminée", GREEN),
        State::Failed => ("Échec", RED),
        State::Cancelled => ("Annulée", MUTED),
        State::RolledBack => ("Changements annulés", MUTED),
    }
}

/// L'état de publication SFS, tel que l'humain doit le lire avant de commander.
pub(crate) fn publication(state: WorkspaceState) -> &'static str {
    match state {
        WorkspaceState::Open => "Versions examinables. Vos documents n'ont pas été modifiés.",
        WorkspaceState::Applying => {
            "Publication interrompue : reprenez-la pour terminer l'intention enregistrée."
        }
        WorkspaceState::Undoing => {
            "Annulation interrompue : reprenez-la pour rétablir vos documents."
        }
        WorkspaceState::Conflict => {
            "Publication interrompue sur un conflit. Les fichiers déplacés sont conservés ; une résolution explicite est nécessaire."
        }
        WorkspaceState::Committed => {
            "Versions publiées dans vos documents. Annulable tant qu'ils n'ont pas changé."
        }
        WorkspaceState::RolledBack => "Publication annulée : documents initiaux rétablis.",
        WorkspaceState::Abandoned => "Travail abandonné sans publication.",
    }
}

fn small(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text).size(12.0).color(MUTED));
}
/// Une étiquette en capitales espacées : les rubriques d'une carte.
fn label(ui: &mut egui::Ui, text: &str) {
    crate::supervision::etiquette(ui, text);
}
/// Un titre : en serif d'affichage dès qu'il nomme, en Inter quand il rubrique.
fn heading(ui: &mut egui::Ui, text: &str, size: f32) {
    if size >= 19.0 {
        ui.label(crate::supervision::titre(text, size));
    } else {
        ui.label(
            RichText::new(text)
                .size(size)
                .family(hud::fort())
                .color(INK),
        );
    }
}
fn sheet(ui: &egui::Ui) -> Frame {
    Frame::new()
        .fill(VERRE_HAUT)
        .corner_radius(3)
        .inner_margin(24)
        .stroke(Stroke::new(1.0, Accent::de(ui.ctx()).fil))
}
fn limited(text: &str, limit: usize) -> String {
    let mut value: String = text.chars().take(limit).collect();
    if text.chars().count() > limit {
        value.push('…');
    }
    value
}

pub(crate) fn draw(ui: &mut egui::Ui, c: &Courant, missions: &mut Missions, tab: &mut Tab) {
    ui.spacing_mut().item_spacing.y = 6.0;
    let reviewing = *tab == Tab::Files;
    let accent = Accent::de(ui.ctx());
    if !reviewing {
        ui.horizontal(|ui| {
            label(ui, "ESPACE DE MISSION");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if bouton(ui, "copier-reference", "Copier la référence", false).clicked() {
                    ui.ctx().copy_text(c.tache.clone());
                }
            });
        });
        ui.add_space(6.0);
    }
    heading(
        ui,
        &if reviewing {
            missions
                .files
                .path
                .clone()
                .unwrap_or_else(|| "Les fichiers de la mission".into())
        } else {
            limited(&c.intitule, 350)
        },
        if reviewing {
            22.0
        } else if ui.available_width() < 540.0 {
            24.0
        } else {
            30.0
        },
    );
    ui.add_space(6.0);
    let mut command = None;
    let mut refresh = false;
    let mut file_requested = None;
    if let Some(info) = missions.snapshot() {
        let (label, color) = status(info.task.state, &accent);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(label).color(color).size(13.0));
            // Qui mène la mission, lisible : « Codex (ChatGPT) », « qwen3-1.7b » (ADR 0035).
            small(
                ui,
                format!(
                    "· {}",
                    agentd::preparation::reference_label(
                        info.task.driver.as_deref().unwrap_or(&info.task.agent)
                    )
                ),
            );
            if info.can_cancel
                && ui
                    .add_enabled_ui(!missions.busy(), |ui| {
                        bouton(
                            ui,
                            "mission-cancel",
                            if info.task.state == State::Planned {
                                "Annuler le plan"
                            } else {
                                "Arrêter"
                            },
                            false,
                        )
                    })
                    .inner
                    .clicked()
            {
                command = Some(Action::Cancel);
            }
            if info.can_apply
                && ui
                    .add_enabled_ui(!missions.busy(), |ui| {
                        bouton(
                            ui,
                            "mission-apply",
                            if info.publication == Some(WorkspaceState::Applying) {
                                "Reprendre la publication"
                            } else {
                                "Appliquer à mes documents"
                            },
                            true,
                        )
                    })
                    .inner
                    .clicked()
            {
                command = Some(Action::Apply);
            }
            if info.can_undo
                && ui
                    .add_enabled_ui(!missions.busy(), |ui| {
                        bouton(
                            ui,
                            "mission-undo",
                            if info.publication == Some(WorkspaceState::Undoing) {
                                "Reprendre l'annulation"
                            } else {
                                "Annuler la publication"
                            },
                            false,
                        )
                    })
                    .inner
                    .clicked()
            {
                command = Some(Action::Undo);
            }
        });
        if let Some(state) = info.publication {
            ui.add_space(4.0);
            small(ui, publication(state));
        }
        if let Some(browsing) = &info.browsing
            && let Some(url) = browsing["url"].as_str()
        {
            // Où l'agent navigue, tel que ses outils l'ont déposé : titre et adresse, sans
            // l'arbre. L'humain peut y aller avec son propre navigateur.
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                small(ui, "Sur le web :");
                ui.label(
                    RichText::new(limited(
                        browsing["title"].as_str().unwrap_or("(sans titre)"),
                        80,
                    ))
                    .size(13.0),
                );
                ui.label(RichText::new(limited(url, 100)).size(12.0).color(MUTED));
                if std::env::var_os("BROWSER").is_some()
                    && bouton(ui, "browsing-open", "Ouvrir", false).clicked()
                {
                    open_in_browser(url);
                }
            });
        }
        // Le dernier geste de l'agent, relu dans le journal : ce qu'il fait se voit sans ouvrir
        // le parcours. La cible est celle que capd a contrôlée ; le contenu n'y est jamais.
        if !reviewing && let Some(geste) = missions.trail().last() {
            ui.add_space(4.0);
            let (marque, couleur, note) = issue(&geste.outcome);
            let ligne = ui
                .horizontal_wrapped(|ui| {
                    small(ui, "Dernier geste :");
                    ui.label(RichText::new(marque).color(couleur).size(13.0));
                    ui.label(
                        RichText::new(&geste.tool)
                            .size(13.0)
                            .monospace()
                            .color(accent.sourd),
                    );
                    if let Some(cible) = &geste.target {
                        ui.label(
                            RichText::new(limited(&cible_lisible(cible), 80))
                                .size(12.0)
                                .color(MUTED),
                        );
                    }
                    if !note.is_empty() {
                        ui.label(RichText::new(limited(&note, 80)).size(12.0).color(RED));
                    }
                })
                .response;
            let decrit = format!(
                "Dernier geste : {} {} — {}",
                geste.tool,
                geste.target.as_deref().unwrap_or(""),
                issue_en_mots(&geste.outcome)
            );
            ui.interact(
                ligne.rect,
                egui::Id::new("mission-dernier-geste"),
                egui::Sense::hover(),
            )
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &decrit));
        }
        if !reviewing {
            ui.add_space(6.0);
            phases(ui, info);
        }
        // Les instruments n'ont de sens qu'une fois le travail commencé : un plan à examiner
        // n'a ni étapes ni débit, et sa place revient au bouton qui le lance.
        if !reviewing && info.task.history.contains(&State::Running) {
            ui.add_space(8.0);
            let spent = &info.task.budget.spent;
            let limits = &info.task.budget.limits;
            let fraction = if limits.tokens > 0 {
                spent.tokens as f32 / limits.tokens as f32
            } else {
                0.0
            };
            let rate = if spent.wall_time_s > 0 {
                spent.steps as f32 * 60.0 / spent.wall_time_s as f32
            } else {
                0.0
            };
            crate::instruments::tableau(
                ui,
                spent.steps,
                rate,
                fraction,
                if info.task.state == State::Done {
                    GREEN
                } else {
                    accent.vif
                },
                true,
            );
        }
        ui.add_space(10.0);
        let current = if *tab == Tab::Auto {
            if info.task.state == State::Planned {
                Tab::Plan
            } else {
                Tab::Result
            }
        } else {
            *tab
        };
        ui.horizontal_wrapped(|ui| {
            for (value, id, label) in [
                (Tab::Result, "mission-result-tab", "Proposition"),
                (Tab::Plan, "mission-plan-tab", "Plan & accès"),
                (Tab::History, "mission-history-tab", "Parcours"),
                (Tab::Files, "mission-files-tab", "Fichiers"),
            ] {
                if bouton(ui, id, label, current == value).clicked() {
                    *tab = value;
                }
            }
        });
        ui.add_space(10.0);
        sheet(ui)
            .inner_margin(if reviewing { 16 } else { 20 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                match current {
                    Tab::Plan => plan(ui, info, missions.busy(), &mut command),
                    Tab::History => history(ui, info, missions.trail()),
                    Tab::Files => {
                        file_requested = crate::file_review_view::draw(ui, info, &missions.files)
                    }
                    _ => result(ui, info, &mut file_requested),
                }
            });
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            small(ui, format!("{} étapes", info.task.budget.spent.steps));
            small(
                ui,
                format!("· {} tokens comptés", info.task.budget.spent.tokens),
            );
            // Le relais de modèles, en une mention : quelle part du travail n'a pas coûté un
            // tour du modèle de la mission (ADR 0034).
            if let Some(share) = agentd::budget::share_outside(
                &info.task.usage,
                info.task.driver.as_deref().unwrap_or_default(),
            ) && info.task.usage.len() > 1
            {
                small(ui, format!("· {share} % confiés à d'autres modèles"));
            }
            small(
                ui,
                format!("· {} s observées", info.task.budget.spent.wall_time_s),
            );
        });
    } else {
        ui.add_space(20.0);
        sheet(ui).show(ui, |ui| {
            heading(
                ui,
                if missions.error().is_some() {
                    "Détail indisponible"
                } else {
                    "Lecture de la mission"
                },
                22.0,
            );
            small(
                ui,
                missions
                    .error()
                    .unwrap_or("Le plan et le résultat sont demandés au service."),
            );
            if bouton(ui, "mission-refresh", "Actualiser", false).clicked() {
                refresh = true;
            }
        });
    }
    if missions.busy() {
        ui.add_space(10.0);
        small(ui, "Commande envoyée · confirmation en cours");
    }
    if let Some(notice) = missions.notice() {
        ui.add_space(10.0);
        ui.label(
            RichText::new(&notice.text)
                .size(13.0)
                .color(if notice.error { RED } else { MUTED }),
        );
    }
    if refresh {
        missions.refresh();
    }
    if let Some(path) = file_requested {
        *tab = Tab::Files;
        if let Err(error) = missions.open_file(&path) {
            ui.label(RichText::new(error).color(RED));
        }
    }
    if let Some(action) = command
        && let Err(error) = missions.command(action)
    {
        ui.label(RichText::new(error).color(RED));
    }
}

fn phases(ui: &mut egui::Ui, info: &Inspection) {
    let accent = Accent::de(ui.ctx());
    let reached = [
        info.plan.is_some(),
        info.task.history.contains(&State::Running),
        info.task.state == State::Done && info.result.is_some(),
    ];
    ui.horizontal_wrapped(|ui| {
        for (index, label) in ["Plan proposé", "Exécution", "Proposition reçue"]
            .into_iter()
            .enumerate()
        {
            if index > 0 {
                ui.label(RichText::new("—").color(LINE));
            }
            let color = if reached[index] { accent.vif } else { MUTED };
            Frame::new()
                .fill(if reached[index] {
                    hud::voile(accent.vif, 36)
                } else {
                    VERRE_HAUT
                })
                .stroke(Stroke::new(
                    1.0,
                    if reached[index] { accent.fil_vif } else { LINE },
                ))
                .corner_radius(3)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("{}  {label}", index + 1))
                            .size(11.0)
                            .color(color),
                    );
                });
        }
    });
}

fn plan(ui: &mut egui::Ui, info: &Inspection, busy: bool, command: &mut Option<Action>) {
    heading(ui, "Le cadre de cette mission", 23.0);
    ui.add_space(8.0);
    let Some(plan) = &info.plan else {
        small(
            ui,
            "Le service n'a pas conservé de plan pour cette ancienne mission.",
        );
        return;
    };
    let scopes = |ui: &mut egui::Ui| {
        label(ui, "PÉRIMÈTRE");
        if plan.scopes.is_empty() {
            ui.label("Aucun accès fichier.");
        }
        for scope in &plan.scopes {
            ui.label(limited(scope, 1000));
        }
    };
    let grants = |ui: &mut egui::Ui| {
        label(ui, "ACCÈS ACCORDÉS");
        for grant in &plan.grants {
            ui.label(RichText::new(limited(grant, 1000)).size(13.0));
        }
    };
    if ui.available_width() > 540.0 {
        ui.columns(2, |columns| {
            scopes(&mut columns[0]);
            grants(&mut columns[1]);
        });
    } else {
        scopes(ui);
        ui.add_space(10.0);
        grants(ui);
    }
    ui.add_space(14.0);
    ui.separator();
    ui.add_space(12.0);
    small(
        ui,
        format!(
            "Limites : {} tokens · {} s · {} étapes",
            plan.limits.tokens, plan.limits.wall_time_s, plan.limits.steps
        ),
    );
    small(
        ui,
        format!(
            "Pilote : {} · niveau demandé {}",
            plan.choice.reference, plan.sandbox_level
        ),
    );
    ui.add_space(12.0);
    ui.label(RichText::new("Les modifications seront préparées dans le travail de la mission. Leur application à vos documents reste distincte.").size(13.0).color(MUTED));
    if info.can_start {
        ui.add_space(12.0);
        if ui
            .add_enabled_ui(!busy, |ui| {
                bouton(ui, "mission-start", "Exécuter ce plan", true)
            })
            .inner
            .clicked()
        {
            *command = Some(Action::Start);
        }
    } else if info.task.state == State::Planned
        && let Some(reason) = &info.start_reason
    {
        ui.add_space(12.0);
        ui.label(RichText::new(reason).color(RED));
    }
}

fn result(ui: &mut egui::Ui, info: &Inspection, file_requested: &mut Option<String>) {
    let state = info.task.state;
    if let Some(reason) = &info.task.reason {
        Frame::new()
            .fill(if state == State::Failed {
                ATTENTE_VOILE
            } else {
                VERRE_HAUT
            })
            .stroke(Stroke::new(
                1.0,
                if state == State::Failed {
                    hud::voile(ATTENTE, 150)
                } else {
                    LINE
                },
            ))
            .corner_radius(3)
            .inner_margin(14)
            .show(ui, |ui| {
                heading(ui, status(state, &Accent::de(ui.ctx())).0, 19.0);
                ui.label(
                    RichText::new(limited(reason, 4000))
                        .size(13.0)
                        .color(if state == State::Failed { RED } else { MUTED }),
                );
            });
        ui.add_space(18.0);
    }
    let Some(value) = &info.result else {
        heading(
            ui,
            if state == State::Running {
                "L'agent prépare sa proposition"
            } else {
                "Aucune proposition reçue"
            },
            24.0,
        );
        ui.add_space(10.0);
        small(
            ui,
            if state == State::Planned {
                "Examinez le plan et ses accès pour lancer cette mission."
            } else {
                "Les fichiers et la réponse finale apparaîtront ici lorsqu'ils seront disponibles."
            },
        );
        return;
    };
    ui.horizontal(|ui| {
        heading(ui, "Proposition de l'agent", 22.0);
        if let Some(text) = value["text"].as_str()
            && bouton(ui, "mission-copy-result", "Copier", false).clicked()
        {
            ui.ctx().copy_text(text.into());
        }
    });
    ui.add_space(12.0);
    if let Some(text) = value["text"].as_str() {
        ui.add(
            egui::Label::new(
                RichText::new(limited(text, 16000))
                    .size(15.0)
                    .line_height(Some(23.0)),
            )
            .selectable(true)
            .wrap(),
        );
        if text.chars().count() > 16000 {
            small(ui, "Aperçu abrégé. Copier conserve la réponse complète.");
        }
    } else {
        small(ui, "Aucun texte final conservé pour cette mission.");
    }
    ui.add_space(14.0);
    ui.separator();
    ui.add_space(12.0);
    heading(ui, "Changements préparés", 19.0);
    ui.add_space(4.0);
    if let Some(changes) = value["diff"]["changes"].as_array() {
        if changes.is_empty() {
            small(ui, "Le résultat ne contient aucun changement de fichier.");
        }
        for change in changes.iter().take(200) {
            let (symbol, color, label) = match change["kind"].as_str() {
                Some("added") => ("+", GREEN, "Ajout"),
                Some("modified") => ("~", Accent::de(ui.ctx()).vif, "Modification"),
                Some("deleted") => ("−", RED, "Suppression"),
                _ => ("?", MUTED, "Type inconnu"),
            };
            Frame::new()
                .inner_margin(egui::Margin::symmetric(0, 9))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(symbol).color(color).size(18.0));
                        ui.label(
                            RichText::new(limited(
                                change["path"].as_str().unwrap_or("Chemin absent"),
                                600,
                            ))
                            .size(13.0),
                        );
                        small(
                            ui,
                            format!(
                                "{label} · {} → {} o",
                                change["size_before"].as_u64().unwrap_or(0),
                                change["size_after"].as_u64().unwrap_or(0)
                            ),
                        );
                        if info.task.state == State::Done
                            && let Some(path) = change["path"].as_str()
                            && bouton(ui, &format!("mission-file-{path}"), "Examiner", false)
                                .clicked()
                        {
                            *file_requested = Some(path.into());
                        }
                    });
                });
            ui.separator();
        }
        if changes.len() > 200 {
            small(
                ui,
                format!(
                    "200 changements affichés sur {}. Le résultat complet reste accessible par la CLI.",
                    changes.len()
                ),
            );
        }
        ui.add_space(12.0);
        small(
            ui,
            "Versions enregistrées à la fin de la mission. Examinez chaque proposition avant de modifier vos originaux.",
        );
    } else {
        small(
            ui,
            "Aucun diff conservé. Cela ne prouve pas l'absence de fichiers préparés avant l'arrêt.",
        );
    }
    ui.add_space(10.0);
    small(
        ui,
        "La fin d'exécution ne constitue pas une vérification de l'objectif.",
    );
}

/// Ouvre une adresse avec la commande que la session a donnée (`BROWSER`), sans rien
/// attendre d'elle : la surface ne lit ni la sortie ni l'issue du navigateur humain.
fn open_in_browser(url: &str) {
    let Some(browser) = std::env::var_os("BROWSER") else {
        return;
    };
    let mut parts = browser
        .to_string_lossy()
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return;
    }
    let program = parts.remove(0);
    let _ = std::process::Command::new(program)
        .args(parts)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn history(ui: &mut egui::Ui, info: &Inspection, trail: &[crate::missions::TrailEntry]) {
    let accent = Accent::de(ui.ctx());
    heading(ui, "Parcours observé", 23.0);
    ui.add_space(14.0);
    for (n, state) in info.task.history.iter().enumerate() {
        let (label, color) = status(*state, &accent);
        ui.horizontal(|ui| {
            small(ui, format!("{:02}", n + 1));
            ui.label(RichText::new(label).color(color));
        });
        ui.add_space(6.0);
    }
    ui.add_space(16.0);
    heading(ui, "Ce que l'agent a touché", 16.0);
    ui.add_space(8.0);
    if trail.is_empty() {
        small(
            ui,
            "Aucun appel d'outil journalisé pour cette mission. Le journal ne contient jamais le contenu lu ou écrit.",
        );
    }
    for entry in trail.iter().take(200) {
        let (mark, color, note) = issue(&entry.outcome);
        ui.horizontal_wrapped(|ui| {
            small(
                ui,
                entry
                    .step
                    .map_or_else(|| "  ".to_owned(), |s| format!("{s:02}")),
            );
            ui.label(RichText::new(mark).color(color).size(13.0));
            ui.label(
                RichText::new(&entry.tool)
                    .size(13.0)
                    .monospace()
                    .color(accent.sourd),
            );
            if let Some(target) = &entry.target {
                ui.label(
                    RichText::new(limited(&cible_lisible(target), 120))
                        .size(13.0)
                        .color(MUTED),
                );
                // Un hôte visité par l'agent s'ouvre dans le navigateur de l'humain, par la
                // commande que la session lui a donnée ; jamais par une adresse venue du modèle
                // au-delà de l'hôte contrôlé par capd.
                if matches!(entry.tool.as_str(), "web.open" | "http.fetch")
                    && std::env::var_os("BROWSER").is_some()
                    && bouton(ui, &format!("trail-open-{}", entry.seq), "Ouvrir", false).clicked()
                {
                    open_in_browser(&format!("https://{target}/"));
                }
            }
            if !note.is_empty() {
                ui.label(RichText::new(note).size(12.0).color(RED));
            }
        });
    }
    if trail.len() > 200 {
        small(
            ui,
            format!(
                "200 appels affichés sur {}. Le journal complet se lit avec `prophet log tail`.",
                trail.len()
            ),
        );
    }
    ui.add_space(12.0);
    small(
        ui,
        "États conservés par le service ; appels relus dans le journal, avec leur cible contrôlée et leur issue.",
    );
}

/// La marque, la couleur et la note d'une issue d'appel, pour le parcours et le dernier geste.
fn issue(outcome: &Outcome) -> (&'static str, Color32, String) {
    match outcome {
        Outcome::Pending => ("·", MUTED, String::new()),
        Outcome::Ok => ("✓", GREEN, String::new()),
        Outcome::Error(code) => ("✕", RED, code.clone()),
        Outcome::Denied(reason) => ("⊘", RED, reason.clone()),
    }
}

/// Une cible lisible : un chemin du dossier de l'humain s'écrit à partir de `~`, comme il
/// l'écrirait lui-même ; toute autre cible reste telle que capd l'a contrôlée.
fn cible_lisible(cible: &str) -> String {
    cible_depuis(cible, &std::env::var("HOME").unwrap_or_default())
}

fn cible_depuis(cible: &str, home: &str) -> String {
    let home = home.trim_end_matches('/');
    match cible.strip_prefix(home) {
        Some(reste) if !home.is_empty() && reste.starts_with('/') => format!("~{reste}"),
        _ => cible.to_owned(),
    }
}

/// Une issue d'appel en mots, pour l'accessibilité.
fn issue_en_mots(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Pending => "en cours".to_owned(),
        Outcome::Ok => "réussi".to_owned(),
        Outcome::Error(code) => format!("en erreur ({code})"),
        Outcome::Denied(reason) => format!("refusé ({reason})"),
    }
}

/// Les sous-missions d'une mission : leurs identifiants sont les siens suivis d'un rang
/// (`m.1`, `m.2`, et `m.1.1` pour une petite-fille).
pub(crate) fn enfants_de<'a>(id: &str, courants: &'a [Courant]) -> Vec<&'a Courant> {
    let prefixe = format!("{id}.");
    courants
        .iter()
        .filter(|k| k.tache.starts_with(&prefixe))
        .collect()
}

/// Le relais vu par l'humain : à qui cette mission a confié quoi, et où chacun en est — Claude
/// Code qui relit, Codex qui code, le modèle local qui exécute (ADR 0034, 0035, 0039).
pub(crate) fn confiees(ui: &mut egui::Ui, c: &Courant, courants: &[Courant]) {
    let enfants = enfants_de(&c.tache, courants);
    if enfants.is_empty() {
        return;
    }
    let accent = Accent::de(ui.ctx());
    ui.add_space(12.0);
    sheet(ui).show(ui, |ui| {
        heading(ui, "Confiées", 18.0);
        small(
            ui,
            "Chacune part de l'espace de cette mission et y rapporte son travail.",
        );
        for enfant in enfants {
            ui.horizontal_wrapped(|ui| {
                let (etat, couleur) = enfant
                    .task_state
                    .map_or(("en cours", DISCRET), |s| status(s, &accent));
                ui.label(RichText::new(etat).color(couleur).size(12.0));
                small(
                    ui,
                    format!(
                        "· {} · {}",
                        agentd::preparation::reference_label(&enfant.agent),
                        limited(&enfant.intitule, 90)
                    ),
                );
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Etat;

    #[test]
    fn une_cible_du_dossier_de_l_humain_se_lit_depuis_le_tilde() {
        assert_eq!(
            cible_depuis("/home/pilot/Documents/Prophet/note.txt", "/home/pilot"),
            "~/Documents/Prophet/note.txt"
        );
        assert_eq!(
            cible_depuis("/home/pilot/Documents/x", "/home/pilot/"),
            "~/Documents/x"
        );
        // Un voisin qui partage le préfixe n'est pas le dossier de l'humain.
        assert_eq!(
            cible_depuis("/home/pilote/secret", "/home/pilot"),
            "/home/pilote/secret"
        );
        assert_eq!(cible_depuis("exemple.fr", "/home/pilot"), "exemple.fr");
        assert_eq!(cible_depuis("/etc/passwd", ""), "/etc/passwd");
    }

    fn courant(id: &str, agent: &str) -> Courant {
        Courant {
            tache: id.to_owned(),
            intitule: format!("mission {id}"),
            agent: agent.to_owned(),
            etat: Etat::Court,
            debit: 0.0,
            budget_consomme: 0.0,
            etapes: 0,
            task_state: None,
            task_revision: 0,
        }
    }

    #[test]
    fn les_sous_missions_sont_celles_dont_l_identifiant_prolonge_le_sien() {
        let courants = vec![
            courant("duo", "driver:codex"),
            courant("duo.1", "driver:claude-code"),
            courant("duo.1.1", "local:qwen3-1.7b"),
            courant("duo-bis", "driver:codex"),
            courant("autre.1", "driver:codex"),
        ];
        let enfants: Vec<&str> = enfants_de("duo", &courants)
            .iter()
            .map(|c| c.tache.as_str())
            .collect();
        assert_eq!(enfants, ["duo.1", "duo.1.1"]);
        assert!(enfants_de("autre", &courants).len() == 1);
        assert!(enfants_de("duo-bis", &courants).is_empty());
    }
}
