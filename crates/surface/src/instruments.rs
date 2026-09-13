//! Les instruments de l'atelier : ce qui se lit d'un coup d'œil, dessiné plutôt qu'écrit.
//!
//! Un anneau dit une proportion mieux qu'un pourcentage ; un monogramme dit un agent mieux
//! qu'un identifiant. Tout est statique : rien ne bouge tant que l'état ne change pas, pour
//! qu'une supervision au repos reste au repos.

use egui::{Align2, Color32, FontId, Rect, Stroke, pos2, vec2};

/// Encre des chiffres et des titres.
pub(crate) const ENCRE: Color32 = Color32::from_rgb(28, 33, 39);
/// Texte secondaire.
pub(crate) const DISCRET: Color32 = Color32::from_rgb(106, 114, 126);
/// Piste d'un anneau ou d'une barre, à peine visible.
pub(crate) const PISTE: Color32 = Color32::from_rgb(228, 232, 236);
/// Fond d'une tuile d'instrument.
pub(crate) const TUILE: Color32 = Color32::from_rgb(246, 248, 250);

/// Trace un arc de cercle par une ligne brisée, de midi dans le sens horaire.
///
/// egui n'a pas de primitive d'arc ; soixante segments suffisent pour qu'un œil n'en voie
/// aucun à ces rayons, et le coût reste négligeable devant le texte.
pub(crate) fn arc(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    fraction: f32,
    stroke: Stroke,
) {
    let points = arc_points(center, radius, fraction);
    if points.len() >= 2 {
        painter.add(egui::Shape::line(points, stroke));
    }
}

/// Les sommets d'un arc, vides pour une fraction nulle, fermés pour une fraction entière.
fn arc_points(center: egui::Pos2, radius: f32, fraction: f32) -> Vec<egui::Pos2> {
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction <= 0.0 {
        return Vec::new();
    }
    let segments = ((60.0 * fraction).ceil() as usize).max(2);
    let start = -std::f32::consts::FRAC_PI_2;
    let sweep = std::f32::consts::TAU * fraction;
    (0..=segments)
        .map(|i| {
            let t = start + sweep * (i as f32 / segments as f32);
            pos2(center.x + radius * t.cos(), center.y + radius * t.sin())
        })
        .collect()
}

/// Un anneau de proportion : piste complète, part colorée, chiffre au centre.
pub(crate) fn anneau(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    thickness: f32,
    fraction: f32,
    accent: Color32,
    label: Option<&str>,
) {
    painter.circle_stroke(center, radius, Stroke::new(thickness, PISTE));
    arc(
        painter,
        center,
        radius,
        fraction,
        Stroke::new(thickness, accent),
    );
    if let Some(label) = label {
        painter.text(
            center,
            Align2::CENTER_CENTER,
            label,
            FontId::new(radius * 0.62, egui::FontFamily::Name("Inter600".into())),
            ENCRE,
        );
    }
}

/// Le monogramme d'un agent : une lettre dans une pastille, toujours la même pour le même
/// pilote, pour qu'on reconnaisse Claude Code, Codex ou le moteur local sans lire.
pub(crate) fn monogramme(painter: &egui::Painter, center: egui::Pos2, agent: &str, size: f32) {
    let (letter, fill) = identite(agent);
    painter.rect_filled(
        Rect::from_center_size(center, vec2(size, size)),
        size * 0.3,
        fill,
    );
    painter.text(
        center,
        Align2::CENTER_CENTER,
        letter,
        FontId::new(size * 0.56, egui::FontFamily::Name("Inter600".into())),
        Color32::WHITE,
    );
}

/// Lettre et teinte d'un pilote. Les teintes sont sourdes : elles distinguent, sans crier.
pub(crate) fn identite(agent: &str) -> (&'static str, Color32) {
    let agent = agent.to_ascii_lowercase();
    if agent.contains("claude") {
        ("C", Color32::from_rgb(163, 98, 62))
    } else if agent.contains("codex") || agent.contains("gpt") {
        ("X", Color32::from_rgb(52, 63, 78))
    } else if agent.contains("gemini") {
        ("G", Color32::from_rgb(58, 94, 150))
    } else if agent.starts_with("local") || agent.contains("qwen") || agent.contains("llama") {
        ("L", Color32::from_rgb(46, 110, 96))
    } else {
        ("P", Color32::from_rgb(86, 72, 130))
    }
}

