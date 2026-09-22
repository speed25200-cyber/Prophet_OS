//! Les formes du tableau de bord : plaques de verre à crochets, jauges graduées, lueurs,
//! étiquettes espacées, chiffres fins.
//!
//! Tout est tracé au trait ; rien n'est une image. Une jauge dit une proportion par son arc et
//! ses graduations, un crochet dit où finit une plaque, une lueur dit ce qui est actif. Aucune
//! de ces formes ne porte un chiffre que les services n'ont pas fourni.

use crate::theme::Accent;
use crate::theme::palette::{CREUX, DISCRET, EFFACE, ENCRE, FOND, TRAIT, VERRE};
use egui::{Align2, Color32, FontFamily, FontId, Frame, Pos2, Rect, Stroke, pos2, vec2};

/// Inter en graisse fine : les grands chiffres et les titres, qui doivent peser peu.
pub(crate) fn fin() -> FontFamily {
    FontFamily::Name("Inter300".into())
}

/// Inter en demi-gras : ce qui doit tenir sans crier.
pub(crate) fn fort() -> FontFamily {
    FontFamily::Name("Inter600".into())
}

/// Un texte espacé, ancré : le vocabulaire des étiquettes du tableau de bord.
pub(crate) fn texte_espace(
    p: &egui::Painter,
    at: Pos2,
    align: Align2,
    text: &str,
    font: FontId,
    color: Color32,
    spacing: f32,
) -> Rect {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: font,
            color,
            extra_letter_spacing: spacing,
            ..Default::default()
        },
    );
    let galley = p.layout_job(job);
    let rect = align.anchor_size(at, galley.size());
    p.galley(rect.min, galley, color);
    rect
}

/// Une étiquette en capitales espacées, peinte à une position.
pub(crate) fn etiquette_peinte(p: &egui::Painter, at: Pos2, text: &str, color: Color32) {
    texte_espace(
        p,
        at,
        Align2::LEFT_TOP,
        text,
        FontId::proportional(9.5),
        color,
        1.6,
    );
}

/// Une étiquette en capitales espacées, dans le flux.
pub(crate) fn etiquette(ui: &mut egui::Ui, text: impl Into<String>, color: Color32) {
    ui.label(
        egui::RichText::new(text)
            .size(9.5)
            .extra_letter_spacing(1.6)
            .color(color),
    );
}

/// Un titre en graisse fine.
pub(crate) fn titre(text: impl Into<String>, size: f32) -> egui::RichText {
    egui::RichText::new(text)
        .size(size)
        .family(fin())
        .color(ENCRE)
}

/// Les crochets d'angle d'une plaque : quatre équerres, qui disent où elle finit sans la
/// fermer d'un cadre.
pub(crate) fn crochets(p: &egui::Painter, r: Rect, color: Color32, len: f32) {
    let s = Stroke::new(1.5, color);
    for (corner, dx, dy) in [
        (r.left_top(), 1.0, 1.0),
        (r.right_top(), -1.0, 1.0),
        (r.right_bottom(), -1.0, -1.0),
        (r.left_bottom(), 1.0, -1.0),
    ] {
        p.line_segment([corner, corner + vec2(dx * len, 0.0)], s);
        p.line_segment([corner, corner + vec2(0.0, dy * len)], s);
    }
}

/// Une plaque de verre : fond translucide, fil d'accent, crochets d'angle, et une graduation
/// sur le bord haut.
pub(crate) fn plaque<R>(
    ui: &mut egui::Ui,
    accent: &Accent,
    margin: i8,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let inner = Frame::new()
        .fill(VERRE)
        .stroke(Stroke::new(1.0, accent.fil))
        .corner_radius(4)
        .inner_margin(margin)
        .show(ui, add);
    let r = inner.response.rect;
    let p = ui.painter();
    crochets(p, r.expand(1.0), accent.fil_vif, 14.0);
    // La graduation : de petits traits sur le bord haut, plus serrés à gauche. Elle donne
    // l'échelle de la plaque, comme le bord d'un instrument.
    let mut x = r.left() + 28.0;
    let mut n = 0;
    while x < r.right() - 28.0 && n < 400 {
        let h = if n % 5 == 0 { 5.0 } else { 2.5 };
        p.line_segment(
            [pos2(x, r.top()), pos2(x, r.top() + h)],
            Stroke::new(
                1.0,
                if n % 5 == 0 {
                    accent.fil_vif
                } else {
                    accent.fil
                },
            ),
        );
        x += 12.0;
        n += 1;
    }
    inner
}

/// Retient le rectangle d'une plaque nommée pour cette image : les parcours vérifient que les
/// plaques tiennent dans leur colonne et dans l'écran. Rien n'est dessiné.
pub(crate) fn retenir(ctx: &egui::Context, nom: &str, rect: egui::Rect) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(("plaque", nom)), rect));
}

/// Le rectangle retenu d'une plaque nommée, s'il a été dessiné.
pub(crate) fn retenue(ctx: &egui::Context, nom: &str) -> Option<egui::Rect> {
    ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new(("plaque", nom))))
}

