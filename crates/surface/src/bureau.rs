//! Espace de travail natif : saisie, sélection, conversation en flux et activité réelle.

use std::time::Duration;

use crate::atelier::Atelier;
use crate::champ::Champ;
use crate::fenetre::Reponse;
use crate::gpu::{Cible, Contexte, FORMAT};
use crate::scene::Scene;
use crate::supervision::Supervision;
use crate::theme::Accent;

/// Dessin et contrôleur de l'interface interactive, aussi utilisables hors écran.
pub struct Bureau {
    /// Contexte de saisie et d'accessibilité partagé avec winit.
    pub ctx: egui::Context,
    /// Conversation et connexion au moteur.
    pub atelier: Atelier,
    rendu: egui_wgpu::Renderer,
    champ: Champ,
    logiciel: bool,
    supervision: Supervision,
    /// La voix de l'OS, si Piper et une voix sont configurés : les fins de mission se disent.
    voix: Option<voice::Tools>,
    /// Instant du dernier geste de l'humain (pointeur, clavier, défilement, toucher).
    dernier_geste: Option<f64>,
    /// L'horloge du champ, arrêtée pendant la veille.
    horloge: HorlogeDuChamp,
}

/// Délai sans geste au-delà duquel le champ ralentit sa cadence.
pub const ATTENTION: f64 = 30.0;

/// Délai sans geste au-delà duquel, sur un rastériseur logiciel, le champ se fige (ADR 0055).
pub const VEILLE: f64 = 120.0;

/// Vrai si le champ doit se figer : sur un rastériseur logiciel, chaque image coûte au
/// processeur qui fait aussi tourner le modèle local ; après [`VEILLE`] secondes sans geste,
/// l'écran ne se redessine plus que lorsque l'état des missions change. Sur une carte
/// graphique, le champ ne se fige jamais.
#[must_use]
pub fn en_veille(logiciel: bool, sans_geste: f64) -> bool {
    logiciel && sans_geste > VEILLE
}

/// L'horloge du champ : celle de l'interface, moins le temps passé en veille. Le champ se fige
/// là où il était et repart de là au premier geste, sans saut.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct HorlogeDuChamp {
    /// Instant de l'interface où la veille a commencé.
    depuis: Option<f64>,
    /// Temps déjà passé en veille.
    decalage: f64,
}

impl HorlogeDuChamp {
    /// L'instant du champ pour l'instant `temps` de l'interface.
    pub fn instant(&mut self, temps: f64, veille: bool) -> f64 {
        match (veille, self.depuis) {
            (true, None) => self.depuis = Some(temps),
            (false, Some(depuis)) => {
                self.decalage += temps - depuis;
                self.depuis = None;
            }
            _ => {}
        }
        self.depuis.unwrap_or(temps) - self.decalage
    }
}

/// La cadence du champ vivant : celle de l'écran tant que l'humain agit, la moitié sur un
/// rastériseur logiciel ; au-delà de [`ATTENTION`] secondes sans geste, 20 images par seconde
/// (10 en logiciel). Le mouvement suit l'horloge, pas le nombre d'images : l'état montré reste
/// exact, seul son lissé baisse quand personne n'interagit.
#[must_use]
pub fn cadence_du_champ(logiciel: bool, sans_geste: f64) -> Duration {
    let millis = match (logiciel, sans_geste > ATTENTION) {
        (false, false) => 16,
        (true, false) => 33,
        (false, true) => 50,
        (true, true) => 100,
    };
    Duration::from_millis(millis)
}

impl Bureau {
    /// Installe le thème, les polices embarquées, le champ et le renderer, sans ouvrir de
    /// connexion au moteur.
    #[must_use]
    pub fn nouveau(contexte: &Contexte, endpoint: String, demonstration: bool) -> Self {
        let ctx = egui::Context::default();
        ctx.set_fonts(polices());
        crate::theme::accent_configure().installer(&ctx);
        crate::supervision::installer_style(&ctx);
        let format = contexte
            .configuration_surface
            .as_ref()
            .map_or(FORMAT, |config| config.format)
            .remove_srgb_suffix();
        Self {
            ctx,
            atelier: Atelier::nouveau(endpoint, demonstration),
            supervision: Supervision::default(),
            voix: voice::Tools::from_env()
                .ok()
                .filter(voice::Tools::can_speak),
            dernier_geste: None,
            horloge: HorlogeDuChamp::default(),
            champ: Champ::nouveau(&contexte.device, format, contexte.logiciel),
            logiciel: contexte.logiciel,
            rendu: egui_wgpu::Renderer::new(&contexte.device, format, Default::default()),
        }
    }

    /// Fige les transitions d'apparition pour les captures à un instant constant.
    pub fn figer_transitions(&self) {
        self.ctx.all_styles_mut(|style| {
            style.animation_time = 0.0;
            style.scroll_animation = egui::style::ScrollAnimation::none();
        });
    }

    /// Change l'accent de cette session sans le conserver.
    pub fn choisir_accent(&self, accent: Accent) {
        accent.installer(&self.ctx);
        crate::supervision::installer_style(&self.ctx);
    }

