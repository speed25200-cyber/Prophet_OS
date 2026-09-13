//! Espace de supervision : les missions reçues, leur contexte et les décisions humaines.

use egui::{Color32, FontId, Frame, RichText, Stroke, pos2, vec2};

use crate::atelier::{Atelier, Page};
use crate::fenetre::Reponse;
use crate::hud;
use crate::scene::{Courant, Etat, Scene};
use crate::theme::palette::{
    ACCOMPLI, ATTENTE, ATTENTE_VOILE, CREUX, DISCRET, EFFACE, ENCRE, TRAIT, VERRE_HAUT, VOILE,
};
use crate::theme::{ACCENTS, Accent};

#[derive(Default)]
pub(crate) struct Supervision {
    pub(crate) missions: crate::missions::Missions,
    pub(crate) preparation: crate::preparation::Preparation,
    composing: bool,
    prepared_selection: Option<String>,
    detail_tab: crate::mission_details::Tab,
    detail_id: Option<String>,
    selection: Option<String>,
    filtre: Filtre,
    examen: Option<String>,
    focus_compact: bool,
    isolate: bool,
    query: String,
    apparence: Option<(String, bool)>,
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

/// Le style d'egui, réglé sur la nuit : verre, fil d'accent, encre.
///
/// Il se réinstalle à chaque changement d'accent, parce que les widgets natifs d'egui (choix
/// déroulants, cases, curseur de saisie) lisent leurs couleurs dans le style.
pub(crate) fn installer_style(ctx: &egui::Context) {
    let accent = Accent::de(ctx);
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(ENCRE);
    style.visuals.panel_fill = Color32::TRANSPARENT;
    style.visuals.window_fill = Color32::from_rgb(10, 14, 19);
    style.visuals.window_stroke = Stroke::new(1.0, accent.fil_vif);
    style.visuals.window_corner_radius = 4.into();
    style.visuals.window_shadow = egui::Shadow::NONE;
    style.visuals.popup_shadow = egui::Shadow::NONE;
    style.visuals.extreme_bg_color = CREUX;
    style.visuals.faint_bg_color = VERRE_HAUT;
    style.visuals.code_bg_color = CREUX;
    style.visuals.hyperlink_color = accent.vif;
    style.visuals.selection.bg_fill = hud::voile(accent.vif, 90);
    style.visuals.selection.stroke = Stroke::new(1.0, accent.vif);
    style.visuals.text_cursor.stroke = Stroke::new(2.0, accent.vif);
    style.visuals.widgets.noninteractive.bg_fill = Color32::TRANSPARENT;
    style.visuals.widgets.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, TRAIT);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, ENCRE);
    style.visuals.widgets.inactive.bg_fill = VERRE_HAUT;
    style.visuals.widgets.inactive.weak_bg_fill = VERRE_HAUT;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, accent.fil);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, ENCRE);
    style.visuals.widgets.hovered.bg_fill = hud::voile(accent.vif, 34);
    style.visuals.widgets.hovered.weak_bg_fill = hud::voile(accent.vif, 34);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, accent.fil_vif);
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, accent.vif);
    style.visuals.widgets.active.bg_fill = hud::voile(accent.vif, 60);
    style.visuals.widgets.active.weak_bg_fill = hud::voile(accent.vif, 60);
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, accent.vif);
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, ENCRE);
    style.visuals.widgets.open.bg_fill = VERRE_HAUT;
    style.visuals.widgets.open.weak_bg_fill = VERRE_HAUT;
    style.visuals.widgets.open.bg_stroke = Stroke::new(1.0, accent.fil_vif);
    style.visuals.widgets.open.fg_stroke = Stroke::new(1.0, accent.vif);
    for widget in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widget.corner_radius = 3.into();
    }
    style.spacing.item_spacing = vec2(10.0, 10.0);
    style.spacing.scroll.fade.strength = 0.0;
    style.spacing.button_padding = vec2(14.0, 8.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(12.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, FontId::new(28.0, hud::fin()));
    ctx.set_style_of(egui::Theme::Dark, style);
}

