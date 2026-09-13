//! Espace de travail natif : saisie, sélection, conversation en flux et activité réelle.

use crate::atelier::Atelier;
use crate::fenetre::Reponse;
use crate::gpu::{Cible, Contexte, FORMAT};
use crate::scene::Scene;
use crate::supervision::Supervision;

/// Dessin et contrôleur de l'interface interactive, aussi utilisables hors écran.
pub struct Bureau {
    /// Contexte de saisie et d'accessibilité partagé avec winit.
    pub ctx: egui::Context,
    /// Conversation et connexion au moteur.
    pub atelier: Atelier,
    rendu: egui_wgpu::Renderer,
    supervision: Supervision,
}

impl Bureau {
    /// Installe le thème et le renderer sans ouvrir de connexion au moteur.
    #[must_use]
    pub fn nouveau(contexte: &Contexte, endpoint: String, demonstration: bool) -> Self {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "Inter".into(),
            egui::FontData::from_static(include_bytes!("../assets/InterVariable.ttf")).into(),
        );
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .expect("famille proportionnelle")
            .insert(0, "Inter".into());
        let mut semibold =
            egui::FontData::from_static(include_bytes!("../assets/InterVariable.ttf"));
        semibold.tweak.coords = egui::epaint::text::VariationCoords::new([(b"wght", 600.0)]);
        fonts.font_data.insert("Inter600".into(), semibold.into());
        fonts.families.insert(
            egui::FontFamily::Name("Inter600".into()),
            vec!["Inter600".into()],
        );
        ctx.set_fonts(fonts);
        crate::supervision::installer_style(&ctx);
        Self {
            ctx,
            atelier: Atelier::nouveau(endpoint, demonstration),
            supervision: Supervision::default(),
            rendu: egui_wgpu::Renderer::new(
                &contexte.device,
                contexte
                    .configuration_surface
                    .as_ref()
                    .map_or(FORMAT, |config| config.format)
                    .remove_srgb_suffix(),
                Default::default(),
            ),
        }
    }

    /// Fige les transitions d'apparition pour les captures à un instant constant.
    pub fn figer_transitions(&self) {
        self.ctx.all_styles_mut(|style| style.animation_time = 0.0);
    }

    /// Raccorde les commandes explicites de mission à un service de confiance.
    /// Les scènes de démonstration ne peuvent pas activer ce transport.
    pub fn brancher_missions(&mut self, socket: std::path::PathBuf) {
        if !self.atelier.demonstration {
            self.supervision.missions = crate::missions::Missions::connect(socket.clone());
            self.supervision.preparation = crate::preparation::Preparation::connect(socket);
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

    /// Prépare les widgets et retourne une éventuelle décision humaine.
    pub fn composer(
        &mut self,
        input: egui::RawInput,
        scene: &Scene,
    ) -> (egui::FullOutput, Option<Reponse>) {
        self.atelier.actualiser();
        self.supervision.missions.update();
        let mut scene = scene.clone();
        self.supervision.missions.align_scene(&mut scene);
        let mut decision = None;
        let atelier = &mut self.atelier;
        let supervision = &mut self.supervision;
        let output = self.ctx.run_ui(input, |root| {
            supervision.dessiner(root, atelier, &scene, &mut decision);
        });
        (output, decision)
    }

    /// Soumet les formes de l'interface au GPU, avec des ressources de texte réutilisées.
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
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bureau"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
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
