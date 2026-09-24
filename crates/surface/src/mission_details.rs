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

/// Dessine l'espace d'une mission ; rend la relance demandée, que la supervision confie à la
/// préparation.
pub(crate) fn draw(
    ui: &mut egui::Ui,
    c: &Courant,
    missions: &mut Missions,
    tab: &mut Tab,
) -> Option<Relance> {
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
    let mut relaunch = None;
    if let Some(info) = missions.snapshot() {
        let (label, color) = status(info.task.state, &accent);
        hud::rangee(ui, |ui| {
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
            // Échouée ou arrêtée : la même intention repasse par la préparation, que l'humain
            // relit et confirme ; rien ne part d'ici.
            if let Some(candidate) = relance(
                info.task.state,
                info.task.parent.is_some(),
                &info.task.intent,
                info.plan
                    .as_ref()
                    .map(|p| p.choice.reference.as_str())
                    .or(info.task.driver.as_deref()),
                info.plan.as_ref().and_then(|p| p.profile.as_deref()),
            ) && bouton(ui, "mission-relancer", "Relancer", true).clicked()
            {
                relaunch = Some(candidate);
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
            hud::rangee(ui, |ui| {
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
        if !reviewing && let Some(geste) = dernier_geste(missions.trail()) {
            ui.add_space(4.0);
            let (marque, couleur, note) = issue(&geste.outcome);
            let (outil, couleur_outil) = libelle(geste, &accent);
            let appel = matches!(nature(geste), Nature::Appel | Nature::Sortie);
            let ligne = ui
                .horizontal_wrapped(|ui| {
                    small(ui, "Dernier geste :");
                    if appel {
                        ui.label(RichText::new(marque).color(couleur).size(13.0));
                    }
                    ui.label(
                        RichText::new(&outil)
                            .size(13.0)
                            .monospace()
                            .color(couleur_outil),
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
                outil,
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
                    _ => result(ui, info, missions.trail(), &mut file_requested),
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
                format!("· {}", duree_observee(info.task.budget.spent.wall_time_s)),
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
    relaunch
}

/// Ce qu'il faut pour relancer une mission : son intention, le modèle qui la menait et le profil
/// du catalogue dont elle a été préparée, s'il est connu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Relance {
    pub intent: String,
    pub model: String,
    pub profile: Option<String>,
}

/// Une mission se relance quand elle a échoué ou été arrêtée ; une sous-mission, par son parent.
fn relance(
    state: State,
    child: bool,
    intent: &str,
    reference: Option<&str>,
    profile: Option<&str>,
) -> Option<Relance> {
    if child || !matches!(state, State::Failed | State::Cancelled) || intent.trim().is_empty() {
        return None;
    }
    let model = reference
        .map(|r| {
            r.strip_prefix("local:")
                .or_else(|| r.strip_prefix("driver:"))
                .unwrap_or(r)
        })
        .unwrap_or_default();
    Some(Relance {
        intent: intent.to_owned(),
        model: model.to_owned(),
        profile: profile.map(str::to_owned),
    })
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

fn result(
    ui: &mut egui::Ui,
    info: &Inspection,
    trail: &[crate::missions::TrailEntry],
    file_requested: &mut Option<String>,
) {
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
        // Pendant l'exécution, les derniers gestes de l'agent se suivent ici même, relus dans
        // le journal : on voit qu'il travaille, et sur quoi, sans ouvrir le parcours.
        if state == State::Running && !trail.is_empty() {
            ui.add_space(18.0);
            let accent = Accent::de(ui.ctx());
            touches(
                ui,
                &trail[trail.len().saturating_sub(DIRECT)..],
                &accent,
                "En direct — ses derniers gestes",
            );
        }
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
                    hud::rangee(ui, |ui| {
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

/// Largeur de la gouttière d'une frise : le numéro d'étape, puis le rail et son nœud.
const GOUTTIERE: f32 = 58.0;
/// Abscisse du rail dans la gouttière.
const RAIL: f32 = 44.0;

/// Ce qu'une ligne de la frise est : un appel de l'agent, ou un geste du service autour de lui.
enum Nature {
    Appel,
    Refus,
    Rappel,
    Publication,
    /// Une requête sortie par egress (ADR 0056).
    Sortie,
}

/// Le libellé d'un geste et sa couleur, les mêmes dans la frise et dans le dernier geste.
fn libelle(entry: &crate::missions::TrailEntry, accent: &Accent) -> (String, Color32) {
    match nature(entry) {
        Nature::Refus => ("refusé par capd".to_owned(), RED),
        Nature::Rappel => ("livrable rappelé".to_owned(), ATTENTE_DOUCE),
        Nature::Publication => (entry.tool.clone(), GREEN),
        Nature::Appel => (entry.tool.clone(), accent.sourd),
        Nature::Sortie => ("sortie réseau".to_owned(), accent.vif),
    }
}

fn nature(entry: &crate::missions::TrailEntry) -> Nature {
    match entry.tool.as_str() {
        "refus" => Nature::Refus,
        "rappel" => Nature::Rappel,
        "publication" | "annulation" => Nature::Publication,
        "sortie" => Nature::Sortie,
        _ => Nature::Appel,
    }
}

/// Le parcours d'une mission, en deux frises : les états que le service a tenus, puis ce que
/// l'agent a touché — chaque appel avec sa cible contrôlée et son issue, les refus de capd et
/// les rappels du service à leur place. Rien de ce que l'agent a lu ou écrit n'y figure.
fn history(ui: &mut egui::Ui, info: &Inspection, trail: &[crate::missions::TrailEntry]) {
    let accent = Accent::de(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        heading(ui, "Parcours observé", 23.0);
        ui.add_space(12.0);
        let appels = trail
            .iter()
            .filter(|e| matches!(nature(e), Nature::Appel))
            .count();
        let refus = trail
            .iter()
            .filter(|e| {
                matches!(nature(e), Nature::Refus)
                    || matches!(&e.outcome, Outcome::Denied(_))
                    || matches!(&e.outcome, Outcome::Error(code) if code == "PolicyDenied")
            })
            .map(|e| e.fois as usize)
            .sum::<usize>();
        let rappels = trail
            .iter()
            .filter(|e| matches!(nature(e), Nature::Rappel))
            .count();
        // Chaque sortie compte, même réunie avec d'autres sur une ligne.
        let sorties = trail
            .iter()
            .filter(|e| matches!(nature(e), Nature::Sortie))
            .map(|e| e.fois as usize)
            .sum::<usize>();
        pastille(
            ui,
            &format!("{appels} appel{}", pluriel(appels)),
            accent.sourd,
        );
        if refus > 0 {
            pastille(ui, &format!("{refus} refus"), RED);
        }
        if rappels > 0 {
            pastille(
                ui,
                &format!("{rappels} rappel{}", pluriel(rappels)),
                ATTENTE_DOUCE,
            );
        }
        if sorties > 0 {
            pastille(
                ui,
                &format!("{sorties} sortie{} réseau", pluriel(sorties)),
                accent.vif,
            );
        }
    });
    ui.add_space(14.0);
    let large = ui.available_width() >= 880.0;
    if large {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(250.0);
                etats(ui, info, &accent);
            });
            ui.add_space(28.0);
            ui.vertical(|ui| touches(ui, trail, &accent, "Ce que l'agent a touché"));
        });
    } else {
        etats(ui, info, &accent);
        ui.add_space(16.0);
        touches(ui, trail, &accent, "Ce que l'agent a touché");
    }
    ui.add_space(12.0);
    small(
        ui,
        "États conservés par le service ; appels et sorties réseau relus dans le journal, avec leur cible contrôlée et leur issue.",
    );
}

fn pluriel(n: usize) -> &'static str {
    if n > 1 { "s" } else { "" }
}

/// L'orange adouci d'un rappel : le service intervient, rien n'est en faute.
const ATTENTE_DOUCE: Color32 = Color32::from_rgb(242, 176, 92);

/// Une pastille de compte, sobre : un fil et un voile de la couleur.
fn pastille(ui: &mut egui::Ui, text: &str, color: Color32) {
    Frame::new()
        .fill(hud::voile(color, 22))
        .stroke(Stroke::new(1.0, hud::voile(color, 110)))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(9, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.5).color(color));
        });
}

/// Le rail et le nœud d'une ligne de frise, peints dans la gouttière réservée à sa gauche.
///
/// Le nœud se tient à la hauteur `y` : celle de la première ligne, quand le texte s'enroule.
#[allow(clippy::too_many_arguments)]
fn noeud(
    ui: &egui::Ui,
    rect: egui::Rect,
    y: f32,
    first: bool,
    last: bool,
    fill: Option<Color32>,
    stroke: Color32,
    losange: bool,
) {
    let painter = ui.painter();
    let x = rect.left() + RAIL;
    let rail = Stroke::new(1.0, LINE);
    if !first {
        painter.line_segment(
            [egui::pos2(x, rect.top() - 4.0), egui::pos2(x, y - 7.0)],
            rail,
        );
    }
    if !last {
        painter.line_segment(
            [egui::pos2(x, y + 7.0), egui::pos2(x, rect.bottom() + 4.0)],
            rail,
        );
    }
    if losange {
        let r = 5.5;
        let points = vec![
            egui::pos2(x, y - r),
            egui::pos2(x + r, y),
            egui::pos2(x, y + r),
            egui::pos2(x - r, y),
        ];
        painter.add(egui::Shape::convex_polygon(
            points,
            fill.unwrap_or(Color32::TRANSPARENT),
            Stroke::new(1.2, stroke),
        ));
    } else {
        if let Some(fill) = fill {
            painter.circle_filled(egui::pos2(x, y), 9.0, hud::voile(fill, 28));
            painter.circle_filled(egui::pos2(x, y), 4.5, fill);
        }
        painter.circle_stroke(egui::pos2(x, y), 4.5, Stroke::new(1.2, stroke));
    }
}

/// Les états que le service a tenus, du premier au dernier ; le dernier luit.
fn etats(ui: &mut egui::Ui, info: &Inspection, accent: &Accent) {
    label(ui, "États de la mission");
    ui.add_space(8.0);
    let count = info.task.history.len();
    for (n, state) in info.task.history.iter().enumerate() {
        let (text, color) = status(*state, accent);
        let last = n + 1 == count;
        let row = ui
            .horizontal(|ui| {
                ui.set_min_height(30.0);
                ui.add_space(GOUTTIERE);
                ui.label(
                    RichText::new(text)
                        .size(if last { 15.0 } else { 13.5 })
                        .color(if last { color } else { hud::voile(color, 200) }),
                );
            })
            .response;
        ui.painter().text(
            egui::pos2(row.rect.left(), row.rect.center().y),
            egui::Align2::LEFT_CENTER,
            format!("{:02}", n + 1),
            egui::FontId::monospace(11.0),
            MUTED,
        );
        noeud(
            ui,
            row.rect,
            row.rect.center().y,
            n == 0,
            last,
            last.then_some(color),
            color,
            false,
        );
    }
}

/// Le geste que l'en-tête montre : le dernier du parcours, sauf une sortie réseau relayée sans
/// histoire — un client officiel parle sans cesse à son éditeur, et ce n'est pas un geste. Un
/// refus ou une erreur réseau, eux, se montrent.
fn dernier_geste(trail: &[crate::missions::TrailEntry]) -> Option<&crate::missions::TrailEntry> {
    trail
        .iter()
        .rev()
        .find(|g| !(matches!(nature(g), Nature::Sortie) && g.outcome == Outcome::Ok))
}

/// Gestes montrés en direct pendant l'exécution.
const DIRECT: usize = 5;

/// Ce que l'agent a touché : chaque appel, chaque refus, chaque rappel, dans l'ordre du journal.
fn touches(ui: &mut egui::Ui, trail: &[crate::missions::TrailEntry], accent: &Accent, titre: &str) {
    label(ui, titre);
    ui.add_space(8.0);
    if trail.is_empty() {
        small(
            ui,
            "Aucun appel d'outil journalisé pour cette mission. Le journal ne contient jamais le contenu lu ou écrit.",
        );
        return;
    }
    let shown = trail.len().min(200);
    for (index, entry) in trail.iter().take(200).enumerate() {
        let (mark, color, note) = issue(&entry.outcome);
        let kind = nature(entry);
        let (outil, couleur_outil) = libelle(entry, accent);
        // La gouttière d'abord, puis le texte, qui s'enroule dans sa propre colonne : une ligne
        // suivante repart sous le nom de l'outil, jamais sous le rail.
        let ligne = ui.horizontal(|ui| {
            ui.add_space(GOUTTIERE);
            ui.horizontal_wrapped(|ui| {
                ui.set_min_height(30.0);
                let tete = ui
                    .label(
                        RichText::new(&outil)
                            .size(13.0)
                            .monospace()
                            .color(couleur_outil),
                    )
                    .rect
                    .center()
                    .y;
                if let Some(target) = &entry.target {
                    ui.label(
                        RichText::new(limited(&cible_lisible(target), 120))
                            .size(13.0)
                            .color(INK),
                    );
                    // Un hôte visité par l'agent s'ouvre dans le navigateur de l'humain, par la
                    // commande que la session lui a donnée ; jamais par une adresse venue du
                    // modèle au-delà de l'hôte contrôlé par capd.
                    if matches!(entry.tool.as_str(), "web.open" | "http.fetch")
                        && std::env::var_os("BROWSER").is_some()
                        && bouton(ui, &format!("trail-open-{}", entry.seq), "Ouvrir", false)
                            .clicked()
                    {
                        open_in_browser(&format!("https://{target}/"));
                    }
                }
                // Un refus répété se compte sur sa ligne ; une sortie relayée le dit dans sa cible.
                if entry.fois > 1 && matches!(entry.outcome, Outcome::Denied(_)) {
                    ui.label(
                        RichText::new(format!("{} fois", entry.fois))
                            .size(12.0)
                            .color(MUTED),
                    );
                }
                if matches!(kind, Nature::Appel | Nature::Sortie) {
                    ui.label(RichText::new(mark).color(color).size(13.0));
                }
                if !note.is_empty() {
                    ui.label(RichText::new(limited(&note, 80)).size(12.0).color(RED));
                }
                tete
            })
            .inner
        });
        let (row, tete) = (ligne.response, ligne.inner);
        if let Some(step) = entry.step {
            ui.painter().text(
                egui::pos2(row.rect.left(), tete),
                egui::Align2::LEFT_CENTER,
                format!("{step:02}"),
                egui::FontId::monospace(11.0),
                MUTED,
            );
        }
        let (fill, stroke, losange) = match (&kind, &entry.outcome) {
            (Nature::Rappel, _) => (Some(ATTENTE_DOUCE), ATTENTE_DOUCE, true),
            (Nature::Refus, _) | (_, Outcome::Denied(_)) => (Some(RED), RED, true),
            (_, Outcome::Error(_)) => (None, RED, false),
            (_, Outcome::Pending) => (None, MUTED, false),
            (Nature::Publication, _) => (Some(GREEN), GREEN, false),
            (Nature::Appel | Nature::Sortie, Outcome::Ok) => (Some(accent.vif), accent.vif, false),
        };
        noeud(
            ui,
            row.rect,
            tete,
            index == 0,
            index + 1 == shown,
            fill,
            stroke,
            losange,
        );
        let decrit = format!(
            "{} {} — {}",
            outil,
            entry.target.as_deref().unwrap_or(""),
            issue_en_mots(&entry.outcome)
        );
        row.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &decrit));
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
}

/// La marque, la couleur et la note d'une issue d'appel, pour le parcours et le dernier geste.
fn issue(outcome: &Outcome) -> (&'static str, Color32, String) {
    match outcome {
        Outcome::Pending => ("·", MUTED, String::new()),
        Outcome::Ok => ("✓", GREEN, String::new()),
        // « × » est dans toutes les polices embarquées ; « ✕ » et « ⊘ » n'y sont pas.
        Outcome::Error(code) => ("×", RED, code.clone()),
        Outcome::Denied(reason) => ("×", RED, format!("refusé : {}", motif_lisible(reason))),
    }
}

/// La durée qu'une mission a tenue, dite comme on la dit : « moins d'une seconde », « 42 s »,
/// « 3 min 05 s », « 1 h 12 min ».
fn duree_observee(secondes: u64) -> String {
    match secondes {
        0 => "moins d'une seconde".to_owned(),
        1..=59 => format!("{secondes} s"),
        60..=3599 => format!("{} min {:02} s", secondes / 60, secondes % 60),
        _ => format!("{} h {:02} min", secondes / 3600, (secondes % 3600) / 60),
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

/// Le motif d'un refus de capd, en mots : l'humain lit « hors de la portée », pas un nom de
/// variante. Un motif inconnu reste tel quel.
fn motif_lisible(reason: &str) -> String {
    match reason {
        "PolicyDenied" => "hors de la portée de la mission".into(),
        "NoGrant" => "aucun droit ne le permet".into(),
        "Expired" => "jeton expiré".into(),
        "RevokedParent" => "droits retirés".into(),
        "BadSignature" => "jeton invalide".into(),
        "ConstraintViolated" => "contrainte du droit non respectée".into(),
        "ApprovalRequired" => "décision humaine attendue".into(),
        "UnknownVersion" => "jeton d'une version inconnue".into(),
        // egress rend les motifs de capd sous leur nom d'échange.
        "policy_denied" => "hors de la portée de la mission".into(),
        "no_grant" => "aucun hôte du jeton ne le permet".into(),
        "expired" => "jeton expiré".into(),
        "revoked_parent" => "droits retirés".into(),
        "constraint_violated" => "contrainte du droit non respectée".into(),
        "approval_required" => "décision humaine attendue".into(),
        autre => autre.into(),
    }
}

/// Une issue d'appel en mots, pour l'accessibilité.
fn issue_en_mots(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Pending => "en cours".to_owned(),
        Outcome::Ok => "réussi".to_owned(),
        Outcome::Error(code) => format!("en erreur ({code})"),
        Outcome::Denied(reason) => format!("refusé ({})", motif_lisible(reason)),
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
    fn le_dernier_geste_passe_les_sorties_relayees_mais_pas_un_refus() {
        let geste = |tool: &str, outcome: Outcome| crate::missions::TrailEntry {
            seq: 0,
            step: None,
            tool: tool.into(),
            target: None,
            outcome,
            fois: 1,
        };
        let mut trail = vec![geste("fs.write", Outcome::Ok), geste("sortie", Outcome::Ok)];
        assert_eq!(dernier_geste(&trail).unwrap().tool, "fs.write");
        trail.push(geste("sortie", Outcome::Denied("no_grant".into())));
        assert_eq!(
            dernier_geste(&trail).unwrap().outcome,
            Outcome::Denied("no_grant".into())
        );
        assert!(dernier_geste(&[geste("sortie", Outcome::Ok)]).is_none());
    }

    #[test]
    fn la_duree_d_une_mission_se_dit_comme_on_la_dit() {
        assert_eq!(duree_observee(0), "moins d'une seconde");
        assert_eq!(duree_observee(42), "42 s");
        assert_eq!(duree_observee(185), "3 min 05 s");
        assert_eq!(duree_observee(4_320), "1 h 12 min");
    }

    #[test]
    fn seule_une_mission_racine_echouee_ou_arretee_se_relance() {
        let relancee = relance(
            State::Failed,
            false,
            "Rédiger la note",
            Some("local:qwen3-1.7b"),
            Some("documents"),
        )
        .unwrap();
        assert_eq!(relancee.intent, "Rédiger la note");
        assert_eq!(relancee.model, "qwen3-1.7b");
        assert_eq!(relancee.profile.as_deref(), Some("documents"));
        // Un client officiel garde son palier (ADR 0040).
        let client = relance(
            State::Cancelled,
            false,
            "Coder",
            Some("driver:claude-code@opus"),
            None,
        )
        .unwrap();
        assert_eq!(client.model, "claude-code@opus");
        assert_eq!(client.profile, None);
        for state in [State::Planned, State::Running, State::Done] {
            assert!(
                relance(state, false, "x", None, None).is_none(),
                "{state:?}"
            );
        }
        assert!(relance(State::Failed, true, "x", None, None).is_none());
        assert!(relance(State::Failed, false, "  ", None, None).is_none());
    }

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
