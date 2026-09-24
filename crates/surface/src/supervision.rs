//! Espace de supervision : les missions reçues, leur contexte et les décisions humaines.

use egui::{Align2, Color32, FontId, Frame, RichText, Stroke, pos2, vec2};

use crate::atelier::{Atelier, ClientCard, CommandeDePoids, EntreeCatalogue, Page};
use crate::fenetre::Reponse;
use crate::hud;
use crate::scene::{Courant, Etat, Scene};
use crate::theme::palette::{
    ACCOMPLI, ATTENTE, ATTENTE_VOILE, CREUX, DISCRET, EFFACE, ENCRE, TRAIT, VERRE_HAUT, VOILE,
};
use crate::theme::{ACCENTS, Accent};

#[derive(Default)]
pub(crate) struct Supervision {
    pub(crate) missions: crate::missions::Missions,
    pub(crate) preparation: crate::preparation::Preparation,
    /// L'arrêt d'urgence de toutes les missions en main.
    pub(crate) arret: crate::arret::Arret,
    composing: bool,
    prepared_selection: Option<String>,
    detail_tab: crate::mission_details::Tab,
    detail_id: Option<String>,
    selection: Option<String>,
    filtre: Filtre,
    examen: Option<String>,
    focus_compact: bool,
    isolate: bool,
    query: String,
    apparence: Option<(String, bool)>,
    /// L'objectif tapé dans l'espace vide, avant d'ouvrir sa préparation.
    brouillon_accueil: String,
    /// Le champ de l'objectif reçoit le clavier à la prochaine image.
    focus_intent: bool,
    /// Un widget avait le focus à l'image précédente : egui le retire dès qu'Échap arrive,
    /// avant que les raccourcis ne soient lus.
    focus_precedent: bool,
    /// Le réglage « Mouvement réduit » tel qu'il a été appliqué au style, pour ne toucher au
    /// style qu'au changement.
    mouvement_applique: Option<bool>,
    /// La demande de code d'approbation que la source transmet (ADR 0057).
    pub(crate) presence: Option<crate::presence::Demande>,
    /// Le code en cours de saisie ; vidé dès qu'il part.
    code_saisi: String,
    /// Ce que capd dit du code d'approbation de cette machine (ADR 0057).
    pub(crate) code: Option<crate::presence::EtatDuCode>,
    /// La page Système demande à choisir le code : la réponse part à la fin de l'image.
    demander_le_code: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Filtre {
    #[default]
    Toutes,
    Attention,
    Actives,
    Terminees,
}

impl Filtre {
    fn inclut(self, courant: &Courant) -> bool {
        match self {
            Self::Toutes => true,
            Self::Attention => courant.reclame(),
            Self::Actives => courant.etat == Etat::Court,
            Self::Terminees => courant
                .task_state
                .map_or(courant.etat == Etat::Fini, agentd::State::is_terminal),
        }
    }
}

/// Le style d'egui, réglé sur la nuit : verre, fil d'accent, encre.
///
/// Il se réinstalle à chaque changement d'accent, parce que les widgets natifs d'egui (choix
/// déroulants, cases, curseur de saisie) lisent leurs couleurs dans le style.
pub(crate) fn installer_style(ctx: &egui::Context) {
    let accent = Accent::de(ctx);
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(ENCRE);
    style.visuals.panel_fill = Color32::TRANSPARENT;
    style.visuals.window_fill = Color32::from_rgb(10, 14, 19);
    style.visuals.window_stroke = Stroke::new(1.0, accent.fil_vif);
    style.visuals.window_corner_radius = 4.into();
    style.visuals.window_shadow = egui::Shadow::NONE;
    style.visuals.popup_shadow = egui::Shadow::NONE;
    style.visuals.extreme_bg_color = CREUX;
    style.visuals.faint_bg_color = VERRE_HAUT;
    style.visuals.code_bg_color = CREUX;
    style.visuals.hyperlink_color = accent.vif;
    style.visuals.selection.bg_fill = hud::voile(accent.vif, 90);
    style.visuals.selection.stroke = Stroke::new(1.0, accent.vif);
    style.visuals.text_cursor.stroke = Stroke::new(2.0, accent.vif);
    style.visuals.widgets.noninteractive.bg_fill = Color32::TRANSPARENT;
    style.visuals.widgets.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, TRAIT);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, ENCRE);
    style.visuals.widgets.inactive.bg_fill = VERRE_HAUT;
    style.visuals.widgets.inactive.weak_bg_fill = VERRE_HAUT;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, accent.fil);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, ENCRE);
    style.visuals.widgets.hovered.bg_fill = hud::voile(accent.vif, 34);
    style.visuals.widgets.hovered.weak_bg_fill = hud::voile(accent.vif, 34);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, accent.fil_vif);
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, accent.vif);
    style.visuals.widgets.active.bg_fill = hud::voile(accent.vif, 60);
    style.visuals.widgets.active.weak_bg_fill = hud::voile(accent.vif, 60);
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, accent.vif);
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, ENCRE);
    style.visuals.widgets.open.bg_fill = VERRE_HAUT;
    style.visuals.widgets.open.weak_bg_fill = VERRE_HAUT;
    style.visuals.widgets.open.bg_stroke = Stroke::new(1.0, accent.fil_vif);
    style.visuals.widgets.open.fg_stroke = Stroke::new(1.0, accent.vif);
    for widget in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widget.corner_radius = 3.into();
    }
    style.spacing.item_spacing = vec2(10.0, 10.0);
    style.spacing.scroll.fade.strength = 0.0;
    style.spacing.button_padding = vec2(14.0, 8.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(12.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, FontId::new(28.0, hud::fin()));
    ctx.set_style_of(egui::Theme::Dark, style);
}

/// Une plaque de verre à crochets, pour tout ce qui se lit.
fn plaque<R>(ui: &mut egui::Ui, margin: i8, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let accent = Accent::de(ui.ctx());
    hud::plaque(ui, &accent, margin, add).inner
}

/// Un texte de titre, en graisse fine.
pub(crate) fn titre(text: impl Into<String>, size: f32) -> RichText {
    hud::titre(text, size)
}

/// Une étiquette en capitales espacées, dans l'accent sourd : le vocabulaire des rubriques.
pub(crate) fn etiquette(ui: &mut egui::Ui, text: impl Into<String>) {
    let accent = Accent::de(ui.ctx());
    hud::etiquette(ui, text, accent.sourd);
}

pub(crate) fn petit(ui: &mut egui::Ui, texte: impl Into<String>) {
    ui.label(RichText::new(texte).size(11.0).color(DISCRET));
}

/// Un bouton du tableau de bord ; l'identifiant reste stable pour les parcours.
pub(crate) fn bouton(ui: &mut egui::Ui, id: &str, texte: &str, actif: bool) -> egui::Response {
    let accent = Accent::de(ui.ctx());
    hud::bouton(ui, id, texte, actif, &accent)
}

fn action(ui: &mut egui::Ui, id: &str, texte: &str) -> egui::Response {
    bouton(ui, id, texte, true)
}

fn largeur(ui: &mut egui::Ui, max: f32, contenu: impl FnOnce(&mut egui::Ui)) {
    let available = ui.available_rect_before_wrap();
    let width = available.width().min(max);
    let rect = egui::Rect::from_min_size(
        pos2(available.center().x - width * 0.5, available.top()),
        vec2(width, available.height()),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.set_width(width);
        contenu(ui);
    });
}

/// Les missions que l'arrêt d'urgence arrêterait : toutes celles qui ne sont pas finies.
fn a_arreter(scene: &Scene) -> usize {
    scene
        .courants
        .iter()
        .filter(|c| {
            c.task_state
                .map_or(c.etat != Etat::Fini, |state| !state.is_terminal())
        })
        .count()
}

fn statut(etat: Etat, accent: &Accent) -> (&'static str, Color32) {
    match etat {
        Etat::Court => ("En cours", accent.vif),
        Etat::Attend => ("Votre décision", ATTENTE),
        Etat::Bloque => ("À examiner", ATTENTE),
        Etat::Fini => ("Terminée", DISCRET),
    }
}

fn statut_mission(c: &Courant, accent: &Accent) -> (&'static str, Color32) {
    c.task_state.map_or_else(
        || statut(c.etat, accent),
        |state| crate::mission_details::status(state, accent),
    )
}

fn pastille(ui: &mut egui::Ui, texte: &str, couleur: Color32) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(6.0, 10.0), egui::Sense::hover());
        ui.painter().circle_filled(r.center(), 2.5, couleur);
        hud::etiquette(ui, texte.to_uppercase(), couleur);
    });
}

impl Supervision {
    /// La mission choisie, pour que le champ l'éclaire.
    pub(crate) fn selection(&self) -> Option<&str> {
        self.selection.as_deref()
    }

