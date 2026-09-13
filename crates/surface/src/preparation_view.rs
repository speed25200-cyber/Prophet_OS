//! Composition d'une mission : une intention, un contexte et un passage de relais explicite.
use crate::preparation::Preparation;
use crate::supervision::bouton;
use egui::{Color32, Frame, RichText, Stroke, vec2};

const INK: Color32 = Color32::from_rgb(27, 35, 50);
const MUTED: Color32 = Color32::from_rgb(105, 115, 132);
const LINE: Color32 = Color32::from_rgb(218, 224, 233);
const BLUE: Color32 = Color32::from_rgb(51, 94, 184);

fn title(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text)
        .size(size)
        .family(egui::FontFamily::Name("Inter600".into()))
}
fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(MUTED));
}

pub(crate) fn draw(ui: &mut egui::Ui, preparation: &mut Preparation) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        if bouton(ui, "mission-prepare-back", "← Espace de supervision", false).clicked() {
            close = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            caption(ui, "NOUVELLE MISSION")
        });
    });
    ui.add_space(18.0);
    egui::ScrollArea::vertical()
        .id_salt("preparation-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let compact = ui.available_width() < 900.0;
            let short = ui.available_height() < 680.0;
            if compact {
                ui.label(title("Que voulez-vous accomplir ?", 27.0).color(INK));
                ui.add_space(10.0);
                form(ui, preparation, true);
            } else {
                let width = ui.available_width();
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        vec2((width * 0.34).min(420.0), 560.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| direction(ui, preparation, short),
                    );
                    ui.add_space(18.0);
                    ui.allocate_ui_with_layout(
                        vec2(ui.available_width(), 560.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| form(ui, preparation, short),
                    );
                });
            }
        });
    close
}

fn direction(ui: &mut egui::Ui, preparation: &Preparation, short: bool) {
    Frame::new()
        .fill(INK)
        .corner_radius(26)
        .inner_margin(if short { 22 } else { 30 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("DE L'INTENTION AU TRAVAIL")
                    .size(10.0)
                    .color(Color32::from_rgb(148, 176, 215)),
            );
            ui.add_space(if short { 18.0 } else { 26.0 });
            ui.label(
                title("Donnez une\ndirection.", if short { 32.0 } else { 38.0 })
                    .line_height(Some(if short { 36.0 } else { 44.0 }))
                    .color(Color32::WHITE),
            );
            ui.label(
                title("Gardez\nla main.", if short { 32.0 } else { 38.0 })
                    .line_height(Some(if short { 36.0 } else { 44.0 }))
                    .color(Color32::from_rgb(165, 191, 229)),
            );
            ui.add_space(if short { 18.0 } else { 32.0 });
            for (number, heading, description) in [
                ("01", "Définir", "Votre objectif et son contexte."),
                ("02", "Examiner", "Les accès et les limites du plan."),
                ("03", "Superviser", "Vous lancez. Vous pouvez arrêter."),
                ("04", "Relire", "Le résultat et les fichiers préparés."),
            ] {
                ui.horizontal_top(|ui| {
                    ui.label(
                        RichText::new(number)
                            .size(11.0)
                            .color(Color32::from_rgb(126, 160, 206)),
                    );
                    ui.vertical(|ui| {
                        ui.label(title(heading, 15.0).color(Color32::WHITE));
                        ui.label(
                            RichText::new(description)
                                .size(12.0)
                                .color(Color32::from_rgb(161, 173, 191)),
                        );
                    });
                });
                ui.add_space(if short { 8.0 } else { 12.0 });
            }
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(12.0);
            ui.label(
                RichText::new(
                    if preparation.selected().is_some_and(|p| !p.models.is_empty()) {
                        "Moteur local disponible"
                    } else {
                        "Disponibilité du moteur à vérifier"
                    },
                )
                .size(11.0)
                .color(Color32::from_rgb(165, 191, 229)),
            );
        });
}