/// Une lueur large et faible autour d'un point : ce qui est actif brille un peu au-delà de
/// ses bords.
pub(crate) fn lueur(p: &egui::Painter, center: Pos2, radius: f32, accent: &Accent) {
    for (k, alpha) in [(2.2, 10), (1.6, 18), (1.15, 30)] {
        p.circle_filled(center, radius * k, voile(accent.vif, alpha));
    }
}

/// Une couleur avec une opacité, prémultipliée comme egui l'attend.
pub(crate) fn voile(c: Color32, alpha: u8) -> Color32 {
    let a = u32::from(alpha);
    Color32::from_rgba_premultiplied(
        (u32::from(c.r()) * a / 255) as u8,
        (u32::from(c.g()) * a / 255) as u8,
        (u32::from(c.b()) * a / 255) as u8,
        alpha,
    )
}

/// Trace un arc de cercle par une ligne brisée, de midi dans le sens horaire.
pub(crate) fn arc(p: &egui::Painter, center: Pos2, radius: f32, fraction: f32, stroke: Stroke) {
    let points = arc_points(center, radius, fraction);
    if points.len() >= 2 {
        p.add(egui::Shape::line(points, stroke));
    }
}

/// Les sommets d'un arc, vides pour une fraction nulle, fermés pour une fraction entière.
pub(crate) fn arc_points(center: Pos2, radius: f32, fraction: f32) -> Vec<Pos2> {
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction <= 0.0 {
        return Vec::new();
    }
    let segments = ((72.0 * fraction).ceil() as usize).max(2);
    let start = -std::f32::consts::FRAC_PI_2;
    let sweep = std::f32::consts::TAU * fraction;
    (0..=segments)
        .map(|i| {
            let t = start + sweep * (i as f32 / segments as f32);
            pos2(center.x + radius * t.cos(), center.y + radius * t.sin())
        })
        .collect()
}

/// Une couronne de graduations : `count` traits, les `lit` premiers éclairés.
pub(crate) fn graduations(
    p: &egui::Painter,
    center: Pos2,
    radius: f32,
    count: usize,
    lit: usize,
    len: f32,
    (on, off): (Color32, Color32),
) {
    for i in 0..count {
        let t = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * (i as f32 / count as f32);
        let (c, s) = (t.cos(), t.sin());
        let long = i % (count / 12).max(1) == 0;
        let l = if long { len * 1.6 } else { len };
        p.line_segment(
            [
                pos2(center.x + radius * c, center.y + radius * s),
                pos2(center.x + (radius - l) * c, center.y + (radius - l) * s),
            ],
            Stroke::new(if long { 1.5 } else { 1.0 }, if i < lit { on } else { off }),
        );
    }
}

/// Une jauge : couronne graduée, arc de proportion, chiffre au centre, étiquette dessous.
///
/// La fraction remplit l'arc et allume les graduations ; le chiffre est celui reçu, jamais
/// déduit de la fraction.
pub(crate) fn jauge(
    p: &egui::Painter,
    center: Pos2,
    radius: f32,
    fraction: f32,
    color: Color32,
    accent: &Accent,
    (value, label): (&str, &str),
) {
    let fraction = fraction.clamp(0.0, 1.0);
    let ticks = 48;
    graduations(
        p,
        center,
        radius,
        ticks,
        (fraction * ticks as f32).round() as usize,
        radius * 0.09,
        (color, voile(accent.eteint, 150)),
    );
    p.circle_stroke(center, radius * 0.78, Stroke::new(1.0, TRAIT));
    arc(
        p,
        center,
        radius * 0.78,
        fraction,
        Stroke::new(radius * 0.06, color),
    );
    p.text(
        center + vec2(0.0, -radius * 0.04),
        Align2::CENTER_CENTER,
        value,
        FontId::new(radius * 0.62, fin()),
        ENCRE,
    );
    texte_espace(
        p,
        center + vec2(0.0, radius * 0.40),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional((radius * 0.17).max(8.0)),
        DISCRET,
        1.2,
    );
}

/// Le cadran d'une mission : une grande couronne graduée par étape franchie, l'arc du budget
/// consommé, et au centre le nombre d'étapes. C'est le réacteur de la plaque : il ne tourne
/// que si la mission avance.
pub(crate) fn cadran(
    p: &egui::Painter,
    center: Pos2,
    radius: f32,
    steps: u32,
    budget: f32,
    color: Color32,
    accent: &Accent,
) {
    let ticks = 96;
    let lit = (steps as usize).min(ticks);
    lueur(p, center, radius * 0.55, accent);
    p.circle_stroke(center, radius, Stroke::new(1.0, accent.fil));
    graduations(
        p,
        center,
        radius - 4.0,
        ticks,
        lit,
        radius * 0.07,
        (color, voile(accent.eteint, 120)),
    );
    p.circle_stroke(center, radius * 0.70, Stroke::new(1.0, TRAIT));
    arc(
        p,
        center,
        radius * 0.70,
        budget.clamp(0.0, 1.0),
        Stroke::new(3.0, color),
    );
    p.circle_stroke(center, radius * 0.52, Stroke::new(1.0, accent.fil));
    p.text(
        center + vec2(0.0, -radius * 0.05),
        Align2::CENTER_CENTER,
        steps.to_string(),
        FontId::new(radius * 0.46, fin()),
        ENCRE,
    );
    texte_espace(
        p,
        center + vec2(0.0, radius * 0.24),
        Align2::CENTER_CENTER,
        "ÉTAPES",
        FontId::proportional(8.5),
        DISCRET,
        1.6,
    );
    texte_espace(
        p,
        center + vec2(0.0, radius + 14.0),
        Align2::CENTER_CENTER,
        &format!("BUDGET {:.0} %", budget.clamp(0.0, 1.0) * 100.0),
        FontId::proportional(8.5),
        color,
        1.4,
    );
}