    pub(crate) fn dessiner(
        &mut self,
        root: &mut egui::Ui,
        atelier: &mut Atelier,
        scene: &Scene,
        reponse: &mut Option<Reponse>,
    ) {
        let ctx = root.ctx().clone();
        self.preparation.update();
        self.arret.update();
        // Les ordres dits pendant l'écoute : préparer l'objectif relu, lancer la mission choisie
        // (l'approbation de l'humain, dite), entendre son résultat. Un refus se lit sous le
        // brouillon, comme une erreur de préparation.
        while let Some(ordre) = self.preparation.take_order() {
            match &ordre {
                voice::Ordre::Preparer => {
                    if let Err(error) = self.preparation.submit(&ctx) {
                        self.preparation
                            .report_error(format!("« prépare » : {error}"));
                    }
                }
                voice::Ordre::Lancer => {
                    if let Err(error) = self.missions.command(crate::missions::Action::Start) {
                        self.preparation
                            .report_error(format!("« lance la mission » : {error}"));
                    }
                }
                voice::Ordre::Resultat => self.missions.announce_now(),
                // La décision montrée, tranchée par la voix, cette fois (ADR 0041).
                voice::Ordre::Accorder | voice::Ordre::Refuser => {
                    if scene.decision.is_some() {
                        *reponse = Some(if ordre == voice::Ordre::Accorder {
                            Reponse::Accepte
                        } else {
                            Reponse::Refuse
                        });
                        self.examen = None;
                    } else {
                        self.preparation.report_error(
                            "« accorde » / « refuse » : aucune décision n'attend.".to_owned(),
                        );
                    }
                }
                // « Ouvre … » : une application du bureau ou un outil publié, par le lanceur
                // de la session ; sans lanceur (hors du bureau), on le dit.
                voice::Ordre::Ouvrir(cible) => {
                    if !ouvrir_application(&voice::arguments_du_lanceur(cible)) {
                        self.preparation.report_error(format!(
                            "« ouvre {cible} » : pas de lanceur du bureau sur cette machine."
                        ));
                    }
                }
                voice::Ordre::Intention => {}
            }
        }
        if let Some(plan) = self.preparation.take_prepared() {
            self.prepared_selection = Some(plan.task);
            self.composing = false;
            self.filtre = Filtre::Toutes;
            self.query.clear();
            self.focus_compact = true;
            self.isolate = true;
            atelier.page = Page::Accueil;
        }
        let compact = root.available_width() < 900.0;
        self.raccourcis(&ctx, atelier);
        appliquer_le_mouvement(&ctx, atelier.mouvement_reduit, &mut self.mouvement_applique);
        crate::desk::chrome(root, atelier, scene, compact);
        if scene.decision.is_some() {
            egui::Panel::top("attention-globale")
                .exact_size(54.0)
                .frame(
                    Frame::new()
                        .fill(ATTENTE_VOILE)
                        .inner_margin(egui::Margin::symmetric(28, 10)),
                )
                .show(root, |ui| {
                    let r = ui.max_rect();
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            pos2(r.left() - 28.0, r.top() - 10.0),
                            vec2(3.0, r.height() + 20.0),
                        ),
                        0,
                        ATTENTE,
                    );
                    hud::rangee(ui, |ui| {
                        let (dot, _) =
                            ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
                        ui.painter().circle_filled(dot.center(), 3.0, ATTENTE);
                        hud::etiquette(ui, "DÉCISION EN ATTENTE", ATTENTE);
                        ui.label(
                            RichText::new("Une action attend votre accord")
                                .size(14.0)
                                .color(ENCRE),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if action(ui, "examiner-decision", "Examiner l'action").clicked() {
                                self.examen = empreinte_decision(scene);
                            }
                        });
                    });
                });
        }
        if atelier.page == Page::Conversation {
            egui::Panel::bottom("composition")
                .resizable(false)
                .frame(
                    Frame::new()
                        .fill(Color32::TRANSPARENT)
                        .inner_margin(if compact { 12 } else { 24 }),
                )
                .show(root, |ui| largeur(ui, 900.0, |ui| composer(ui, atelier)));
        }
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(Color32::TRANSPARENT)
                    .inner_margin(if compact { 14 } else { 30 }),
            )
            .show(root, |ui| match atelier.page {
                Page::Accueil if self.composing => largeur(ui, 1240.0, |ui| {
                    if crate::preparation_view::draw(ui, &mut self.preparation) {
                        self.composing = false;
                    }
                    if std::mem::take(&mut self.focus_intent) {
                        ui.memory_mut(|m| m.request_focus(egui::Id::new("mission-intent")));
                    }
                }),
                Page::Accueil => largeur(ui, 1560.0, |ui| self.accueil(ui, scene)),
                Page::Conversation => largeur(ui, 900.0, |ui| {
                    if !atelier.generation
                        && (!atelier.brouillon.trim().is_empty() || !atelier.tours.is_empty())
                        && bouton(
                            ui,
                            "conversation-vers-mission",
                            "Préparer une mission à partir de ma demande",
                            false,
                        )
                        .clicked()
                    {
                        if self.preparation.attempted_id().is_some()
                            && self.preparation.error().is_none()
                            && !self.preparation.pending()
                        {
                            self.preparation.reset();
                        }
                        if self.preparation.attempted_id().is_none() {
                            self.preparation.intent = if atelier.brouillon.trim().is_empty() {
                                atelier
                                    .tours
                                    .last()
                                    .map(|t| t.demande.clone())
                                    .unwrap_or_default()
                            } else {
                                atelier.brouillon.clone()
                            };
                        }
                        self.composing = true;
                        atelier.page = Page::Accueil;
                        self.preparation.discover(&ctx);
                    }
                    conversation(ui, atelier);
                }),
                Page::Modeles => largeur(ui, 1100.0, |ui| modeles(ui, atelier)),
                Page::Activite => largeur(ui, 1100.0, |ui| self.systeme(ui, scene)),
            });
        if self.examen != empreinte_decision(scene) {
            self.examen = None;
        }
        if self.examen.is_some() {
            self.decision(&ctx, scene, reponse);
        }
        if self.arret.confirmation() {
            self.confirmer_l_arret(&ctx, scene);
        }
        if std::mem::take(&mut self.demander_le_code) {
            *reponse = Some(Reponse::DemanderLeCode);
        }
        if self.presence.is_some() {
            self.code_d_approbation(&ctx, reponse);
        } else {
            self.code_saisi.clear();
        }
        self.focus_precedent = ctx.memory(|m| m.focused().is_some());
    }

    fn accueil(&mut self, ui: &mut egui::Ui, scene: &Scene) {
        let accent = Accent::de(ui.ctx());
        let compact = ui.available_width() < 850.0;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                etiquette(ui, "SUPERVISION");
                ui.label(titre("Vos missions", if compact { 28.0 } else { 34.0 }));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if action(ui, "preparer-mission", "Nouvel objectif  ↗")
                    .on_hover_text("Ctrl N")
                    .clicked()
                {
                    self.ouvrir_la_preparation(ui.ctx());
                }
                // L'arrêt d'urgence n'apparaît que s'il y a quelque chose à arrêter.
                if a_arreter(scene) > 0 || self.arret.en_cours() {
                    let en_cours = self.arret.en_cours();
                    if ui
                        .add_enabled_ui(!en_cours, |ui| {
                            bouton(
                                ui,
                                "arret-tout",
                                if en_cours {
                                    "Arrêt…"
                                } else {
                                    "Tout arrêter"
                                },
                                false,
                            )
                        })
                        .inner
                        .on_hover_text("Ctrl Maj Échap")
                        .clicked()
                    {
                        self.arret.demander();
                    }
                }
            });
        });
        if let Some(issue) = self.arret.issue().cloned() {
            ui.add_space(8.0);
            hud::rangee(ui, |ui| {
                ui.label(
                    RichText::new(&issue.texte)
                        .size(13.0)
                        .color(if issue.erreur { ATTENTE } else { ACCOMPLI }),
                );
                if bouton(ui, "arret-ecarter", "Compris", false).clicked() {
                    self.arret.ecarter();
                }
            });
        }
        ui.add_space(if compact { 10.0 } else { 14.0 });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (filtre, id, label) in [
                (Filtre::Toutes, "filter-all", "Tout"),
                (Filtre::Attention, "filter-attention", "À examiner"),
                (Filtre::Actives, "filter-active", "En cours"),
                (Filtre::Terminees, "filter-done", "Terminées"),
            ] {
                if bouton(ui, id, label, self.filtre == filtre).clicked() {
                    self.filtre = filtre;
                    self.focus_compact = false;
                    self.isolate = false;
                }
            }
            if crate::preparation::speech_ready() {
                ui.add_space(6.0);
                let lit = self.missions.announce();
                if bouton(
                    ui,
                    "voice-results",
                    if lit { "Voix : lue" } else { "Voix : muette" },
                    lit,
                )
                .on_hover_text("Lire à voix haute le résultat d'une mission qu'on regarde finir")
                .clicked()
                {
                    self.missions.set_announce(!lit);
                }
            }
            ui.add_space(10.0);
            let search_id = egui::Id::new("mission-search");
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::K)) {
                ui.memory_mut(|m| m.request_focus(search_id));
            }
            let search = hud::cadre_saisie(&accent)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .id(search_id)
                            .desired_width(150.0)
                            .frame(Frame::NONE)
                            .font(FontId::proportional(12.0))
                            .hint_text("Rechercher  ·  Ctrl K")
                            .char_limit(160),
                    )
                })
                .inner;
            if search.changed() {
                self.isolate = false;
                self.focus_compact = false;
            }
            if !compact && scene.courants.len() > 1 {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if bouton(
                        ui,
                        "workspace-focus",
                        if self.isolate {
                            "Voir les missions"
                        } else {
                            "Focale  ↗"
                        },
                        self.isolate,
                    )
                    .clicked()
                    {
                        self.isolate = !self.isolate;
                        self.focus_compact = self.isolate;
                    }
                });
            }
        });
        ui.add_space(if compact { 12.0 } else { 16.0 });
        if let Some(id) = &self.prepared_selection {
            if scene.courants.iter().any(|c| &c.tache == id) {
                self.selection = self.prepared_selection.take();
            } else {
                self.missions.select(Some(id));
                ui.label("Plan confirmé. Mise à jour de l'espace de supervision…");
                if let Some(error) = self.missions.error() {
                    ui.label(error);
                }
                return;
            }
        }
        let query = self.query.trim().to_lowercase();
        let visibles: Vec<_> = scene
            .courants
            .iter()
            .filter(|c| {
                self.filtre.inclut(c)
                    && (query.is_empty()
                        || c.intitule.to_lowercase().contains(&query)
                        || c.agent.to_lowercase().contains(&query)
                        || c.tache.to_lowercase().contains(&query))
            })
            .collect();
        if !visibles
            .iter()
            .any(|c| self.selection.as_deref() == Some(&c.tache))
        {
            self.selection = visibles
                .iter()
                .find(|c| scene.decision.as_ref().is_some_and(|d| d.tache == c.tache))
                .or_else(|| visibles.iter().find(|c| c.reclame()))
                .or_else(|| visibles.first())
                .map(|c| c.tache.clone());
        }
        if scene.courants.is_empty() {
            self.missions.select(None);
            let soumis = egui::ScrollArea::vertical()
                .id_salt("supervision-vide")
                .show(ui, |ui| {
                    crate::desk::empty(ui, compact, &mut self.brouillon_accueil)
                })
                .inner;
            if soumis {
                self.preparer_depuis_l_accueil(ui.ctx());
            }
            return;
        }
        let selection = self.selection.clone();
        if self.detail_id != selection {
            self.detail_tab = crate::mission_details::Tab::Auto;
            self.detail_id = selection.clone();
        }
        self.missions.select(selection.as_deref());
        let courant = visibles
            .iter()
            .copied()
            .find(|c| selection.as_deref() == Some(&c.tache));
        if compact {
            egui::ScrollArea::vertical()
                .id_salt("missions-compactes")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if self.focus_compact {
                        if bouton(ui, "retour-missions", "← Toutes les missions", false).clicked()
                        {
                            self.focus_compact = false;
                            self.isolate = false;
                        }
                        ui.add_space(8.0);
                        if let Some(c) = courant {
                            plaque(ui, 18, |ui| {
                                ui.set_width(ui.available_width());
                                self.inspecteur(ui, c, scene);
                            });
                        }
                    } else {
                        let precedente = self.selection.clone();
                        plaque(ui, 6, |ui| {
                            ui.set_width(ui.available_width());
                            self.liste(ui, &visibles, f32::INFINITY);
                        });
                        if self.selection != precedente {
                            self.focus_compact = true;
                        }
                    }
                });
            return;
        }
        // Sur un grand écran, la liste tient à gauche et l'espace de mission à droite, avec
        // de l'air entre les deux ; la Focale retire la liste pour lire un plan ou comparer
        // des versions sur toute la largeur.
        let available = ui.available_width();
        let show_list = !self.isolate && scene.courants.len() > 1;
        let list_width = if show_list {
            (available * 0.30).clamp(300.0, 420.0)
        } else {
            0.0
        };
        // La rangée ajoute son espacement après la liste : il fait partie de l'intervalle.
        let gap = if show_list { 32.0 } else { 0.0 };
        let spacing = if show_list {
            ui.spacing().item_spacing.x
        } else {
            0.0
        };
        let detail_width = available - list_width - gap - spacing;
        let height = ui.available_height();
        ui.horizontal_top(|ui| {
            if show_list {
                ui.allocate_ui_with_layout(
                    vec2(list_width, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        let count = visibles.len();
                        hud::etiquette(
                            ui,
                            format!(
                                "{count} MISSION{} DANS CETTE VUE",
                                if count == 1 { "" } else { "S" }
                            ),
                            EFFACE,
                        );
                        ui.add_space(6.0);
                        plaque(ui, 6, |ui| {
                            ui.set_width(ui.available_width());
                            self.liste(ui, &visibles, height - 56.0);
                        });
                    },
                );
                ui.add_space(gap);
            }
            ui.allocate_ui_with_layout(
                vec2(detail_width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("contexte")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if let Some(c) = courant {
                                // Marges intérieures (2 × 22) et trait (2 × 1) de la plaque.
                                let accent = Accent::de(ui.ctx());
                                let espace = hud::plaque(ui, &accent, 22, |ui| {
                                    ui.set_width(detail_width - 46.0);
                                    self.inspecteur(ui, c, scene);
                                });
                                hud::retenir(ui.ctx(), "espace-de-mission", espace.response.rect);
                            } else {
                                ui.label("Aucune mission dans cette vue.");
                            }
                        });
                },
            );
        });
    }

    /// Ouvre la préparation d'une mission : un brouillon réussi est remis à neuf, une
    /// tentative en cours ou en erreur est gardée pour sa reprise. Rien n'est soumis ici.
    fn ouvrir_la_preparation(&mut self, ctx: &egui::Context) {
        self.composing = true;
        self.focus_intent = true;
        if self.preparation.attempted_id().is_some()
            && self.preparation.error().is_none()
            && !self.preparation.pending()
        {
            self.preparation.reset();
        }
        self.preparation.discover(ctx);
    }

    /// Une mission échouée ou arrêtée repasse par la préparation : même intention, même modèle
    /// et même profil du catalogue s'il est connu, que le catalogue reçu confirme ou corrige.
    /// L'humain relit et prépare un nouveau plan ; rien n'est émis avant.
    fn relancer(&mut self, ctx: &egui::Context, relance: crate::mission_details::Relance) {
        // Une préparation déjà envoyée attend sa réponse : on la montre, sans l'écraser.
        if self.preparation.pending() {
            self.ouvrir_la_preparation(ctx);
            return;
        }
        self.preparation.reset();
        self.ouvrir_la_preparation(ctx);
        self.preparation.intent = relance.intent;
        self.preparation.model = relance.model;
        if let Some(profile) = relance.profile {
            self.preparation.profile = profile;
        }
    }

    /// L'objectif tapé dans l'espace vide devient celui de la préparation, que l'humain relit
    /// et complète avant de préparer le plan. Une tentative gardée pour sa reprise n'est pas
    /// écrasée : le brouillon de l'accueil attend alors, intact.
    fn preparer_depuis_l_accueil(&mut self, ctx: &egui::Context) {
        if self.brouillon_accueil.trim().is_empty() {
            return;
        }
        self.ouvrir_la_preparation(ctx);
        if self.preparation.attempted_id().is_none() {
            self.preparation.intent = std::mem::take(&mut self.brouillon_accueil)
                .trim()
                .to_owned();
        }
    }

    /// Le clavier seul suffit : Ctrl+1 à 4 pour les pages, Ctrl+N pour un nouvel objectif,
    /// Ctrl+Maj+Échap pour l'arrêt d'urgence (sa confirmation d'abord),
    /// Échap pour refermer l'examen ou la préparation. Échap ne ferme rien tant qu'un champ a
    /// le focus : il lui rend d'abord la main. Refermer l'examen n'autorise ni ne refuse rien.
    fn raccourcis(&mut self, ctx: &egui::Context, atelier: &mut Atelier) {
        let pages = [
            (egui::Key::Num1, Page::Accueil),
            (egui::Key::Num2, Page::Conversation),
            (egui::Key::Num3, Page::Modeles),
            (egui::Key::Num4, Page::Activite),
        ];
        let saisie = self.focus_precedent || ctx.memory(|m| m.focused().is_some());
        let (page, nouveau, arret, echap) = ctx.input_mut(|i| {
            (
                pages
                    .into_iter()
                    .find(|(key, _)| i.consume_key(egui::Modifiers::CTRL, *key))
                    .map(|(_, page)| page),
                i.consume_key(egui::Modifiers::CTRL, egui::Key::N),
                // L'arrêt d'urgence se demande même depuis un champ : il ouvre sa
                // confirmation, rien de plus.
                i.consume_key(
                    egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
                    egui::Key::Escape,
                ),
                !saisie && i.key_pressed(egui::Key::Escape),
            )
        });
        if arret {
            self.arret.demander();
        }
        if let Some(page) = page {
            atelier.page = page;
        }
        if nouveau {
            atelier.page = Page::Accueil;
            self.ouvrir_la_preparation(ctx);
        }
        if echap {
            if self.examen.is_some() {
                self.examen = None;
            } else if self.composing && atelier.page == Page::Accueil {
                self.composing = false;
            }
        }
    }

    fn liste(&mut self, ui: &mut egui::Ui, courants: &[&Courant], height: f32) {
        if courants.is_empty() {
            ui.label("Aucune mission dans cette vue.");
        }
        if let Some(picked) = crate::desk::liste(ui, courants, self.selection.as_deref(), height) {
            self.selection = Some(picked);
            self.focus_compact = true;
        }
    }

    fn inspecteur(&mut self, ui: &mut egui::Ui, c: &Courant, scene: &Scene) {
        let accent = Accent::de(ui.ctx());
        if self.missions.connected() {
            if let Some(relance) =
                crate::mission_details::draw(ui, c, &mut self.missions, &mut self.detail_tab)
            {
                self.relancer(ui.ctx(), relance);
            }
            crate::mission_details::confiees(ui, c, &scene.courants);
            if let Some(d) = scene.decision.as_ref().filter(|d| d.tache == c.tache) {
                ui.add_space(12.0);
                ui.label(titre(&d.question, 20.0));
                if action(ui, "inspecter-action", "Lire les conséquences").clicked() {
                    self.examen = empreinte_decision(scene);
                }
            }
            return;
        }
        let (label, color) = statut_mission(c, &accent);
        let wide = ui.available_width() > 620.0;
        hud::rangee(ui, |ui| {
            etiquette(ui, "MISSION EN FOCALE");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if bouton(ui, "copier-reference", "Copier la référence", false).clicked() {
                    ui.ctx().copy_text(c.tache.clone());
                }
            });
        });
        ui.add_space(14.0);
        // Le cadran à droite, le titre à gauche : la mission se lit d'un seul regard.
        let cadran = if wide { 96.0 } else { 0.0 };
        ui.horizontal_top(|ui| {
            // La colonne du titre laisse au cadran sa place et l'espacement qui l'en sépare.
            let reserve = if wide {
                cadran * 2.0 + 24.0 + ui.spacing().item_spacing.x
            } else {
                0.0
            };
            ui.allocate_ui_with_layout(
                vec2(ui.available_width() - reserve, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.label(titre(&c.intitule, if wide { 34.0 } else { 26.0 }).line_height(Some(if wide { 40.0 } else { 31.0 })));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
                        crate::instruments::monogramme(ui.painter(), r.center(), &c.agent, 18.0);
                        ui.label(RichText::new(&c.agent).size(13.0).color(DISCRET));
                        pastille(ui, label, color);
                    });
                    ui.add_space(20.0);
                    Frame::new()
                        .fill(if c.reclame() { ATTENTE_VOILE } else { VERRE_HAUT })
                        .stroke(Stroke::new(1.0, if c.reclame() { hud::voile(ATTENTE, 150) } else { accent.fil }))
                        .corner_radius(3)
                        .inner_margin(18)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            hud::etiquette(ui, if c.reclame() { "VOTRE ATTENTION" } else { "POINT DE SUPERVISION" }, if c.reclame() { ATTENTE } else { accent.sourd });
                            ui.add_space(4.0);
                            if let Some(d) = scene.decision.as_ref().filter(|d| d.tache == c.tache) {
                                ui.label(titre(&d.question, 20.0));
                                ui.add_space(6.0);
                                if action(ui, "inspecter-action", "Lire les conséquences").clicked() { self.examen = empreinte_decision(scene); }
                            } else {
                                ui.label(RichText::new(match c.etat {
                                    Etat::Court => "La mission est signalée en cours. Aucune demande d'accord reçue pour cette mission.",
                                    Etat::Bloque => "Le service signale un blocage. Sa cause détaillée n'a pas été fournie.",
                                    Etat::Attend => "La mission attend. Le détail de sa demande n'est pas disponible dans cette scène.",
                                    Etat::Fini => "Le service signale la fin de la mission. Les livrables restent à examiner lorsqu'ils sont disponibles.",
                                }).size(13.0).color(DISCRET));
                            }
                        });
                },
            );
            if wide {
                let (r, zone) = ui.allocate_exact_size(vec2(cadran * 2.0 + 24.0, cadran * 2.0 + 84.0), egui::Sense::hover());
                // Le cadran est peint : un lecteur d'écran le lit en une phrase.
                let decrit = format!(
                    "{} étape{}, {:.0} par minute, {:.0} % du budget consommé",
                    c.etapes,
                    if c.etapes > 1 { "s" } else { "" },
                    c.debit,
                    c.budget_consomme.clamp(0.0, 1.0) * 100.0
                );
                zone.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &decrit));
                let centre = pos2(r.center().x + 12.0, r.top() + cadran + 8.0);
                hud::cadran(ui.painter(), centre, cadran, c.etapes, c.budget_consomme, color, &accent);
                // Le cadran dit déjà étapes et budget : la cadence, seule, se relève dessous.
                hud::releve(
                    ui.painter(),
                    pos2(centre.x, r.bottom() - 18.0),
                    Align2::CENTER_CENTER,
                    "ÉTAPES PAR MINUTE",
                    &format!("{:.0}", c.debit),
                    ENCRE,
                );
            }
        });
        ui.add_space(24.0);
        ui.separator();
        ui.add_space(18.0);
        // Sans cadran, faute de largeur, la rangée d'instruments porte les trois mesures.
        if !wide {
            crate::instruments::tableau(ui, c.etapes, c.debit, c.budget_consomme, color, false);
            ui.add_space(24.0);
        }
        etiquette(ui, "RÉSULTATS ET CHANGEMENTS");
        ui.label(
            RichText::new("Aucun livrable ni diff reçu.")
                .size(13.0)
                .color(DISCRET),
        );
        ui.add_space(6.0);
        ui.label(RichText::new(&c.tache).size(11.0).color(EFFACE));
    }

    /// La confirmation de l'arrêt d'urgence : ce qui s'arrête, ce qui reste. Rien ne part avant
    /// le second geste.
    fn confirmer_l_arret(&mut self, ctx: &egui::Context, scene: &Scene) {
        let n = a_arreter(scene);
        let id = egui::Id::new("arret-urgence");
        let reponse = egui::Modal::new(id)
            .area(egui::Modal::default_area(id).fade_in(false))
            .backdrop_color(VOILE)
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(10, 12, 16))
                    .stroke(Stroke::new(1.0, hud::voile(ATTENTE, 160)))
                    .corner_radius(4)
                    .inner_margin(32),
            )
            .show(ctx, |ui| {
                let largeur =
                    (ctx.content_rect().width() - 2.0 * (16.0 + 32.0)).clamp(260.0, 560.0);
                ui.set_max_width(largeur);
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot.center(), 3.0, ATTENTE);
                    hud::etiquette(ui, "ARRÊT D'URGENCE", ATTENTE);
                });
                ui.add_space(14.0);
                ui.label(
                    titre(
                        &match n {
                            0 => "Arrêter les missions en main ?".to_owned(),
                            1 => "Arrêter la mission en cours ?".to_owned(),
                            n => format!("Arrêter les {n} missions en cours ?"),
                        },
                        30.0,
                    )
                    .line_height(Some(36.0)),
                );
                ui.add_space(14.0);
                ui.label(
                    RichText::new(
                        "Chaque agent s'arrête à son prochain pas ; un client officiel lancé \
                         pour une mission est fermé ; les plans en attente sont annulés.",
                    )
                    .size(15.0)
                    .color(ENCRE),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "Les fichiers déjà préparés restent à examiner : rien n'est publié ni \
                         défait dans vos documents.",
                    )
                    .size(14.0)
                    .color(DISCRET),
                );
                ui.add_space(22.0);
                let mut choix = None;
                hud::rangee(ui, |ui| {
                    if bouton(ui, "arret-retour", "Revenir", false).clicked() {
                        choix = Some(false);
                    }
                    if action(ui, "arret-confirmer", "Tout arrêter").clicked() {
                        choix = Some(true);
                    }
                });
                choix
            });
        match reponse.inner {
            Some(true) => self.arret.confirmer(),
            Some(false) => self.arret.renoncer(),
            None if reponse.should_close() => self.arret.renoncer(),
            None => {}
        }
    }

    /// capd demande le code d'approbation pour accorder (ADR 0057) : l'humain le tape ici, ou le
    /// choisit s'il n'en a pas encore. Rien ne s'accorde sans lui ; renoncer laisse la demande
    /// en attente.
    fn code_d_approbation(&mut self, ctx: &egui::Context, reponse: &mut Option<Reponse>) {
        let Some(demande) = self.presence.clone() else {
            return;
        };
        let id = egui::Id::new("preuve-de-presence");
        let choix = egui::Modal::new(id)
            .area(egui::Modal::default_area(id).fade_in(false))
            .backdrop_color(VOILE)
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(10, 12, 16))
                    .stroke(Stroke::new(1.0, hud::voile(ATTENTE, 160)))
                    .corner_radius(4)
                    .inner_margin(32),
            )
            .show(ctx, |ui| {
                let largeur =
                    (ctx.content_rect().width() - 2.0 * (16.0 + 32.0)).clamp(260.0, 520.0);
                ui.set_max_width(largeur);
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot.center(), 3.0, ATTENTE);
                    hud::etiquette(ui, "PREUVE DE PRÉSENCE", ATTENTE);
                });
                ui.add_space(14.0);
                ui.label(titre(
                    if demande.definir {
                        "Choisissez votre code d'approbation"
                    } else {
                        "Votre code d'approbation"
                    },
                    28.0,
                ));
                ui.add_space(12.0);
                ui.label(
                    RichText::new(if demande.definir {
                        "Il sera demandé pour accorder une action : aucun programme de votre \
                         session ne pourra accorder sans lui. Six caractères au moins."
                    } else {
                        "Accorder depuis votre session demande votre code : aucun programme ne \
                         peut accorder à votre place. Il vaut ensuite dix minutes dans cette \
                         fenêtre."
                    })
                    .size(14.0)
                    .color(ENCRE),
                );
                if !demande.message.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(&demande.message).size(13.0).color(ATTENTE));
                }
                ui.add_space(16.0);
                let champ = ui.add(
                    egui::TextEdit::singleline(&mut self.code_saisi)
                        .id(egui::Id::new("code-approbation"))
                        .password(true)
                        .hint_text("Code d'approbation")
                        .font(egui::FontId::proportional(17.0))
                        .margin(egui::Margin::symmetric(14, 10))
                        .desired_width(f32::INFINITY),
                );
                if !champ.has_focus() && !ctx.memory(|m| m.focused().is_some()) {
                    champ.request_focus();
                }
                let pret = self.code_saisi.chars().count() >= crate::presence::LONGUEUR_MIN;
                let entree = champ.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(20.0);
                let mut choix = None;
                hud::rangee(ui, |ui| {
                    let renoncer = if demande.sans_decision() {
                        "Plus tard"
                    } else {
                        "Renoncer"
                    };
                    if bouton(ui, "code-renoncer", renoncer, false).clicked() {
                        choix = Some(false);
                    }
                    let libelle = match (demande.definir, demande.sans_decision()) {
                        (true, true) => "Définir",
                        (true, false) => "Définir et accorder",
                        (false, _) => "Accorder",
                    };
                    if ui
                        .add_enabled_ui(pret, |ui| action(ui, "code-confirmer", libelle))
                        .inner
                        .clicked()
                        || (entree && pret)
                    {
                        choix = Some(true);
                    }
                });
                choix
            });
        match choix.inner {
            Some(true) => {
                let code = std::mem::take(&mut self.code_saisi);
                *reponse = Some(if demande.definir {
                    Reponse::DefinirCode(code)
                } else {
                    Reponse::Code(code)
                });
            }
            Some(false) => {
                self.code_saisi.clear();
                *reponse = Some(Reponse::RenoncerAuCode);
            }
            None if choix.should_close() => {
                self.code_saisi.clear();
                *reponse = Some(Reponse::RenoncerAuCode);
            }
            None => {}
        }
    }

    fn decision(&mut self, ctx: &egui::Context, scene: &Scene, reponse: &mut Option<Reponse>) {
        let Some(d) = &scene.decision else {
            return;
        };
        let id = egui::Id::new("decision");
        egui::Modal::new(id)
            .area(egui::Modal::default_area(id).fade_in(false))
            .backdrop_color(VOILE)
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(10, 12, 16))
                    .stroke(Stroke::new(1.0, hud::voile(ATTENTE, 160)))
                    .corner_radius(4)
                    .inner_margin(32),
            )
            .show(ctx, |ui| {
                // Sur un écran étroit, la décision garde sa gouttière de 16 points de chaque
                // côté, marges intérieures comprises.
                let largeur =
                    (ctx.content_rect().width() - 2.0 * (16.0 + 32.0)).clamp(260.0, 600.0);
                ui.set_max_width(largeur);
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot.center(), 3.0, ATTENTE);
                    hud::etiquette(ui, "DÉCISION HUMAINE", ATTENTE);
                });
                ui.add_space(14.0);
                ui.label(titre(&d.question, 30.0).line_height(Some(36.0)));
                ui.add_space(14.0);
                ui.label(RichText::new(&d.consequence).size(15.0).color(ENCRE));
                if d.irreversible {
                    ui.add_space(4.0);
                    hud::etiquette(ui, "ACTION IRRÉVERSIBLE", ATTENTE);
                }
                // Le motif du modèle, s'il en a donné un : un dire, montré comme tel, à part
                // de ce que le système sait de l'action.
                if let Some(motif) = &d.motif {
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(format!("Le modèle dit : « {motif} »"))
                            .size(14.0)
                            .italics()
                            .color(DISCRET),
                    );
                }
                ui.add_space(10.0);
                petit(
                    ui,
                    format!("Mission {} · attente {} s", d.tache, d.depuis_secondes),
                );
                ui.add_space(22.0);
                ui.horizontal_wrapped(|ui| {
                    if bouton(ui, "decision-retour", "Revenir aux missions", false).clicked() {
                        self.examen = None;
                    }
                    if bouton(ui, "decision-refuser", "Refuser", false).clicked() {
                        *reponse = Some(Reponse::Refuse);
                        self.examen = None;
                    }
                    if action(ui, "decision-autoriser", "Autoriser cette action").clicked() {
                        *reponse = Some(Reponse::Accepte);
                        self.examen = None;
                    }
                    // Une action irréversible ne s'autorise qu'une fois : capd n'en fait jamais
                    // une règle de mission (ADR 0054), la surface ne le propose donc pas.
                    if !d.irreversible
                        && bouton(
                            ui,
                            "decision-autoriser-mission",
                            "Autoriser pour toute la mission",
                            false,
                        )
                        .clicked()
                    {
                        *reponse = Some(Reponse::AccepteMission);
                        self.examen = None;
                    }
                });
                if d.irreversible {
                    ui.add_space(10.0);
                    petit(
                        ui,
                        "Une action irréversible s'autorise une fois : si l'agent veut la refaire, \
                         il vous la redemandera.",
                    );
                }
                // Les crochets sur ce que le panneau occupe réellement, une fois composé.
                hud::crochets(ui.painter(), ui.min_rect().expand(33.0), ATTENTE, 16.0);
            });
    }

    fn systeme(&mut self, ui: &mut egui::Ui, scene: &Scene) {
        let accent = Accent::de(ui.ctx());
        etiquette(ui, "SYSTÈME");
        ui.label(titre("Le cadre d'exécution.", 40.0));
        ui.add_space(26.0);
        egui::ScrollArea::vertical()
            .id_salt("systeme")
            .show(ui, |ui| {
                plaque(ui, 28, |ui| {
                    ui.set_width(ui.available_width());
                    etiquette(ui, "ISOLATION DISPONIBLE");
                    crate::instruments::echelle_isolation(ui, scene.isolation.niveau_max, scene.isolation.manque.as_deref());
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(120.0, 120.0), egui::Sense::hover());
                        hud::jauge(
                            ui.painter(),
                            r.center(),
                            56.0,
                            f32::from(scene.isolation.niveau_max) / 2.0,
                            accent.vif,
                            &accent,
                            (&scene.isolation.niveau_max.to_string(), "NIVEAU"),
                        );
                        ui.add_space(12.0);
                        ui.vertical(|ui| {
                            ui.label(titre(format!("Niveau {} sur 2", scene.isolation.niveau_max), 26.0));
                            if let Some(manque) = &scene.isolation.manque {
                                ui.label(RichText::new(format!("Il manque : {manque}.")).color(DISCRET));
                            }
                            // Les microVM que sandboxd tient prêtes : une exécution de niveau 2
                            // part sans attendre le démarrage d'un noyau (ADR 0045).
                            if let Some(reserve) = &scene.isolation.reserve {
                                ui.add_space(8.0);
                                let texte = reserve_en_mots(reserve);
                                let ligne = ui.label(
                                    RichText::new(&texte)
                                        .size(13.0)
                                        .color(if reserve.erreur.is_some() { ATTENTE } else { accent.vif }),
                                );
                                ui.interact(ligne.rect, egui::Id::new("isolation-reserve"), egui::Sense::hover())
                                    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &texte));
                            }
                            ui.add_space(8.0);
                            petit(
                                ui,
                                "Cette capacité annoncée ne prouve pas le confinement de chaque mission.",
                            );
                        });
                    });
                });
                ui.add_space(22.0);
                if let Some(code) = self.code {
                    plaque(ui, 28, |ui| {
                        ui.set_width(ui.available_width());
                        if approbations(ui, code, &accent) {
                            self.demander_le_code = true;
                        }
                    });
                    ui.add_space(22.0);
                }
                plaque(ui, 28, |ui| {
                    ui.set_width(ui.available_width());
                    etiquette(ui, "MACHINE, VUE PAR L'INSTALLEUR");
                    ui.add_space(4.0);
                    let releve = inventaire_de_la_machine();
                    if releve.is_empty() {
                        ui.label(titre("Aucun relevé.", 22.0));
                        petit(
                            ui,
                            "Cette machine n'a pas été installée par l'installeur de Prophet OS, qui relève l'écran, le réseau, le micro et la virtualisation avant d'effacer le disque.",
                        );
                    } else {
                        ui.label(titre("Ce que la clé a vu avant d'effacer le disque.", 22.0));
                        ui.add_space(8.0);
                        for (manque, ligne) in releve {
                            ui.horizontal_wrapped(|ui| {
                                let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
                                ui.painter().circle_filled(
                                    dot.center(),
                                    3.0,
                                    if *manque { ATTENTE } else { accent.vif },
                                );
                                ui.label(
                                    RichText::new(ligne)
                                        .size(13.0)
                                        .color(if *manque { ATTENTE } else { ENCRE }),
                                );
                            });
                        }
                        ui.add_space(6.0);
                        petit(ui, "Un point d'alerte est un manque que l'installeur a signalé. Le système installé a les mêmes pilotes que la clé.");
                    }
                });
                ui.add_space(22.0);
                plaque(ui, 28, |ui| {
                    ui.set_width(ui.available_width());
                    etiquette(ui, "APPARENCE");
                    ui.add_space(4.0);
                    ui.label(titre("La couleur de ce qui signale.", 22.0));
                    ui.label(
                        RichText::new(
                            "L'accent colore le champ, les fils et ce qui est actif. La nuit, l'encre et l'alerte ne changent pas.",
                        )
                        .size(12.0)
                        .color(DISCRET),
                    );
                    ui.add_space(14.0);
                    let mut chosen = None;
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(18.0, 10.0);
                        for candidate in ACCENTS {
                            let selected = candidate.nom == accent.nom;
                            let (r, response) = ui.allocate_exact_size(vec2(96.0, 74.0), egui::Sense::click());
                            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                            response.widget_info(|| {
                                egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, candidate.titre)
                            });
                            ui.ctx().check_for_id_clash(egui::Id::new(format!("accent-{}", candidate.nom)), r, "accent");
                            let p = ui.painter();
                            let center = pos2(r.center().x, r.top() + 26.0);
                            if selected || response.hovered() {
                                hud::lueur(p, center, 14.0, &candidate);
                            }
                            p.circle_filled(center, 13.0, candidate.vif);
                            if selected {
                                p.circle_stroke(center, 19.0, Stroke::new(1.5, candidate.vif));
                                hud::graduations(p, center, 24.0, 24, 24, 2.5, (candidate.fil_vif, candidate.fil));
                            } else {
                                p.circle_stroke(center, 19.0, Stroke::new(1.0, candidate.fil));
                            }
                            hud::texte_espace(
                                p,
                                pos2(r.center().x, r.bottom() - 8.0),
                                egui::Align2::CENTER_CENTER,
                                &candidate.titre.to_uppercase(),
                                FontId::proportional(9.0),
                                if selected { candidate.vif } else { DISCRET },
                                1.6,
                            );
                            if response.has_focus() {
                                p.rect_stroke(r, 3, Stroke::new(1.0, candidate.fil_vif), egui::StrokeKind::Inside);
                            }
                            if response.clicked() {
                                chosen = Some(candidate);
                            }
                        }
                    });
                    if let Some(candidate) = chosen {
                        candidate.installer(ui.ctx());
                        installer_style(ui.ctx());
                        self.apparence = Some(match crate::theme::enregistrer_accent(candidate) {
                            Ok(chemin) => (format!("Accent {} conservé dans {}.", candidate.titre, chemin.display()), false),
                            Err(error) => (format!("Accent {} appliqué à cette session ; il n'a pas pu être conservé : {error}", candidate.titre), true),
                        });
                    }
                    if let Some((text, error)) = &self.apparence {
                        ui.add_space(6.0);
                        ui.label(RichText::new(text).size(11.0).color(if *error { ATTENTE } else { DISCRET }));
                    }
                    ui.add_space(6.0);
                    petit(ui, "Ligne de commande : --accent <nom> ; variable : PROPHET_SURFACE_ACCENT.");
                });
                ui.add_space(22.0);
                plaque(ui, 28, |ui| {
                    ui.set_width(ui.available_width());
                    etiquette(ui, "MISSIONS REÇUES");
                    ui.add_space(6.0);
                    if scene.courants.is_empty() {
                        ui.label("Aucune mission reçue.");
                    }
                    for c in &scene.courants {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(&c.intitule).size(14.0).color(ENCRE));
                            let (label, color) = statut_mission(c, &accent);
                            pastille(ui, label, color);
                        });
                        petit(
                            ui,
                            format!("{} · {} · {} étapes", c.tache, c.agent, c.etapes),
                        );
                        ui.add_space(8.0);
                    }
                });
            });
    }
}

