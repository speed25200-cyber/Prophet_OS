//! Pictogrammes vectoriels natifs, à taille fixe, et l'emblème de Prophet OS.
//!
//! Tout est tracé au trait, jamais rempli d'une image : la même forme reste nette à toute
//! échelle, sur l'écran d'un portable comme sur un mur, et ne pèse rien dans le binaire.

use egui::{Color32, Pos2, Rect, Shape, Stroke, pos2, vec2};

#[derive(Clone, Copy)]
pub(crate) enum Icon {
    /// La galerie des missions : quatre cases.
    Missions,
    /// Le dialogue : une bulle et deux lignes.
    Dialogue,
    /// Les modèles : une puce et ses broches.
    Models,
    /// Le système : un anneau d'isolation et son cœur.
    System,
}

pub(crate) fn icon(painter: &egui::Painter, center: Pos2, kind: Icon, size: f32, color: Color32) {
    let p = |x: f32, y: f32| center + vec2(x, y) * size;
    let stroke = Stroke::new(1.4, color);
    match kind {
        Icon::Missions => {
            for (x, y) in [(-0.42, -0.42), (0.06, -0.42), (-0.42, 0.06), (0.06, 0.06)] {
                painter.rect_stroke(
                    Rect::from_min_size(p(x, y), vec2(0.36, 0.36) * size),
                    (0.08 * size) as u8,
                    stroke,
                    egui::StrokeKind::Inside,
                );
            }
        }
        Icon::Dialogue => {
            painter.rect_stroke(
                Rect::from_center_size(center + vec2(0.0, -0.06 * size), vec2(0.9, 0.62) * size),
                (0.18 * size) as u8,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment([p(-0.22, 0.25), p(-0.36, 0.46)], stroke);
            painter.line_segment([p(-0.24, -0.16), p(0.24, -0.16)], stroke);
            painter.line_segment([p(-0.24, 0.04), p(0.08, 0.04)], stroke);
        }
        Icon::Models => {
            painter.rect_stroke(
                Rect::from_min_max(p(-0.30, -0.30), p(0.30, 0.30)),
                (0.08 * size) as u8,
                stroke,
                egui::StrokeKind::Inside,
            );
            for n in [-0.16, 0.16] {
                for (a, b) in [
                    ((n, -0.5), (n, -0.30)),
                    ((n, 0.30), (n, 0.5)),
                    ((-0.5, n), (-0.30, n)),
                    ((0.30, n), (0.5, n)),
                ] {
                    painter.line_segment([p(a.0, a.1), p(b.0, b.1)], stroke);
                }
            }
        }
        Icon::System => {
            painter.circle_stroke(center, 0.44 * size, stroke);
            painter.circle_stroke(center, 0.22 * size, stroke);
            painter.circle_filled(center, 0.06 * size, color);
            for (a, b) in [
                ((0.0, -0.58), (0.0, -0.44)),
                ((0.0, 0.44), (0.0, 0.58)),
                ((-0.58, 0.0), (-0.44, 0.0)),
                ((0.44, 0.0), (0.58, 0.0)),
            ] {
                painter.line_segment([p(a.0, a.1), p(b.0, b.1)], stroke);
            }
        }
    }
}

/// L'emblème : un œil au trait, l'iris cerclé, la pupille pleine.
///
/// `width` est la largeur de l'amande ; la hauteur en découle. Il se lit à 22 pixels dans la
/// barre du système et à 120 dans une page vide sans changer de forme.
pub(crate) fn oeil(painter: &egui::Painter, center: Pos2, width: f32, color: Color32) {
    let half = width * 0.5;
    let height = width * 0.27;
    let trait_ = Stroke::new((width * 0.055).max(1.2), color);
    let segments = 28;
    let mut haut = Vec::with_capacity(segments + 1);
    let mut bas = Vec::with_capacity(segments + 1);
    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let x = center.x - half + width * t;
        let y = height * (t * std::f32::consts::PI).sin();
        haut.push(pos2(x, center.y - y));
        bas.push(pos2(x, center.y + y));
    }
    painter.add(Shape::line(haut, trait_));
    painter.add(Shape::line(bas, trait_));
    painter.circle_stroke(center, width * 0.16, trait_);
    painter.circle_filled(center, width * 0.065, color);
}