fn form(ui: &mut egui::Ui, preparation: &mut Preparation, compact: bool) {
    let ctx = ui.ctx().clone();
    let locked = preparation.pending() || preparation.attempted_id().is_some();
    Frame::new().fill(Color32::WHITE).stroke(Stroke::new(1.0,LINE)).corner_radius(24).inner_margin(if compact{22}else{30}).show(ui,|ui| {
        ui.set_width(ui.available_width());
        caption(ui,"01 / VOTRE OBJECTIF");
        if !compact {ui.label(title("Que voulez-vous accomplir ?",29.0));}
        ui.add_space(6.0);
        ui.add_enabled_ui(!locked,|ui| {
            ui.add(egui::TextEdit::multiline(&mut preparation.intent).id(egui::Id::new("mission-intent")).hint_text("Décrivez le résultat attendu, les documents utiles et vos contraintes…").font(egui::FontId::proportional(if compact{18.0}else{21.0})).desired_width(f32::INFINITY).desired_rows(if compact{3}else{4}).char_limit(16_384).frame(Frame::NONE));
        });
        ui.add_space(8.0);ui.separator();ui.add_space(10.0);
        ui.horizontal(|ui| {
            caption(ui,"02 / CONTEXTE ET MODÈLE");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui| {
                ui.add_enabled_ui(!locked && !preparation.loading(),|ui| {
                    if bouton(ui,"mission-options-refresh",if preparation.loading(){"Recherche…"}else{"Actualiser"},false).clicked(){preparation.discover(&ctx);}
                });
            });
        });
        let profiles=preparation.options().map(|o|o.profiles.clone()).unwrap_or_default();
        if profiles.is_empty() && !preparation.loading() {
            ui.label(RichText::new("Aucun contexte de mission configuré.").color(MUTED));
        }
        ui.add_enabled_ui(!locked && !preparation.loading(),|ui| {
            egui::ComboBox::from_id_salt("mission-profile").width(ui.available_width().min(440.0)).selected_text(preparation.selected().map_or("Choisir un contexte",|p|p.name.as_str())).show_ui(ui,|ui| {
                for profile in &profiles {ui.selectable_value(&mut preparation.profile,profile.id.clone(),&profile.name);}
            });
            preparation.reconcile();
            if let Some(profile)=preparation.selected().cloned() {
                caption(ui,&profile.scopes.join(" · "));
                ui.add_space(4.0);
                egui::ComboBox::from_id_salt("mission-model").width(ui.available_width().min(440.0)).selected_text(if preparation.model.is_empty(){"Aucun modèle disponible"}else{&preparation.model}).show_ui(ui,|ui| {
                    for model in &profile.models {ui.selectable_value(&mut preparation.model,model.clone(),model);}
                });
            }
        });
        ui.add_space(12.0);
        if let Some(profile)=preparation.selected() {
            Frame::new().fill(Color32::from_rgb(245,247,251)).corner_radius(12).inner_margin(14).show(ui,|ui| {
                ui.set_width(ui.available_width());
                ui.label(title("Le cadre de la mission",13.0).color(BLUE));
                ui.label(RichText::new(format!("{} tokens · {} s au maximum",profile.limits.tokens,profile.limits.wall_time_s)).size(12.0).color(INK));
                caption(ui,"Les accès détaillés seront présentés dans le plan.");
                if profile.web {
                    match preparation.options().and_then(|o|o.browser.as_ref()) {
                        Some(state) if state.ready => caption(ui,&format!("Consulte le web par le navigateur piloté · {}",state.detail)),
                        Some(state) => {ui.label(RichText::new(format!("Navigateur piloté indisponible : {}",state.detail)).size(12.0).color(Color32::from_rgb(159,70,48)));}
                        None => {ui.label(RichText::new("Ce contexte consulte le web, mais le service ne configure aucun navigateur piloté.").size(12.0).color(Color32::from_rgb(159,70,48)));}
                    }
                }
            });
        }
        if let Some(error)=preparation.options().and_then(|o|o.model_error.as_ref()) {
            ui.label(RichText::new(format!("Moteur indisponible : {error}")).size(12.0).color(Color32::from_rgb(159,70,48)));
        }
        if let Some(error)=preparation.error() {ui.label(RichText::new(error).size(12.0).color(Color32::from_rgb(159,70,48)));}
        ui.add_space(14.0);
        if let Some(id)=preparation.attempted_id() {
            caption(ui,&format!("Référence : {id}"));
            if preparation.pending(){ui.label("Préparation en cours…");}
            else {
                caption(ui,"Retrouvez le plan avant de préparer une autre mission.");
                ui.horizontal_wrapped(|ui| {
                    if bouton(ui,"mission-prepare-recover","Retrouver le plan",true).clicked(){preparation.recover(&ctx);}
                    if bouton(ui,"mission-prepare-reset","Nouveau brouillon",false).clicked(){preparation.reset();}
                });
            }
        } else {
            let browser_ready=preparation.options().and_then(|o|o.browser.as_ref()).is_some_and(|b|b.ready);
        let enabled=!locked && !preparation.loading() && !preparation.intent.trim().is_empty() && preparation.selected().is_some_and(|p|p.models.contains(&preparation.model) && (!p.web || browser_ready));
            ui.add_enabled_ui(enabled,|ui| {
                if bouton(ui,"mission-prepare-submit","Préparer le plan →",true).clicked()
                    && let Err(error)=preparation.submit(&ctx){preparation.report_error(error);}
            });
            ui.add_space(4.0);
            caption(ui,"Vous pourrez examiner le plan avant de lancer la mission.");
        }
    });
}
