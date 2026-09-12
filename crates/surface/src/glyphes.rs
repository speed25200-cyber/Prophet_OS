//! Pictogrammes vectoriels natifs, à taille fixe.

use egui::{Color32, Pos2, Rect, Stroke, vec2};

#[derive(Clone, Copy)]
pub(crate) enum Icon {
    Models,
}

pub(crate) fn icon(painter: &egui::Painter, center: Pos2, _kind: Icon, size: f32, color: Color32) {
    let p = |x: f32, y: f32| center + vec2(x, y) * size;
    let stroke = Stroke::new(1.2, color);
    painter.rect_stroke(
        Rect::from_min_max(p(-0.30, -0.30), p(0.30, 0.30)),
        3,
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
