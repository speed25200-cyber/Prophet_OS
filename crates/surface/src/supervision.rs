//! Espace de supervision : les missions reçues, leur contexte et les décisions humaines.

use egui::{Align2, Color32, FontId, Frame, RichText, Stroke, pos2, vec2};

use crate::atelier::{Atelier, Page};
use crate::fenetre::Reponse;
use crate::glyphes::{Icon, icon};
use crate::scene::{Courant, Etat, Scene};

pub(crate) const FOND: Color32 = Color32::from_rgb(241, 243, 246);
const BLANC: Color32 = Color32::from_rgb(254, 254, 255);
const TEXTE: Color32 = Color32::from_rgb(30, 35, 44);
const DISCRET: Color32 = Color32::from_rgb(106, 114, 126);
const TRAIT: Color32 = Color32::from_rgb(222, 226, 232);
const BLEU: Color32 = Color32::from_rgb(43, 94, 175);
const VERT: Color32 = Color32::from_rgb(38, 116, 96);
const AMBRE: Color32 = Color32::from_rgb(160, 102, 35);

#[derive(Default)]
pub(crate) struct Supervision {
    pub(crate) missions: crate::missions::Missions,
    detail_tab: crate::mission_details::Tab,
    detail_id: Option<String>,
    selection: Option<String>,
    filtre: Filtre,
    examen: Option<String>,
    focus_compact: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Filtre {
    #[default]
    Toutes,
    Attention,
    Actives,
    Terminees,
}

impl Filtre {
    fn inclut(self, courant: &Courant) -> bool {
        match self {
            Self::Toutes => true,
            Self::Attention => courant.reclame(),
            Self::Actives => courant.etat == Etat::Court,
            Self::Terminees => courant
                .task_state
                .map_or(courant.etat == Etat::Fini, agentd::State::is_terminal),
        }
    }
}

pub(crate) fn installer_style(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    let mut style = (*ctx.style_of(egui::Theme::Light)).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.override_text_color = Some(TEXTE);
    style.visuals.panel_fill = FOND;
    style.visuals.window_fill = BLANC;
    style.visuals.extreme_bg_color = Color32::from_rgb(235, 238, 242);
    style.visuals.selection.bg_fill = Color32::from_rgb(219, 230, 248);
    style.visuals.selection.stroke = Stroke::new(1.0, BLEU);
    style.visuals.widgets.inactive.bg_fill = BLANC;
    style.visuals.widgets.inactive.weak_bg_fill = BLANC;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, TRAIT);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(231, 236, 244);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(231, 236, 244);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(184, 197, 217));
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(218, 228, 244);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, TRAIT);
    style.visuals.widgets.inactive.corner_radius = 9.into();
    style.visuals.widgets.hovered.corner_radius = 9.into();
    style.visuals.widgets.active.corner_radius = 9.into();
    style.visuals.faint_bg_color = FOND;
    style.spacing.item_spacing = vec2(10.0, 10.0);
    style.spacing.scroll.fade.strength = 0.0;
    style.spacing.button_padding = vec2(14.0, 9.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(13.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, FontId::proportional(28.0));
    ctx.set_style_of(egui::Theme::Light, style);
}

fn surface() -> Frame {
    Frame::new()
        .fill(BLANC)
        .corner_radius(22)
        .inner_margin(24)
        .stroke(Stroke::new(1.0, Color32::from_white_alpha(220)))
        .shadow(egui::Shadow {
            offset: [0, 5],
            blur: 22,
            spread: 0,
            color: Color32::from_black_alpha(8),
        })
}

fn petit(ui: &mut egui::Ui, texte: impl Into<String>) {
    ui.label(RichText::new(texte).size(11.0).color(DISCRET));
}

pub(crate) fn bouton(ui: &mut egui::Ui, id: &str, texte: &str, actif: bool) -> egui::Response {
    let width = ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(texte.to_owned(), FontId::proportional(12.0), TEXTE)
            .size()
            .x
    }) + 28.0;
    let (_, rect) = ui.allocate_space(vec2(width, 34.0));
    let response = ui.interact(rect, egui::Id::new(id), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, actif, texte)
    });
    ui.painter().rect_filled(
        rect,
        9,
        if !ui.is_enabled() {
            Color32::from_rgb(224, 228, 233)
        } else if actif {
            TEXTE
        } else if response.hovered() {
            Color32::from_rgb(230, 234, 240)
        } else {
            Color32::TRANSPARENT
        },
    );
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            11,
            Stroke::new(2.0, BLEU),
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        texte,
        FontId::proportional(12.0),
        if actif && ui.is_enabled() {
            BLANC
        } else {
            DISCRET
        },
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn action(ui: &mut egui::Ui, id: &str, texte: &str) -> egui::Response {
    bouton(ui, id, texte, true)
}