/// Une plaque de verre à crochets, pour tout ce qui se lit.
fn plaque<R>(ui: &mut egui::Ui, margin: i8, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let accent = Accent::de(ui.ctx());
    hud::plaque(ui, &accent, margin, add).inner
}

/// Un texte de titre, en graisse fine.
pub(crate) fn titre(text: impl Into<String>, size: f32) -> RichText {
    hud::titre(text, size)
}

/// Une étiquette en capitales espacées, dans l'accent sourd : le vocabulaire des rubriques.
pub(crate) fn etiquette(ui: &mut egui::Ui, text: impl Into<String>) {
    let accent = Accent::de(ui.ctx());
    hud::etiquette(ui, text, accent.sourd);
}

pub(crate) fn petit(ui: &mut egui::Ui, texte: impl Into<String>) {
    ui.label(RichText::new(texte).size(11.0).color(DISCRET));
}

/// Un bouton du tableau de bord ; l'identifiant reste stable pour les parcours.
pub(crate) fn bouton(ui: &mut egui::Ui, id: &str, texte: &str, actif: bool) -> egui::Response {
    let accent = Accent::de(ui.ctx());
    hud::bouton(ui, id, texte, actif, &accent)
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

fn statut(etat: Etat, accent: &Accent) -> (&'static str, Color32) {
    match etat {
        Etat::Court => ("En cours", accent.vif),
        Etat::Attend => ("Votre décision", ATTENTE),
        Etat::Bloque => ("À examiner", ATTENTE),
        Etat::Fini => ("Terminée", DISCRET),
    }
}

fn statut_mission(c: &Courant, accent: &Accent) -> (&'static str, Color32) {
    c.task_state.map_or_else(
        || statut(c.etat, accent),
        |state| crate::mission_details::status(state, accent),
    )
}

fn pastille(ui: &mut egui::Ui, texte: &str, couleur: Color32) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(6.0, 10.0), egui::Sense::hover());
        ui.painter().circle_filled(r.center(), 2.5, couleur);
        hud::etiquette(ui, texte.to_uppercase(), couleur);
    });
}

impl Supervision {
    /// La mission choisie, pour que le champ l'éclaire.
    pub(crate) fn selection(&self) -> Option<&str> {
        self.selection.as_deref()
    }

