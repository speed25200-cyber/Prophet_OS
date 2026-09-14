//! Le rendu proprement dit.
//!
//! Trois passes, dans cet ordre : le champ, les panneaux, le texte. L'ordre importe — le champ
//! traverse tout l'écran et passe donc *derrière* les panneaux, qui sont légèrement opaques ; on
//! voit les filaments transparaître, ce qui rattache les panneaux à ce qui se passe au lieu de les
//! poser dessus.

use glyphon::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport, Weight,
};
use wgpu::util::DeviceExt as _;

use crate::disposition::{Contenu, Rect, disposer};
use crate::gpu::{Cible, Contexte, ErreurGpu, FORMAT};
use crate::scene::{Etat, Scene};
use crate::theme;

/// Particules par courant. Assez pour un ruban continu, assez peu pour qu'une machine modeste
/// tienne les soixante images par seconde.
///
/// Cette valeur doit rester identique à `par_courant` dans `flux.wgsl` : le shader en déduit à
/// quel courant appartient chaque instance, et une divergence mélangerait silencieusement les
/// filaments.
const PARTICULES: u32 = 5000;

/// Ce qui peut empêcher un rendu.
#[derive(Debug, thiserror::Error)]
pub enum ErreurRendu {
    /// Le contexte graphique.
    #[error(transparent)]
    Gpu(#[from] ErreurGpu),
    /// Aucune police n'est installée.
    #[error(
        "aucune police disponible : le texte serait invisible et le rendu paraîtrait réussi. \
         Installez au moins une police système."
    )]
    AucunePolice,
    /// La préparation du texte a échoué.
    #[error("texte impossible à préparer : {0}")]
    Texte(String),
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CadreFlux {
    resolution: [f32; 2],
    temps: f32,
    attenuation: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CourantGpu {
    base: f32,
    amplitude: f32,
    vitesse: f32,
    clarte: f32,
    phase: f32,
    teinte: f32,
    _remplissage: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CadrePanneaux {
    resolution: [f32; 2],
    _remplissage: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PanneauGpu {
    origine: [f32; 2],
    taille: [f32; 2],
    fond: [f32; 4],
    trait_: [f32; 4],
    rayon: f32,
    epaisseur: f32,
    _fin: [f32; 2],
}

/// Le moteur de rendu. Construit une fois, réutilisé à chaque image.
pub struct Rendu {
    pipeline_flux: wgpu::RenderPipeline,
    disposition_flux: wgpu::BindGroupLayout,
    pipeline_panneaux: wgpu::RenderPipeline,
    disposition_panneaux: wgpu::BindGroupLayout,
    polices: FontSystem,
    cache_glyphes: SwashCache,
    atlas: TextAtlas,
    rendu_texte: TextRenderer,
    fenetre_texte: Viewport,
}

impl std::fmt::Debug for Rendu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Rendu")
    }
}

impl Rendu {
    /// Met en place les pipelines.
    ///
    /// # Errors
    /// Si aucune police n'est installée : un rendu sans texte paraîtrait réussi, ce qui est pire
    /// qu'un échec.
    pub fn nouveau(contexte: &Contexte) -> Result<Self, ErreurRendu> {
        let device = &contexte.device;

        let (pipeline_flux, disposition_flux) = pipeline(
            device,
            "flux",
            include_str!("flux.wgsl"),
            // Additif : les filaments qui se croisent s'additionnent, comme de la lumière. Un
            // mélange classique les ferait se masquer, ce qui écraserait la profondeur.
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
            },
        );

        let (pipeline_panneaux, disposition_panneaux) = pipeline(
            device,
            "panneaux",
            include_str!("panneaux.wgsl"),
            // Prémultiplié : le fragment rend déjà une couleur pondérée par son opacité.
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        );

        let polices = FontSystem::new();
        if polices.db().is_empty() {
            return Err(ErreurRendu::AucunePolice);
        }

        // `Cache` partage son contenu par compteur de références : l'atlas et la fenêtre en
        // gardent ce qu'il leur faut, il n'y a donc rien à retenir ici.
        let cache_texte = glyphon::Cache::new(device);
        let mut atlas = TextAtlas::new(device, &contexte.queue, &cache_texte, FORMAT);
        let rendu_texte =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        let fenetre_texte = Viewport::new(device, &cache_texte);

        Ok(Self {
            pipeline_flux,
            disposition_flux,
            pipeline_panneaux,
            disposition_panneaux,
            polices,
            cache_glyphes: SwashCache::new(),
            atlas,
            rendu_texte,
            fenetre_texte,
        })
    }

    /// Dessine une scène dans une cible, à un instant donné.
    ///
    /// `temps` est en secondes depuis le début : c'est lui qui fait avancer les courants. Le même
    /// couple (scène, temps) donne toujours la même image, ce qui rend le rendu comparable.
    ///
    /// # Errors
    /// Si le texte ne peut pas être préparé.
    pub fn dessiner(
        &mut self,
        contexte: &Contexte,
        cible: &Cible,
        scene: &Scene,
        temps: f32,
    ) -> Result<(), ErreurRendu> {
        let device = &contexte.device;
        let queue = &contexte.queue;
        let largeur = cible.largeur as f32;
        let hauteur = cible.hauteur as f32;

        // --- Le champ ---
        let courants: Vec<CourantGpu> = scene
            .courants
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let rang = i as f32;
                CourantGpu {
                    // Les filaments se répartissent sur la hauteur sans jamais se superposer
                    // exactement : deux courants confondus ne se distingueraient plus.
                    base: 0.30 + 0.13 * rang,
                    amplitude: 0.055 + 0.012 * (rang % 3.0),
                    vitesse: c.vitesse(),
                    clarte: c.clarte(),
                    phase: rang * 2.39,
                    teinte: if c.reclame() { 1.0 } else { 0.0 },
                    _remplissage: [0.0; 2],
                }
            })
            .collect();
        // Un tampon de stockage vide est refusé ; on garde toujours un courant, éteint, qui ne
        // dessine rien mais laisse le pipeline valide.
        let courants = if courants.is_empty() {
            vec![CourantGpu {
                base: 0.5,
                amplitude: 0.0,
                vitesse: 0.0,
                clarte: 0.0,
                phase: 0.0,
                teinte: 0.0,
                _remplissage: [0.0; 2],
            }]
        } else {
            courants
        };

        let cadre_flux = CadreFlux {
            resolution: [largeur, hauteur],
            temps,
            attenuation: scene.attenuation_du_champ(),
        };
        let groupe_flux = groupe(
            device,
            &self.disposition_flux,
            bytemuck::bytes_of(&cadre_flux),
            bytemuck::cast_slice(&courants),
        );

        // --- Les panneaux ---
        let places = disposer(scene, largeur, hauteur);
        let mut panneaux = Vec::with_capacity(places.len());
        for place in &places {
            let urgent = matches!(place.contenu, Contenu::Decision);
            panneaux.push(PanneauGpu {
                origine: [place.rect.x, place.rect.y],
                taille: [place.rect.l, place.rect.h],
                fond: premultiplie(theme::panneau()),
                trait_: premultiplie(if urgent {
                    theme::attente().opacite(0.55)
                } else {
                    theme::bordure()
                }),
                rayon: theme::RAYON,
                epaisseur: if urgent { 1.6 } else { 1.0 },
                _fin: [0.0; 2],
            });
        }
        let cadre_panneaux = CadrePanneaux {
            resolution: [largeur, hauteur],
            _remplissage: [0.0; 2],
        };
        let groupe_panneaux = groupe(
            device,
            &self.disposition_panneaux,
            bytemuck::bytes_of(&cadre_panneaux),
            bytemuck::cast_slice(&panneaux),
        );

        // --- Le texte ---
        self.fenetre_texte.update(
            queue,
            Resolution {
                width: cible.largeur,
                height: cible.hauteur,
            },
        );
        let lignes = self.composer(scene, &places);
        let zones: Vec<TextArea<'_>> = lignes
            .iter()
            .map(|(tampon, rect, couleur)| TextArea {
                buffer: tampon,
                left: rect.x,
                top: rect.y,
                scale: 1.0,
                bounds: TextBounds {
                    left: rect.x as i32,
                    top: rect.y as i32,
                    right: rect.droite() as i32,
                    bottom: rect.bas() as i32,
                },
                default_color: *couleur,
                custom_glyphs: &[],
            })
            .collect();
        self.rendu_texte
            .prepare(
                device,
                queue,
                &mut self.polices,
                &mut self.atlas,
                &self.fenetre_texte,
                zones,
                &mut self.cache_glyphes,
            )
            .map_err(|e| ErreurRendu::Texte(e.to_string()))?;

        // --- La passe ---
        let fond = theme::fond();
        let mut encodeur = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("surface"),
        });
        {
            let mut passe = encodeur.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("surface"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &cible.vue,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(fond.r),
                            g: f64::from(fond.v),
                            b: f64::from(fond.b),
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

            passe.set_pipeline(&self.pipeline_flux);
            passe.set_bind_group(0, &groupe_flux, &[]);
            passe.draw(0..6, 0..(courants.len() as u32 * PARTICULES));

            if !panneaux.is_empty() {
                passe.set_pipeline(&self.pipeline_panneaux);
                passe.set_bind_group(0, &groupe_panneaux, &[]);
                passe.draw(0..6, 0..(panneaux.len() as u32));
            }

            self.rendu_texte
                .render(&self.atlas, &self.fenetre_texte, &mut passe)
                .map_err(|e| ErreurRendu::Texte(e.to_string()))?;
        }
        queue.submit(Some(encodeur.finish()));
        self.atlas.trim();
        Ok(())
    }

    /// Compose le texte de chaque panneau.
    fn composer(
        &mut self,
        scene: &Scene,
        places: &[crate::disposition::Place],
    ) -> Vec<(Buffer, Rect, Color)> {
        let mut sorties = Vec::new();
        for place in places {
            let interieur = place.rect.retreci(theme::PAS * 1.2);
            match &place.contenu {
                Contenu::Courant(rang) => {
                    let Some(c) = scene.courants.get(*rang) else {
                        continue;
                    };
                    let etat = match c.etat {
                        Etat::Court => "en cours",
                        Etat::Attend => "attend une décision",
                        Etat::Bloque => "empêchée",
                        Etat::Fini => "terminée",
                    };
                    sorties.push(self.bloc(
                        &c.intitule,
                        18.0,
                        Weight::NORMAL,
                        interieur,
                        couleur(theme::texte()),
                    ));
                    sorties.push(self.bloc(
                        &format!("{}  ·  {}  ·  {} étapes", c.agent, etat, c.etapes),
                        12.0,
                        Weight::NORMAL,
                        Rect {
                            y: interieur.y + 30.0,
                            ..interieur
                        },
                        couleur(if c.reclame() {
                            theme::attente()
                        } else {
                            theme::texte_discret()
                        }),
                    ));
                    sorties.push(self.bloc(
                        &format!("{} % du budget", (c.budget_consomme * 100.0).round() as u32),
                        12.0,
                        Weight::NORMAL,
                        Rect {
                            y: interieur.y + 52.0,
                            ..interieur
                        },
                        couleur(theme::or_eteint()),
                    ));
                }
                Contenu::Isolation => {
                    sorties.push(self.bloc(
                        "ISOLATION",
                        11.0,
                        Weight::BOLD,
                        interieur,
                        couleur(theme::or_eteint()),
                    ));
                    sorties.push(self.bloc(
                        &format!("niveau {}", scene.isolation.niveau_max),
                        30.0,
                        Weight::NORMAL,
                        Rect {
                            y: interieur.y + 26.0,
                            ..interieur
                        },
                        couleur(theme::or()),
                    ));
                    let dit = scene.isolation.manque.as_ref().map_or_else(
                        || "tout ce qui est prévu est disponible".to_owned(),
                        |m| format!("il manque {m}"),
                    );
                    sorties.push(self.bloc(
                        &dit,
                        12.0,
                        Weight::NORMAL,
                        Rect {
                            y: interieur.y + 72.0,
                            ..interieur
                        },
                        couleur(theme::texte_discret()),
                    ));
                }
                Contenu::Horloge => {
                    sorties.push(self.bloc(
                        &scene.heure,
                        40.0,
                        Weight::NORMAL,
                        interieur,
                        couleur(theme::texte()),
                    ));
                    sorties.push(self.bloc(
                        &scene.date,
                        12.0,
                        Weight::NORMAL,
                        Rect {
                            y: interieur.y + 54.0,
                            ..interieur
                        },
                        couleur(theme::texte_discret()),
                    ));
                }
                Contenu::Decision => {
                    let Some(d) = &scene.decision else { continue };
                    sorties.push(self.bloc(
                        if d.irreversible {
                            "DÉCISION — SANS RETOUR"
                        } else {
                            "DÉCISION"
                        },
                        11.0,
                        Weight::BOLD,
                        interieur,
                        couleur(theme::attente()),
                    ));
                    sorties.push(self.bloc(
                        &d.question,
                        26.0,
                        Weight::NORMAL,
                        Rect {
                            y: interieur.y + 30.0,
                            ..interieur
                        },
                        couleur(theme::texte()),
                    ));
                    sorties.push(self.bloc(
                        &d.consequence,
                        15.0,
                        Weight::NORMAL,
                        Rect {
                            y: interieur.y + 96.0,
                            ..interieur
                        },
                        couleur(theme::texte_discret()),
                    ));
                    if let Some(motif) = &d.motif {
                        sorties.push(self.bloc(
                            &format!("Le modèle dit : « {motif} »"),
                            14.0,
                            Weight::NORMAL,
                            Rect {
                                y: interieur.y + 140.0,
                                ..interieur
                            },
                            couleur(theme::texte_discret()),
                        ));
                    }
                    sorties.push(self.bloc(
                        "  ENTRÉE pour accepter          ÉCHAP pour refuser  ",
                        14.0,
                        Weight::BOLD,
                        Rect {
                            y: interieur.bas() - 30.0,
                            ..interieur
                        },
                        couleur(theme::or()),
                    ));
                }
            }
        }
        sorties
    }

    fn bloc(
        &mut self,
        texte: &str,
        taille: f32,
        graisse: Weight,
        rect: Rect,
        couleur: Color,
    ) -> (Buffer, Rect, Color) {
        let mut tampon = Buffer::new(&mut self.polices, Metrics::new(taille, taille * 1.32));
        tampon.set_size(Some(rect.l), Some(rect.h));
        tampon.set_text(
            texte,
            &Attrs::new().family(Family::SansSerif).weight(graisse),
            Shaping::Advanced,
            None,
        );
        tampon.shape_until_scroll(&mut self.polices, false);
        (tampon, rect, couleur)
    }
}

