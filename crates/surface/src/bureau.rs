//! Espace de travail natif : saisie, sélection, conversation en flux et activité réelle.

use egui::{Align2, Color32, FontId, Frame, RichText, Stroke, Vec2, pos2, vec2};

use crate::atelier::{Atelier, Page};
use crate::fenetre::Reponse;
use crate::gpu::{Cible, Contexte, FORMAT};
use crate::scene::{Etat, Scene};

const FOND: Color32 = Color32::from_rgb(12, 14, 17);
const PANNEAU: Color32 = Color32::from_rgb(20, 23, 27);
const TRAIT: Color32 = Color32::from_rgb(44, 47, 51);
const TEXTE: Color32 = Color32::from_rgb(238, 235, 226);
const DISCRET: Color32 = Color32::from_rgb(157, 162, 170);
const OR: Color32 = Color32::from_rgb(232, 193, 119);
const VERT: Color32 = Color32::from_rgb(151, 208, 175);

/// Dessin et contrôleur de l'interface interactive, aussi utilisables hors écran.
pub struct Bureau {
    /// Contexte de saisie et d'accessibilité partagé avec winit.
    pub ctx: egui::Context,
    /// Conversation et connexion au moteur.
    pub atelier: Atelier,
    rendu: egui_wgpu::Renderer,
}