fn empreinte_decision(scene: &Scene) -> Option<String> {
    scene.decision.as_ref().map(|d| {
        format!(
            "{}\0{}\0{}\0{}\0{}",
            d.tache,
            d.question,
            d.consequence,
            d.irreversible,
            d.motif.as_deref().unwrap_or_default()
        )
    })
}

fn composer(ui: &mut egui::Ui, atelier: &mut Atelier) {
    let ctx = ui.ctx().clone();
    let accent = Accent::de(&ctx);
    if let Some(error) = &atelier.erreur {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(if atelier.modeles.is_empty() {
                    "Moteur local indisponible"
                } else {
                    error
                })
                .size(12.0)
                .color(ATTENTE),
            );
            if ui.link("Vérifier la connexion").clicked() {
                atelier.page = Page::Modeles;
            }
        });
    }
    plaque(ui, 18, |ui| {
        ui.set_width(ui.available_width());
        let response = ui.add(
            egui::TextEdit::multiline(&mut atelier.brouillon)
                .id(egui::Id::new("intention"))
                .hint_text("Un objectif, une contrainte, une question…")
                .font(FontId::proportional(16.0))
                .desired_width(f32::INFINITY)
                .desired_rows(1)
                .char_limit(16_384)
                .frame(Frame::NONE),
        );
        let shortcut = response.has_focus()
            && ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Enter));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("choix-modele")
                .width(185.0)
                .selected_text(if atelier.choisi.is_empty() {
                    "Choisir un modèle"
                } else {
                    &atelier.choisi
                })
                .show_ui(ui, |ui| {
                    for model in &atelier.modeles {
                        ui.selectable_value(&mut atelier.choisi, model.clone(), model);
                    }
                });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if atelier.generation {
                    if action(ui, "interrompre", "Interrompre").clicked() {
                        atelier.interrompre();
                    }
                    ui.spinner();
                } else {
                    let enabled = !atelier.demonstration
                        && !atelier.choisi.is_empty()
                        && !atelier.brouillon.trim().is_empty();
                    let clicked = ui
                        .add_enabled_ui(enabled, |ui| {
                            hud::bouton(ui, "envoyer", "Envoyer ↑", true, &accent)
                        })
                        .inner
                        .clicked();
                    if clicked || (enabled && shortcut) {
                        atelier.envoyer(&ctx);
                    }
                }
            });
        });
    });
    ui.horizontal(|ui| {
        hud::etiquette(ui, "CONVERSATION LOCALE · EN MÉMOIRE", EFFACE);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            hud::etiquette(ui, "CTRL + ENTRÉE POUR ENVOYER", EFFACE);
        });
    });
}