fn largeur(ui: &mut egui::Ui, max: f32, contenu: impl FnOnce(&mut egui::Ui)) {
    let available = ui.available_rect_before_wrap();
    let width = available.width().min(max);
    let rect = egui::Rect::from_min_size(
        pos2(available.center().x - width * 0.5, available.top()),
        vec2(width, available.height()),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.set_width(width);
        contenu(ui);
    });
}

fn statut(etat: Etat) -> (&'static str, Color32) {
    match etat {
        Etat::Court => ("En cours", VERT),
        Etat::Attend => ("Votre décision", AMBRE),
        Etat::Bloque => ("À examiner", AMBRE),
        Etat::Fini => ("Terminée", DISCRET),
    }
}

fn statut_mission(c: &Courant) -> (&'static str, Color32) {
    c.task_state
        .map_or_else(|| statut(c.etat), crate::mission_details::status)
}

fn pastille(ui: &mut egui::Ui, texte: &str, couleur: Color32) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(6.0, 10.0), egui::Sense::hover());
        ui.painter().circle_filled(r.center(), 2.5, couleur);
        ui.label(RichText::new(texte).size(11.0).color(couleur));
    });
}

impl Supervision {
    pub(crate) fn dessiner(
        &mut self,
        root: &mut egui::Ui,
        atelier: &mut Atelier,
        scene: &Scene,
        reponse: &mut Option<Reponse>,
    ) {
        let ctx = root.ctx().clone();
        let compact = root.available_width() < 900.0;
        if atelier.mouvement_reduit {
            ctx.all_styles_mut(|s| s.animation_time = 0.0);
        }
        egui::Panel::top("barre-systeme")
            .exact_size(if compact { 100.0 } else { 76.0 })
            .frame(
                Frame::new()
                    .fill(FOND)
                    .inner_margin(if compact { 12 } else { 22 }),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("prophet")
                            .size(23.0)
                            .family(egui::FontFamily::Name("Inter600".into())),
                    );
                    ui.label(RichText::new("OS").size(10.0).color(DISCRET));
                    if !compact {
                        ui.add_space(38.0);
                        navigation(ui, atelier);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        petit(ui, format!("{} UTC", scene.heure));
                        if atelier.demonstration {
                            petit(ui, "DÉMONSTRATION");
                        } else if !compact {
                            pastille(
                                ui,
                                if atelier.modeles.is_empty() {
                                    "Dialogue local déconnecté"
                                } else {
                                    "Dialogue local connecté"
                                },
                                if atelier.modeles.is_empty() {
                                    DISCRET
                                } else {
                                    VERT
                                },
                            );
                        }
                    });
                });
                if compact {
                    ui.horizontal(|ui| navigation(ui, atelier));
                }
            });
        egui::Panel::bottom("etat-systeme")
            .exact_size(34.0)
            .frame(
                Frame::new()
                    .fill(FOND)
                    .inner_margin(egui::Margin::symmetric(22, 8)),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    petit(
                        ui,
                        if atelier.demonstration {
                            "Scène d'exemple · aucune exécution"
                        } else {
                            "États reçus des services Prophet"
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.checkbox(
                            &mut atelier.mouvement_reduit,
                            RichText::new("Mouvement réduit").size(10.0).color(DISCRET),
                        );
                    });
                });
            });
        if scene.decision.is_some() {
            egui::Panel::top("attention-globale")
                .exact_size(52.0)
                .frame(
                    Frame::new()
                        .fill(Color32::from_rgb(249, 243, 232))
                        .inner_margin(egui::Margin::symmetric(24, 9)),
                )
                .show(root, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Une action attend votre accord")
                                .size(13.0)
                                .color(AMBRE),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if action(ui, "examiner-decision", "Examiner l'action").clicked() {
                                self.examen = empreinte_decision(scene);
                            }
                        });
                    });
                });
        }
        if atelier.page == Page::Conversation {
            egui::Panel::bottom("composition")
                .resizable(false)
                .frame(
                    Frame::new()
                        .fill(FOND)
                        .inner_margin(if compact { 12 } else { 22 }),
                )
                .show(root, |ui| largeur(ui, 880.0, |ui| composer(ui, atelier)));
        }
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(FOND)
                    .inner_margin(if compact { 14 } else { 28 }),
            )
            .show(root, |ui| match atelier.page {
                Page::Accueil => largeur(ui, 1400.0, |ui| self.accueil(ui, atelier, scene)),
                Page::Conversation => largeur(ui, 880.0, |ui| conversation(ui, atelier)),
                Page::Modeles => largeur(ui, 1100.0, |ui| modeles(ui, atelier)),
                Page::Activite => largeur(ui, 1100.0, |ui| systeme(ui, scene)),
            });
        if self.examen != empreinte_decision(scene) {
            self.examen = None;
        }
        if self.examen.is_some() {
            self.decision(&ctx, scene, reponse);
        }
    }

    fn accueil(&mut self, ui: &mut egui::Ui, atelier: &mut Atelier, scene: &Scene) {
        let compact = ui.available_width() < 850.0;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("L'espace de vos agents")
                        .size(if compact { 25.0 } else { 34.0 })
                        .family(egui::FontFamily::Name("Inter600".into())),
                );
                petit(
                    ui,
                    format!(
                        "{} mission{} · {} en cours · {} à examiner",
                        scene.courants.len(),
                        if scene.courants.len() == 1 { "" } else { "s" },
                        scene.actives(),
                        scene.courants.iter().filter(|c| c.reclame()).count()
                    ),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if action(ui, "preparer-mission", "+ Préparer un objectif").clicked() {
                    atelier.page = Page::Conversation;
                }
            });
        });
        ui.add_space(if compact { 12.0 } else { 18.0 });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (filtre, id, label) in [
                (Filtre::Toutes, "filter-all", "Tout"),
                (Filtre::Attention, "filter-attention", "À examiner"),
                (Filtre::Actives, "filter-active", "En cours"),
                (Filtre::Terminees, "filter-done", "Terminées"),
            ] {
                if bouton(ui, id, label, self.filtre == filtre).clicked() {
                    self.filtre = filtre;
                    self.focus_compact = false;
                }
            }
        });
        ui.add_space(12.0);
        let visibles: Vec<_> = scene
            .courants
            .iter()
            .filter(|c| self.filtre.inclut(c))
            .collect();
        if !visibles
            .iter()
            .any(|c| self.selection.as_deref() == Some(&c.tache))
        {
            self.selection = visibles
                .iter()
                .find(|c| scene.decision.as_ref().is_some_and(|d| d.tache == c.tache))
                .or_else(|| visibles.iter().find(|c| c.reclame()))
                .or_else(|| visibles.first())
                .map(|c| c.tache.clone());
        }
        if scene.courants.is_empty() {
            self.missions.select(None);
            egui::ScrollArea::vertical().id_salt("supervision-vide").show(ui, |ui| {
                surface().inner_margin(if compact { 24 } else { 44 }).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    petit(ui, "VOTRE POINT DE VUE");
                    ui.add_space(12.0);
                    ui.label(RichText::new("Chaque mission,\nun contexte. Chaque décision,\nvotre choix.").size(if compact { 26.0 } else { 40.0 }).line_height(Some(if compact { 34.0 } else { 49.0 })));
                    ui.add_space(22.0);
                    ui.label(RichText::new("Aucune mission reçue pour le moment.").color(DISCRET));
                    ui.label(RichText::new("Les missions des agents apparaîtront ici avec leur état et les décisions qui vous reviennent.").size(13.0).color(DISCRET));
                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(8.0);
                    petit(ui, "Préparer un objectif ouvre le dialogue local. Le lancement d'agents depuis ce dialogue reste à intégrer.");
                });
            });
            return;
        }
        let selection = self.selection.clone();
        if self.detail_id != selection {
            self.detail_tab = crate::mission_details::Tab::Auto;
            self.detail_id = selection.clone();
        }
        self.missions.select(selection.as_deref());
        let courant = visibles
            .iter()
            .copied()
            .find(|c| selection.as_deref() == Some(&c.tache));
        if compact {
            egui::ScrollArea::vertical()
                .id_salt("missions-compactes")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if self.focus_compact {
                        if bouton(ui, "retour-missions", "← Toutes les missions", false).clicked()
                        {
                            self.focus_compact = false;
                        }
                        if let Some(c) = courant {
                            self.inspecteur(ui, c, scene);
                        }
                    } else {
                        let precedente = self.selection.clone();
                        self.liste(ui, &visibles);
                        if self.selection != precedente {
                            self.focus_compact = true;
                        }
                    }
                });
        } else {
            let width = ui.available_width();
            let sidebar = (width * 0.29).clamp(250.0, 360.0);
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    vec2(sidebar, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("missions")
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.liste(ui, &visibles));
                    },
                );
                ui.add_space(12.0);
                ui.allocate_ui_with_layout(
                    vec2(width - sidebar - 22.0, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("contexte")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if let Some(c) = courant {
                                    self.inspecteur(ui, c, scene);
                                }
                            });
                    },
                );
            });
        }
    }

    fn liste(&mut self, ui: &mut egui::Ui, courants: &[&Courant]) {
        if ui.available_width() >= 620.0 {
            for row in courants.chunks(2) {
                ui.columns(2, |columns| {
                    for (column, c) in columns.iter_mut().zip(row) {
                        if mission_carte(column, c, self.selection.as_deref() == Some(&c.tache)) {
                            self.selection = Some(c.tache.clone());
                            self.focus_compact = true;
                        }
                    }
                });
                ui.add_space(4.0);
            }
            return;
        }
        if courants.is_empty() {
            surface().show(ui, |ui| {
                ui.label("Aucune mission dans cette vue.");
            });
        }
        for c in courants {
            let selected = self.selection.as_deref() == Some(&c.tache);
            let (_, rect) = ui.allocate_space(vec2(ui.available_width(), 115.0));
            let response = ui
                .interact(
                    rect,
                    egui::Id::new(format!("mission-{}", c.tache)),
                    egui::Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    true,
                    selected,
                    &c.intitule,
                )
            });
            let p = ui.painter_at(rect);
            p.rect_filled(
                rect,
                18,
                if selected {
                    BLANC
                } else if response.hovered() {
                    Color32::from_rgb(249, 250, 252)
                } else {
                    Color32::from_rgb(246, 248, 250)
                },
            );
            p.rect_stroke(
                rect,
                18,
                Stroke::new(
                    1.0,
                    if selected || response.has_focus() {
                        Color32::from_rgb(159, 179, 210)
                    } else {
                        TRAIT
                    },
                ),
                egui::StrokeKind::Inside,
            );
            if selected {
                p.rect_filled(
                    egui::Rect::from_min_size(rect.min + vec2(0.0, 28.0), vec2(3.0, 59.0)),
                    2,
                    BLEU,
                );
            }
            let badge = egui::Rect::from_min_size(rect.min + vec2(20.0, 20.0), vec2(32.0, 32.0));
            p.rect_filled(badge, 10, Color32::from_rgb(232, 237, 244));
            icon(&p, badge.center(), Icon::Models, 16.0, DISCRET);
            let mut agent_job = egui::text::LayoutJob::simple(
                c.agent.clone(),
                FontId::proportional(11.0),
                DISCRET,
                rect.width() - 88.0,
            );
            agent_job.wrap.max_rows = 1;
            let agent = ui.fonts_mut(|fonts| fonts.layout_job(agent_job));
            p.galley(rect.min + vec2(64.0, 21.0), agent, DISCRET);
            let mut title_job = egui::text::LayoutJob::simple(
                c.intitule.clone(),
                FontId::new(16.0, egui::FontFamily::Name("Inter600".into())),
                TEXTE,
                rect.width() - 88.0,
            );
            title_job.wrap.max_rows = 2;
            let title = ui.fonts_mut(|fonts| fonts.layout_job(title_job));
            p.galley(rect.min + vec2(64.0, 40.0), title, TEXTE);
            let (label, color) = statut_mission(c);
            p.circle_filled(rect.min + vec2(24.0, 93.0), 2.5, color);
            p.text(
                rect.min + vec2(35.0, 93.0),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(11.0),
                color,
            );
            p.text(
                pos2(rect.right() - 20.0, rect.top() + 93.0),
                Align2::RIGHT_CENTER,
                format!("{} étapes", c.etapes),
                FontId::proportional(11.0),
                DISCRET,
            );
            if response.clicked() {
                self.selection = Some(c.tache.clone());
                self.focus_compact = true;
            }
            ui.add_space(2.0);
        }
    }

    fn inspecteur(&mut self, ui: &mut egui::Ui, c: &Courant, scene: &Scene) {
        if self.missions.connected() {
            crate::mission_details::draw(ui, c, &mut self.missions, &mut self.detail_tab);
            if let Some(d) = scene.decision.as_ref().filter(|d| d.tache == c.tache) {
                ui.add_space(12.0);
                ui.label(&d.question);
                if action(ui, "inspecter-action", "Lire les conséquences").clicked() {
                    self.examen = empreinte_decision(scene);
                }
            }
            return;
        }
        surface().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                petit(ui, "MISSION EN FOCALE");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if bouton(ui, "copier-reference", "Copier la référence", false).clicked() { ui.ctx().copy_text(c.tache.clone()); }
                });
            });
            ui.add_space(16.0);
            ui.label(RichText::new(&c.intitule).size(28.0).family(egui::FontFamily::Name("Inter600".into())).line_height(Some(34.0)));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&c.agent).size(13.0).color(DISCRET));
                let (label, color) = statut_mission(c);
                pastille(ui, label, color);
            });
            ui.add_space(22.0);
            Frame::new().fill(if c.reclame() { Color32::from_rgb(250, 245, 235) } else { Color32::from_rgb(241, 245, 248) }).corner_radius(13).inner_margin(18).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(if c.reclame() { "Votre attention" } else { "Point de supervision" }).size(14.0).family(egui::FontFamily::Name("Inter600".into())));
                if let Some(d) = scene.decision.as_ref().filter(|d| d.tache == c.tache) {
                    ui.label(RichText::new(&d.question).size(17.0).family(egui::FontFamily::Name("Inter600".into())));
                    if action(ui, "inspecter-action", "Lire les conséquences").clicked() { self.examen = empreinte_decision(scene); }
                } else {
                    ui.label(RichText::new(match c.etat {
                        Etat::Court => "La mission est signalée en cours. Aucune demande d'accord reçue pour cette mission.",
                        Etat::Bloque => "Le service signale un blocage. Sa cause détaillée n'a pas été fournie.",
                        Etat::Attend => "La mission attend. Le détail de sa demande n'est pas disponible dans cette scène.",
                        Etat::Fini => "Le service signale la fin de la mission. Les livrables restent à examiner lorsqu'ils sont disponibles.",
                    }).size(13.0).color(DISCRET));
                }
            });
            ui.add_space(22.0);
            ui.separator();
            ui.add_space(14.0);
            ui.columns(2, |columns| {
                petit(&mut columns[0], "ÉTAPES OBSERVÉES");
                columns[0].label(RichText::new(c.etapes.to_string()).size(32.0));
                petit(&mut columns[1], "ACTIVITÉ");
                columns[1].label(RichText::new(format!("{:.0} / min", c.debit)).size(32.0));
            });
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                petit(ui, "Budget consommé");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { petit(ui, format!("{:.0} %", c.budget_consomme.clamp(0.0, 1.0) * 100.0)); });
            });
            let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 4.0), egui::Sense::hover());
            ui.painter().rect_filled(bar, 2, FOND);
            ui.painter().rect_filled(egui::Rect::from_min_size(bar.min, vec2(bar.width() * c.budget_consomme.clamp(0.0, 1.0), 4.0)), 2, if c.reclame() { AMBRE } else { BLEU });
            ui.add_space(24.0);
            ui.add_space(22.0);
            petit(ui, "RÉSULTATS ET CHANGEMENTS");
            ui.label(RichText::new("Aucun livrable ni diff reçu.").size(13.0).color(DISCRET));
            ui.add_space(6.0);
            petit(ui, &c.tache);
        });
    }

    fn decision(&mut self, ctx: &egui::Context, scene: &Scene, reponse: &mut Option<Reponse>) {
        let Some(d) = &scene.decision else {
            return;
        };
        let id = egui::Id::new("decision");
        egui::Modal::new(id)
            .area(egui::Modal::default_area(id).fade_in(false))
            .backdrop_color(Color32::from_black_alpha(90))
            .frame(surface().inner_margin(28))
            .show(ctx, |ui| {
                ui.set_max_width(570.0);
                petit(ui, "DÉCISION HUMAINE");
                ui.add_space(12.0);
                ui.label(
                    RichText::new(&d.question)
                        .size(27.0)
                        .family(egui::FontFamily::Name("Inter600".into())),
                );
                ui.add_space(14.0);
                ui.label(&d.consequence);
                if d.irreversible {
                    ui.label(RichText::new("Action irréversible").size(13.0).color(AMBRE));
                }
                ui.add_space(8.0);
                petit(
                    ui,
                    format!("Mission {} · attente {} s", d.tache, d.depuis_secondes),
                );
                ui.add_space(18.0);
                ui.horizontal_wrapped(|ui| {
                    if bouton(ui, "decision-retour", "Revenir aux missions", false).clicked() {
                        self.examen = None;
                    }
                    if bouton(ui, "decision-refuser", "Refuser", false).clicked() {
                        *reponse = Some(Reponse::Refuse);
                        self.examen = None;
                    }
                    if action(ui, "decision-autoriser", "Autoriser cette action").clicked() {
                        *reponse = Some(Reponse::Accepte);
                        self.examen = None;
                    }
                });
            });
    }
}

