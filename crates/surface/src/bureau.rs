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
            champ: Champ::nouveau(&contexte.device, format, contexte.logiciel),
            logiciel: contexte.logiciel,
            rendu: egui_wgpu::Renderer::new(&contexte.device, format, Default::default()),
        }
    }

    /// Fige les transitions d'apparition pour les captures à un instant constant.
    pub fn figer_transitions(&self) {
        self.ctx.all_styles_mut(|style| style.animation_time = 0.0);
    }

    /// Change l'accent de cette session sans le conserver.
    pub fn choisir_accent(&self, accent: Accent) {
        accent.installer(&self.ctx);
        crate::supervision::installer_style(&self.ctx);
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
        let mut decision = None;
        let atelier = &mut self.atelier;
        let supervision = &mut self.supervision;
        let output = self.ctx.run_ui(input, |root| {
            supervision.dessiner(root, atelier, &scene, &mut decision);
        });
        self.champ.preparer(
            &scene,
            supervision.selection(),
            temps,
            atelier.mouvement_reduit,
            Accent::de(&self.ctx),
        );
        if self.champ.vivant(atelier.mouvement_reduit) {
            // Le champ avance à la cadence de l'écran tant qu'une mission progresse ; au repos,
            // la surveillance des services garde son propre rythme et rien ne se redessine.
            // Un rastériseur logiciel reçoit la moitié de cette cadence : le processeur
            // dessine, et il a d'autres choses à faire pour les missions.
            self.ctx
                .request_repaint_after(Duration::from_millis(if self.logiciel { 33 } else { 16 }));
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
