//! Espace de lecture du document, avec contexte, lignes et versions explicitement nommées.
use agentd::{Inspection, PreviewContent};
use egui::{Color32, Frame, RichText, Stroke};

use crate::file_review::{Kind, Review};
use crate::supervision::bouton;
use crate::theme::Accent;
use crate::theme::palette::{ACCOMPLI, ATTENTE, DISCRET, ENCRE, VERRE_HAUT};

const INK: Color32 = ENCRE;
const MUTED: Color32 = DISCRET;
const GREEN: Color32 = ACCOMPLI;
const RED: Color32 = ATTENTE;
/// Le fond d'une ligne ajoutée : un jade de nuit, lisible sous le monospace clair.
const AJOUT: Color32 = Color32::from_rgba_premultiplied(8, 30, 20, 200);
/// Le fond d'une ligne retirée : la braise, éteinte.
const RETRAIT: Color32 = Color32::from_rgba_premultiplied(40, 12, 8, 200);

fn small(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text).size(12.0).color(MUTED));
}

pub(crate) fn draw(ui: &mut egui::Ui, info: &Inspection, files: &Review) -> Option<String> {
    let mut requested = None;
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("review-file-select")
            .width(ui.available_width().min(300.0))
            .selected_text(files.path.as_deref().unwrap_or("Choisir un changement"))
            .show_ui(ui, |ui| {
                if let Some(changes) = info
                    .result
                    .as_ref()
                    .and_then(|r| r["diff"]["changes"].as_array())
                {
                    for path in changes.iter().filter_map(|c| c["path"].as_str()) {
                        if ui
                            .selectable_label(files.path.as_deref() == Some(path), path)
                            .clicked()
                        {
                            requested = Some(path.into());
                        }
                    }
                }
            });
        if files.path.is_some()
            && ui
                .add_enabled_ui(!files.loading(), |ui| {
                    bouton(ui, "review-refresh", "Actualiser", false)
                })
                .inner
                .clicked()
        {
            requested = files.path.clone();
        }
    });
    ui.add_space(16.0);
    if let Some(error) = &files.error {
        ui.label(crate::supervision::titre("Aperçu non confirmé", 24.0));
        ui.add_space(8.0);
        ui.label(RichText::new(error).color(RED).size(14.0));
        return requested;
    }
    let Some(document) = &files.document else {
        ui.label(crate::supervision::titre(
            if files.loading() {
                "Lecture des versions…"
            } else {
                "Choisissez un fichier à examiner"
            },
            25.0,
        ));
        ui.add_space(8.0);
        small(
            ui,
            "L'état de départ et la proposition sont lus dans le travail de cette mission.",
        );
        return requested;
    };
    let file = &document.review.file;
    ui.horizontal_wrapped(|ui| {
        for (label, version, color) in [
            ("État de départ", &file.before, MUTED),
            ("Proposition", &file.after, GREEN),
        ] {
            Frame::new()
                .fill(VERRE_HAUT)
                .stroke(Stroke::new(1.0, Accent::de(ui.ctx()).fil))
                .corner_radius(3)
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.label(RichText::new(label).size(12.0).color(color));
                    ui.label(
                        RichText::new(version.as_ref().map_or_else(
                            || "Fichier absent".into(),
                            |v| format!("{} octets", v.size),
                        ))
                        .size(14.0)
                        .color(INK),
                    );
                });
        }
        if let Some(version) = &file.after
            && let PreviewContent::Text { text } = &version.content
            && bouton(ui, "review-copy-after", "Copier la proposition", false).clicked()
        {
            ui.ctx().copy_text(text.clone());
        }
    });
    ui.add_space(18.0);
    if let Some(comparison) = &document.lines {
        let rows = &comparison.rows;
        if comparison.grouped {
            small(
                ui,
                "Comparaison simplifiée : le bloc central est présenté en deux versions complètes.",
            );
        }
        let added = rows.iter().filter(|r| r.kind == Kind::Added).count();
        let removed = rows.iter().filter(|r| r.kind == Kind::Removed).count();
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "+ {added} {}",
                    if added == 1 { "ligne" } else { "lignes" }
                ))
                .color(GREEN)
                .size(12.0),
            );
            ui.label(
                RichText::new(format!(
                    "− {removed} {}",
                    if removed == 1 { "ligne" } else { "lignes" }
                ))
                .color(RED)
                .size(12.0),
            );
            small(ui, "Départ / Proposition");
        });
        ui.add_space(8.0);
        if rows.is_empty() {
            small(
                ui,
                "Le contenu textuel est vide. Comparez les propriétés des versions.",
            );
        }
        let row_width = (ui.available_width() - 16.0).max(0.0);
        ui.scope(|ui| {
            // show_rows mesure l'espacement avant son callback : il doit déjà être fixé ici.
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::both()
                .id_salt(("review-lines", &document.review.task, &file.path))
                .max_height(420.0)
                .auto_shrink([false, true])
                .show_rows(ui, 24.0, rows.len(), |ui, range| {
                    for line in &rows[range] {
                        let (fill, color, marker) = match line.kind {
                            Kind::Equal => (Color32::TRANSPARENT, INK, " "),
                            Kind::Added => (AJOUT, GREEN, "+"),
                            Kind::Removed => (RETRAIT, RED, "−"),
                        };
                        Frame::new()
                            .fill(fill)
                            .inner_margin(egui::Margin::symmetric(8, 0))
                            .show(ui, |ui| {
                                ui.set_min_width(row_width);
                                ui.horizontal(|ui| {
                                    for number in [line.before, line.after] {
                                        ui.add_sized(
                                            [32.0, 24.0],
                                            egui::Label::new(
                                                RichText::new(
                                                    number.map_or_else(String::new, |n| {
                                                        n.to_string()
                                                    }),
                                                )
                                                .monospace()
                                                .size(11.0)
                                                .color(MUTED),
                                            ),
                                        );
                                    }
                                    ui.label(RichText::new(marker).monospace().color(color));
                                    let content =
                                        line.text.strip_suffix('\n').unwrap_or(&line.text);
                                    let content = content.strip_suffix('\r').unwrap_or(content);
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(content)
                                                .monospace()
                                                .size(13.0)
                                                .color(color),
                                        )
                                        .extend(),
                                    )
                                    .on_hover_text(
                                        if line.text.ends_with("\r\n") {
                                            "Fin de ligne CRLF"
                                        } else if line.text.ends_with('\n') {
                                            "Fin de ligne LF"
                                        } else {
                                            "Sans retour à la ligne final"
                                        },
                                    );
                                });
                            });
                    }
                });
        });
    } else {
        for (name, version) in [
            ("État de départ", &file.before),
            ("Proposition", &file.after),
        ] {
            if let Some(version) = version {
                let reason = match version.content {
                    PreviewContent::Binary => "Ce fichier contient des données binaires.",
                    PreviewContent::TooLarge => "Ce fichier dépasse la limite d'aperçu de 64 Kio.",
                    PreviewContent::Text { .. } => "Version textuelle conservée.",
                };
                ui.label(format!("{name} : {reason}"));
            }
        }
    }
    ui.add_space(14.0);
    ui.separator();
    ui.add_space(10.0);
    small(
        ui,
        "Comparaison avec la capture initiale · Les originaux sont préservés",
    );
    ui.horizontal_wrapped(|ui| {
        for (label, version) in [("Départ", &file.before), ("Proposition", &file.after)] {
            if let Some(version) = version {
                small(
                    ui,
                    format!("{label} · permissions {:04o}", version.mode & 0o7777),
                );
            }
        }
    });
    requested
}