/// Une ligne de trois instruments : étapes, activité, budget.
///
/// La forme compacte sert à l'inspecteur relié aux services, où la proposition et ses fichiers
/// doivent rester visibles sans défiler : les instruments s'y lisent, ils n'y règnent pas.
pub(crate) fn tableau(
    ui: &mut egui::Ui,
    steps: u32,
    rate_per_minute: f32,
    budget_fraction: f32,
    accent: Color32,
    compact: bool,
) {
    let width = ui.available_width();
    let gap = 12.0;
    let tile_width = ((width - 2.0 * gap) / 3.0).max(120.0);
    let height = if compact { 60.0 } else { 96.0 };
    let value_font = if compact { 22.0 } else { 30.0 };
    let value_y = if compact { 24.0 } else { 42.0 };
    let ring_radius = if compact { 18.0 } else { 26.0 };
    let (row, _) = ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
    let painter = ui.painter();
    let tiles = [
        ("ÉTAPES", steps.to_string(), None),
        ("ACTIVITÉ", format!("{rate_per_minute:.0}"), Some("/ min")),
        (
            "BUDGET",
            format!("{:.0} %", budget_fraction.clamp(0.0, 1.0) * 100.0),
            None,
        ),
    ];
    for (i, (label, value, unit)) in tiles.iter().enumerate() {
        let r = Rect::from_min_size(
            row.min + vec2(i as f32 * (tile_width + gap), 0.0),
            vec2(tile_width, height),
        );
        painter.rect_filled(r, 14, TUILE);
        painter.text(
            r.min + vec2(18.0, if compact { 10.0 } else { 18.0 }),
            Align2::LEFT_TOP,
            *label,
            FontId::proportional(10.0),
            DISCRET,
        );
        if i == 2 {
            let center = pos2(r.right() - ring_radius - 14.0, r.center().y);
            anneau(
                painter,
                center,
                ring_radius,
                if compact { 4.0 } else { 5.0 },
                budget_fraction,
                accent,
                None,
            );
            painter.text(
                r.min + vec2(18.0, value_y + value_font * 0.5),
                Align2::LEFT_CENTER,
                value,
                FontId::new(value_font - 2.0, egui::FontFamily::Name("Inter600".into())),
                ENCRE,
            );
        } else {
            let galley = painter.layout_no_wrap(
                value.clone(),
                FontId::new(value_font, egui::FontFamily::Name("Inter600".into())),
                ENCRE,
            );
            let at = r.min + vec2(18.0, value_y);
            painter.galley(at, galley.clone(), ENCRE);
            if let Some(unit) = unit {
                painter.text(
                    at + vec2(galley.size().x + 6.0, galley.size().y - 6.0),
                    Align2::LEFT_BOTTOM,
                    *unit,
                    FontId::proportional(12.0),
                    DISCRET,
                );
            }
            if i == 1 {
                // Une échelle d'activité : cinq traits, éclairés selon le débit observé.
                let lit = ((rate_per_minute / 12.0) * 5.0).ceil().clamp(0.0, 5.0) as usize;
                for k in 0..5 {
                    let x = r.right() - 60.0 + k as f32 * 9.0;
                    let h = (8.0 + k as f32 * 4.0) * if compact { 0.6 } else { 1.0 };
                    painter.rect_filled(
                        Rect::from_min_size(pos2(x, r.bottom() - 14.0 - h), vec2(5.0, h)),
                        2,
                        if k < lit { accent } else { PISTE },
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chaque_pilote_connu_a_son_monogramme() {
        assert_eq!(identite("claude-code").0, "C");
        assert_eq!(identite("codex").0, "X");
        assert_eq!(identite("gemini").0, "G");
        assert_eq!(identite("local:qwen3-1.7b").0, "L");
        assert_eq!(identite("prophet-agent").0, "P");
    }

    #[test]
    fn un_anneau_vide_ne_trace_rien_et_un_plein_fait_le_tour() {
        assert!(arc_points(pos2(50.0, 50.0), 20.0, 0.0).is_empty());
        assert!(arc_points(pos2(50.0, 50.0), 20.0, -3.0).is_empty());
        let plein = arc_points(pos2(50.0, 50.0), 20.0, 1.0);
        assert_eq!(plein.len(), 61);
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
}
