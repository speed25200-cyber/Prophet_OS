//! Espace de travail natif : saisie, sélection, conversation en flux et activité réelle.

use egui::{Align2, Color32, FontId, Frame, RichText, Stroke, pos2, vec2};

use crate::atelier::{Atelier, Page};
use crate::fenetre::Reponse;
use crate::gpu::{Cible, Contexte, FORMAT};
use crate::iris::{Icon, Iris, glow, icon};
use crate::scene::{Etat, Scene};

const FOND: Color32 = Color32::from_rgb(10, 12, 20);
const PANNEAU: Color32 = Color32::from_rgb(21, 24, 37);
const TRAIT: Color32 = Color32::from_rgb(43, 48, 67);
const TEXTE: Color32 = Color32::from_rgb(240, 242, 250);
const DISCRET: Color32 = Color32::from_rgb(150, 159, 181);
const ACCENT: Color32 = Color32::from_rgb(194, 179, 255);
const VERT: Color32 = Color32::from_rgb(129, 226, 189);

/// Dessin et contrôleur de l'interface interactive, aussi utilisables hors écran.
pub struct Bureau {
    /// Contexte de saisie et d'accessibilité partagé avec winit.
    pub ctx: egui::Context,
    /// Conversation et connexion au moteur.
    pub atelier: Atelier,
    rendu: egui_wgpu::Renderer,
    iris: Iris,
}

impl Bureau {
    /// Installe le thème et le renderer sans ouvrir de connexion au moteur.
    #[must_use]
    pub fn nouveau(contexte: &Contexte, endpoint: String, demonstration: bool) -> Self {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "Inter".into(),
            egui::FontData::from_static(include_bytes!("../assets/InterVariable.ttf")).into(),
        );
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .expect("famille proportionnelle")
            .insert(0, "Inter".into());
        ctx.set_fonts(fonts);
        ctx.set_theme(egui::Theme::Dark);
        let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.override_text_color = Some(TEXTE);
        style.visuals.panel_fill = FOND;
        style.visuals.window_fill = PANNEAU;
        style.visuals.extreme_bg_color = FOND;
        style.visuals.selection.bg_fill = Color32::from_rgb(62, 51, 105);
        style.visuals.selection.stroke = Stroke::new(1.0, ACCENT);
        style.visuals.widgets.inactive.bg_fill = PANNEAU;
        style.visuals.widgets.inactive.weak_bg_fill = PANNEAU;
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, TRAIT);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(39, 40, 63);
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(39, 40, 63);
        style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
        style.visuals.widgets.inactive.corner_radius = 10.into();
        style.visuals.widgets.hovered.corner_radius = 10.into();
        style.visuals.widgets.active.corner_radius = 10.into();
        style.visuals.widgets.noninteractive.bg_stroke = Stroke::NONE;
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(62, 51, 105);
        style.visuals.faint_bg_color = Color32::from_rgb(24, 27, 42);
        style.spacing.item_spacing = vec2(12.0, 12.0);
        style.spacing.button_padding = vec2(16.0, 11.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(16.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(15.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, FontId::proportional(30.0));
        ctx.set_style_of(egui::Theme::Dark, style);
        Self {
            ctx,
            atelier: Atelier::nouveau(endpoint, demonstration),
            iris: Iris::default(),
            rendu: egui_wgpu::Renderer::new(
                &contexte.device,
                contexte
                    .configuration_surface
                    .as_ref()
                    .map_or(FORMAT, |config| config.format),
                Default::default(),
            ),
        }
    }

    /// Fige les transitions d'apparition pour les captures à un instant constant.
    pub fn figer_transitions(&self) {
        self.ctx.all_styles_mut(|style| style.animation_time = 0.0);
    }

    /// Prépare les widgets et retourne une éventuelle décision humaine.
    pub fn composer(
        &mut self,
        input: egui::RawInput,
        scene: &Scene,
    ) -> (egui::FullOutput, Option<Reponse>) {
        self.atelier.actualiser();
        let mut decision = None;
        let atelier = &mut self.atelier;
        let iris = &self.iris;
        let output = self.ctx.run_ui(input, |root| {
            dessiner(root, atelier, scene, &mut decision, iris);
        });
        (output, decision)
    }

    /// Soumet les formes de l'interface au GPU, avec des ressources de texte réutilisées.
    pub fn rendre(&mut self, contexte: &Contexte, cible: &Cible, output: &mut egui::FullOutput) {
        let device = &contexte.device;
        let queue = &contexte.queue;
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                self.rendu.update_texture(device, queue, *id, delta);
            }
        }
        let jobs = self
            .ctx
            .tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [cible.largeur, cible.hauteur],
            pixels_per_point: output.pixels_per_point,
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("bureau"),
        });
        let callbacks = self
            .rendu
            .update_buffers(device, queue, &mut encoder, &jobs, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bureau"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &cible.vue,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.rendu
                .render(&mut pass.forget_lifetime(), &jobs, &screen);
        }
        queue.submit(callbacks.into_iter().chain([encoder.finish()]));
        for id in &output.textures_delta.free {
            self.rendu.free_texture(id);
        }
        output.textures_delta.clear();
    }
}