    pub(crate) fn dessiner(
        &mut self,
        root: &mut egui::Ui,
        atelier: &mut Atelier,
        scene: &Scene,
        reponse: &mut Option<Reponse>,
    ) {
        let ctx = root.ctx().clone();
        self.preparation.update();
        if let Some(plan) = self.preparation.take_prepared() {
            self.prepared_selection = Some(plan.task);
            self.composing = false;
            self.filtre = Filtre::Toutes;
            self.query.clear();
            self.focus_compact = true;
            self.isolate = true;
            atelier.page = Page::Accueil;
        }
        let compact = root.available_width() < 900.0;
        if atelier.mouvement_reduit {
            ctx.all_styles_mut(|s| s.animation_time = 0.0);
        }
        crate::desk::chrome(root, atelier, scene, compact);
        if scene.decision.is_some() {
            egui::Panel::top("attention-globale")
                .exact_size(54.0)
                .frame(
                    Frame::new()
                        .fill(ATTENTE_VOILE)
                        .inner_margin(egui::Margin::symmetric(28, 10)),
                )
                .show(root, |ui| {
                    let r = ui.max_rect();
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            pos2(r.left() - 28.0, r.top() - 10.0),
                            vec2(3.0, r.height() + 20.0),
                        ),
                        0,
                        ATTENTE,
                    );
                    ui.horizontal(|ui| {
                        let (dot, _) =
                            ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
                        ui.painter().circle_filled(dot.center(), 3.0, ATTENTE);
                        hud::etiquette(ui, "DÉCISION EN ATTENTE", ATTENTE);
                        ui.label(
                            RichText::new("Une action attend votre accord")
                                .size(14.0)
                                .color(ENCRE),
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
                        .fill(Color32::TRANSPARENT)
                        .inner_margin(if compact { 12 } else { 24 }),
                )
                .show(root, |ui| largeur(ui, 900.0, |ui| composer(ui, atelier)));
        }
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(Color32::TRANSPARENT)
                    .inner_margin(if compact { 14 } else { 30 }),
            )
            .show(root, |ui| match atelier.page {
                Page::Accueil if self.composing => largeur(ui, 1240.0, |ui| {
                    if crate::preparation_view::draw(ui, &mut self.preparation) {
                        self.composing = false;
                    }
                }),
                Page::Accueil => largeur(ui, 1560.0, |ui| self.accueil(ui, scene)),
                Page::Conversation => largeur(ui, 900.0, |ui| {
                    if !atelier.generation
                        && (!atelier.brouillon.trim().is_empty() || !atelier.tours.is_empty())
                        && bouton(
                            ui,
                            "conversation-vers-mission",
                            "Préparer une mission à partir de ma demande",
                            false,
                        )
                        .clicked()
                    {
                        if self.preparation.attempted_id().is_some()
                            && self.preparation.error().is_none()
                            && !self.preparation.pending()
                        {
                            self.preparation.reset();
                        }
                        if self.preparation.attempted_id().is_none() {
                            self.preparation.intent = if atelier.brouillon.trim().is_empty() {
                                atelier
                                    .tours
                                    .last()
                                    .map(|t| t.demande.clone())
                                    .unwrap_or_default()
                            } else {
                                atelier.brouillon.clone()
                            };
                        }
                        self.composing = true;
                        atelier.page = Page::Accueil;
                        self.preparation.discover(&ctx);
                    }
                    conversation(ui, atelier);
                }),
                Page::Modeles => largeur(ui, 1100.0, |ui| modeles(ui, atelier)),
                Page::Activite => largeur(ui, 1100.0, |ui| self.systeme(ui, scene)),
            });
        if self.examen != empreinte_decision(scene) {
            self.examen = None;
        }
        if self.examen.is_some() {
            self.decision(&ctx, scene, reponse);
        }
    }

    fn accueil(&mut self, ui: &mut egui::Ui, scene: &Scene) {
        let accent = Accent::de(ui.ctx());
        let compact = ui.available_width() < 850.0;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                etiquette(ui, "SUPERVISION");
                ui.label(titre("Vos missions", if compact { 28.0 } else { 34.0 }));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if action(ui, "preparer-mission", "Nouvel objectif  ↗").clicked() {
                    self.composing = true;
                    if self.preparation.attempted_id().is_some()
                        && self.preparation.error().is_none()
                        && !self.preparation.pending()
                    {
                        self.preparation.reset();
                    }
                    self.preparation.discover(ui.ctx());
                }
            });
        });
        ui.add_space(if compact { 10.0 } else { 14.0 });
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
                    self.isolate = false;
                }
            }
            ui.add_space(10.0);
            let search_id = egui::Id::new("mission-search");
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::K)) {
                ui.memory_mut(|m| m.request_focus(search_id));
            }
            let search = hud::cadre_saisie(&accent)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .id(search_id)
                            .desired_width(150.0)
                            .frame(Frame::NONE)
                            .font(FontId::proportional(12.0))
                            .hint_text("Rechercher  ·  Ctrl K")
                            .char_limit(160),
                    )
                })
                .inner;
            if search.changed() {
                self.isolate = false;
                self.focus_compact = false;
            }
            if !compact && scene.courants.len() > 1 {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if bouton(
                        ui,
                        "workspace-focus",
                        if self.isolate {
                            "Voir les missions"
                        } else {
                            "Focale  ↗"
                        },
                        self.isolate,
                    )
                    .clicked()
                    {
                        self.isolate = !self.isolate;
                        self.focus_compact = self.isolate;
                    }
                });
            }
        });
        ui.add_space(if compact { 12.0 } else { 16.0 });
        if let Some(id) = &self.prepared_selection {
            if scene.courants.iter().any(|c| &c.tache == id) {
                self.selection = self.prepared_selection.take();
            } else {
                self.missions.select(Some(id));
                ui.label("Plan confirmé. Mise à jour de l'espace de supervision…");
                if let Some(error) = self.missions.error() {
                    ui.label(error);
                }
                return;
            }
        }
        let query = self.query.trim().to_lowercase();
        let visibles: Vec<_> = scene
            .courants
            .iter()
            .filter(|c| {
                self.filtre.inclut(c)
                    && (query.is_empty()
                        || c.intitule.to_lowercase().contains(&query)
                        || c.agent.to_lowercase().contains(&query)
                        || c.tache.to_lowercase().contains(&query))
            })
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
            egui::ScrollArea::vertical()
                .id_salt("supervision-vide")
                .show(ui, |ui| crate::desk::empty(ui, compact));
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
                            self.isolate = false;
                        }
                        ui.add_space(8.0);
                        if let Some(c) = courant {
                            plaque(ui, 18, |ui| {
                                ui.set_width(ui.available_width());
                                self.inspecteur(ui, c, scene);
                            });
                        }
                    } else {
                        let precedente = self.selection.clone();
                        plaque(ui, 6, |ui| {
                            ui.set_width(ui.available_width());
                            self.liste(ui, &visibles, f32::INFINITY);
                        });
                        if self.selection != precedente {
                            self.focus_compact = true;
                        }
                    }
                });
            return;
        }
        // Sur un grand écran, la liste tient à gauche et l'espace de mission à droite, avec
        // de l'air entre les deux ; la Focale retire la liste pour lire un plan ou comparer
        // des versions sur toute la largeur.
        let available = ui.available_width();
        let show_list = !self.isolate && scene.courants.len() > 1;
        let list_width = if show_list {
            (available * 0.30).clamp(300.0, 420.0)
        } else {
            0.0
        };
        let gap = if show_list { 32.0 } else { 0.0 };
        let detail_width = available - list_width - gap;
        let height = ui.available_height();
        ui.horizontal_top(|ui| {
            if show_list {
                ui.allocate_ui_with_layout(
                    vec2(list_width, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        let count = visibles.len();
                        hud::etiquette(
                            ui,
                            format!(
                                "{count} MISSION{} DANS CETTE VUE",
                                if count == 1 { "" } else { "S" }
                            ),
                            EFFACE,
                        );
                        ui.add_space(6.0);
                        plaque(ui, 6, |ui| {
                            ui.set_width(ui.available_width());
                            self.liste(ui, &visibles, height - 56.0);
                        });
                    },
                );
                ui.add_space(gap);
            }
            ui.allocate_ui_with_layout(
                vec2(detail_width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("contexte")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if let Some(c) = courant {
                                plaque(ui, 22, |ui| {
                                    ui.set_width(detail_width - 44.0);
                                    self.inspecteur(ui, c, scene);
                                });
                            } else {
                                ui.label("Aucune mission dans cette vue.");
                            }
                        });
                },
            );
        });
    }

    fn liste(&mut self, ui: &mut egui::Ui, courants: &[&Courant], height: f32) {
        if courants.is_empty() {
            ui.label("Aucune mission dans cette vue.");
        }
        if let Some(picked) = crate::desk::liste(ui, courants, self.selection.as_deref(), height) {
            self.selection = Some(picked);
            self.focus_compact = true;
        }
    }

    fn inspecteur(&mut self, ui: &mut egui::Ui, c: &Courant, scene: &Scene) {
        let accent = Accent::de(ui.ctx());
        if self.missions.connected() {
            crate::mission_details::draw(ui, c, &mut self.missions, &mut self.detail_tab);
            if let Some(d) = scene.decision.as_ref().filter(|d| d.tache == c.tache) {
                ui.add_space(12.0);
                ui.label(titre(&d.question, 20.0));
                if action(ui, "inspecter-action", "Lire les conséquences").clicked() {
                    self.examen = empreinte_decision(scene);
                }
            }
            return;
        }
        let (label, color) = statut_mission(c, &accent);
        let wide = ui.available_width() > 620.0;
        ui.horizontal(|ui| {
            etiquette(ui, "MISSION EN FOCALE");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if bouton(ui, "copier-reference", "Copier la référence", false).clicked() {
                    ui.ctx().copy_text(c.tache.clone());
                }
            });
        });
        ui.add_space(14.0);
        // Le cadran à droite, le titre à gauche : la mission se lit d'un seul regard.
        let cadran = if wide { 96.0 } else { 0.0 };
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                vec2(ui.available_width() - cadran * 2.0 - 24.0, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.label(titre(&c.intitule, if wide { 34.0 } else { 26.0 }).line_height(Some(if wide { 40.0 } else { 31.0 })));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
                        crate::instruments::monogramme(ui.painter(), r.center(), &c.agent, 18.0);
                        ui.label(RichText::new(&c.agent).size(13.0).color(DISCRET));
                        pastille(ui, label, color);
                    });
                    ui.add_space(20.0);
                    Frame::new()
                        .fill(if c.reclame() { ATTENTE_VOILE } else { VERRE_HAUT })
                        .stroke(Stroke::new(1.0, if c.reclame() { hud::voile(ATTENTE, 150) } else { accent.fil }))
                        .corner_radius(3)
                        .inner_margin(18)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            hud::etiquette(ui, if c.reclame() { "VOTRE ATTENTION" } else { "POINT DE SUPERVISION" }, if c.reclame() { ATTENTE } else { accent.sourd });
                            ui.add_space(4.0);
                            if let Some(d) = scene.decision.as_ref().filter(|d| d.tache == c.tache) {
                                ui.label(titre(&d.question, 20.0));
                                ui.add_space(6.0);
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
                },
            );
            if wide {
                let (r, _) = ui.allocate_exact_size(vec2(cadran * 2.0 + 24.0, cadran * 2.0 + 44.0), egui::Sense::hover());
                hud::cadran(ui.painter(), pos2(r.center().x + 12.0, r.top() + cadran + 8.0), cadran, c.etapes, c.budget_consomme, color, &accent);
            }
        });
        ui.add_space(24.0);
        ui.separator();
        ui.add_space(18.0);
        crate::instruments::tableau(ui, c.etapes, c.debit, c.budget_consomme, color, false);
        ui.add_space(24.0);
        etiquette(ui, "RÉSULTATS ET CHANGEMENTS");
        ui.label(
            RichText::new("Aucun livrable ni diff reçu.")
                .size(13.0)
                .color(DISCRET),
        );
        ui.add_space(6.0);
        ui.label(RichText::new(&c.tache).size(11.0).color(EFFACE));
    }

    fn decision(&mut self, ctx: &egui::Context, scene: &Scene, reponse: &mut Option<Reponse>) {
        let Some(d) = &scene.decision else {
            return;
        };
        let id = egui::Id::new("decision");
        egui::Modal::new(id)
            .area(egui::Modal::default_area(id).fade_in(false))
            .backdrop_color(VOILE)
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(10, 12, 16))
                    .stroke(Stroke::new(1.0, hud::voile(ATTENTE, 160)))
                    .corner_radius(4)
                    .inner_margin(32),
            )
            .show(ctx, |ui| {
                ui.set_max_width(600.0);
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot.center(), 3.0, ATTENTE);
                    hud::etiquette(ui, "DÉCISION HUMAINE", ATTENTE);
                });
                ui.add_space(14.0);
                ui.label(titre(&d.question, 30.0).line_height(Some(36.0)));
                ui.add_space(14.0);
                ui.label(RichText::new(&d.consequence).size(15.0).color(ENCRE));
                if d.irreversible {
                    ui.add_space(4.0);
                    hud::etiquette(ui, "ACTION IRRÉVERSIBLE", ATTENTE);
                }
                ui.add_space(10.0);
                petit(
                    ui,
                    format!("Mission {} · attente {} s", d.tache, d.depuis_secondes),
                );
                ui.add_space(22.0);
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
                // Les crochets sur ce que le panneau occupe réellement, une fois composé.
                hud::crochets(ui.painter(), ui.min_rect().expand(33.0), ATTENTE, 16.0);
            });
    }

    fn systeme(&mut self, ui: &mut egui::Ui, scene: &Scene) {
        let accent = Accent::de(ui.ctx());
        etiquette(ui, "SYSTÈME");
        ui.label(titre("Le cadre d'exécution.", 40.0));
        ui.add_space(26.0);
        egui::ScrollArea::vertical()
            .id_salt("systeme")
            .show(ui, |ui| {
                plaque(ui, 28, |ui| {
                    ui.set_width(ui.available_width());
                    etiquette(ui, "ISOLATION DISPONIBLE");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(120.0, 120.0), egui::Sense::hover());
                        hud::jauge(
                            ui.painter(),
                            r.center(),
                            56.0,
                            f32::from(scene.isolation.niveau_max) / 2.0,
                            accent.vif,
                            &accent,
                            (&scene.isolation.niveau_max.to_string(), "NIVEAU"),
                        );
                        ui.add_space(12.0);
                        ui.vertical(|ui| {
                            ui.label(titre(format!("Niveau {} sur 2", scene.isolation.niveau_max), 26.0));
                            if let Some(manque) = &scene.isolation.manque {
                                ui.label(RichText::new(manque).color(DISCRET));
                            }
                            ui.add_space(8.0);
                            petit(
                                ui,
                                "Cette capacité annoncée ne prouve pas le confinement de chaque mission.",
                            );
                        });
                    });
                });
                ui.add_space(22.0);
                plaque(ui, 28, |ui| {
                    ui.set_width(ui.available_width());
                    etiquette(ui, "APPARENCE");
                    ui.add_space(4.0);
                    ui.label(titre("La couleur de ce qui signale.", 22.0));
                    ui.label(
                        RichText::new(
                            "L'accent colore le champ, les fils et ce qui est actif. La nuit, l'encre et l'alerte ne changent pas.",
                        )
                        .size(12.0)
                        .color(DISCRET),
                    );
                    ui.add_space(14.0);
                    let mut chosen = None;
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(18.0, 10.0);
                        for candidate in ACCENTS {
                            let selected = candidate.nom == accent.nom;
                            let (r, response) = ui.allocate_exact_size(vec2(96.0, 74.0), egui::Sense::click());
                            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                            response.widget_info(|| {
                                egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, candidate.titre)
                            });
                            ui.ctx().check_for_id_clash(egui::Id::new(format!("accent-{}", candidate.nom)), r, "accent");
                            let p = ui.painter();
                            let center = pos2(r.center().x, r.top() + 26.0);
                            if selected || response.hovered() {
                                hud::lueur(p, center, 14.0, &candidate);
                            }
                            p.circle_filled(center, 13.0, candidate.vif);
                            if selected {
                                p.circle_stroke(center, 19.0, Stroke::new(1.5, candidate.vif));
                                hud::graduations(p, center, 24.0, 24, 24, 2.5, (candidate.fil_vif, candidate.fil));
                            } else {
                                p.circle_stroke(center, 19.0, Stroke::new(1.0, candidate.fil));
                            }
                            hud::texte_espace(
                                p,
                                pos2(r.center().x, r.bottom() - 8.0),
                                egui::Align2::CENTER_CENTER,
                                &candidate.titre.to_uppercase(),
                                FontId::proportional(9.0),
                                if selected { candidate.vif } else { DISCRET },
                                1.6,
                            );
                            if response.has_focus() {
                                p.rect_stroke(r, 3, Stroke::new(1.0, candidate.fil_vif), egui::StrokeKind::Inside);
                            }
                            if response.clicked() {
                                chosen = Some(candidate);
                            }
                        }
                    });
                    if let Some(candidate) = chosen {
                        candidate.installer(ui.ctx());
                        installer_style(ui.ctx());
                        self.apparence = Some(match crate::theme::enregistrer_accent(candidate) {
                            Ok(chemin) => (format!("Accent {} conservé dans {}.", candidate.titre, chemin.display()), false),
                            Err(error) => (format!("Accent {} appliqué à cette session ; il n'a pas pu être conservé : {error}", candidate.titre), true),
                        });
                    }
                    if let Some((text, error)) = &self.apparence {
                        ui.add_space(6.0);
                        ui.label(RichText::new(text).size(11.0).color(if *error { ATTENTE } else { DISCRET }));
                    }
                    ui.add_space(6.0);
                    petit(ui, "Ligne de commande : --accent <nom> ; variable : PROPHET_SURFACE_ACCENT.");
                });
                ui.add_space(22.0);
                plaque(ui, 28, |ui| {
                    ui.set_width(ui.available_width());
                    etiquette(ui, "MISSIONS REÇUES");
                    ui.add_space(6.0);
                    if scene.courants.is_empty() {
                        ui.label("Aucune mission reçue.");
                    }
                    for c in &scene.courants {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(&c.intitule).size(14.0).color(ENCRE));
                            let (label, color) = statut_mission(c, &accent);
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
}

fn empreinte_decision(scene: &Scene) -> Option<String> {
    scene.decision.as_ref().map(|d| {
        format!(
            "{}\0{}\0{}\0{}",
            d.tache, d.question, d.consequence, d.irreversible
        )
    })
}

fn composer(ui: &mut egui::Ui, atelier: &mut Atelier) {
    let ctx = ui.ctx().clone();
    let accent = Accent::de(&ctx);
    if let Some(error) = &atelier.erreur {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(if atelier.modeles.is_empty() {
                    "Moteur local indisponible"
                } else {
                    error
                })
                .size(12.0)
                .color(ATTENTE),
            );
            if ui.link("Vérifier la connexion").clicked() {
                atelier.page = Page::Modeles;
            }
        });
    }
    plaque(ui, 18, |ui| {
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
                        .add_enabled_ui(enabled, |ui| {
                            hud::bouton(ui, "envoyer", "Envoyer ↑", true, &accent)
                        })
                        .inner
                        .clicked();
                    if clicked || (enabled && shortcut) {
                        atelier.envoyer(&ctx);
                    }
                }
            });
        });
    });
    ui.horizontal(|ui| {
        hud::etiquette(ui, "CONVERSATION LOCALE · EN MÉMOIRE", EFFACE);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            hud::etiquette(ui, "CTRL + ENTRÉE POUR ENVOYER", EFFACE);
        });
    });
}

