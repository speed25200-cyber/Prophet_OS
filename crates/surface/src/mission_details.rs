//! Le travail et son plan occupent la surface principale ; les compteurs restent secondaires.
use crate::missions::{Action, Missions};
use crate::scene::Courant;
use crate::supervision::bouton;
use agentd::{Inspection, State, WorkspaceState};
use egui::{Color32, Frame, RichText, Stroke};

const INK: Color32 = Color32::from_rgb(28, 35, 44);
const MUTED: Color32 = Color32::from_rgb(103, 113, 127);
const LINE: Color32 = Color32::from_rgb(226, 231, 237);
const BLUE: Color32 = Color32::from_rgb(47, 91, 169);
const GREEN: Color32 = Color32::from_rgb(38, 112, 92);
const RED: Color32 = Color32::from_rgb(168, 64, 57);

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    Auto,
    Result,
    Plan,
    History,
    Files,
}

pub(crate) fn status(state: State) -> (&'static str, Color32) {
    match state {
        State::Pending => ("À préparer", MUTED),
        State::Planned => ("Plan à examiner", BLUE),
        State::Running => ("En cours", GREEN),
        State::WaitingApproval => ("Votre décision", Color32::from_rgb(160, 102, 35)),
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
fn heading(ui: &mut egui::Ui, text: &str, size: f32) {
    ui.label(
        RichText::new(text)
            .size(size)
            .family(egui::FontFamily::Name("Inter600".into()))
            .color(INK),
    );
}
fn sheet() -> Frame {
    Frame::new()
        .fill(Color32::from_rgb(247, 249, 249))
        .corner_radius(16)
        .inner_margin(24)
        .stroke(Stroke::new(1.0, LINE))
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
    if !reviewing {
        ui.horizontal(|ui| {
            small(ui, "ESPACE DE MISSION");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if bouton(ui, "copier-reference", "Copier la référence", false).clicked() {
                    ui.ctx().copy_text(c.tache.clone());
                }
            });
        });
        ui.add_space(10.0);
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
            32.0
        },
    );
    ui.add_space(8.0);
    let mut command = None;
    let mut refresh = false;
    let mut file_requested = None;
    if let Some(info) = missions.snapshot() {
        let (label, color) = status(info.task.state);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(label).color(color).size(13.0));
            small(
                ui,
                format!(
                    "· {}",
                    info.task.driver.as_deref().unwrap_or(&info.task.agent)
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
        if !reviewing {
            ui.add_space(10.0);
            phases(ui, info);
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
        ui.add_space(14.0);
        sheet()
            .inner_margin(if reviewing { 16 } else { 24 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                match current {
                    Tab::Plan => plan(ui, info, missions.busy(), &mut command),
                    Tab::History => history(ui, info),
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
            small(
                ui,
                format!("· {} s observées", info.task.budget.spent.wall_time_s),
            );
        });
    } else {
        ui.add_space(20.0);
        sheet().show(ui, |ui| {
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
            let color = if reached[index] { BLUE } else { MUTED };
            Frame::new()
                .fill(if reached[index] {
                    Color32::from_rgb(230, 237, 248)
                } else {
                    Color32::from_rgb(235, 238, 242)
                })
                .corner_radius(8)
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
        small(ui, "PÉRIMÈTRE");
        if plan.scopes.is_empty() {
            ui.label("Aucun accès fichier.");
        }
        for scope in &plan.scopes {
            ui.label(limited(scope, 1000));
        }
    };
    let grants = |ui: &mut egui::Ui| {
        small(ui, "ACCÈS ACCORDÉS");
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
                Color32::from_rgb(251, 239, 236)
            } else {
                Color32::from_rgb(239, 242, 246)
            })
            .corner_radius(10)
            .inner_margin(14)
            .show(ui, |ui| {
                heading(ui, status(state).0, 19.0);
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
        heading(ui, "Proposition de l'agent", 23.0);
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
    ui.add_space(22.0);
    ui.separator();
    ui.add_space(18.0);
    heading(ui, "Changements préparés", 19.0);
    ui.add_space(8.0);
    if let Some(changes) = value["diff"]["changes"].as_array() {
        if changes.is_empty() {
            small(ui, "Le résultat ne contient aucun changement de fichier.");
        }
        for change in changes.iter().take(200) {
            let (symbol, color, label) = match change["kind"].as_str() {
                Some("added") => ("+", GREEN, "Ajout"),
                Some("modified") => ("~", BLUE, "Modification"),
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

fn history(ui: &mut egui::Ui, info: &Inspection) {
    heading(ui, "Parcours observé", 23.0);
    ui.add_space(14.0);
    for (n, state) in info.task.history.iter().enumerate() {
        let (label, color) = status(*state);
        ui.horizontal(|ui| {
            small(ui, format!("{:02}", n + 1));
            ui.label(RichText::new(label).color(color));
        });
        ui.add_space(6.0);
    }
    ui.add_space(12.0);
    small(
        ui,
        "États conservés par le service. Le journal détaillé des actions n'est pas encore affiché ici.",
    );
}
