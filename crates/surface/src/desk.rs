//! Composition du bureau et objets de mission, dessinés avec les données de la scène.
use crate::atelier::{Atelier, Page};
use crate::scene::{Courant, Etat, Scene};
use egui::{Align2, Color32, FontId, Frame, Rect, RichText, Stroke, pos2, vec2};

pub(crate) const INK: Color32 = Color32::from_rgb(28, 33, 39);
const MUTED: Color32 = Color32::from_rgb(108, 119, 128);
const RAIL: Color32 = Color32::from_rgb(34, 41, 47);
const WHITE: Color32 = Color32::from_rgb(252, 253, 253);
const BLUE: Color32 = Color32::from_rgb(49, 99, 142);

pub(crate) fn background(ui: &egui::Ui) {
    let r = ui.max_rect();
    let mut mesh = egui::Mesh::default();
    for (p, c) in [
        (r.left_top(), Color32::from_rgb(239, 243, 244)),
        (r.right_top(), Color32::from_rgb(246, 246, 240)),
        (r.right_bottom(), Color32::from_rgb(230, 232, 225)),
        (r.left_bottom(), Color32::from_rgb(211, 225, 229)),
    ] {
        mesh.colored_vertex(p, c);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(mesh);
}

fn symbol(p: &egui::Painter, center: egui::Pos2, page: Page, color: Color32) {
    let at = |x, y| center + vec2(x, y);
    let stroke = Stroke::new(1.5, color);
    match page {
        Page::Accueil => {
            for (x, y, w, h) in [(-9., -9., 7., 18.), (2., -9., 7., 7.), (2., 2., 7., 7.)] {
                p.rect_stroke(
                    Rect::from_min_size(at(x, y), vec2(w, h)),
                    2,
                    stroke,
                    egui::StrokeKind::Inside,
                );
            }
        }
        Page::Conversation => {
            p.rect_stroke(
                Rect::from_center_size(center, vec2(21., 16.)),
                5,
                stroke,
                egui::StrokeKind::Inside,
            );
            p.line_segment([at(-5., 8.), at(-8., 12.)], stroke);
            p.line_segment([at(-5., -2.), at(5., -2.)], stroke);
            p.line_segment([at(-5., 3.), at(2., 3.)], stroke);
        }
        Page::Modeles => crate::glyphes::icon(p, center, crate::glyphes::Icon::Models, 22., color),
        Page::Activite => {
            p.line_segment([at(-10., 9.), at(10., 9.)], stroke);
            for (x, y) in [(-7., -1.), (0., -9.), (7., -5.)] {
                p.line_segment([at(x, 5.), at(x, y)], Stroke::new(2.5, color));
            }
        }
    }
}

fn nav(ui: &mut egui::Ui, atelier: &mut Atelier, wide: bool) {
    let items = [
        (Page::Accueil, "nav-accueil", "Missions"),
        (Page::Conversation, "nav-conversation", "Dialogue"),
        (Page::Modeles, "nav-modeles", "Modèles"),
        (Page::Activite, "nav-activite", "Système"),
    ];
    for (page, id, label) in items {
        let selected = atelier.page == page;
        let size = if wide {
            vec2(62., 64.)
        } else {
            vec2(110., 42.)
        };
        let (_, r) = ui.allocate_space(size);
        let response = ui.interact(r, egui::Id::new(id), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label)
        });
        let p = ui.painter();
        p.rect_filled(
            r,
            14,
            if selected {
                Color32::from_rgb(246, 247, 244)
            } else if response.hovered() {
                Color32::from_rgb(55, 66, 73)
            } else {
                Color32::TRANSPARENT
            },
        );
        let color = if selected {
            INK
        } else {
            Color32::from_rgb(191, 204, 208)
        };
        symbol(
            p,
            if wide {
                pos2(r.center().x, r.top() + 22.)
            } else {
                pos2(r.left() + 22., r.center().y)
            },
            page,
            color,
        );
        p.text(
            if wide {
                pos2(r.center().x, r.bottom() - 13.)
            } else {
                pos2(r.left() + 42., r.center().y)
            },
            if wide {
                Align2::CENTER_CENTER
            } else {
                Align2::LEFT_CENTER
            },
            label,
            FontId::proportional(if wide { 10. } else { 12. }),
            color,
        );
        if response.has_focus() {
            p.rect_stroke(
                r.expand(2.),
                16,
                Stroke::new(2., Color32::from_rgb(143, 204, 218)),
                egui::StrokeKind::Inside,
            );
        }
        if response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            atelier.page = page;
        }
    }
}