fn empreinte_decision(scene: &Scene) -> Option<String> {
    scene.decision.as_ref().map(|d| {
        format!(
            "{}\0{}\0{}\0{}",
            d.tache, d.question, d.consequence, d.irreversible
        )
    })
}

fn mission_carte(ui: &mut egui::Ui, c: &Courant, selected: bool) -> bool {
    let (_, rect) = ui.allocate_space(vec2(ui.available_width(), 164.0));
    let response = ui
        .interact(
            rect,
            egui::Id::new(format!("mission-{}", c.tache)),
            egui::Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            selected,
            &c.intitule,
        )
    });
    let p = ui.painter_at(rect);
    p.rect_filled(
        rect,
        18,
        if selected {
            BLANC
        } else {
            Color32::from_rgb(247, 249, 251)
        },
    );
    p.rect_stroke(
        rect,
        18,
        Stroke::new(
            if selected { 1.5 } else { 1.0 },
            if selected || response.hovered() || response.has_focus() {
                Color32::from_rgb(150, 172, 202)
            } else {
                Color32::from_rgb(224, 229, 236)
            },
        ),
        egui::StrokeKind::Inside,
    );
    let badge = egui::Rect::from_min_size(rect.min + vec2(18.0, 18.0), vec2(26.0, 26.0));
    p.rect_filled(
        badge,
        8,
        if c.reclame() {
            Color32::from_rgb(242, 233, 217)
        } else {
            Color32::from_rgb(228, 234, 243)
        },
    );
    icon(&p, badge.center(), Icon::Models, 14.0, DISCRET);
    p.text(
        rect.min + vec2(54.0, 31.0),
        Align2::LEFT_CENTER,
        &c.agent,
        FontId::proportional(11.0),
        DISCRET,
    );
    let title = p.layout(
        c.intitule.clone(),
        FontId::new(17.0, egui::FontFamily::Name("Inter600".into())),
        TEXTE,
        rect.width() - 38.0,
    );
    p.galley(rect.min + vec2(18.0, 61.0), title, TEXTE);
    let (label, color) = statut_mission(c);
    p.circle_filled(rect.min + vec2(21.0, 140.0), 2.5, color);
    p.text(
        rect.min + vec2(31.0, 140.0),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(10.0),
        color,
    );
    p.text(
        pos2(rect.right() - 18.0, rect.top() + 140.0),
        Align2::RIGHT_CENTER,
        format!("{} étapes", c.etapes),
        FontId::proportional(10.0),
        DISCRET,
    );
    response.clicked()
}