    /// Vrai si l'interface se dessine sur un rastériseur logiciel.
    #[must_use]
    pub fn logiciel(&self) -> bool {
        self.logiciel
    }

    /// L'accent en vigueur.
    #[must_use]
    pub fn accent(&self) -> Accent {
        Accent::de(&self.ctx)
    }

    /// Raccorde les commandes explicites de mission à un service de confiance.
    /// Les scènes de démonstration ne peuvent pas activer ce transport.
    pub fn brancher_missions(&mut self, socket: std::path::PathBuf) {
        if !self.atelier.demonstration {
            self.supervision.missions = crate::missions::Missions::connect(socket.clone());
            self.supervision.arret = crate::arret::Arret::connect(socket.clone());
            self.supervision.preparation = crate::preparation::Preparation::connect(socket);
        }
    }

    /// Raccorde la lecture du journal : les appels d'outils de la mission sélectionnée, avec
    /// leur cible contrôlée et leur issue, jamais leur contenu.
    pub fn brancher_journal(&mut self, socket: std::path::PathBuf) {
        if !self.atelier.demonstration {
            self.supervision.missions.brancher_journal(socket);
        }
    }

    /// Contrôleur de la mission sélectionnée, pour l'intégration native et ses essais.
    pub fn missions(&mut self) -> &mut crate::missions::Missions {
        &mut self.supervision.missions
    }

    /// Le rectangle d'une plaque à la dernière image, si elle était dessinée :
    /// `espace-de-mission` ou `espace-vide`. Les parcours vérifient qu'elle tient dans sa
    /// colonne et dans l'écran.
    #[must_use]
    pub fn plaque(&self, nom: &str) -> Option<egui::Rect> {
        crate::hud::retenue(&self.ctx, nom)
    }

    /// Brouillon et catalogue de préparation de mission.
    pub fn preparation(&mut self) -> &mut crate::preparation::Preparation {
        &mut self.supervision.preparation
    }

    /// Vrai si le champ avancera à l'image suivante : une mission progresse et le mouvement
    /// n'est pas réduit. C'est ce qui décide du rythme de redessin de la fenêtre.
    #[must_use]
    pub fn champ_vivant(&self) -> bool {
        self.champ.vivant(self.atelier.mouvement_reduit)
    }

    /// Impose le champ complet, même sur un rastériseur logiciel : pour des captures et des
    /// mesures comparables à celles d'une carte graphique.
    pub fn forcer_champ_complet(&mut self, contexte: &Contexte) {
        let format = contexte
            .configuration_surface
            .as_ref()
            .map_or(FORMAT, |config| config.format)
            .remove_srgb_suffix();
        self.champ = Champ::nouveau(&contexte.device, format, false);
        self.logiciel = false;
    }

    /// Le nombre de particules que le champ trace par image, pour les mesures.
    #[must_use]
    pub fn particules_du_champ(&self) -> u32 {
        self.champ.particules()
    }

    /// Prépare les widgets et retourne une éventuelle décision humaine.
    pub fn composer(
        &mut self,
        input: egui::RawInput,
        scene: &Scene,
    ) -> (egui::FullOutput, Option<Reponse>) {
        self.atelier.actualiser();
        self.supervision.missions.update();
        // Une décision qui attend l'humain se dit, une fois : il peut l'accorder ou la refuser
        // de vive voix sans regarder l'écran (ADR 0036, 0041).
        self.supervision
            .missions
            .dire_la_decision(scene.decision.as_ref());
        if let Some(texte) = self.supervision.missions.take_announcement()
            && let Some(voix) = self.voix.clone()
        {
            // Hors du fil graphique : la synthèse et la lecture prennent des secondes.
            std::thread::spawn(move || {
                if let Err(e) = voix.say(&texte) {
                    eprintln!("prophet-surface : résultat non dit : {e}");
                }
            });
        }
        let mut scene = scene.clone();
        self.supervision.missions.align_scene(&mut scene);
        let temps = input.time.unwrap_or(0.0);
        let geste = input.events.iter().any(|e| {
            matches!(
                e,
                egui::Event::PointerMoved(_)
                    | egui::Event::PointerButton { .. }
                    | egui::Event::MouseWheel { .. }
                    | egui::Event::Key { .. }
                    | egui::Event::Text(_)
                    | egui::Event::Paste(_)
                    | egui::Event::Touch { .. }
            )
        });
        if geste || self.dernier_geste.is_none() {
            self.dernier_geste = Some(temps);
        }
        let sans_geste = temps - self.dernier_geste.unwrap_or(temps);
        let veille = en_veille(self.logiciel, sans_geste);
        let temps_du_champ = self.horloge.instant(temps, veille);
        let mut decision = None;
        let atelier = &mut self.atelier;
        let supervision = &mut self.supervision;
        let output = self.ctx.run_ui(input, |root| {
            supervision.dessiner(root, atelier, &scene, &mut decision);
        });
        self.champ.preparer(
            &scene,
            supervision.selection(),
            temps_du_champ,
            atelier.mouvement_reduit,
            Accent::de(&self.ctx),
        );
        if self.champ.vivant(atelier.mouvement_reduit || veille) {
            // Le champ avance à la cadence de l'écran tant qu'une mission progresse ; au repos,
            // la surveillance des services garde son propre rythme et rien ne se redessine.
            // Un rastériseur logiciel reçoit la moitié de cette cadence : le processeur
            // dessine, et il a d'autres choses à faire pour les missions. Sans geste de
            // l'humain depuis un moment, la cadence baisse encore (`cadence_du_champ`), puis,
            // en logiciel, le champ se fige (`en_veille`).
            self.ctx
                .request_repaint_after(cadence_du_champ(self.logiciel, sans_geste));
        }
        (output, decision)
    }