pub(crate) fn chrome(root: &mut egui::Ui, atelier: &mut Atelier, scene: &Scene, compact: bool) {
    if !compact {
        egui::Panel::left("navigation-atelier")
            .exact_size(86.)
            .frame(Frame::new().fill(RAIL).inner_margin(12))
            .show(root, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(7.);
                    let (r, _) = ui.allocate_exact_size(vec2(46., 46.), egui::Sense::hover());
                    ui.painter().text(
                        r.center(),
                        Align2::CENTER_CENTER,
                        "p",
                        FontId::new(36., egui::FontFamily::Name("Inter600".into())),
                        WHITE,
                    );
                    ui.add_space(33.);
                    nav(ui, atelier, true);
                });
            });
    } else {
        egui::Panel::bottom("navigation-atelier-mobile")
            .exact_size(60.)
            .frame(Frame::new().fill(RAIL).inner_margin(9))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    let width = 4. * 110. + 3. * ui.spacing().item_spacing.x;
                    ui.add_space(((ui.available_width() - width) * 0.5).max(0.));
                    nav(ui, atelier, false);
                });
            });
    }
    egui::Panel::top("barre-systeme")
        .exact_size(if compact { 48. } else { 64. })
        .frame(
            Frame::new()
                .fill(Color32::TRANSPARENT)
                .inner_margin(egui::Margin::symmetric(if compact { 16 } else { 28 }, 14)),
        )
        .show(root, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("prophet")
                        .size(20.)
                        .family(egui::FontFamily::Name("Inter600".into()))
                        .color(INK),
                );
                ui.label(RichText::new("/  ATELIER").size(10.).color(MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{} UTC", scene.heure))
                            .size(11.)
                            .color(MUTED),
                    );
                    if atelier.demonstration {
                        ui.label(
                            RichText::new("DÉMONSTRATION")
                                .size(10.)
                                .color(Color32::from_rgb(143, 90, 35)),
                        );
                    } else if !compact {
                        let text = if atelier.decouverte {
                            "Recherche de modèles…".into()
                        } else {
                            format!(
                                "Dialogue · {} modèle{} détecté{}",
                                atelier.modeles.len(),
                                if atelier.modeles.len() == 1 { "" } else { "s" },
                                if atelier.modeles.len() == 1 { "" } else { "s" }
                            )
                        };
                        if ui
                            .add(
                                egui::Button::new(RichText::new(text).size(11.).color(MUTED))
                                    .fill(Color32::from_rgb(248, 250, 248))
                                    .corner_radius(12),
                            )
                            .clicked()
                        {
                            atelier.page = Page::Modeles;
                        }
                    }
                });
            });
        });
    egui::Panel::bottom("etat-systeme")
        .exact_size(28.)
        .frame(
            Frame::new()
                .fill(Color32::TRANSPARENT)
                .inner_margin(egui::Margin::symmetric(18, 5)),
        )
        .show(root, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if atelier.demonstration {
                        "Scène d'exemple · aucune exécution"
                    } else {
                        "Supervision humaine · états reçus des services"
                    })
                    .size(10.)
                    .color(MUTED),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(
                        &mut atelier.mouvement_reduit,
                        RichText::new("Mouvement réduit").size(10.).color(MUTED),
                    );
                });
            });
        });
}

pub(crate) fn work_surface() -> Frame {
    Frame::new()
        .fill(WHITE)
        .corner_radius(24)
        .inner_margin(24)
        .stroke(Stroke::new(1., Color32::from_rgb(226, 231, 229)))
        .shadow(egui::Shadow {
            offset: [0, 8],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(12),
        })
}

fn text(
    p: &egui::Painter,
    content: &str,
    at: egui::Pos2,
    width: f32,
    font: FontId,
    rows: usize,
    color: Color32,
) {
    let mut job = egui::text::LayoutJob::simple(content.into(), font, color, width);
    job.wrap.max_rows = rows;
    let galley = p.layout_job(job);
    p.galley(at, galley, color);
}

