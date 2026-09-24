//! Composition du bureau et objets de mission, dessinés avec les données de la scène.
//!
//! La direction Réacteur : une nuit où le champ vit, un rail de verre à gauche, une barre du
//! système qui ne relève que des comptes réels, et des plaques de verre à crochets pour tout
//! ce qui se lit. L'accent signale, l'encre nomme, les capitales espacées rubriquent.

use crate::atelier::{Atelier, Page};
use crate::hud;
use crate::scene::{Courant, Etat, Scene};
use crate::theme::Accent;
use crate::theme::palette::{ATTENTE, CREUX, DISCRET, EFFACE, ENCRE, TRAIT, VERRE};
use egui::{Align2, Color32, FontId, Frame, Rect, RichText, Stroke, pos2, vec2};

/// Largeur du rail de navigation, panneau compris.
const RAIL: f32 = 92.0;
/// Hauteur d'une ligne de mission dans la liste.
const LIGNE: f32 = 72.0;

fn nav(ui: &mut egui::Ui, atelier: &mut Atelier, wide: bool, accent: &Accent) {
    let items = [
        (
            Page::Accueil,
            "nav-accueil",
            "MISSIONS",
            crate::glyphes::Icon::Missions,
        ),
        (
            Page::Conversation,
            "nav-conversation",
            "DIALOGUE",
            crate::glyphes::Icon::Dialogue,
        ),
        (
            Page::Modeles,
            "nav-modeles",
            "MODÈLES",
            crate::glyphes::Icon::Models,
        ),
        (
            Page::Activite,
            "nav-activite",
            "SYSTÈME",
            crate::glyphes::Icon::System,
        ),
    ];
    for (page, id, label, icon) in items {
        let selected = atelier.page == page;
        let size = if wide {
            vec2(64., 64.)
        } else {
            vec2(112., 44.)
        };
        let (_, r) = ui.allocate_space(size);
        let response = ui.interact(r, egui::Id::new(id), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label)
        });
        let p = ui.painter();
        let icon_at = if wide {
            pos2(r.center().x, r.top() + 24.)
        } else {
            pos2(r.left() + 24., r.center().y)
        };
        if selected {
            hud::lueur(p, icon_at, 15.0, accent);
            // Un trait d'accent sur le bord : la page où l'on est, sans le lire.
            if wide {
                p.rect_filled(
                    Rect::from_min_size(
                        pos2(r.left() - 2., r.top() + 16.),
                        vec2(2., r.height() - 32.),
                    ),
                    1,
                    accent.vif,
                );
            } else {
                p.rect_filled(
                    Rect::from_min_size(
                        pos2(r.left() + 18., r.bottom() - 2.),
                        vec2(r.width() - 36., 2.),
                    ),
                    1,
                    accent.vif,
                );
            }
        } else if response.hovered() {
            p.rect_filled(r, 4, hud::voile(accent.vif, 14));
        }
        let color = if selected {
            accent.vif
        } else if response.hovered() {
            ENCRE
        } else {
            DISCRET
        };
        crate::glyphes::icon(p, icon_at, icon, 22., color);
        hud::texte_espace(
            p,
            if wide {
                pos2(r.center().x, r.bottom() - 12.)
            } else {
                pos2(r.left() + 42., r.center().y)
            },
            if wide {
                Align2::CENTER_CENTER
            } else {
                Align2::LEFT_CENTER
            },
            label,
            FontId::proportional(if wide { 8. } else { 9.5 }),
            color,
            1.4,
        );
        if response.has_focus() {
            p.rect_stroke(
                r.expand(2.),
                4,
                Stroke::new(1., accent.fil_vif),
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
    let accent = Accent::de(root.ctx());
    egui::Panel::top("barre-systeme")
        .exact_size(if compact { 52. } else { 64. })
        .frame(
            Frame::new()
                .fill(Color32::TRANSPARENT)
                .inner_margin(egui::Margin::symmetric(if compact { 16 } else { 28 }, 0)),
        )
        .show(root, |ui| {
            let r = ui.max_rect();
            let p = ui.painter();
            p.line_segment(
                [
                    pos2(r.left() - 28., r.bottom()),
                    pos2(r.right() + 28., r.bottom()),
                ],
                Stroke::new(1., accent.fil),
            );
            // Le mot-marque : l'œil, puis le nom en capitales espacées. Ce n'est pas un logo
            // posé, c'est le fil qui cercle toutes les plaques, écrit une fois en toutes lettres.
            let eye = pos2(r.left() + 16., r.center().y);
            hud::lueur(p, eye, 10.0, &accent);
            crate::glyphes::oeil(p, eye, if compact { 24. } else { 30. }, accent.vif);
            let mark = hud::texte_espace(
                p,
                pos2(r.left() + (if compact { 38. } else { 48. }), r.center().y),
                Align2::LEFT_CENTER,
                "PROPHET OS",
                FontId::new(if compact { 12. } else { 13.5 }, hud::fort()),
                accent.vif,
                if compact { 3.0 } else { 4.2 },
            );
            if !compact {
                hud::texte_espace(
                    p,
                    pos2(mark.right() + 16., r.center().y + 0.5),
                    Align2::LEFT_CENTER,
                    "ATELIER",
                    FontId::proportional(8.5),
                    EFFACE,
                    2.2,
                );
                // L'heure au centre, en chiffres fins : la machine est là.
                let clock = hud::texte_espace(
                    p,
                    pos2(r.center().x, r.center().y - 6.),
                    Align2::CENTER_CENTER,
                    &scene.heure,
                    FontId::new(22., hud::fin()),
                    ENCRE,
                    1.0,
                );
                hud::texte_espace(
                    p,
                    pos2(r.center().x, clock.bottom() + 2.),
                    Align2::CENTER_TOP,
                    &format!("{} · UTC", scene.date.to_uppercase()),
                    FontId::proportional(8.),
                    EFFACE,
                    1.5,
                );
                p.circle_filled(pos2(clock.left() - 14., clock.center().y), 2.4, accent.vif);
                // Les relevés, à droite : des comptes réels, rien d'autre.
                let mut x = r.right();
                let actives = scene.actives();
                let reclament = scene.courants.iter().filter(|c| c.reclame()).count();
                let readouts = [
                    (
                        "MODÈLES",
                        if atelier.demonstration {
                            "—".to_owned()
                        } else {
                            format!("{:02}", atelier.modeles.len())
                        },
                        DISCRET,
                    ),
                    (
                        "ISOLATION",
                        format!("{:02}", scene.isolation.niveau_max),
                        ENCRE,
                    ),
                    (
                        "À EXAMINER",
                        format!("{reclament:02}"),
                        if reclament > 0 { ATTENTE } else { DISCRET },
                    ),
                    (
                        "ACTIVES",
                        format!("{actives:02}"),
                        if actives > 0 { accent.vif } else { DISCRET },
                    ),
                ];
                for (label, value, color) in readouts {
                    let rect = hud::releve(
                        p,
                        pos2(x, r.center().y - 9.),
                        Align2::RIGHT_CENTER,
                        label,
                        &value,
                        color,
                    );
                    x = rect.left() - 26.;
                    p.line_segment(
                        [
                            pos2(x + 13., r.center().y - 12.),
                            pos2(x + 13., r.center().y + 12.),
                        ],
                        Stroke::new(1., TRAIT),
                    );
                }
                if atelier.demonstration {
                    hud::texte_espace(
                        p,
                        pos2(x - 4., r.center().y),
                        Align2::RIGHT_CENTER,
                        "DÉMONSTRATION",
                        FontId::proportional(9.),
                        ATTENTE,
                        1.8,
                    );
                }
            } else if atelier.demonstration {
                hud::texte_espace(
                    p,
                    pos2(r.right(), r.center().y),
                    Align2::RIGHT_CENTER,
                    "DÉMONSTRATION",
                    FontId::proportional(9.),
                    ATTENTE,
                    1.8,
                );
            }
        });
    if !compact {
        egui::Panel::left("navigation-atelier")
            .exact_size(RAIL)
            .frame(Frame::new().fill(Color32::TRANSPARENT).inner_margin(0))
            .show(root, |ui| {
                let full = ui.max_rect();
                let rail = Rect::from_min_max(
                    pos2(full.left() + 14., full.top() + 20.),
                    pos2(full.right() - 14., full.bottom() - 20.),
                );
                let p = ui.painter();
                p.rect_filled(rail, 4, VERRE);
                p.rect_stroke(
                    rail,
                    4,
                    Stroke::new(1., accent.fil),
                    egui::StrokeKind::Inside,
                );
                hud::crochets(p, rail.expand(1.), accent.fil_vif, 10.);
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(rail.shrink2(vec2(0., 16.))),
                    |ui| {
                        ui.vertical_centered(|ui| {
                            let (r, _) =
                                ui.allocate_exact_size(vec2(52., 52.), egui::Sense::hover());
                            emblem(ui.painter(), r.center(), scene, &accent);
                            ui.add_space(18.);
                            nav(ui, atelier, true, &accent);
                        });
                    },
                );
            });
    } else {
        egui::Panel::bottom("navigation-atelier-mobile")
            .exact_size(60.)
            .frame(Frame::new().fill(VERRE).inner_margin(8))
            .show(root, |ui| {
                let r = ui.max_rect();
                ui.painter().line_segment(
                    [
                        pos2(r.left() - 8., r.top() - 8.),
                        pos2(r.right() + 8., r.top() - 8.),
                    ],
                    Stroke::new(1., accent.fil),
                );
                ui.horizontal(|ui| {
                    let width = 4. * 112. + 3. * ui.spacing().item_spacing.x;
                    ui.add_space(((ui.available_width() - width) * 0.5).max(0.));
                    nav(ui, atelier, false, &accent);
                });
            });
    }
    egui::Panel::bottom("etat-systeme")
        .exact_size(28.)
        .frame(
            Frame::new()
                .fill(Color32::TRANSPARENT)
                .inner_margin(egui::Margin::symmetric(18, 5)),
        )
        .show(root, |ui| {
            ui.horizontal(|ui| {
                hud::etiquette(
                    ui,
                    if atelier.demonstration {
                        "SCÈNE D'EXEMPLE · AUCUNE EXÉCUTION"
                    } else {
                        "SUPERVISION HUMAINE · ÉTATS REÇUS DES SERVICES"
                    },
                    EFFACE,
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(
                        &mut atelier.mouvement_reduit,
                        RichText::new("Mouvement réduit").size(10.).color(DISCRET),
                    );
                    // Le clavier suffit à tout : on le dit une fois, là où l'œil ne s'attarde pas,
                    // dans la version qui tient entre l'état et la case — sinon elle passerait
                    // sur l'état.
                    if !compact {
                        let place = ui.available_width() - 22. - 16.;
                        if let Some(texte) =
                            raccourcis_qui_tiennent(place, |t| hud::largeur_etiquette(ui, t))
                        {
                            ui.add_space(22.);
                            hud::etiquette(ui, texte, EFFACE);
                        }
                    }
                });
            });
        });
}