fn navigation(ui: &mut egui::Ui, atelier: &mut Atelier) {
    ui.spacing_mut().item_spacing.x = 4.0;
    for (page, id, label) in [
        (Page::Accueil, "nav-accueil", "Superviser"),
        (Page::Conversation, "nav-conversation", "Dialoguer"),
        (Page::Modeles, "nav-modeles", "Modèles"),
        (Page::Activite, "nav-activite", "Système"),
    ] {
        if bouton(ui, id, label, atelier.page == page).clicked() {
            atelier.page = page;
        }
    }
}

fn composer(ui: &mut egui::Ui, atelier: &mut Atelier) {
    let ctx = ui.ctx().clone();
    if let Some(error) = &atelier.erreur {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(if atelier.modeles.is_empty() {
                    "Moteur local indisponible"
                } else {
                    error
                })
                .size(12.0)
                .color(AMBRE),
            );
            if ui.link("Vérifier la connexion").clicked() {
                atelier.page = Page::Modeles;
            }
        });
    }
    surface().inner_margin(18).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let response = ui.add(
            egui::TextEdit::multiline(&mut atelier.brouillon)
                .id(egui::Id::new("intention"))
                .hint_text("Un objectif, une contrainte, une question…")
                .font(FontId::proportional(16.0))
                .desired_width(f32::INFINITY)
                .desired_rows(1)
                .char_limit(16_384)
                .frame(Frame::NONE),
        );
        let shortcut = response.has_focus()
            && ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Enter));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("choix-modele")
                .width(185.0)
                .selected_text(if atelier.choisi.is_empty() {
                    "Choisir un modèle"
                } else {
                    &atelier.choisi
                })
                .show_ui(ui, |ui| {
                    for model in &atelier.modeles {
                        ui.selectable_value(&mut atelier.choisi, model.clone(), model);
                    }
                });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if atelier.generation {
                    if action(ui, "interrompre", "Interrompre").clicked() {
                        atelier.interrompre();
                    }
                    ui.spinner();
                } else {
                    let enabled = !atelier.demonstration
                        && !atelier.choisi.is_empty()
                        && !atelier.brouillon.trim().is_empty();
                    let clicked = ui
                        .add_enabled(
                            enabled,
                            egui::Button::new(RichText::new("Envoyer ↑").color(BLANC))
                                .fill(TEXTE)
                                .min_size(vec2(96.0, 34.0)),
                        )
                        .clicked();
                    if clicked || (enabled && shortcut) {
                        atelier.envoyer(&ctx);
                    }
                }
            });
        });
    });
    ui.horizontal(|ui| {
        petit(ui, "Conversation locale · en mémoire");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            petit(ui, "Ctrl + Entrée pour envoyer");
        });
    });
}