impl Bureau {
    /// Installe le thème et le renderer sans ouvrir de connexion au moteur.
    #[must_use]
    pub fn nouveau(contexte: &Contexte, endpoint: String, demonstration: bool) -> Self {
        let ctx = egui::Context::default();
        ctx.set_theme(egui::Theme::Dark);
        let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.override_text_color = Some(TEXTE);
        style.visuals.panel_fill = FOND;
        style.visuals.window_fill = PANNEAU;
        style.visuals.extreme_bg_color = FOND;
        style.visuals.selection.bg_fill = Color32::from_rgb(92, 75, 43);
        style.visuals.selection.stroke = Stroke::new(1.0, OR);
        style.visuals.widgets.inactive.bg_fill = PANNEAU;
        style.visuals.widgets.inactive.weak_bg_fill = PANNEAU;
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, TRAIT);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(42, 40, 34);
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(42, 40, 34);
        style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, OR);
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
        let output = self.ctx.run_ui(input, |root| {
            dessiner(root, atelier, scene, &mut decision);
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
        .corner_radius(14)
        .inner_margin(22)
}

fn dessiner(
    root: &mut egui::Ui,
    atelier: &mut Atelier,
    scene: &Scene,
    decision: &mut Option<Reponse>,
) {
    let ctx = root.ctx().clone();
    let etroit = root.available_width() < 900.0;
    if !etroit {
        egui::Panel::left("navigation")
            .exact_size(222.0)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(16, 18, 21))
                    .inner_margin(20),
            )
            .show(root, |ui| {
                let compact_nav = ui.available_height() < 600.0;
                ui.add_space(15.0);
                ui.label(RichText::new("P R O P H E T").size(21.0).color(OR).strong());
                ui.label(
                    RichText::new("INTELLIGENCE LOCALE")
                        .size(10.0)
                        .color(DISCRET),
                );
                ui.add_space(if compact_nav { 16.0 } else { 42.0 });
                for (page, label) in [
                    (Page::Accueil, "01   Espace"),
                    (Page::Conversation, "02   Conversation"),
                    (Page::Modeles, "03   Modèles"),
                    (Page::Activite, "04   Activité"),
                ] {
                    let active = atelier.page == page;
                    let button = egui::Button::new(RichText::new(label).color(if active {
                        OR
                    } else {
                        DISCRET
                    }))
                    .selected(active)
                    .min_size(vec2(
                        ui.available_width(),
                        if compact_nav { 34.0 } else { 48.0 },
                    ));
                    if ui.add(button).clicked() {
                        atelier.page = page;
                    }
                }
                ui.add_space(24.0);
                if ui
                    .add_sized(
                        [ui.available_width(), 44.0],
                        egui::Button::new("+   Conversation"),
                    )
                    .clicked()
                {
                    atelier.nouvelle();
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new("PROPHET OS   /   0.1 DEV")
                            .size(10.0)
                            .color(DISCRET),
                    );
                    ui.add_space(8.0);
                    ui.checkbox(&mut atelier.mouvement_reduit, "Mouvement réduit");
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(if atelier.modeles.is_empty() {
                            if atelier.decouverte {
                                "Connexion en cours"
                            } else {
                                "Aucun modèle disponible"
                            }
                        } else {
                            "Moteur connecté"
                        })
                        .size(12.0)
                        .color(if atelier.modeles.is_empty() {
                            DISCRET
                        } else {
                            VERT
                        }),
                    );
                });
            });
        egui::Panel::top("entete")
            .exact_size(79.0)
            .frame(Frame::new().fill(FOND).inner_margin(24))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("ESPACE DE TRAVAIL").size(12.0).color(DISCRET));
                    ui.label(RichText::new(" / ").color(TRAIT));
                    ui.label(
                        RichText::new(match atelier.page {
                            Page::Accueil => "VUE D'ENSEMBLE",
                            Page::Conversation => "CONVERSATION",
                            Page::Modeles => "MODÈLES",
                            Page::Activite => "ACTIVITÉ",
                        })
                        .size(12.0)
                        .color(OR),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(format!("{} UTC", scene.heure)).size(16.0));
                        if atelier.demonstration {
                            ui.colored_label(OR, "DÉMONSTRATION");
                        }
                    });
                });
            });
    } else {
        egui::Panel::top("navigation-compacte")
            .exact_size(100.0)
            .frame(Frame::new().fill(PANNEAU).inner_margin(14))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("PROPHET").size(18.0).color(OR));
                    if atelier.demonstration {
                        ui.label(RichText::new("DÉMONSTRATION").size(11.0).color(DISCRET));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(format!("{} UTC", scene.heure)).size(12.0));
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.spacing_mut().button_padding = vec2(10.0, 7.0);
                    for (page, label) in [
                        (Page::Accueil, "Espace"),
                        (Page::Conversation, "Chat"),
                        (Page::Modeles, "Modèles"),
                        (Page::Activite, "Activité"),
                    ] {
                        if ui
                            .add(egui::Button::new(label).selected(atelier.page == page))
                            .clicked()
                        {
                            atelier.page = page;
                        }
                    }
                    if ui.button("+ Nouveau").clicked() {
                        atelier.nouvelle();
                    }
                    ui.checkbox(&mut atelier.mouvement_reduit, "Mouvement réduit");
                });
            });
    }
    if matches!(atelier.page, Page::Accueil | Page::Conversation) {
        egui::Panel::bottom("composition")
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(FOND)
                    .inner_margin(if etroit { 18 } else { 30 }),
            )
            .show(root, |ui| {
                composer(ui, atelier, &ctx);
            });
    }
    egui::CentralPanel::default()
        .frame(
            Frame::new()
                .fill(FOND)
                .inner_margin(if etroit { 18 } else { 30 }),
        )
        .show(root, |ui| match atelier.page {
            Page::Accueil => accueil(ui, atelier, scene),
            Page::Conversation => conversation(ui, atelier),
            Page::Modeles => modeles(ui, atelier, &ctx),
            Page::Activite => activite(ui, scene),
        });
    if let Some(d) = &scene.decision {
        let id = egui::Id::new("decision");
        egui::Modal::new(id)
            .area(egui::Modal::default_area(id).fade_in(false))
            .backdrop_color(Color32::from_black_alpha(170))
            .frame(carte())
            .show(&ctx, |ui| {
                ui.set_max_width(640.0);
                ui.label(RichText::new("VOTRE DÉCISION").size(12.0).color(OR));
                ui.add_space(12.0);
                ui.heading(&d.question);
                ui.label(&d.consequence);
                if d.irreversible {
                    ui.colored_label(
                        Color32::from_rgb(235, 149, 123),
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

fn accueil(ui: &mut egui::Ui, atelier: &mut Atelier, scene: &Scene) {
    let compact = ui.available_height() < 500.0;
    let tres_petit = ui.available_height() < 260.0 && ui.available_width() < 650.0;
    egui::ScrollArea::vertical().show(ui, |ui| {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(vec2(width, if tres_petit {132.0} else if compact { 190.0 } else { 300.0 }), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.text(rect.min + vec2(0.0, 14.0), Align2::LEFT_TOP, "VOTRE ESPACE. VOTRE INTELLIGENCE.", FontId::proportional(11.0), OR);
        let font = if tres_petit {26.0} else if width < 650.0 { 32.0 } else if compact { 36.0 } else { 51.0 };
        let title_y = if tres_petit {32.0} else if compact { 40.0 } else { 51.0 };
        painter.text(rect.min + vec2(0.0, title_y), Align2::LEFT_TOP, "L'idée vous appartient.", FontId::proportional(font), TEXTE);
        painter.text(rect.min + vec2(0.0, title_y + font * 1.23), Align2::LEFT_TOP, "L'intelligence aussi.", FontId::proportional(font), OR);
        painter.text(rect.min + vec2(0.0, rect.height()-if tres_petit {24.0} else if compact { 48.0 } else { 72.0 }), Align2::LEFT_TOP, if tres_petit {"Un modèle local. Votre prochaine idée."} else {"Pensez, explorez, créez avec un modèle sur votre machine.\nVos conversations restent auprès de vous."}, FontId::proportional(if compact {13.0} else {15.0}), DISCRET);
        if width > 960.0 {
            let time = if atelier.mouvement_reduit { 0.0 } else { ui.input(|i| i.time) as f32 };
            orbite(&painter, pos2(rect.right()-150.0, rect.center().y), time, if compact { 0.72 } else { 1.0 });
        }
        if tres_petit { return; }
        ui.add_space(12.0);
        ui.columns(3, |columns| {
            for (column, (value, title, detail)) in columns.iter_mut().zip([
                (atelier.modeles.len().to_string(), "MODÈLES", "Disponibles sur le moteur"),
                (scene.courants.len().to_string(), "TÂCHES REÇUES", "Signalées par les services"),
                (atelier.tours.len().to_string(), "ÉCHANGES", "Dans cette conversation"),
            ]) {
                carte().inner_margin(if compact {14} else {22}).show(column, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(title).size(10.0).color(DISCRET));
                    ui.label(RichText::new(value).size(34.0).color(TEXTE));
                    ui.label(RichText::new(detail).size(12.0).color(DISCRET));
                });
            }
        });
        ui.add_space(26.0);
        ui.label(RichText::new("PAR OÙ COMMENCER ?").size(11.0).color(DISCRET));
        ui.horizontal_wrapped(|ui| {
            for (title, prompt) in [
                ("Explorer une idée  ↗", "Aide-moi à développer cette idée : "),
                ("Comprendre un sujet  ↗", "Explique-moi simplement ce sujet : "),
                ("Améliorer un texte  ↗", "Aide-moi à améliorer ce texte : "),
            ] {
                if ui.button(title).clicked() { atelier.brouillon = prompt.to_owned(); }
            }
        });
        if atelier.modeles.is_empty() {
            ui.add_space(14.0);
            ui.label(RichText::new("Connectez votre moteur local dans Modèles pour commencer.").color(DISCRET));
        }
    });
}

fn composer(ui: &mut egui::Ui, atelier: &mut Atelier, ctx: &egui::Context) {
    let compact = ui.available_width() < 650.0;
    if let Some(error) = &atelier.erreur {
        ui.colored_label(Color32::from_rgb(235, 149, 123), error);
    }
    carte()
        .inner_margin(if compact { 14 } else { 22 })
        .stroke(Stroke::new(1.0, Color32::from_rgb(92, 77, 51)))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let response = ui.add(
                egui::TextEdit::multiline(&mut atelier.brouillon)
                    .hint_text("Qu'avez-vous en tête ?")
                    .desired_width(f32::INFINITY)
                    .desired_rows(if compact { 1 } else { 2 })
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
                    .width(180.0)
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
                                egui::Button::new(RichText::new("Envoyer").color(FOND)).fill(OR),
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
            RichText::new("CONVERSATION LOCALE  ·  SESSION EN MÉMOIRE")
                .size(10.0)
                .color(DISCRET),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new("Ctrl + Entrée pour envoyer")
                    .size(11.0)
                    .color(DISCRET),
            );
        });
    });
}