fn carte() -> Frame {
    Frame::new()
        .fill(PANNEAU)
        .stroke(Stroke::new(1.0, TRAIT))
        .corner_radius(20)
        .inner_margin(22)
}

fn navigation(ui: &mut egui::Ui, page: Page, active: bool, compact: bool) -> egui::Response {
    let (id, label, glyph) = match page {
        Page::Accueil => ("nav-accueil", "Espace", Icon::Home),
        Page::Conversation => ("nav-conversation", "Conversation", Icon::Chat),
        Page::Modeles => ("nav-modeles", "Modèles", Icon::Models),
        Page::Activite => ("nav-activite", "Activité", Icon::Activity),
    };
    let width = if compact {
        if page == Page::Conversation {
            137.0
        } else {
            103.0
        }
    } else {
        ui.available_width()
    };
    let (_, rect) = ui.allocate_space(vec2(width, if compact { 34.0 } else { 46.0 }));
    let response = ui.interact(rect, egui::Id::new(id), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, active, label)
    });
    let tint = if active {
        ACCENT
    } else if response.hovered() {
        TEXTE
    } else {
        DISCRET
    };
    if active || response.hovered() || response.has_focus() {
        ui.painter().rect_filled(
            rect,
            11,
            if active {
                Color32::from_rgb(35, 31, 57)
            } else {
                PANNEAU
            },
        );
        if response.has_focus() {
            ui.painter()
                .rect_stroke(rect, 11, Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside);
        }
    }
    icon(
        ui.painter(),
        pos2(rect.left() + 23.0, rect.center().y),
        glyph,
        17.0,
        tint,
    );
    ui.painter().text(
        pos2(rect.left() + 44.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(if compact { 12.0 } else { 14.0 }),
        tint,
    );
    if active && !compact {
        ui.painter()
            .circle_filled(pos2(rect.right() - 14.0, rect.center().y), 2.5, ACCENT);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn largeur(ui: &mut egui::Ui, maximum: f32, contents: impl FnOnce(&mut egui::Ui)) {
    let available = ui.available_rect_before_wrap();
    let width = available.width().min(maximum);
    let rect = egui::Rect::from_min_size(
        pos2(available.center().x - width * 0.5, available.top()),
        vec2(width, available.height()),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.set_width(width);
        contents(ui);
    });
}

fn dessiner(
    root: &mut egui::Ui,
    atelier: &mut Atelier,
    scene: &Scene,
    decision: &mut Option<Reponse>,
    iris: &Iris,
) {
    let ctx = root.ctx().clone();
    let etroit = root.available_width() < 900.0;
    if !etroit {
        let screen = root.max_rect();
        let painter = root.painter_at(screen);
        painter.rect_filled(screen, 0, FOND);
        glow(
            &painter,
            pos2(screen.right() * 0.79, screen.height() * 0.31),
            vec2(screen.width() * 0.44, screen.height() * 0.59),
            [90, 59, 175],
            95,
        );
        glow(
            &painter,
            pos2(screen.width() * 0.12, screen.height() * 0.85),
            vec2(screen.width() * 0.50, screen.height() * 0.46),
            [26, 96, 131],
            55,
        );
        for band in 0..4 {
            let points = (0..=100)
                .map(|n| {
                    let x = n as f32 / 100.0;
                    pos2(
                        screen.left() + x * screen.width(),
                        screen.top()
                            + screen.height() * (0.72 + 0.16 * (x * 3.8 + 0.35).sin())
                            + band as f32 * 5.0,
                    )
                })
                .collect();
            painter.add(egui::Shape::line(
                points,
                Stroke::new(0.6, Color32::from_rgba_unmultiplied(118, 143, 220, 13)),
            ));
        }
        egui::Panel::top("entete")
            .exact_size(84.0)
            .frame(Frame::new().inner_margin(26))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    let (logo, _) = ui.allocate_exact_size(vec2(168.0, 30.0), egui::Sense::hover());
                    icon(
                        ui.painter(),
                        pos2(logo.left() + 14.0, logo.center().y),
                        Icon::Spark,
                        26.0,
                        ACCENT,
                    );
                    ui.painter().text(
                        pos2(logo.left() + 40.0, logo.center().y - 1.0),
                        Align2::LEFT_CENTER,
                        "prophet",
                        FontId::proportional(26.0),
                        TEXTE,
                    );
                    ui.label(
                        RichText::new(match atelier.page {
                            Page::Accueil => "ESPACE PERSONNEL",
                            Page::Conversation => "CONVERSATION",
                            Page::Modeles => "BIBLIOTHÈQUE",
                            Page::Activite => "ACTIVITÉ",
                        })
                        .size(10.0)
                        .color(DISCRET),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} UTC", scene.heure))
                                .size(11.0)
                                .color(DISCRET),
                        );
                        if atelier.demonstration {
                            ui.label(RichText::new("DÉMONSTRATION").size(10.0).color(ACCENT));
                        } else {
                            ui.label(
                                RichText::new(if atelier.decouverte {
                                    "Connexion en cours"
                                } else if atelier.modeles.is_empty() {
                                    "Moteur à connecter"
                                } else {
                                    "Moteur local connecté"
                                })
                                .size(11.0)
                                .color(
                                    if atelier.modeles.is_empty() {
                                        DISCRET
                                    } else {
                                        VERT
                                    },
                                ),
                            );
                        }
                    });
                });
            });
        egui::Panel::bottom("dock")
            .exact_size(96.0)
            .resizable(false)
            .frame(Frame::new().inner_margin(16))
            .show(root, |ui| {
                largeur(ui, 576.0, |ui| {
                    Frame::new()
                        .fill(Color32::from_rgba_unmultiplied(24, 28, 44, 235))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(58, 61, 82)))
                        .shadow(egui::Shadow {
                            offset: [0, 8],
                            blur: 32,
                            spread: 0,
                            color: Color32::from_black_alpha(95),
                        })
                        .corner_radius(20)
                        .inner_margin(12)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 5.0;
                                for page in [
                                    Page::Accueil,
                                    Page::Conversation,
                                    Page::Modeles,
                                    Page::Activite,
                                ] {
                                    if navigation(ui, page, atelier.page == page, true).clicked() {
                                        atelier.page = page;
                                    }
                                }
                                ui.add_space(10.0);
                                ui.checkbox(
                                    &mut atelier.mouvement_reduit,
                                    RichText::new("Mouvement\nréduit").size(10.0),
                                );
                            });
                        });
                });
            });
    } else {
        egui::Panel::top("navigation-compacte")
            .exact_size(94.0)
            .frame(Frame::new().fill(FOND).inner_margin(12))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("prophet").size(20.0).color(TEXTE));
                    if atelier.demonstration {
                        ui.label(RichText::new("DÉMONSTRATION").size(9.0).color(ACCENT));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.checkbox(
                            &mut atelier.mouvement_reduit,
                            RichText::new("Mouvement réduit").size(10.0),
                        );
                    });
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    for page in [
                        Page::Accueil,
                        Page::Conversation,
                        Page::Modeles,
                        Page::Activite,
                    ] {
                        if navigation(ui, page, atelier.page == page, true).clicked() {
                            atelier.page = page;
                        }
                    }
                    if ui.small_button("+ Nouveau").clicked() {
                        atelier.nouvelle();
                    }
                });
            });
    }
    if atelier.page == Page::Conversation {
        egui::Panel::bottom("composition")
            .resizable(false)
            .frame(Frame::new().inner_margin(if etroit { 16 } else { 24 }))
            .show(root, |ui| {
                largeur(ui, 940.0, |ui| composer(ui, atelier, &ctx));
            });
    }
    egui::CentralPanel::default()
        .frame(
            Frame::new()
                .fill(if etroit { FOND } else { Color32::TRANSPARENT })
                .inner_margin(if etroit { 16 } else { 28 }),
        )
        .show(root, |ui| match atelier.page {
            Page::Accueil => accueil(ui, atelier, scene, iris),
            Page::Conversation => largeur(ui, 940.0, |ui| conversation(ui, atelier)),
            Page::Modeles => largeur(ui, 1040.0, |ui| modeles(ui, atelier, &ctx)),
            Page::Activite => largeur(ui, 1040.0, |ui| activite(ui, scene)),
        });
    if let Some(d) = &scene.decision {
        let id = egui::Id::new("decision");
        egui::Modal::new(id)
            .area(egui::Modal::default_area(id).fade_in(false))
            .backdrop_color(Color32::from_black_alpha(190))
            .frame(carte().inner_margin(30))
            .show(&ctx, |ui| {
                ui.set_max_width(640.0);
                ui.label(RichText::new("VOTRE DÉCISION").size(11.0).color(ACCENT));
                ui.add_space(12.0);
                ui.heading(&d.question);
                ui.label(&d.consequence);
                if d.irreversible {
                    ui.colored_label(
                        Color32::from_rgb(245, 158, 148),
                        "Cette action est irréversible.",
                    );
                }
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if ui.button("Refuser").clicked() {
                        *decision = Some(Reponse::Refuse);
                    }
                    if ui.button("Autoriser cette action").clicked() {
                        *decision = Some(Reponse::Accepte);
                    }
                });
            });
    }
}