fn conversation(ui: &mut egui::Ui, atelier: &mut Atelier) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Le dialogue, à votre rythme.")
                .size(26.0)
                .family(egui::FontFamily::Name("Inter600".into())),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if bouton(ui, "nouveau-dialogue", "+ Nouveau", false).clicked() {
                atelier.nouvelle();
                atelier.page = Page::Conversation;
            }
        });
    });
    ui.add_space(12.0);
    egui::ScrollArea::vertical().id_salt("dialogue").stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
        if atelier.tours.is_empty() {
            surface().show(ui, |ui| {
                ui.set_width(ui.available_width());
                petit(ui, "AVANT DE DÉLÉGUER");
                ui.label(RichText::new("Précisez le résultat attendu.").size(25.0));
                ui.label(RichText::new("Explorez un objectif avec votre modèle local. Ce dialogue ne lance pas d'agent et n'accorde aucun droit système.").size(13.0).color(DISCRET));
                ui.add_space(12.0);
                if bouton(ui, "cadrer-objectif", "Structurer mon objectif", false).clicked() {
                    atelier.brouillon = "Aide-moi à préciser cet objectif, ses contraintes et les critères qui permettront de vérifier le résultat : ".into();
                    ui.ctx().memory_mut(|m| m.request_focus(egui::Id::new("intention")));
                }
            });
        }
        for (i, tour) in atelier.tours.iter().enumerate() {
            ui.push_id(i, |ui| {
                Frame::new().fill(Color32::from_rgb(230, 235, 242)).corner_radius(16).inner_margin(18).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    petit(ui, "VOUS");
                    ui.label(RichText::new(&tour.demande).size(15.0));
                });
                ui.add_space(6.0);
                surface().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        petit(ui, &tour.modele);
                        if !tour.reponse.is_empty() && ui.small_button("Copier").clicked() { ui.ctx().copy_text(tour.reponse.clone()); }
                    });
                    ui.add_space(10.0);
                    if tour.reponse.is_empty() && tour.erreur.is_none() { petit(ui, "Le modèle prépare sa réponse…"); }
                    else { ui.label(RichText::new(&tour.reponse).size(15.0).line_height(Some(24.0))); }
                    if let Some(error) = &tour.erreur { ui.label(RichText::new(error).color(AMBRE)); }
                    if let Some(mesure) = &tour.mesure {
                        ui.add_space(12.0);
                        petit(ui, format!("{} tokens · {:.2} s · premier texte {} ms", mesure.usage.tokens_out, mesure.elapsed.as_secs_f64(), mesure.first_token.unwrap_or_default().as_millis()));
                    }
                });
                ui.add_space(16.0);
            });
        }
    });
}