/// Les départs proposés à un dialogue vide : ce qu'on confie le plus souvent à un agent, formulé
/// pour que le modèle aide à le cadrer avant de le déléguer.
const SUGGESTIONS: [(&str, &str, &str); 3] = [
    (
        "suggestion-resumer",
        "Résumer un dossier",
        "Aide-moi à cadrer une mission qui résume les documents de ~/Documents/Prophet dans un fichier de synthèse : que dois-je préciser avant de la confier ? ",
    ),
    (
        "suggestion-corriger",
        "Corriger un document",
        "Je veux faire corriger un document sans rien changer d'autre. Aide-moi à écrire l'objectif : le fichier visé, ce qui doit changer, ce qui ne doit pas bouger. ",
    ),
    (
        "suggestion-comparer",
        "Comparer des offres",
        "Aide-moi à cadrer une comparaison d'offres : les critères, les sources à consulter et la forme du résultat attendu. ",
    ),
];

/// Une suggestion : une pastille arrondie, discrète, qui s'éclaire au survol.
fn suggestion(ui: &mut egui::Ui, id: &str, texte: &str) -> egui::Response {
    let accent = Accent::de(ui.ctx());
    let survol = ui
        .ctx()
        .read_response(egui::Id::new(id))
        .is_some_and(|r| r.hovered());
    let reponse = Frame::new()
        .fill(if survol {
            hud::voile(accent.vif, 26)
        } else {
            VERRE_HAUT
        })
        .stroke(Stroke::new(
            1.0,
            if survol { accent.fil_vif } else { accent.fil },
        ))
        .corner_radius(16)
        .inner_margin(egui::Margin::symmetric(14, 7))
        .show(ui, |ui| {
            ui.label(RichText::new(texte).size(13.0).color(if survol {
                ENCRE
            } else {
                accent.sourd
            }));
        })
        .response;
    let reponse = ui.interact(reponse.rect, egui::Id::new(id), egui::Sense::click());
    reponse.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, texte));
    reponse.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn conversation(ui: &mut egui::Ui, atelier: &mut Atelier) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            etiquette(ui, "DIALOGUE");
            ui.label(titre("Le dialogue, à votre rythme.", 32.0));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if bouton(ui, "nouveau-dialogue", "+ Nouveau", false).clicked() {
                atelier.nouvelle();
                atelier.page = Page::Conversation;
            }
        });
    });
    ui.add_space(16.0);
    egui::ScrollArea::vertical().id_salt("dialogue").stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
        if atelier.tours.is_empty() {
            plaque(ui, 28, |ui| {
                ui.set_width(ui.available_width());
                etiquette(ui, "AVANT DE DÉLÉGUER");
                ui.label(titre("Précisez le résultat attendu.", 26.0));
                ui.label(RichText::new("Explorez un objectif avec votre modèle local. Ce dialogue ne lance pas d'agent et n'accorde aucun droit système.").size(13.0).color(DISCRET));
                ui.add_space(14.0);
                if bouton(ui, "cadrer-objectif", "Structurer mon objectif", false).clicked() {
                    atelier.brouillon = "Aide-moi à préciser cet objectif, ses contraintes et les critères qui permettront de vérifier le résultat : ".into();
                    ui.ctx().memory_mut(|m| m.request_focus(egui::Id::new("intention")));
                }
                ui.add_space(18.0);
                etiquette(ui, "POUR COMMENCER");
                ui.add_space(6.0);
                // Trois départs : le brouillon se remplit, rien n'est envoyé ; l'humain complète.
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
                    for (id, titre, amorce) in SUGGESTIONS {
                        if suggestion(ui, id, titre).clicked() {
                            atelier.brouillon = (*amorce).into();
                            ui.ctx().memory_mut(|m| m.request_focus(egui::Id::new("intention")));
                        }
                    }
                });
            });
        }
        for (i, tour) in atelier.tours.iter().enumerate() {
            ui.push_id(i, |ui| {
                Frame::new().fill(VERRE_HAUT).stroke(Stroke::new(1.0, TRAIT)).corner_radius(3).inner_margin(18).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    hud::etiquette(ui, "VOUS", EFFACE);
                    ui.label(RichText::new(&tour.demande).size(15.0));
                });
                ui.add_space(8.0);
                plaque(ui, 22, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        etiquette(ui, tour.modele.to_uppercase());
                        if !tour.reponse.is_empty() && ui.small_button("Copier").clicked() { ui.ctx().copy_text(tour.reponse.clone()); }
                    });
                    ui.add_space(10.0);
                    if tour.reponse.is_empty() && tour.erreur.is_none() { petit(ui, "Le modèle prépare sa réponse…"); }
                    else { ui.label(RichText::new(&tour.reponse).size(15.0).line_height(Some(24.0))); }
                    if let Some(error) = &tour.erreur { ui.label(RichText::new(error).color(ATTENTE)); }
                    if let Some(mesure) = &tour.mesure {
                        ui.add_space(12.0);
                        petit(ui, format!("{} tokens · {:.2} s · premier texte {} ms", mesure.usage.tokens_out, mesure.elapsed.as_secs_f64(), mesure.first_token.unwrap_or_default().as_millis()));
                        if mesure.forgotten > 0 {
                            // Le modèle n'a pas relu le début : l'humain doit le savoir avant de s'y référer.
                            petit(ui, oubli(mesure.forgotten));
                        }
                    }
                });
                ui.add_space(18.0);
            });
        }
    });
}