fn accueil(ui: &mut egui::Ui, atelier: &mut Atelier, scene: &Scene, iris: &Iris) {
    let height = ui.available_height();
    let petite = height < 500.0 && ui.available_width() < 700.0;
    let raccourcis_compacts = height < 640.0;
    let ctx = ui.ctx().clone();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            largeur(ui, 1040.0, |ui| {
                let width = ui.available_width();
                let compact = width < 700.0;
                ui.add_space(if height > 820.0 { 30.0 } else { 0.0 });
                let hero_height = if petite {
                    108.0
                } else if height < 640.0 {
                    180.0
                } else if height < 720.0 {
                    264.0
                } else {
                    322.0
                };
                let (rect, _) =
                    ui.allocate_exact_size(vec2(width, hero_height), egui::Sense::hover());
                let painter = ui.painter_at(rect);
                let sculpture = egui::Rect::from_center_size(
                    pos2(rect.right() - width * 0.23, rect.center().y),
                    vec2(width * 0.48, hero_height),
                );
                let time = if atelier.mouvement_reduit {
                    8.0
                } else {
                    ui.input(|i| i.time) as f32
                };
                iris.paint(&painter, sculpture, time);
                let font = if compact {
                    32.0
                } else if height < 640.0 {
                    40.0
                } else {
                    52.0
                };
                let y = if petite {
                    14.0
                } else if height < 640.0 {
                    32.0
                } else {
                    42.0
                };
                if !petite {
                    painter.text(
                        rect.min + vec2(0.0, y - 30.0),
                        Align2::LEFT_TOP,
                        "UNE NOUVELLE PERSPECTIVE",
                        FontId::proportional(10.0),
                        ACCENT,
                    );
                }
                painter.text(
                    rect.min + vec2(0.0, y),
                    Align2::LEFT_TOP,
                    "L'espace de vos",
                    FontId::proportional(font),
                    TEXTE,
                );
                painter.text(
                    rect.min + vec2(0.0, y + font * 1.18),
                    Align2::LEFT_TOP,
                    "prochaines idées.",
                    FontId::proportional(font),
                    ACCENT,
                );
                if !petite {
                    painter.text(
                        rect.min + vec2(0.0, y + font * 2.55),
                        Align2::LEFT_TOP,
                        "Pensez librement. Créez avec votre intelligence locale.",
                        FontId::proportional(if compact { 12.0 } else { 14.0 }),
                        DISCRET,
                    );
                }
                composer(ui, atelier, &ctx);
                ui.add_space(if raccourcis_compacts { 10.0 } else { 26.0 });
                if !raccourcis_compacts {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Un point de départ").size(13.0).color(TEXTE));
                        ui.label(
                            RichText::new("/  à vous la suite")
                                .size(12.0)
                                .color(DISCRET),
                        );
                    });
                    ui.add_space(4.0);
                }
                let suggestions = [
                    (
                        "Explorer une idée",
                        "Du premier déclic au plan d'action",
                        "Aide-moi à développer cette idée : ",
                        Icon::Spark,
                    ),
                    (
                        "Comprendre un sujet",
                        "Un autre regard sur les choses",
                        "Explique-moi simplement ce sujet : ",
                        Icon::Models,
                    ),
                    (
                        "Affiner un texte",
                        "Trouvez les mots justes",
                        "Aide-moi à améliorer ce texte : ",
                        Icon::Chat,
                    ),
                ];
                ui.columns(3, |columns| {
                    for (column, (title, detail, prompt, glyph)) in
                        columns.iter_mut().zip(suggestions)
                    {
                        let h = if raccourcis_compacts { 54.0 } else { 108.0 };
                        let (_, rect) = column.allocate_space(vec2(column.available_width(), h));
                        let response = column
                            .interact(rect, egui::Id::new(title), egui::Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title)
                        });
                        let p = column.painter();
                        p.rect_filled(
                            rect,
                            16,
                            if response.hovered() {
                                Color32::from_rgb(30, 33, 49)
                            } else {
                                Color32::from_rgb(17, 20, 31)
                            },
                        );
                        p.rect_stroke(
                            rect,
                            16,
                            Stroke::new(
                                1.0,
                                if response.has_focus() || response.hovered() {
                                    ACCENT
                                } else {
                                    Color32::from_rgb(35, 40, 56)
                                },
                            ),
                            egui::StrokeKind::Inside,
                        );
                        if !raccourcis_compacts {
                            icon(p, rect.min + vec2(23.0, 24.0), glyph, 19.0, ACCENT);
                        }
                        p.text(
                            rect.min + vec2(16.0, if raccourcis_compacts { 18.0 } else { 56.0 }),
                            Align2::LEFT_TOP,
                            title,
                            FontId::proportional(if compact { 11.0 } else { 14.0 }),
                            TEXTE,
                        );
                        if !compact && !raccourcis_compacts {
                            p.text(
                                rect.min + vec2(16.0, 83.0),
                                Align2::LEFT_TOP,
                                detail,
                                FontId::proportional(11.0),
                                DISCRET,
                            );
                        }
                        if !raccourcis_compacts {
                            icon(
                                p,
                                pos2(rect.right() - 22.0, rect.top() + 23.0),
                                Icon::Arrow,
                                12.0,
                                DISCRET,
                            );
                        }
                        if response.clicked() {
                            atelier.brouillon = prompt.to_owned();
                            ctx.memory_mut(|m| m.request_focus(egui::Id::new("intention")));
                        }
                    }
                });
                if height >= 760.0 {
                    ui.add_space(24.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} modèle(s) disponible(s)",
                                atelier.modeles.len()
                            ))
                            .size(11.0)
                            .color(if atelier.modeles.is_empty() {
                                DISCRET
                            } else {
                                VERT
                            }),
                        );
                        ui.label(RichText::new("·").color(TRAIT));
                        ui.label(
                            RichText::new(format!("{} tâche(s) reçue(s)", scene.courants.len()))
                                .size(11.0)
                                .color(DISCRET),
                        );
                        if ui
                            .link(
                                RichText::new("Ouvrir la bibliothèque")
                                    .size(11.0)
                                    .color(ACCENT),
                            )
                            .clicked()
                        {
                            atelier.page = Page::Modeles;
                        }
                    });
                }
            });
        });
}