fn conversation(ui: &mut egui::Ui, atelier: &mut Atelier) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            etiquette(ui, "DIALOGUE");
            ui.label(titre("Le dialogue, à votre rythme.", 32.0));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if bouton(ui, "nouveau-dialogue", "+ Nouveau", false).clicked() {
                atelier.nouvelle();
                atelier.page = Page::Conversation;
            }
        });
    });
    ui.add_space(16.0);
    egui::ScrollArea::vertical().id_salt("dialogue").stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
        if atelier.tours.is_empty() {
            plaque(ui, 28, |ui| {
                ui.set_width(ui.available_width());
                etiquette(ui, "AVANT DE DÉLÉGUER");
                ui.label(titre("Précisez le résultat attendu.", 26.0));
                ui.label(RichText::new("Explorez un objectif avec votre modèle local. Ce dialogue ne lance pas d'agent et n'accorde aucun droit système.").size(13.0).color(DISCRET));
                ui.add_space(14.0);
                if bouton(ui, "cadrer-objectif", "Structurer mon objectif", false).clicked() {
                    atelier.brouillon = "Aide-moi à préciser cet objectif, ses contraintes et les critères qui permettront de vérifier le résultat : ".into();
                    ui.ctx().memory_mut(|m| m.request_focus(egui::Id::new("intention")));
                }
            });
        }
        for (i, tour) in atelier.tours.iter().enumerate() {
            ui.push_id(i, |ui| {
                Frame::new().fill(VERRE_HAUT).stroke(Stroke::new(1.0, TRAIT)).corner_radius(3).inner_margin(18).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    hud::etiquette(ui, "VOUS", EFFACE);
                    ui.label(RichText::new(&tour.demande).size(15.0));
                });
                ui.add_space(8.0);
                plaque(ui, 22, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        etiquette(ui, tour.modele.to_uppercase());
                        if !tour.reponse.is_empty() && ui.small_button("Copier").clicked() { ui.ctx().copy_text(tour.reponse.clone()); }
                    });
                    ui.add_space(10.0);
                    if tour.reponse.is_empty() && tour.erreur.is_none() { petit(ui, "Le modèle prépare sa réponse…"); }
                    else { ui.label(RichText::new(&tour.reponse).size(15.0).line_height(Some(24.0))); }
                    if let Some(error) = &tour.erreur { ui.label(RichText::new(error).color(ATTENTE)); }
                    if let Some(mesure) = &tour.mesure {
                        ui.add_space(12.0);
                        petit(ui, format!("{} tokens · {:.2} s · premier texte {} ms", mesure.usage.tokens_out, mesure.elapsed.as_secs_f64(), mesure.first_token.unwrap_or_default().as_millis()));
                    }
                });
                ui.add_space(18.0);
            });
        }
    });
}