/// Le relevé que l'installeur a fait de la machine avant d'effacer le disque, gardé dans la
/// source posée sur elle (`image/machine/inventaire.txt` ; `PROPHET_INVENTAIRE` le déplace),
/// lu une fois : chaque ligne, et si l'installeur l'a marquée « ! » comme un manque.
fn inventaire_de_la_machine() -> &'static [(bool, String)] {
    static RELEVE: std::sync::OnceLock<Vec<(bool, String)>> = std::sync::OnceLock::new();
    RELEVE.get_or_init(|| {
        let chemin = std::env::var_os("PROPHET_INVENTAIRE").map_or_else(
            || std::path::PathBuf::from("/etc/prophet/source/image/machine/inventaire.txt"),
            std::path::PathBuf::from,
        );
        std::fs::read_to_string(chemin)
            .map(|texte| lire_inventaire(&texte))
            .unwrap_or_default()
    })
}

/// Les lignes d'un relevé : « ! … » est un manque, le reste une constatation ; le vide est
/// ignoré.
fn lire_inventaire(texte: &str) -> Vec<(bool, String)> {
    texte
        .lines()
        .filter_map(|ligne| {
            let ligne = ligne.trim();
            if ligne.is_empty() {
                None
            } else if let Some(manque) = ligne.strip_prefix('!') {
                Some((true, manque.trim().to_owned()))
            } else {
                Some((false, ligne.to_owned()))
            }
        })
        .collect()
}

/// Lance une application du bureau, ou un outil publié (`outils <nom>`), par le lanceur de la
/// session ; faux s'il n'est pas installé.
fn ouvrir_application(args: &[String]) -> bool {
    let Some(launcher) = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("prophet-ouvrir"))
            .find(|candidate| candidate.is_file())
    }) else {
        return false;
    };
    std::process::Command::new(launcher)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