fn composer(ui: &mut egui::Ui, atelier: &mut Atelier, ctx: &egui::Context) {
    let compact = ui.available_width() < 650.0;
    if let Some(error) = &atelier.erreur {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(if atelier.modeles.is_empty() {
                    "Le moteur local est indisponible."
                } else {
                    error
                })
                .size(12.0)
                .color(Color32::from_rgb(245, 173, 155)),
            );
            if atelier.modeles.is_empty()
                && ui
                    .link(
                        RichText::new("Vérifier la connexion")
                            .size(12.0)
                            .color(ACCENT),
                    )
                    .clicked()
            {
                atelier.page = Page::Modeles;
            }
        });
    }
    let available = ui.available_rect_before_wrap();
    glow(
        ui.painter(),
        pos2(available.center().x, available.top() + 64.0),
        vec2(available.width() * 0.60, 115.0),
        [104, 71, 226],
        23,
    );
    let focused = ctx.memory(|m| m.has_focus(egui::Id::new("intention")));
    carte()
        .inner_margin(if compact { 16 } else { 22 })
        .fill(Color32::from_rgb(22, 25, 39))
        .stroke(Stroke::new(
            1.0,
            if focused {
                ACCENT
            } else {
                Color32::from_rgb(65, 62, 92)
            },
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let response = ui.add(
                egui::TextEdit::multiline(&mut atelier.brouillon)
                    .id(egui::Id::new("intention"))
                    .hint_text("Une idée, une question, un nouveau départ…")
                    .font(FontId::proportional(if compact { 16.0 } else { 20.0 }))
                    .desired_width(f32::INFINITY)
                    .desired_rows(if atelier.page == Page::Accueil || compact {
                        1
                    } else {
                        2
                    })
                    .char_limit(16_384)
                    .frame(Frame::NONE),
            );
            let shortcut = response.has_focus()
                && ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Enter));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("choix-modele")
                    .selected_text(if atelier.choisi.is_empty() {
                        "Choisir un modèle"
                    } else {
                        &atelier.choisi
                    })
                    .width(if compact { 170.0 } else { 220.0 })
                    .show_ui(ui, |ui| {
                        for model in &atelier.modeles {
                            ui.selectable_value(&mut atelier.choisi, model.clone(), model);
                        }
                    });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if atelier.generation {
                        if ui.button("Interrompre  ■").clicked() {
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
                                egui::Button::new(
                                    RichText::new("Envoyer  ↗").size(13.0).color(FOND),
                                )
                                .fill(ACCENT)
                                .min_size(vec2(105.0, 38.0)),
                            )
                            .clicked();
                        if clicked || (enabled && shortcut) {
                            atelier.envoyer(ctx);
                        }
                    }
                });
            });
        });
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Local  ·  Conversation en mémoire")
                .size(if compact { 9.0 } else { 10.0 })
                .color(DISCRET),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new("Ctrl + Entrée pour envoyer")
                    .size(if compact { 9.0 } else { 10.0 })
                    .color(DISCRET),
            );
        });
    });
}