fn modeles(ui: &mut egui::Ui, atelier: &mut Atelier) {
    ui.label(
        RichText::new("L'intelligence sur votre machine.")
            .size(30.0)
            .family(egui::FontFamily::Name("Inter600".into())),
    );
    petit(ui, "Modèles réellement exposés par le moteur local");
    ui.add_space(22.0);
    egui::ScrollArea::vertical()
        .id_salt("bibliotheque")
        .show(ui, |ui| {
            surface().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(&atelier.endpoint)
                            .monospace()
                            .size(12.0)
                            .color(DISCRET),
                    );
                    if ui
                        .add_enabled(!atelier.decouverte, egui::Button::new("Actualiser"))
                        .clicked()
                    {
                        atelier.decouvrir(&ui.ctx().clone());
                    }
                    if atelier.decouverte {
                        ui.spinner();
                    }
                });
                if let Some(error) = &atelier.erreur {
                    egui::CollapsingHeader::new("Diagnostic de connexion").show(ui, |ui| {
                        ui.label(RichText::new(error).size(12.0).color(AMBRE));
                    });
                }
                if atelier.modeles.is_empty() {
                    ui.add_space(28.0);
                    ui.label(
                        RichText::new(if atelier.decouverte {
                            "Recherche en cours…"
                        } else {
                            "Aucun modèle connecté."
                        })
                        .size(28.0),
                    );
                    ui.label(
                        RichText::new(
                            "Démarrez un moteur local compatible, puis actualisez cette vue.",
                        )
                        .color(DISCRET),
                    );
                    ui.add_space(8.0);
                    petit(
                        ui,
                        "Le téléchargement et le lancement des moteurs restent à intégrer.",
                    );
                }
            });
            ui.add_space(14.0);
            for model in &atelier.modeles {
                surface().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(model).size(23.0));
                            pastille(ui, "Disponible pour dialoguer", VERT);
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .button(if atelier.choisi == *model {
                                    "Sélectionné"
                                } else {
                                    "Choisir"
                                })
                                .clicked()
                            {
                                atelier.choisi.clone_from(model);
                            }
                        });
                    });
                });
                ui.add_space(14.0);
            }
        });
}