fn couleur(c: theme::Couleur) -> Color {
    // glyphon attend du sRGB sur huit bits ; le thème travaille en linéaire.
    let vers_srgb = |v: f32| {
        let s = if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (s.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    Color::rgba(vers_srgb(c.r), vers_srgb(c.v), vers_srgb(c.b), 255)
}

fn premultiplie(c: theme::Couleur) -> [f32; 4] {
    [c.r * c.a, c.v * c.a, c.b * c.a, c.a]
}

fn pipeline(
    device: &wgpu::Device,
    nom: &str,
    source: &str,
    melange: wgpu::BlendState,
) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(nom),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let disposition = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(nom),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let agencement = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(nom),
        bind_group_layouts: &[Some(&disposition)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(nom),
        layout: Some(&agencement),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            targets: &[Some(wgpu::ColorTargetState {
                format: FORMAT,
                blend: Some(melange),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    (pipeline, disposition)
}

fn groupe(
    device: &wgpu::Device,
    disposition: &wgpu::BindGroupLayout,
    uniforme: &[u8],
    stockage: &[u8],
) -> wgpu::BindGroup {
    let tampon_uniforme = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("uniforme"),
        contents: uniforme,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let tampon_stockage = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("stockage"),
        contents: stockage,
        usage: wgpu::BufferUsages::STORAGE,
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("groupe"),
        layout: disposition,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: tampon_uniforme.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: tampon_stockage.as_entire_binding(),
            },
        ],
    })
}