fn clients_officiels(ui: &mut egui::Ui, atelier: &Atelier) {
    let accent = Accent::de(ui.ctx());
    etiquette(ui, "CLIENTS OFFICIELS");
    ui.add_space(6.0);
    petit(
        ui,
        "Sondés par leurs propres commandes ; l'OS ne lit ni ne copie leurs identifiants.",
    );
    ui.add_space(12.0);
    if atelier.clients.is_empty() {
        ui.horizontal(|ui| {
            ui.spinner();
            petit(ui, "Sonde des clients en cours…");
        });
        return;
    }
    let width = ui.available_width();
    let gap = 12.0;
    // Trois cartes de front seulement s'il y a la place : sous 250 points, l'état et le bouton
    // « Ouvrir » se recouvrent ; les cartes passent alors à deux, puis à une par ligne.
    let colonnes: f32 = if width >= 3.0 * 250.0 + 2.0 * gap {
        3.0
    } else if width >= 2.0 * 250.0 + gap {
        2.0
    } else {
        1.0
    };
    let tile = ((width - (colonnes - 1.0) * gap) / colonnes - 0.5).floor();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(gap, gap);
        for card in &atelier.clients {
            let (rect, carte) = ui.allocate_exact_size(vec2(tile, 118.0), egui::Sense::hover());
            let p = ui.painter();
            p.rect_filled(rect, 14, VERRE_HAUT);
            crate::instruments::monogramme(p, rect.min + vec2(30.0, 30.0), &card.driver, 24.0);
            p.text(
                rect.min + vec2(52.0, 22.0),
                Align2::LEFT_CENTER,
                &card.driver,
                FontId::new(16.0, egui::FontFamily::Name("Inter600".into())),
                ENCRE,
            );
            p.text(
                rect.min + vec2(52.0, 40.0),
                Align2::LEFT_CENTER,
                card.version
                    .as_deref()
                    .map_or_else(|| "version inconnue".to_owned(), |v| format!("version {v}")),
                FontId::proportional(11.0),
                DISCRET,
            );
            let (etat, label, conseil) = etat_du_client(card);
            // La carte est peinte : un lecteur d'écran la lit en une phrase.
            let decrit = format!(
                "{}, {} : {label}. {conseil}",
                card.driver,
                card.version
                    .as_deref()
                    .map_or_else(|| "version inconnue".to_owned(), |v| format!("version {v}")),
            );
            carte.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &decrit));
            let dot = match etat {
                EtatDuClient::Pret => ACCOMPLI,
                EtatDuClient::AConnecter => ATTENTE,
                EtatDuClient::Incertain | EtatDuClient::Absent => DISCRET,
            };
            p.circle_filled(rect.min + vec2(24.0, 70.0), 3.0, dot);
            p.text(
                rect.min + vec2(34.0, 70.0),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(12.0),
                dot,
            );
            // Le conseil s'enroule dans la place que le bouton « Ouvrir » lui laisse.
            let bouton = card.present && matches!(card.driver.as_str(), "claude-code" | "codex");
            let largeur = rect.width() - 20.0 - if bouton { 94.0 } else { 20.0 };
            let conseil = p.layout(
                conseil.to_owned(),
                FontId::proportional(11.0),
                DISCRET,
                largeur,
            );
            p.galley(rect.min + vec2(20.0, 85.0), conseil, DISCRET);
            if bouton {
                let button = egui::Rect::from_min_size(
                    pos2(rect.right() - 82.0, rect.bottom() - 40.0),
                    vec2(66.0, 26.0),
                );
                let response = ui.interact(
                    button,
                    egui::Id::new(format!("client-open-{}", card.driver)),
                    egui::Sense::click(),
                );
                let nom = format!("Ouvrir {}", card.driver);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &nom)
                });
                // Le vocabulaire des boutons du tableau de bord : coins francs, capitales
                // espacées, fil d'accent qui s'avive au survol.
                let p = ui.painter();
                p.rect_filled(
                    button,
                    3,
                    if response.hovered() {
                        VERRE_HAUT
                    } else {
                        CREUX
                    },
                );
                p.rect_stroke(
                    button,
                    3,
                    Stroke::new(
                        1.0,
                        if response.hovered() || response.has_focus() {
                            accent.fil_vif
                        } else {
                            accent.fil
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                hud::texte_espace(
                    p,
                    button.center(),
                    Align2::CENTER_CENTER,
                    "OUVRIR",
                    FontId::proportional(10.5),
                    ENCRE,
                    1.3,
                );
                if response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    ouvrir_application(std::slice::from_ref(&card.driver));
                }
            }
        }
    });
}

/// Où en est un client officiel, pour la couleur de sa pastille.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EtatDuClient {
    Pret,
    AConnecter,
    Incertain,
    Absent,
}

/// L'état d'un client officiel en mots, et ce que l'humain peut y faire : la seconde ligne
/// de la carte ne répète pas la première. Une sonde incertaine ou en échec se dit comme telle,
/// et non comme une connexion requise.
fn etat_du_client(card: &ClientCard) -> (EtatDuClient, String, &'static str) {
    if !card.present {
        return (
            EtatDuClient::Absent,
            "Absent de cette machine".to_owned(),
            "Installez-le pour lui confier des missions.",
        );
    }
    if card.connected {
        return (
            EtatDuClient::Pret,
            "Session ouverte".to_owned(),
            "Prêt à mener des missions, dans sa cage.",
        );
    }
    if card.connection == providers::official::ConnectionState::LoginRequired.label() {
        return (
            EtatDuClient::AConnecter,
            "Installé, connexion requise".to_owned(),
            "Connectez-vous dans sa fenêtre.",
        );
    }
    let label = if card.connection == providers::official::ConnectionState::Unknown.label() {
        "Installé, session non vérifiée".to_owned()
    } else {
        format!("Installé, {}", card.connection)
    };
    (
        EtatDuClient::Incertain,
        label,
        "Ouvrez-le pour vérifier sa session.",
    )
}

/// Octets en mégaoctets ou gigaoctets, pour l'humain.
fn octets(n: u64) -> String {
    let n = n as f64;
    if n >= 1e9 {
        format!("{:.2} Go", n / 1e9).replace('.', ",")
    } else {
        format!("{:.0} Mo", n / 1e6)
    }
}

/// Le catalogue du système : ce qu'il sait télécharger, vérifié avant d'être posé (ADR 0046).
/// Rend la commande que l'humain a demandée, s'il en a demandé une.
fn catalogue_du_systeme(ui: &mut egui::Ui, atelier: &Atelier) -> Option<(CommandeDePoids, String)> {
    let accent = Accent::de(ui.ctx());
    etiquette(ui, "CATALOGUE DU SYSTÈME");
    ui.add_space(6.0);
    petit(
        ui,
        "Téléchargés par le proxy de sortie, sous un droit borné au dépôt ; l'empreinte SHA-256 et l'en-tête sont vérifiés avant que le moteur les voie.",
    );
    ui.add_space(10.0);
    let entrees = match &atelier.catalogue {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                petit(ui, "Lecture du catalogue…");
            });
            return None;
        }
        Some(Err(erreur)) => {
            ui.label(RichText::new(format!("Catalogue indisponible : {erreur}")).color(ATTENTE));
            return None;
        }
        Some(Ok(entrees)) => entrees,
    };
    if let Some(erreur) = &atelier.erreur_de_poids {
        ui.label(RichText::new(erreur).size(12.0).color(ATTENTE));
        ui.add_space(6.0);
    }
    let mut demande = None;
    for (rang, e) in entrees.iter().enumerate() {
        if rang > 0 {
            ui.add_space(12.0);
        }
        let ligne = ui
            .horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(titre(&e.name, 18.0));
                        if let Some(q) = &e.quantization {
                            ui.label(RichText::new(q).size(12.0).color(ENCRE));
                        }
                        if let Some(n) = e.bytes.or_else(|| e.pull.as_ref().and_then(|p| p.total)) {
                            ui.label(RichText::new(octets(n)).size(12.0).color(DISCRET));
                        }
                        if let Some(m) = &e.memory {
                            let couleur = match m.fit {
                                Some(
                                    providers::memory::Fit::TooLarge
                                    | providers::memory::Fit::Tight,
                                ) => ATTENTE,
                                _ => accent.sourd,
                            };
                            ui.label(
                                RichText::new(format!(
                                    "≈ {} en mémoire",
                                    providers::memory::gigabytes(m.need.total)
                                ))
                                .size(12.0)
                                .color(couleur),
                            );
                        }
                    });
                    if let Some(note) = &e.note {
                        petit(ui, note);
                    }
                    etat_du_poids(ui, e, &accent);
                });
                // Le routeur du moteur connaît ce poids sans le servir : un clic le fait charger.
                let a_servir = e.au_moteur.as_ref().is_some_and(|m| !m.charge());
                if e.provided.is_some() && !e.installed && !a_servir {
                    return;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if e.provided.is_some() && !e.installed {
                        if bouton(ui, &format!("catalogue-servir-{}", e.id), "Servir", true)
                            .clicked()
                        {
                            demande = Some((CommandeDePoids::Servir, e.id.clone()));
                        }
                        return;
                    }
                    let (commande, texte, cle) = if e.en_cours() {
                        (CommandeDePoids::Arreter, "Arrêter", "arreter")
                    } else if e.installed {
                        (CommandeDePoids::Retirer, "Retirer", "retirer")
                    } else if e.partial_bytes.is_some() {
                        (CommandeDePoids::Telecharger, "Reprendre", "telecharger")
                    } else {
                        (CommandeDePoids::Telecharger, "Télécharger", "telecharger")
                    };
                    if bouton(ui, &format!("catalogue-{cle}-{}", e.id), texte, true).clicked() {
                        demande = Some((commande, e.id.clone()));
                    }
                    if a_servir
                        && bouton(ui, &format!("catalogue-servir-{}", e.id), "Servir", true)
                            .clicked()
                    {
                        demande = Some((CommandeDePoids::Servir, e.id.clone()));
                    }
                });
            })
            .response;
        ui.interact(
            ligne.rect,
            egui::Id::new(format!("catalogue-{}", e.id)),
            egui::Sense::hover(),
        )
        .widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, description_du_poids(e))
        });
    }
    demande
}

/// Ce que la machine a d'une entrée, en une ligne : posé, en cours (avec la barre), échoué,
/// interrompu ou disponible.
fn etat_du_poids(ui: &mut egui::Ui, e: &EntreeCatalogue, accent: &Accent) {
    if let Some(suivi) = e.pull.as_ref().filter(|_| e.en_cours()) {
        let part = suivi
            .total
            .filter(|t| *t > 0)
            .map(|t| (suivi.received as f64 / t as f64).clamp(0.0, 1.0) as f32);
        ui.add_space(4.0);
        let largeur = ui.available_width().min(420.0);
        let (rect, _) = ui.allocate_exact_size(vec2(largeur, 6.0), egui::Sense::hover());
        let peintre = ui.painter();
        peintre.rect_filled(rect, 3.0, CREUX);
        let fraction = part.unwrap_or(0.12);
        let plein =
            egui::Rect::from_min_size(rect.min, vec2(rect.width() * fraction, rect.height()));
        peintre.rect_filled(plein, 3.0, accent.vif);
        // Un halo à la tête de la barre : on voit qu'elle avance.
        peintre.circle_filled(
            pos2(plein.right(), rect.center().y),
            5.0,
            accent.vif.gamma_multiply(0.35),
        );
        let texte = match (part, suivi.total) {
            (Some(p), Some(total)) => format!(
                "{} % · {} sur {}",
                (p * 100.0).floor(),
                octets(suivi.received),
                octets(total)
            ),
            _ => format!("{} reçus", octets(suivi.received)),
        };
        ui.label(RichText::new(texte).size(12.0).color(accent.vif));
        return;
    }
    let origine = if e.installed {
        Some("Téléchargé · vérifié")
    } else if e.provided.is_some() {
        Some("Fourni par le système")
    } else {
        None
    };
    if let Some(origine) = origine {
        ui.horizontal(|ui| {
            pastille(ui, origine, ACCOMPLI);
            match &e.au_moteur {
                Some(m) if m.charge() => {
                    pastille(ui, &format!("Servi · {}", m.nom), accent.vif);
                }
                Some(m) if m.etat.as_deref() == Some("loading") => {
                    pastille(ui, &format!("Chargement · {}", m.nom), accent.sourd);
                }
                _ => {}
            }
        });
        return;
    }
    if let Some(erreur) = e
        .pull
        .as_ref()
        .filter(|p| p.state == "failed")
        .and_then(|p| p.error.as_ref())
    {
        ui.label(RichText::new(erreur).size(12.0).color(ATTENTE));
        return;
    }
    let trop_grand = e
        .memory
        .is_some_and(|m| m.fit == Some(providers::memory::Fit::TooLarge));
    match e.partial_bytes {
        Some(n) => pastille(ui, &format!("Interrompu à {}", octets(n)), DISCRET),
        None if trop_grand => pastille(ui, "Trop grand pour cette machine", ATTENTE),
        None => pastille(ui, "Disponible", DISCRET),
    }
}