fn conversation(ui: &mut egui::Ui, atelier: &mut Atelier) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Conversation").size(32.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(RichText::new("+  Nouveau").size(12.0)).clicked() {
                atelier.nouvelle();
            }
        });
    });
    ui.label(
        RichText::new("Un échange avec un modèle sur votre machine.")
            .size(13.0)
            .color(DISCRET),
    );
    ui.add_space(24.0);
    if atelier.tours.is_empty() {
        carte().inner_margin(32).show(ui, |ui| {
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), 64.0), egui::Sense::hover());
            glow(
                ui.painter(),
                rect.center(),
                vec2(100.0, 60.0),
                [113, 83, 228],
                45,
            );
            icon(ui.painter(), rect.center(), Icon::Chat, 32.0, ACCENT);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Une question ouvre des possibilités.").size(23.0));
                ui.label(
                    RichText::new("Choisissez votre modèle et écrivez votre première demande.")
                        .size(13.0)
                        .color(DISCRET),
                );
            });
        });
        return;
    }
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (index, tour) in atelier.tours.iter().enumerate() {
                ui.push_id(index, |ui| {
                    Frame::new()
                        .fill(Color32::from_rgba_unmultiplied(35, 37, 55, 210))
                        .corner_radius(18)
                        .inner_margin(22)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new("VOUS").size(10.0).color(DISCRET));
                            ui.label(RichText::new(&tour.demande).size(17.0));
                        });
                    ui.add_space(12.0);
                    carte().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (rect, _) =
                                ui.allocate_exact_size(vec2(24.0, 24.0), egui::Sense::hover());
                            icon(ui.painter(), rect.center(), Icon::Spark, 20.0, ACCENT);
                            ui.label(RichText::new(&tour.modele).size(12.0).color(ACCENT));
                            if !tour.reponse.is_empty() && ui.small_button("Copier").clicked() {
                                ui.ctx().copy_text(tour.reponse.clone());
                            }
                        });
                        ui.add_space(10.0);
                        if tour.reponse.is_empty() && tour.erreur.is_none() {
                            ui.label(RichText::new("Le modèle prépare sa réponse…").color(DISCRET));
                        } else {
                            ui.label(
                                RichText::new(&tour.reponse)
                                    .size(16.0)
                                    .line_height(Some(25.0)),
                            );
                        }
                        if let Some(error) = &tour.erreur {
                            ui.add_space(8.0);
                            ui.colored_label(Color32::from_rgb(235, 149, 123), error);
                        }
                        if let Some(mesure) = &tour.mesure {
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(format!(
                                    "{} tokens  ·  {:.2} s  ·  premier texte {} ms",
                                    mesure.usage.tokens_out,
                                    mesure.elapsed.as_secs_f64(),
                                    mesure.first_token.unwrap_or_default().as_millis()
                                ))
                                .size(11.0)
                                .color(DISCRET),
                            );
                        }
                    });
                    ui.add_space(24.0);
                });
            }
        });
}