    /// Soumet le champ puis les formes de l'interface au GPU, avec des ressources réutilisées.
    pub fn rendre(&mut self, contexte: &Contexte, cible: &Cible, output: &mut egui::FullOutput) {
        let device = &contexte.device;
        let queue = &contexte.queue;
        // egui prémultiplie dans l'espace gamma : le mélange doit se faire en Unorm.
        // La texture autorise cette vue jumelle ; le renderer historique garde sa vue sRGB.
        let view = cible.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(cible.texture.format().remove_srgb_suffix()),
            ..Default::default()
        });
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                self.rendu.update_texture(device, queue, *id, delta);
            }
        }
        let jobs = self
            .ctx
            .tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [cible.largeur, cible.hauteur],
            pixels_per_point: output.pixels_per_point,
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("bureau"),
        });
        let callbacks = self
            .rendu
            .update_buffers(device, queue, &mut encoder, &jobs, &screen);
        {
            let fond = crate::theme::palette::FOND;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bureau"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // La vue est Unorm : la valeur écrite est celle qu'on lit à l'écran.
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(fond.r()) / 255.0,
                            g: f64::from(fond.g()) / 255.0,
                            b: f64::from(fond.b()) / 255.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // Le champ d'abord, l'interface ensuite : les surfaces de verre le laissent passer.
            self.champ
                .dessiner(queue, &mut pass, cible.largeur, cible.hauteur);
            self.rendu
                .render(&mut pass.forget_lifetime(), &jobs, &screen);
        }
        queue.submit(callbacks.into_iter().chain([encoder.finish()]));
        for id in &output.textures_delta.free {
            self.rendu.free_texture(id);
        }
        output.textures_delta.clear();
    }
}

/// La police embarquée : Inter, en trois graisses. Une graisse est une coordonnée sur le
/// même fichier variable, pas une copie : la fine pour les grands chiffres et les titres, la
/// régulière pour lire, la demi-grasse pour ce qui doit tenir sans crier.
fn polices() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    let inter = include_bytes!("../assets/InterVariable.ttf");
    fonts
        .font_data
        .insert("Inter".into(), egui::FontData::from_static(inter).into());
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .expect("famille proportionnelle")
        .insert(0, "Inter".into());
    for (name, weight) in [("Inter300", 300.0), ("Inter600", 600.0)] {
        let mut font = egui::FontData::from_static(inter);
        font.tweak.coords = egui::epaint::text::VariationCoords::new([(b"wght", weight)]);
        fonts.font_data.insert(name.into(), font.into());
        fonts
            .families
            .insert(egui::FontFamily::Name(name.into()), vec![name.into()]);
    }
    fonts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_champ_ralentit_quand_personne_n_agit_et_reprend_au_premier_geste() {
        assert_eq!(cadence_du_champ(false, 0.0), Duration::from_millis(16));
        assert_eq!(cadence_du_champ(true, 0.0), Duration::from_millis(33));
        assert_eq!(
            cadence_du_champ(false, ATTENTION),
            Duration::from_millis(16)
        );
        assert_eq!(
            cadence_du_champ(false, ATTENTION + 1.0),
            Duration::from_millis(50)
        );
        assert_eq!(
            cadence_du_champ(true, ATTENTION + 1.0),
            Duration::from_millis(100)
        );
    }

    #[test]
    fn seul_un_rasteriseur_logiciel_fige_le_champ_et_seulement_apres_la_veille() {
        assert!(!en_veille(true, VEILLE));
        assert!(en_veille(true, VEILLE + 1.0));
        assert!(
            !en_veille(false, VEILLE * 10.0),
            "une carte graphique ne fige jamais"
        );
    }

    #[test]
    fn le_champ_se_fige_ou_il_est_et_repart_de_la_sans_saut() {
        let mut horloge = HorlogeDuChamp::default();
        assert_eq!(horloge.instant(10.0, false), 10.0);
        // La veille commence à 130 : le champ reste à 130 tant qu'elle dure.
        assert_eq!(horloge.instant(130.0, true), 130.0);
        assert_eq!(horloge.instant(500.0, true), 130.0);
        // Au premier geste, à 600, le champ repart de 130 et avance de nouveau.
        assert_eq!(horloge.instant(600.0, false), 130.0);
        assert_eq!(horloge.instant(601.0, false), 131.0);
        // Une deuxième veille s'ajoute à la première.
        assert_eq!(horloge.instant(700.0, true), 230.0);
        assert_eq!(horloge.instant(710.0, false), 230.0);
    }
}