fn description_du_poids(e: &EntreeCatalogue) -> String {
    let etat = if e.en_cours() {
        let suivi = e.pull.as_ref().map_or(0, |p| p.received);
        format!("téléchargement en cours, {} reçus", octets(suivi))
    } else if e.installed {
        "téléchargé et vérifié".to_owned()
    } else if e.provided.is_some() {
        "fourni par le système".to_owned()
    } else {
        "disponible".to_owned()
    };
    let memoire = e.memory.map_or_else(String::new, |m| {
        format!(
            " — demande environ {} de mémoire à {} tokens{}",
            providers::memory::gigabytes(m.need.total),
            m.need.context,
            m.fit
                .map_or_else(String::new, |f| format!(", {}", f.describe()))
        )
    });
    format!("{} — {etat}{memoire}", e.name)
}

/// Les poids installés, tels que leurs fichiers les décrivent : l'agent qui confie une étape et
/// l'humain qui choisit un modèle lisent la même fenêtre de contexte et la même quantification.
fn poids_installes(ui: &mut egui::Ui, atelier: &Atelier) {
    let accent = Accent::de(ui.ctx());
    etiquette(ui, "POIDS INSTALLÉS");
    ui.add_space(6.0);
    petit(
        ui,
        format!(
            "Lus dans {} : l'en-tête de chaque fichier, sans charger les poids.",
            atelier.dossier_des_poids.display()
        ),
    );
    ui.add_space(10.0);
    if atelier.demonstration {
        petit(ui, "Catalogue non lu dans une scène d'exemple.");
        return;
    }
    if !atelier.poids_lus {
        ui.horizontal(|ui| {
            ui.spinner();
            petit(ui, "Lecture du catalogue…");
        });
        return;
    }
    if atelier.poids.is_empty() {
        ui.label(titre("Aucun poids sur cette machine.", 22.0));
        petit(
            ui,
            "L'installation fournit le modèle par défaut ; le catalogue du système, plus bas, en télécharge d'autres, vérifiés.",
        );
        return;
    }
    // Celui qu'un agent choisirait : le plus gros qui tient et déclare les outils.
    let recommande = providers::memory::recommended(
        atelier.poids.iter().flatten(),
        atelier.contexte_local,
        atelier.memoire.as_ref(),
    )
    .map(|w| w.path.clone());
    for (rang, entree) in atelier.poids.iter().enumerate() {
        let servi = atelier.servi.as_ref().filter(|s| s.rang == Some(rang));
        match entree {
            Ok(w) => {
                let fichier = w
                    .path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                let ligne = ui
                    .horizontal_wrapped(|ui| {
                        ui.label(titre(
                            w.name.clone().unwrap_or_else(|| fichier.clone()),
                            18.0,
                        ));
                        for (valeur, couleur) in [
                            (w.architecture.clone(), accent.sourd),
                            (w.size_label.clone(), ENCRE),
                            (w.quantization.clone(), ENCRE),
                            (
                                w.context_length
                                    .map(|c| format!("{} tokens de contexte", groupes(c))),
                                ENCRE,
                            ),
                            (Some(format!("{:.1} Go", w.gigabytes())), DISCRET),
                        ]
                        .into_iter()
                        .filter_map(|(v, c)| v.map(|v| (v, c)))
                        {
                            ui.label(RichText::new(valeur).size(12.0).color(couleur));
                        }
                        // Ce que son gabarit déclare : un agent n'y confie pas d'outils sinon.
                        if let Some(t) = w.template {
                            let (texte, couleur) = if t.tool_calls {
                                ("OUTILS", accent.vif)
                            } else {
                                ("SANS OUTILS", DISCRET)
                            };
                            ui.label(RichText::new(texte).size(11.0).strong().color(couleur));
                            if t.reasoning {
                                ui.label(
                                    RichText::new("RÉFLEXION")
                                        .size(11.0)
                                        .strong()
                                        .color(accent.sourd),
                                );
                            }
                        }
                        if recommande.as_ref() == Some(&w.path) {
                            let marque = ui.label(
                                RichText::new("RECOMMANDÉ AUX AGENTS")
                                    .size(11.0)
                                    .strong()
                                    .color(ACCOMPLI),
                            );
                            ui.interact(
                                marque.rect,
                                egui::Id::new(format!("poids-recommande-{fichier}")),
                                egui::Sense::hover(),
                            );
                        }
                        // Le fichier que le moteur a chargé, et la fenêtre qu'il accorde
                        // vraiment : souvent dix fois moins que ce que le fichier annonce.
                        if let Some(servi) = servi {
                            let texte = servi.fenetre.map_or_else(
                                || "SERVI".to_owned(),
                                |n| format!("SERVI · FENÊTRE {}", groupes(n)),
                            );
                            let marque = ui
                                .label(RichText::new(texte).size(11.0).strong().color(accent.vif));
                            ui.interact(
                                marque.rect,
                                egui::Id::new(format!("poids-servi-{fichier}")),
                                egui::Sense::hover(),
                            );
                        }
                    })
                    .response;
                let decrit = format!(
                    "{} — {} {} {}{}{}",
                    fichier,
                    w.architecture.as_deref().unwrap_or(""),
                    w.quantization.as_deref().unwrap_or(""),
                    w.context_length
                        .map_or_else(String::new, |c| format!("{c} tokens de contexte")),
                    match w.template {
                        Some(t) if t.tool_calls => " — appelle des outils",
                        Some(_) => " — sans appels d'outils déclarés",
                        None => "",
                    },
                    servi.map_or_else(String::new, |s| s.fenetre.map_or_else(
                        || " — servi par le moteur".to_owned(),
                        |n| format!(" — servi par le moteur, fenêtre de {n} tokens par requête")
                    ))
                );
                ui.interact(
                    ligne.rect,
                    egui::Id::new(format!("poids-{fichier}")),
                    egui::Sense::hover(),
                )
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &decrit));
                if let Some(estimation) =
                    providers::memory::assess(w, atelier.contexte_local, atelier.memoire.as_ref())
                {
                    let tenue = providers::memory::resident_for(&w.path, &atelier.instances);
                    jauge_de_memoire(
                        ui,
                        &fichier,
                        &estimation,
                        tenue,
                        atelier.memoire.as_ref(),
                        &accent,
                    );
                }
            }
            Err(raison) => {
                ui.label(RichText::new(raison).size(12.0).color(ATTENTE));
            }
        }
        ui.add_space(8.0);
    }
}

/// Ce qu'un poids demande à la mémoire de la machine : la barre est la machine entière ; la
/// part sombre, ce que d'autres occupent déjà ; le segment, ce que le moteur réservera pour ce
/// poids à la fenêtre du système. Il déborde en orange quand il ne tient pas.
fn jauge_de_memoire(
    ui: &mut egui::Ui,
    fichier: &str,
    estimation: &providers::memory::Assessment,
    tenue: Option<providers::memory::Resident>,
    machine: Option<&providers::memory::System>,
    accent: &Accent,
) {
    use providers::memory::{Fit, gigabytes};
    let besoin = estimation.need;
    let couleur = match estimation.fit {
        Some(Fit::TooLarge | Fit::Tight) => ATTENTE,
        Some(Fit::Fits) => accent.vif,
        None => DISCRET,
    };
    ui.add_space(4.0);
    if let Some(machine) = machine.filter(|m| m.total > 0) {
        let largeur = ui.available_width().min(420.0);
        let (rect, _) = ui.allocate_exact_size(vec2(largeur, 6.0), egui::Sense::hover());
        let peintre = ui.painter();
        peintre.rect_filled(rect, 3.0, CREUX);
        let part = |octets: u64| (octets as f64 / machine.total as f64).clamp(0.0, 1.0) as f32;
        let occupe = part(machine.total.saturating_sub(machine.available));
        let debut = rect.left() + rect.width() * occupe;
        peintre.rect_filled(
            egui::Rect::from_min_max(rect.min, pos2(debut, rect.bottom())),
            3.0,
            EFFACE.gamma_multiply(0.45),
        );
        let fin = (debut + rect.width() * part(besoin.total)).min(rect.right());
        peintre.rect_filled(
            egui::Rect::from_min_max(pos2(debut, rect.top()), pos2(fin, rect.bottom())),
            3.0,
            couleur,
        );
        if estimation.fit != Some(Fit::Fits) && tenue.is_none() {
            // Le bord de la machine : ce qui dépasse n'a pas de place.
            peintre.circle_filled(
                pos2(rect.right(), rect.center().y),
                5.0,
                ATTENTE.gamma_multiply(0.45),
            );
        }
        // Servi, le poids est déjà compté dans ce que d'autres occupent : un repère dit ce que
        // l'instance du moteur tient vraiment, mesuré depuis le début de la barre.
        if let Some(t) = tenue {
            let x = rect.left() + rect.width() * part(t.rss);
            peintre.line_segment(
                [pos2(x, rect.top() - 3.0), pos2(x, rect.bottom() + 3.0)],
                Stroke::new(2.0, ENCRE),
            );
        }
    }
    let verdict = estimation
        .fit
        .map_or("mémoire de la machine inconnue", Fit::describe);
    let texte = format!(
        "≈ {} à {} tokens — {verdict}{}",
        gigabytes(besoin.total),
        groupes(besoin.context),
        machine.map_or_else(String::new, |m| format!(
            " · {} libres sur {}",
            gigabytes(m.available),
            gigabytes(m.total)
        ))
    );
    let etiquette = ui.label(RichText::new(&texte).size(11.0).color(couleur));
    if let Some(t) = tenue {
        ui.label(
            RichText::new(format!(
                "Servi : le moteur en tient {} résidents, dont {} anonymes",
                gigabytes(t.rss),
                gigabytes(t.anonymous)
            ))
            .size(11.0)
            .color(ENCRE),
        );
    }
    let detail = format!(
        "{fichier} : mémoire estimée {} (poids {}, cache KV {}, calcul {}) — {verdict}{}",
        gigabytes(besoin.total),
        gigabytes(besoin.weights),
        gigabytes(besoin.kv_cache),
        gigabytes(besoin.compute),
        tenue.map_or_else(String::new, |t| format!(
            " ; le moteur en tient {} résidents",
            gigabytes(t.rss)
        ))
    );
    ui.interact(
        etiquette.rect,
        egui::Id::new(format!("poids-memoire-{fichier}")),
        egui::Sense::hover(),
    )
    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &detail));
}