/// Les raccourcis de la barre d'état, du plus complet au plus bref : la recherche et l'arrêt
/// d'urgence restent dits le plus longtemps.
const RACCOURCIS: [&str; 2] = [
    "CTRL K RECHERCHER · CTRL N OBJECTIF · CTRL 1–4 PAGES · ÉCHAP FERMER · CTRL MAJ ÉCHAP TOUT ARRÊTER",
    "CTRL K RECHERCHER · CTRL MAJ ÉCHAP TOUT ARRÊTER",
];

/// La première version des raccourcis qui tient dans `place`, mesurée par `largeur`.
fn raccourcis_qui_tiennent(place: f32, largeur: impl Fn(&str) -> f32) -> Option<&'static str> {
    RACCOURCIS.into_iter().find(|texte| largeur(texte) <= place)
}

/// L'emblème du rail : l'anneau des missions actives autour du chiffre qui les compte.
///
/// Rien n'y bouge. L'anneau se remplit avec les missions en cours, part par part, et s'éteint
/// quand tout est fini : c'est la seule chose que le rail affirme, et il l'affirme sans texte.
fn emblem(p: &egui::Painter, center: egui::Pos2, scene: &Scene, accent: &Accent) {
    let total = scene.courants.len().max(1) as f32;
    let active = scene.actives() as f32;
    if scene.actives() > 0 {
        hud::lueur(p, center, 14., accent);
    }
    p.circle_filled(center, 24., CREUX);
    hud::graduations(
        p,
        center,
        24.,
        36,
        ((active / total) * 36.0).round() as usize,
        3.0,
        (accent.vif, hud::voile(accent.eteint, 120)),
    );
    p.circle_stroke(center, 17., Stroke::new(1., accent.fil));
    p.text(
        center + vec2(0., 0.5),
        Align2::CENTER_CENTER,
        scene.actives().to_string(),
        FontId::new(18., hud::fin()),
        if scene.actives() > 0 {
            accent.vif
        } else {
            DISCRET
        },
    );
}