fn systeme(ui: &mut egui::Ui, scene: &Scene) {
    ui.label(
        RichText::new("Le cadre d'exécution.")
            .size(30.0)
            .family(egui::FontFamily::Name("Inter600".into())),
    );
    petit(ui, "Capacités et états signalés par les services");
    ui.add_space(24.0);
    egui::ScrollArea::vertical()
        .id_salt("systeme")
        .show(ui, |ui| {
            surface().show(ui, |ui| {
                ui.set_width(ui.available_width());
                petit(ui, "ISOLATION DISPONIBLE");
                ui.label(
                    RichText::new(format!("Niveau {}", scene.isolation.niveau_max)).size(34.0),
                );
                if let Some(manque) = &scene.isolation.manque {
                    ui.label(RichText::new(manque).color(DISCRET));
                }
                ui.add_space(12.0);
                petit(
                    ui,
                    "Cette capacité annoncée ne prouve pas le confinement de chaque mission.",
                );
            });
            ui.add_space(16.0);
            surface().show(ui, |ui| {
                ui.set_width(ui.available_width());
                petit(ui, "MISSIONS REÇUES");
                if scene.courants.is_empty() {
                    ui.label("Aucune mission reçue.");
                }
                for c in &scene.courants {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(&c.intitule).size(14.0));
                        let (label, color) = statut_mission(c);
                        pastille(ui, label, color);
                    });
                    petit(
                        ui,
                        format!("{} · {} · {} étapes", c.tache, c.agent, c.etapes),
                    );
                    ui.add_space(8.0);
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_filtres_ne_transforment_pas_un_blocage_en_execution() {
        let c = Courant {
            tache: "a".into(),
            intitule: "mission".into(),
            agent: "local".into(),
            etat: Etat::Bloque,
            debit: 0.0,
            budget_consomme: 0.2,
            etapes: 3,
            task_state: None,
            task_revision: 0,
        };
        assert!(Filtre::Attention.inclut(&c));
        assert!(!Filtre::Actives.inclut(&c));
        assert!(!Filtre::Terminees.inclut(&c));
    }
}
