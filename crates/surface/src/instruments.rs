//! Les instruments de l'atelier : ce qui se lit d'un coup d'œil, dessiné plutôt qu'écrit.
//!
//! Un anneau dit une proportion mieux qu'un pourcentage ; un monogramme dit un agent mieux
//! qu'un identifiant. Tout est statique : rien ne bouge tant que l'état ne change pas, pour
//! qu'une supervision au repos reste au repos.

use crate::hud;
use crate::theme::Accent;
use crate::theme::palette::{ENCRE, TRAIT};
use egui::{Align2, Color32, FontId, Rect, Stroke, pos2, vec2};

/// Trace un arc de cercle par une ligne brisée, de midi dans le sens horaire.
pub(crate) fn arc(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    fraction: f32,
    stroke: Stroke,
) {
    hud::arc(painter, center, radius, fraction, stroke);
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
    painter.circle_stroke(center, radius, Stroke::new(thickness, TRAIT));
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
            FontId::new(radius * 0.62, hud::fin()),
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
        size * 0.22,
        fill,
    );
    painter.rect_stroke(
        Rect::from_center_size(center, vec2(size, size)),
        size * 0.22,
        Stroke::new(1.0, Color32::from_white_alpha(24)),
        egui::StrokeKind::Inside,
    );
    painter.text(
        center,
        Align2::CENTER_CENTER,
        letter,
        FontId::new(size * 0.56, hud::fort()),
        ENCRE,
    );
}

/// Lettre et teinte d'un pilote. Les teintes sont sourdes : elles distinguent, sans crier.
pub(crate) fn identite(agent: &str) -> (&'static str, Color32) {
    let agent = agent.to_ascii_lowercase();
    if agent.contains("claude") {
        ("C", Color32::from_rgb(132, 74, 44))
    } else if agent.contains("codex") || agent.contains("gpt") {
        ("X", Color32::from_rgb(52, 60, 72))
    } else if agent.contains("gemini") {
        ("G", Color32::from_rgb(46, 74, 120))
    } else if agent.starts_with("local") || agent.contains("qwen") || agent.contains("llama") {
        ("L", Color32::from_rgb(34, 88, 76))
    } else {
        ("P", Color32::from_rgb(74, 58, 116))
    }
}

/// Une ligne de trois jauges : étapes, activité, budget.
///
/// La forme compacte sert à l'inspecteur relié aux services, où la proposition et ses fichiers
/// doivent rester visibles sans défiler : les instruments s'y lisent, ils n'y règnent pas.
pub(crate) fn tableau(
    ui: &mut egui::Ui,
    steps: u32,
    rate_per_minute: f32,
    budget_fraction: f32,
    color: Color32,
    compact: bool,
) {
    let accent = Accent::de(ui.ctx());
    let width = ui.available_width();
    let radius = if compact { 24.0 } else { 52.0 };
    let height = radius * 2.0 + 10.0;
    let (row, _) = ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
    let painter = ui.painter();
    // Les étapes n'ont pas de plafond naturel : la couronne se remplit par centaine, et le
    // chiffre dit le vrai compte. L'activité sature à douze étapes par minute, au-delà de
    // quoi l'œil ne distingue plus.
    let gauges = [
        ((steps % 100) as f32 / 100.0, steps.to_string(), "ÉTAPES"),
        (
            (rate_per_minute / 12.0).clamp(0.0, 1.0),
            format!("{rate_per_minute:.0}"),
            "PAR MIN",
        ),
        (
            budget_fraction.clamp(0.0, 1.0),
            format!("{:.0}", budget_fraction.clamp(0.0, 1.0) * 100.0),
            "% BUDGET",
        ),
    ];
    let slot = width / 3.0;
    for (i, (fraction, value, label)) in gauges.iter().enumerate() {
        let center = pos2(
            row.left() + slot * (i as f32 + 0.5),
            row.top() + radius + 4.0,
        );
        hud::jauge(
            painter,
            center,
            radius,
            *fraction,
            color,
            &accent,
            (value, label),
        );
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
}