fn modeles(ui: &mut egui::Ui, atelier: &mut Atelier, ctx: &egui::Context) {
    ui.label(
        RichText::new("VOTRE INTELLIGENCE LOCALE")
            .size(10.0)
            .color(ACCENT),
    );
    ui.label(
        RichText::new("Bibliothèque de modèles").size(if ui.available_width() < 700.0 {
            28.0
        } else {
            40.0
        }),
    );
    ui.label(
        RichText::new("Choisissez l'intelligence qui accompagne votre prochaine idée.")
            .size(14.0)
            .color(DISCRET),
    );
    ui.add_space(16.0);
    ui.horizontal_wrapped(|ui| {
        ui.monospace(&atelier.endpoint);
        if ui
            .add_enabled(!atelier.decouverte, egui::Button::new("Actualiser"))
            .clicked()
        {
            atelier.decouvrir(ctx);
        }
        if atelier.decouverte {
            ui.spinner();
        }
    });
    if let Some(error) = &atelier.erreur {
        egui::CollapsingHeader::new(
            RichText::new("Détails du diagnostic")
                .size(12.0)
                .color(DISCRET),
        )
        .show(ui, |ui| {
            ui.label(
                RichText::new(error)
                    .size(12.0)
                    .color(Color32::from_rgb(235, 149, 123)),
            );
        });
    }
    ui.add_space(24.0);
    egui::ScrollArea::vertical().show(ui, |ui| {
        if atelier.modeles.is_empty() {
            carte().inner_margin(30).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 70.0), egui::Sense::hover());
                glow(ui.painter(), rect.center(), vec2(140.0, 60.0), [77, 119, 209], 45);
                icon(ui.painter(), rect.center(), Icon::Models, 36.0, ACCENT);
                ui.heading(if atelier.decouverte { "Recherche des modèles…" } else { "Aucun modèle disponible" });
                ui.label("Démarrez un moteur local compatible, puis actualisez la liste.");
                ui.label(RichText::new("Le téléchargement et le lancement des moteurs ne sont pas encore intégrés à cette version.").color(DISCRET));
            });
        }
        for model in &atelier.modeles {
            carte().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(model).size(23.0));
                        ui.label(RichText::new("Disponible pour la conversation").size(12.0).color(VERT));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let selected = atelier.choisi == *model;
                        if ui.button(if selected { "Sélectionné" } else { "Choisir" }).clicked() { atelier.choisi.clone_from(model); }
                    });
                });
            });
            ui.add_space(8.0);
        }
    });
}