fn tile(ui: &mut egui::Ui, c: &Courant, r: Rect, selected: bool) -> bool {
    let (status, color) = c.task_state.map_or_else(
        || match c.etat {
            Etat::Court => ("En cours", Color32::from_rgb(38, 112, 92)),
            Etat::Attend => ("Votre décision", Color32::from_rgb(152, 101, 43)),
            Etat::Bloque => ("À examiner", Color32::from_rgb(152, 101, 43)),
            Etat::Fini => ("Terminée", MUTED),
        },
        crate::mission_details::status,
    );
    let response = ui.interact(
        r,
        egui::Id::new(format!("mission-{}", c.tache)),
        egui::Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            selected,
            format!("{} · {} · {}", c.intitule, status, c.agent),
        )
    });
    let p = ui.painter_at(r.expand(3.));
    p.rect_filled(
        r,
        18,
        if selected {
            WHITE
        } else if response.hovered() {
            Color32::from_rgb(250, 252, 250)
        } else {
            Color32::from_rgb(242, 246, 243)
        },
    );
    p.rect_stroke(
        r,
        18,
        Stroke::new(
            if selected { 1.5 } else { 1. },
            if selected || response.has_focus() {
                BLUE
            } else {
                Color32::from_rgb(227, 233, 229)
            },
        ),
        egui::StrokeKind::Inside,
    );
    let top = r.min + vec2(18., 18.);
    p.circle_filled(top + vec2(3., 7.), 3., color);
    text(
        &p,
        status,
        top + vec2(14., 0.),
        r.width() - 54.,
        FontId::proportional(11.),
        1,
        color,
    );
    text(
        &p,
        &c.intitule,
        top + vec2(0., 27.),
        r.width() - 36.,
        FontId::new(18., egui::FontFamily::Name("Inter600".into())),
        2,
        INK,
    );
    text(
        &p,
        &c.agent,
        pos2(r.left() + 18., r.bottom() - 29.),
        r.width() - 128.,
        FontId::proportional(10.),
        1,
        MUTED,
    );
    p.text(
        pos2(r.right() - 18., r.bottom() - 22.),
        Align2::RIGHT_CENTER,
        format!("{} étapes", c.etapes),
        FontId::proportional(10.),
        MUTED,
    );
    if selected {
        p.rect_filled(
            Rect::from_center_size(pos2(r.center().x, r.bottom() - 1.), vec2(42., 3.)),
            2,
            BLUE,
        );
    }
    response
        .on_hover_text(format!("{}\n{}\n{}", c.intitule, status, c.agent))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

pub(crate) fn gallery(
    ui: &mut egui::Ui,
    courants: &[&Courant],
    selection: Option<&str>,
    horizontal: bool,
) -> Option<String> {
    let mut picked = None;
    if horizontal {
        let width = ((ui.available_width() - 24.) / 3.).clamp(250., 360.);
        let stride = width + 12.;
        egui::ScrollArea::horizontal()
            .id_salt("objets-missions")
            .animated(false)
            .max_height(146.)
            .auto_shrink([false, true])
            .show_viewport(ui, |ui, view| {
                let (_, bounds) =
                    ui.allocate_space(vec2((courants.len() as f32 * stride - 12.).max(0.), 136.));
                // Une sélection faite ailleurs (filtre, nouvelle mission) doit rester visible.
                let memory_id = ui.id().with("selection-galerie");
                let previous = ui.data(|d| d.get_temp::<String>(memory_id));
                if previous.as_deref() != selection {
                    ui.data_mut(|d| {
                        d.insert_temp(memory_id, selection.unwrap_or_default().to_owned());
                    });
                    if let Some(index) = courants
                        .iter()
                        .position(|c| Some(c.tache.as_str()) == selection)
                    {
                        ui.scroll_to_rect(
                            Rect::from_min_size(
                                bounds.min + vec2(index as f32 * stride, 0.),
                                vec2(width, 136.),
                            ),
                            None,
                        );
                    }
                }
                let first = (view.min.x / stride).floor().max(0.) as usize;
                let last = ((view.max.x / stride).ceil() as usize + 1).min(courants.len());
                for (i, c) in courants.iter().enumerate().take(last).skip(first) {
                    let r = Rect::from_min_size(
                        bounds.min + vec2(i as f32 * stride, 0.),
                        vec2(width, 136.),
                    );
                    if tile(ui, c, r, selection == Some(c.tache.as_str())) {
                        picked = Some(c.tache.clone());
                    }
                }
            });
    } else {
        for c in courants {
            let (_, r) = ui.allocate_space(vec2(ui.available_width(), 136.));
            if tile(ui, c, r, selection == Some(c.tache.as_str())) {
                picked = Some(c.tache.clone());
            }
            ui.add_space(4.);
        }
    }
    picked
}

pub(crate) fn empty(ui: &mut egui::Ui, compact: bool) {
    work_surface().inner_margin(if compact {24} else {42}).show(ui,|ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("VOTRE ESPACE DE TRAVAIL").size(11.).color(MUTED));
        ui.add_space(20.);
        ui.label(RichText::new("Que voulez-vous\naccomplir ?").size(if compact {34.} else {54.}).line_height(Some(if compact {41.} else {63.})).family(egui::FontFamily::Name("Inter600".into())).color(INK));
        ui.add_space(18.);
        ui.label(RichText::new("Préparez un objectif. Examinez le plan.\nGardez la main sur le travail de vos agents.").size(16.).color(MUTED));
        ui.add_space(32.);
        ui.separator();
        ui.add_space(18.);
        ui.horizontal_wrapped(|ui| {
            for (n,label) in [("01","Intention"),("02","Plan à examiner"),("03","Travail supervisé")] {
                ui.label(RichText::new(n).monospace().size(11.).color(BLUE));
                ui.label(RichText::new(label).size(13.).color(INK));
                ui.add_space(16.);
            }
        });
        ui.add_space(16.);
        ui.label(RichText::new("Aucune mission reçue pour le moment.").size(11.).color(MUTED));
    });
}