fn modeles(ui: &mut egui::Ui, atelier: &mut Atelier) {
    let accent = Accent::de(ui.ctx());
    etiquette(ui, "MODÈLES");
    ui.label(titre("L'intelligence sur votre machine.", 40.0));
    hud::etiquette(ui, "MODÈLES RÉELLEMENT EXPOSÉS PAR LE MOTEUR LOCAL", EFFACE);
    ui.add_space(26.0);
    egui::ScrollArea::vertical()
        .id_salt("bibliotheque")
        .show(ui, |ui| {
            plaque(ui, 28, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(&atelier.endpoint)
                            .monospace()
                            .size(12.0)
                            .color(DISCRET),
                    );
                    if ui
                        .add_enabled_ui(!atelier.decouverte, |ui| {
                            hud::bouton(ui, "modeles-actualiser", "Actualiser", false, &accent)
                        })
                        .inner
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
                        ui.label(RichText::new(error).size(12.0).color(ATTENTE));
                    });
                }
                if atelier.modeles.is_empty() {
                    ui.add_space(28.0);
                    ui.label(titre(
                        if atelier.decouverte {
                            "Recherche en cours…"
                        } else {
                            "Aucun modèle connecté."
                        },
                        30.0,
                    ));
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
            ui.add_space(18.0);
            for model in &atelier.modeles {
                plaque(ui, 24, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(titre(model, 24.0));
                            pastille(ui, "Disponible pour dialoguer", ACCOMPLI);
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let selected = atelier.choisi == *model;
                            if hud::bouton(
                                ui,
                                &format!("modele-choisir-{model}"),
                                if selected { "Sélectionné" } else { "Choisir" },
                                selected,
                                &accent,
                            )
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