fn activite(ui: &mut egui::Ui, scene: &Scene) {
    ui.label(
        RichText::new("UNE VUE SUR VOTRE SYSTÈME")
            .size(10.0)
            .color(ACCENT),
    );
    ui.label(
        RichText::new("L'activité, en perspective.").size(if ui.available_width() < 700.0 {
            28.0
        } else {
            40.0
        }),
    );
    ui.label(
        RichText::new("Tâches et isolation signalées par les services Prophet.").color(DISCRET),
    );
    ui.add_space(24.0);
    carte().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("ISOLATION").size(11.0).color(ACCENT));
        ui.label(format!(
            "Niveau maximal annoncé : {}",
            scene.isolation.niveau_max
        ));
        if let Some(manque) = &scene.isolation.manque {
            ui.label(RichText::new(manque).color(DISCRET));
        }
    });
    ui.add_space(20.0);
    egui::ScrollArea::vertical().show(ui, |ui| {
        if scene.courants.is_empty() {
            ui.label("Aucune tâche reçue des services.");
        }
        for courant in &scene.courants {
            carte().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(&courant.intitule).size(21.0));
                let etat = match courant.etat {
                    Etat::Court => "en cours",
                    Etat::Attend => "décision attendue",
                    Etat::Bloque => "empêchée",
                    Etat::Fini => "terminée",
                };
                ui.label(
                    RichText::new(format!(
                        "{}  ·  {}  ·  {} étapes",
                        courant.agent, etat, courant.etapes
                    ))
                    .color(DISCRET),
                );
                ui.add(
                    egui::ProgressBar::new(courant.budget_consomme.clamp(0.0, 1.0))
                        .text("Budget consommé"),
                );
            });
            ui.add_space(8.0);
        }
    });
}