/// Une ligne de mission dans la liste : anneau de budget, état, titre, pilote, étapes.
fn ligne(ui: &mut egui::Ui, c: &Courant, r: Rect, selected: bool, accent: &Accent) -> bool {
    let (status, color) = c.task_state.map_or_else(
        || match c.etat {
            Etat::Court => ("En cours", accent.vif),
            Etat::Attend => ("Votre décision", ATTENTE),
            Etat::Bloque => ("À examiner", ATTENTE),
            Etat::Fini => ("Terminée", DISCRET),
        },
        |state| crate::mission_details::status(state, accent),
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
    let p = ui.painter_at(r.expand(2.));
    if selected {
        p.rect_filled(r, 3, hud::voile(accent.vif, 20));
        p.rect_filled(
            Rect::from_min_size(r.min, vec2(2., r.height())),
            1,
            accent.vif,
        );
        hud::crochets(&p, r, accent.fil_vif, 8.);
    } else if response.hovered() {
        p.rect_filled(r, 3, hud::voile(accent.vif, 10));
    }
    if response.has_focus() {
        p.rect_stroke(
            r,
            3,
            Stroke::new(1., accent.fil_vif),
            egui::StrokeKind::Inside,
        );
    }
    p.line_segment(
        [
            pos2(r.left() + 14., r.bottom()),
            pos2(r.right() - 8., r.bottom()),
        ],
        Stroke::new(1., TRAIT),
    );
    let ring = pos2(r.left() + 30., r.center().y);
    crate::instruments::anneau(&p, ring, 12., 2., c.budget_consomme, color, None);
    p.circle_filled(ring, 3., color);
    let text_left = r.left() + 56.;
    let mut job = egui::text::LayoutJob::simple(
        c.intitule.clone(),
        FontId::proportional(14.),
        ENCRE,
        r.width() - 56. - 70.,
    );
    job.wrap.max_rows = 1;
    let galley = p.layout_job(job);
    p.galley(pos2(text_left, r.top() + 15.), galley, ENCRE);
    hud::etiquette_peinte(
        &p,
        pos2(text_left, r.top() + 40.),
        &status.to_uppercase(),
        color,
    );
    // Le nombre et son unité se lisent ensemble, loin du trait qui sépare les lignes et des
    // crochets de la sélection.
    crate::instruments::monogramme(&p, pos2(r.right() - 58., r.center().y - 6.), &c.agent, 15.);
    p.text(
        pos2(r.right() - 12., r.center().y - 6.),
        Align2::RIGHT_CENTER,
        format!("{}", c.etapes),
        FontId::new(15., hud::fin()),
        ENCRE,
    );
    hud::texte_espace(
        &p,
        pos2(r.right() - 12., r.center().y + 11.),
        Align2::RIGHT_CENTER,
        "ÉTAPES",
        FontId::proportional(7.5),
        EFFACE,
        1.4,
    );
    response
        .on_hover_text(format!("{}\n{}\n{}", c.intitule, status, c.agent))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// La liste des missions, virtualisée : seules les lignes proches de la vue sont composées.
pub(crate) fn liste(
    ui: &mut egui::Ui,
    courants: &[&Courant],
    selection: Option<&str>,
    height: f32,
) -> Option<String> {
    let accent = Accent::de(ui.ctx());
    let mut picked = None;
    egui::ScrollArea::vertical()
        .id_salt("objets-missions")
        .animated(false)
        .max_height(height)
        .auto_shrink([false, true])
        .show_viewport(ui, |ui, view| {
            let width = ui.available_width();
            let (_, bounds) =
                ui.allocate_space(vec2(width, (courants.len() as f32 * LIGNE).max(0.)));
            // Une sélection faite ailleurs (filtre, nouvelle mission) doit rester visible.
            let memory_id = ui.id().with("selection-liste");
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
                            bounds.min + vec2(0., index as f32 * LIGNE),
                            vec2(width, LIGNE),
                        ),
                        None,
                    );
                }
            }
            let first = (view.min.y / LIGNE).floor().max(0.) as usize;
            let last = ((view.max.y / LIGNE).ceil() as usize + 1).min(courants.len());
            for (i, c) in courants.iter().enumerate().take(last).skip(first) {
                let r = Rect::from_min_size(
                    bounds.min + vec2(0., i as f32 * LIGNE),
                    vec2(width, LIGNE),
                );
                if ligne(ui, c, r, selection == Some(c.tache.as_str()), &accent) {
                    picked = Some(c.tache.clone());
                }
            }
        });
    picked
}