fn conversation(ui: &mut egui::Ui, atelier: &Atelier) {
    if atelier.tours.is_empty() {
        ui.add_space(35.0);
        ui.heading("Une conversation, des possibilités.");
        ui.label("Choisissez un modèle et écrivez votre première demande ci-dessous.");
        return;
    }
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (index, tour) in atelier.tours.iter().enumerate() {
                ui.push_id(index, |ui| {
                    ui.label(RichText::new("VOUS").size(11.0).color(DISCRET));
                    ui.label(RichText::new(&tour.demande).size(20.0));
                    ui.add_space(12.0);
                    carte().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&tour.modele).size(12.0).color(OR));
                            if !tour.reponse.is_empty() && ui.small_button("Copier").clicked() {
                                ui.ctx().copy_text(tour.reponse.clone());
                            }
                        });
                        ui.add_space(10.0);
                        if tour.reponse.is_empty() && tour.erreur.is_none() {
                            ui.label(RichText::new("Le modèle prépare sa réponse…").color(DISCRET));
                        } else {
                            ui.label(&tour.reponse);
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
    ui.heading("Votre bibliothèque locale");
    ui.label(RichText::new("Les modèles réellement disponibles sur votre moteur.").color(DISCRET));
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
        ui.colored_label(Color32::from_rgb(235, 149, 123), error);
    }
    ui.add_space(24.0);
    egui::ScrollArea::vertical().show(ui, |ui| {
        if atelier.modeles.is_empty() {
            carte().show(ui, |ui| {
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
    ui.heading("Ce qui se passe sur votre machine");
    ui.label(
        RichText::new("Tâches et isolation signalées par les services Prophet.").color(DISCRET),
    );
    ui.add_space(24.0);
    carte().show(ui, |ui| {
        ui.label(RichText::new("ISOLATION").size(11.0).color(OR));
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

// Sculpture décorative. Ses anneaux ne représentent ni une charge ni un résultat mesuré.
fn orbite(painter: &egui::Painter, center: egui::Pos2, time: f32, scale: f32) {
    for ring in 0..36 {
        let phase = ring as f32 * std::f32::consts::TAU / 36.0;
        let mut points = Vec::with_capacity(100);
        for point in 0..=96 {
            let angle = point as f32 * std::f32::consts::TAU / 96.0;
            let radius = 96.0 + 30.0 * phase.cos();
            let x = radius * angle.cos();
            let y = radius * angle.sin();
            let z = 30.0 * phase.sin();
            let spin = 0.42 + time * 0.035;
            let yr = y * spin.cos() - z * spin.sin();
            let zr = y * spin.sin() + z * spin.cos();
            let perspective = scale * 400.0 / (400.0 + zr);
            points.push(
                center
                    + Vec2::new(
                        (x * 0.88 - yr * 0.47) * perspective,
                        (x * 0.47 + yr * 0.88) * perspective,
                    ),
            );
        }
        let alpha = (65.0 + 90.0 * (phase.sin() * 0.5 + 0.5)) as u8;
        painter.add(egui::Shape::line(
            points,
            Stroke::new(0.85, Color32::from_rgba_unmultiplied(232, 193, 119, alpha)),
        ));
    }
}