/// La réserve de microVM, dite en une ligne.
/// Le code d'approbation de cette machine, tel que capd le dit (ADR 0057) : défini, verrouillé
/// ou à choisir. Rend vrai quand l'humain demande à le choisir maintenant.
fn approbations(ui: &mut egui::Ui, code: crate::presence::EtatDuCode, accent: &Accent) -> bool {
    etiquette(ui, "APPROBATIONS");
    ui.add_space(4.0);
    let (titre_du_code, couleur) = match code {
        crate::presence::EtatDuCode {
            verrou_s: Some(s), ..
        } => (
            format!(
                "Verrouillé après trop de codes faux : encore {} min.",
                (s + 59) / 60
            ),
            ATTENTE,
        ),
        crate::presence::EtatDuCode { defini: true, .. } => (
            "Votre code d'approbation est défini.".to_owned(),
            accent.vif,
        ),
        crate::presence::EtatDuCode { defini: false, .. } => (
            "Aucun code d'approbation n'est encore choisi.".to_owned(),
            ATTENTE,
        ),
    };
    let mut demande = false;
    ui.horizontal_wrapped(|ui| {
        let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
        ui.painter().circle_filled(dot.center(), 3.0, couleur);
        ui.label(titre(&titre_du_code, 22.0));
    });
    ui.add_space(6.0);
    petit(
        ui,
        if code.defini {
            "Il est demandé pour accorder une action ; refuser n'en demande jamais. Aucun programme de votre session ne peut accorder sans lui. La commande « prophet cap code » le change."
        } else {
            "Choisissez-le avant qu'une décision n'attende : sans lui, rien ne s'accorde, et le premier à le définir sur cette machine le garde."
        },
    );
    if !code.defini {
        ui.add_space(12.0);
        demande = action(ui, "code-definir-maintenant", "Définir maintenant").clicked();
    }
    demande
}

fn reserve_en_mots(reserve: &crate::scene::Reserve) -> String {
    match &reserve.erreur {
        Some(erreur) if reserve.pretes == 0 => {
            format!("Réserve de microVM vide : {erreur}. Le niveau 2 démarre à froid.")
        }
        _ => format!(
            "Réserve : {} microVM prête{} sur {} — le niveau 2 part sans attendre de démarrage.",
            reserve.pretes,
            if reserve.pretes > 1 { "s" } else { "" },
            reserve.cible
        ),
    }
}

/// Ce que la conversation a laissé hors de l'envoi pour tenir dans la fenêtre du modèle.
fn oubli(messages: usize) -> String {
    if messages == 1 {
        "Le premier message ne tient plus dans la fenêtre du modèle : il ne l'a pas relu.".into()
    } else {
        format!(
            "Les {messages} premiers messages ne tiennent plus dans la fenêtre du modèle : il ne les a pas relus."
        )
    }
}

/// Un nombre en groupes de trois chiffres, séparés d'une espace fine : 40 960.
fn groupes(n: u64) -> String {
    let chiffres = n.to_string();
    let mut out = String::new();
    for (i, c) in chiffres.chars().enumerate() {
        if i > 0 && (chiffres.len() - i).is_multiple_of(3) {
            out.push('\u{202f}');
        }
        out.push(c);
    }
    out
}

fn modeles(ui: &mut egui::Ui, atelier: &mut Atelier) {
    let accent = Accent::de(ui.ctx());
    etiquette(ui, "MODÈLES");
    ui.label(titre("L'intelligence sur votre machine.", 40.0));
    hud::etiquette(ui, "MOTEUR LOCAL ET CLIENTS OFFICIELS", EFFACE);
    atelier.sonder_les_clients(&ui.ctx().clone());
    atelier.lire_les_poids(&ui.ctx().clone());
    atelier.lire_le_catalogue(&ui.ctx().clone());
    ui.add_space(26.0);
    egui::ScrollArea::vertical()
        .id_salt("bibliotheque")
        .show(ui, |ui| {
            plaque(ui, 28, |ui| {
                ui.set_width(ui.available_width());
                clients_officiels(ui, atelier);
            });
            ui.add_space(14.0);
            plaque(ui, 28, |ui| {
                ui.set_width(ui.available_width());
                // L'adresse et ses commandes se centrent sur une même ligne.
                hud::rangee(ui, |ui| {
                    ui.label(
                        RichText::new(&atelier.endpoint)
                            .monospace()
                            .size(12.0)
                            .color(DISCRET),
                    );
                    if ui
                        .add_enabled_ui(!atelier.decouverte, |ui| {
                            hud::bouton(ui, "modeles-actualiser", "Actualiser", false, &accent)
                        })
                        .inner
                        .clicked()
                    {
                        atelier.decouvrir(&ui.ctx().clone());
                    }
                    if atelier.decouverte {
                        ui.spinner();
                    }
                });
                if let Some(error) = &atelier.erreur {
                    egui::CollapsingHeader::new("Diagnostic de connexion").show(ui, |ui| {
                        ui.label(RichText::new(error).size(12.0).color(ATTENTE));
                    });
                }
                if atelier.modeles.is_empty() {
                    ui.add_space(28.0);
                    ui.label(titre(
                        if atelier.decouverte {
                            "Recherche en cours…"
                        } else {
                            "Aucun modèle connecté."
                        },
                        30.0,
                    ));
                    ui.label(
                        RichText::new(
                            "Démarrez un moteur local compatible, puis actualisez cette vue.",
                        )
                        .color(DISCRET),
                    );
                    ui.add_space(8.0);
                    petit(
                        ui,
                        "Sur l'image installée, le moteur démarre avec le modèle que l'installation a déposé ; ailleurs, lancez llama-server puis actualisez.",
                    );
                }
            });
            ui.add_space(14.0);
            plaque(ui, 28, |ui| {
                ui.set_width(ui.available_width());
                poids_installes(ui, atelier);
            });
            ui.add_space(14.0);
            let demande = plaque(ui, 28, |ui| {
                ui.set_width(ui.available_width());
                catalogue_du_systeme(ui, atelier)
            });
            if let Some((commande, id)) = demande {
                atelier.commander_un_poids(&ui.ctx().clone(), commande, &id);
            }
            ui.add_space(18.0);
            for model in &atelier.modeles {
                plaque(ui, 24, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(titre(model, 24.0));
                            pastille(ui, "Disponible pour dialoguer", ACCOMPLI);
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let selected = atelier.choisi == *model;
                            if hud::bouton(
                                ui,
                                &format!("modele-choisir-{model}"),
                                if selected { "Sélectionné" } else { "Choisir" },
                                selected,
                                &accent,
                            )
                            .clicked()
                            {
                                atelier.choisi.clone_from(model);
                            }
                        });
                    });
                });
                ui.add_space(14.0);
            }
        });
}

/// Applique « Mouvement réduit » au style, au changement seulement : réduit, ni transition ni
/// défilement animé — la molette et le clavier déplacent la page d'un coup ; rétabli, les
/// durées d'egui reviennent. Au premier appel sans réduction, le style reste tel qu'il est (un
/// essai qui a figé les transitions les garde figées).
fn appliquer_le_mouvement(ctx: &egui::Context, reduit: bool, applique: &mut Option<bool>) {
    if *applique == Some(reduit) {
        return;
    }
    let premier = applique.is_none();
    *applique = Some(reduit);
    if reduit {
        ctx.all_styles_mut(|s| {
            s.animation_time = 0.0;
            s.scroll_animation = egui::style::ScrollAnimation::none();
        });
    } else if !premier {
        let defaut = egui::Style::default();
        ctx.all_styles_mut(|s| {
            s.animation_time = defaut.animation_time;
            s.scroll_animation = defaut.scroll_animation;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_mouvement_reduit_coupe_transitions_et_defilement_puis_les_rend() {
        let ctx = egui::Context::default();
        let defaut = egui::Style::default();
        let mut applique = None;
        appliquer_le_mouvement(&ctx, true, &mut applique);
        let style = ctx.style_of(egui::Theme::Dark);
        assert!(style.animation_time.abs() < f32::EPSILON);
        assert_eq!(style.scroll_animation, egui::style::ScrollAnimation::none());
        appliquer_le_mouvement(&ctx, false, &mut applique);
        let style = ctx.style_of(egui::Theme::Dark);
        assert!((style.animation_time - defaut.animation_time).abs() < f32::EPSILON);
        assert_eq!(style.scroll_animation, defaut.scroll_animation);
        // Sans réduction dès le départ, le style n'est pas touché.
        let fige = egui::Context::default();
        fige.all_styles_mut(|s| s.animation_time = 0.0);
        let mut applique = None;
        appliquer_le_mouvement(&fige, false, &mut applique);
        assert!(fige.style_of(egui::Theme::Dark).animation_time.abs() < f32::EPSILON);
    }

    #[test]
    fn la_carte_d_un_client_ne_repete_pas_son_etat_et_dit_une_sonde_incertaine() {
        use providers::official::ConnectionState;
        let carte = |present: bool, etat: ConnectionState| ClientCard {
            driver: "codex".into(),
            present,
            version: None,
            connection: etat.label().to_owned(),
            connected: etat == ConnectionState::Connected,
        };
        let (etat, label, conseil) = etat_du_client(&carte(true, ConnectionState::LoginRequired));
        assert_eq!(etat, EtatDuClient::AConnecter);
        assert_eq!(label, "Installé, connexion requise");
        assert!(!conseil.contains("connexion requise"), "{conseil}");
        let (etat, label, _) = etat_du_client(&carte(true, ConnectionState::Unknown));
        assert_eq!(etat, EtatDuClient::Incertain);
        assert_eq!(label, "Installé, session non vérifiée");
        let (etat, label, _) = etat_du_client(&carte(true, ConnectionState::ProbeFailed));
        assert_eq!(etat, EtatDuClient::Incertain);
        assert_eq!(label, "Installé, diagnostic en échec");
        assert_eq!(
            etat_du_client(&carte(true, ConnectionState::Connected)).0,
            EtatDuClient::Pret
        );
        assert_eq!(
            etat_du_client(&carte(false, ConnectionState::ClientMissing)).0,
            EtatDuClient::Absent
        );
    }

    #[test]
    fn un_grand_nombre_se_lit_par_groupes_de_trois() {
        assert_eq!(groupes(7), "7");
        assert_eq!(groupes(40_960), "40\u{202f}960");
        assert_eq!(groupes(1_048_576), "1\u{202f}048\u{202f}576");
    }

    #[test]
    fn la_reserve_se_dit_pleine_partielle_ou_vide() {
        use crate::scene::Reserve;
        let pleine = Reserve {
            pretes: 2,
            cible: 2,
            erreur: None,
        };
        assert!(reserve_en_mots(&pleine).starts_with("Réserve : 2 microVM prêtes sur 2"));
        let une = Reserve {
            pretes: 1,
            cible: 2,
            erreur: None,
        };
        assert!(reserve_en_mots(&une).starts_with("Réserve : 1 microVM prête sur 2"));
        let vide = Reserve {
            pretes: 0,
            cible: 2,
            erreur: Some("instantané refusé".into()),
        };
        assert!(reserve_en_mots(&vide).contains("instantané refusé"));
        assert!(reserve_en_mots(&vide).contains("à froid"));
    }

    #[test]
    fn l_oubli_de_la_conversation_se_dit_au_singulier_comme_au_pluriel() {
        assert!(oubli(1).starts_with("Le premier message"));
        assert!(oubli(6).starts_with("Les 6 premiers messages"));
    }

    #[test]
    fn les_filtres_ne_transforment_pas_un_blocage_en_execution() {
        let c = Courant {
            tache: "a".into(),
            intitule: "mission".into(),
            agent: "local".into(),
            etat: Etat::Bloque,
            debit: 0.0,
            budget_consomme: 0.2,
            etapes: 3,
            task_state: None,
            task_revision: 0,
        };
        assert!(Filtre::Attention.inclut(&c));
        assert!(!Filtre::Actives.inclut(&c));
        assert!(!Filtre::Terminees.inclut(&c));
    }
}

#[cfg(test)]
mod releve_de_la_machine {
    #[test]
    fn un_releve_se_lit_avec_ses_manques_et_sans_ses_vides() {
        let releve = super::lire_inventaire(
            "  processeur : 8 cœurs, AMD Ryzen 5\n\n! son : aucune carte détectée\n  TPM : présent\n",
        );
        assert_eq!(
            releve,
            vec![
                (false, "processeur : 8 cœurs, AMD Ryzen 5".to_owned()),
                (true, "son : aucune carte détectée".to_owned()),
                (false, "TPM : présent".to_owned()),
            ]
        );
        assert!(super::lire_inventaire("").is_empty());
    }
}