/// Un relevé : une étiquette espacée et, dessous, un chiffre fin. Le vocabulaire de la barre
/// du système, qui ne montre que des comptes réels.
pub(crate) fn releve(
    p: &egui::Painter,
    at: Pos2,
    align: Align2,
    label: &str,
    value: &str,
    color: Color32,
) -> Rect {
    let value_rect = texte_espace(p, at, align, value, FontId::new(19.0, fin()), color, 0.5);
    let label_rect = texte_espace(
        p,
        pos2(
            match align.x() {
                egui::Align::Min => value_rect.left(),
                egui::Align::Center => value_rect.center().x,
                egui::Align::Max => value_rect.right(),
            },
            value_rect.bottom() + 1.0,
        ),
        Align2([align.x(), egui::Align::Min]),
        label,
        FontId::proportional(8.0),
        EFFACE,
        1.5,
    );
    value_rect.union(label_rect)
}

/// Un bouton du tableau de bord : capitales espacées, fil d'accent quand il attend, plein
/// d'accent et lueur quand il commande.
pub(crate) fn bouton(
    ui: &mut egui::Ui,
    id: &str,
    texte: &str,
    actif: bool,
    accent: &Accent,
) -> egui::Response {
    let label = texte.to_uppercase();
    let font = FontId::proportional(10.5);
    let width = ui.fonts_mut(|fonts| {
        let mut job = egui::text::LayoutJob::default();
        job.append(
            &label,
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color: ENCRE,
                extra_letter_spacing: 1.3,
                ..Default::default()
            },
        );
        fonts.layout_job(job).size().x
    }) + 30.0;
    let (_, rect) = ui.allocate_space(vec2(width, 32.0));
    let response = ui.interact(rect, egui::Id::new(id), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, actif, texte)
    });
    let enabled = ui.is_enabled();
    let p = ui.painter();
    let (fill, stroke, color) = if !enabled {
        (Color32::TRANSPARENT, Stroke::new(1.0, TRAIT), EFFACE)
    } else if actif {
        lueur(p, rect.center(), rect.height() * 0.55, accent);
        (accent.vif, Stroke::NONE, FOND)
    } else if response.hovered() {
        (
            voile(accent.vif, 28),
            Stroke::new(1.0, accent.fil_vif),
            accent.vif,
        )
    } else {
        (Color32::TRANSPARENT, Stroke::new(1.0, accent.fil), ENCRE)
    };
    p.rect_filled(rect, 3, fill);
    if stroke != Stroke::NONE {
        p.rect_stroke(rect, 3, stroke, egui::StrokeKind::Inside);
    }
    if response.has_focus() {
        p.rect_stroke(
            rect.expand(3.0),
            4,
            Stroke::new(1.0, accent.fil_vif),
            egui::StrokeKind::Inside,
        );
    }
    texte_espace(
        p,
        rect.center(),
        Align2::CENTER_CENTER,
        &label,
        font,
        color,
        1.3,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Un champ de saisie du tableau de bord : un creux, un fil, un curseur d'accent.
pub(crate) fn cadre_saisie(accent: &Accent) -> Frame {
    Frame::new()
        .fill(CREUX)
        .stroke(Stroke::new(1.0, accent.fil))
        .corner_radius(3)
        .inner_margin(egui::Margin::symmetric(12, 8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_arc_vide_ne_trace_rien_et_un_plein_fait_le_tour() {
        assert!(arc_points(pos2(50.0, 50.0), 20.0, 0.0).is_empty());
        assert!(arc_points(pos2(50.0, 50.0), 20.0, -3.0).is_empty());
        let plein = arc_points(pos2(50.0, 50.0), 20.0, 1.0);
        assert_eq!(plein.len(), 73);
        assert!(
            plein[0].distance(*plein.last().unwrap()) < 1e-3,
            "le tour se referme"
        );
        assert!((plein[0].y - 30.0).abs() < 1e-3, "l'arc part de midi");
        let moitie = arc_points(pos2(50.0, 50.0), 20.0, 0.5);
        assert!(
            (moitie.last().unwrap().y - 70.0).abs() < 1e-3,
            "la moitié finit à six heures"
        );
    }

    #[test]
    fn un_voile_premultiplie_ses_canaux() {
        let v = voile(Color32::from_rgb(200, 100, 0), 128);
        assert_eq!(v.a(), 128);
        assert_eq!(v.r(), 100);
        assert_eq!(v.g(), 50);
    }
}