/// L'espace vide : une seule plaque, à gauche, qui laisse le champ respirer à droite.
///
/// L'objectif se tape ici même : Entrée, ou le bouton, ouvre sa préparation où l'humain
/// choisit le contexte et le modèle, puis examine le plan. Taper ne soumet rien. Rend vrai
/// quand l'humain demande à préparer.
pub(crate) fn empty(ui: &mut egui::Ui, compact: bool, brouillon: &mut String) -> bool {
    let accent = Accent::de(ui.ctx());
    let mut soumis = false;
    let width = if compact {
        ui.available_width()
    } else {
        ui.available_width().min(720.)
    };
    ui.allocate_ui_with_layout(
        vec2(width, ui.available_height()),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            let plaque = hud::plaque(ui, &accent, if compact { 24 } else { 44 }, |ui| {
                ui.set_width(width - if compact { 50. } else { 90. });
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(26., 14.), egui::Sense::hover());
                    crate::glyphes::oeil(ui.painter(), r.center(), 22., accent.vif);
                    hud::etiquette(ui, "VOTRE ESPACE DE TRAVAIL", accent.sourd);
                });
                ui.add_space(26.);
                ui.label(
                    hud::titre(
                        "Que voulez-vous\naccomplir ?",
                        if compact { 36. } else { 52. },
                    )
                    .line_height(Some(if compact { 40. } else { 58. })),
                );
                ui.add_space(14.);
                ui.label(
                    RichText::new(
                        "Préparez un objectif. Examinez le plan.\nGardez la main sur le travail de vos agents.",
                    )
                    .size(15.)
                    .color(DISCRET),
                );
                ui.add_space(24.);
                let champ = hud::cadre_saisie(&accent)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(brouillon)
                                .id(egui::Id::new("intention-accueil"))
                                .desired_width(f32::INFINITY)
                                .frame(Frame::NONE)
                                .font(FontId::proportional(16.))
                                .hint_text("Décrivez le résultat attendu…")
                                .char_limit(16_384),
                        )
                    })
                    .inner;
                let entree = champ.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(10.);
                hud::rangee(ui, |ui| {
                    let pret = !brouillon.trim().is_empty();
                    let bouton = ui
                        .add_enabled_ui(pret, |ui| {
                            hud::bouton(ui, "preparer-depuis-accueil", "Préparer  ↗", true, &accent)
                        })
                        .inner;
                    if pret && (entree || bouton.clicked()) {
                        soumis = true;
                    }
                    hud::etiquette(ui, "ENTRÉE POUR PRÉPARER · RIEN N'EST LANCÉ", EFFACE);
                });
                ui.add_space(26.);
                let (line, _) = ui.allocate_exact_size(vec2(64., 1.), egui::Sense::hover());
                ui.painter().rect_filled(line, 0, accent.vif);
                ui.add_space(18.);
                let etapes = [
                    ("01", "Intention", "Ce que vous voulez obtenir, dans vos mots."),
                    ("02", "Plan à examiner", "Les accès et les limites, avant tout lancement."),
                    ("03", "Travail supervisé", "Vous lancez, vous voyez, vous pouvez arrêter."),
                ];
                let etape = |ui: &mut egui::Ui, (n, label, detail): (&str, &str, &str)| {
                    ui.label(RichText::new(n).family(hud::fin()).size(15.).color(accent.vif));
                    ui.label(RichText::new(label).size(14.).color(ENCRE));
                    ui.label(RichText::new(detail).size(12.).color(DISCRET));
                };
                // Sur un grand écran, les trois étapes se lisent côte à côte, d'un seul regard.
                if compact {
                    for e in etapes {
                        etape(ui, e);
                        ui.add_space(6.);
                    }
                } else {
                    // Les colonnes d'egui justifient le texte ; une description se lit en
                    // drapeau, sans espaces étirés.
                    ui.columns(3, |colonnes| {
                        for (colonne, e) in colonnes.iter_mut().zip(etapes) {
                            colonne.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                etape(ui, e);
                            });
                        }
                    });
                }
                ui.add_space(18.);
                hud::etiquette(ui, "AUCUNE MISSION REÇUE POUR LE MOMENT", EFFACE);
            });
            hud::retenir(ui.ctx(), "espace-vide", plaque.response.rect);
        },
    );
    soumis
}

#[cfg(test)]
mod tests {
    #[test]
    fn la_barre_dit_les_raccourcis_qui_tiennent_sans_passer_sur_l_etat() {
        // Une mesure à 6 points par caractère : assez pour ordonner les versions.
        let largeur = |t: &str| t.chars().count() as f32 * 6.0;
        assert_eq!(
            super::raccourcis_qui_tiennent(2_000.0, largeur),
            Some(super::RACCOURCIS[0])
        );
        assert_eq!(
            super::raccourcis_qui_tiennent(400.0, largeur),
            Some(super::RACCOURCIS[1])
        );
        assert_eq!(super::raccourcis_qui_tiennent(100.0, largeur), None);
    }
}
