//! Le champ vivant : ce qui, derrière les surfaces de l'atelier, montre les missions en marche.
//!
//! Le champ n'est pas un fond d'écran. Chaque ruban est une mission reçue des services ; sa
//! vitesse est le rythme réel des étapes, sa clarté le budget qui reste, sa teinte dit si un
//! humain est attendu, et la mission choisie s'éclaire. Quand rien ne tourne, rien ne bouge :
//! la grille et la poussière de la voûte sont fixes, et une image au repos est identique à la
//! suivante. La couleur est celle de l'accent choisi.
//!
//! Il se dessine dans la même passe que l'interface, avant elle, dans la vue `Unorm` où egui
//! mélange ses couleurs ; les surfaces de verre le laissent transparaître.

use crate::scene::{Courant, Scene};
use crate::theme::Accent;
use bytemuck::Zeroable as _;
use wgpu::util::DeviceExt as _;

/// Grains de la voûte. Fixes : ils donnent la profondeur, pas le mouvement.
const POUSSIERE: u32 = 3000;
/// Particules par ruban, sur une carte graphique : assez pour un ruban continu sur un écran
/// large. Une carte les trace en une fraction de milliseconde.
const PAR_COURANT: u32 = 6000;
/// Ce qu'un rastériseur logiciel reçoit : les rubans sont plus clairsemés et la voûte plus
/// rare, pour qu'une machine virtuelle sans carte tienne le rythme au lieu de le subir.
const POUSSIERE_LOGICIEL: u32 = 1200;
const PAR_COURANT_LOGICIEL: u32 = 1600;
/// Rubans dessinés au plus. Au-delà, l'écran serait une nappe indistincte ; les missions qui
/// réclament l'humain et les plus vives passent devant, comme dans la scène.
pub(crate) const RUBANS_MAX: usize = 10;
/// Points de la grille. Un écran de 3840 × 2160 au pas de 56 en demande 2 769 ; au-delà de
/// l'écran, les points sont simplement hors champ.
const GRILLE: u32 = 2800;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CadreGpu {
    resolution: [f32; 2],
    temps: f32,
    attenuation: f32,
    poussiere: u32,
    par_courant: u32,
    grille: u32,
    rubans: u32,
    accent: [f32; 3],
    _r0: f32,
    alerte: [f32; 3],
    _r1: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct CourantGpu {
    base: f32,
    amplitude: f32,
    vitesse: f32,
    clarte: f32,
    phase: f32,
    teinte: f32,
    accent: f32,
    largeur: f32,
}

/// Le champ, prêt à dessiner ce qu'on lui a préparé.
pub(crate) struct Champ {
    pipeline: wgpu::RenderPipeline,
    groupe: wgpu::BindGroup,
    uniforme: wgpu::Buffer,
    stockage: wgpu::Buffer,
    rubans: Vec<CourantGpu>,
    temps: f32,
    attenuation: f32,
    accent: [f32; 3],
    poussiere: u32,
    par_courant: u32,
}

impl Champ {
    /// Construit le pipeline pour le format de la cible où l'interface se dessine. Sur un
    /// rastériseur logiciel, le champ est plus léger ; il le dit à qui mesure.
    pub(crate) fn nouveau(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        logiciel: bool,
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("champ"),
            source: wgpu::ShaderSource::Wgsl(include_str!("champ.wgsl").into()),
        });
        let disposition = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("champ"),
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
            label: Some("champ"),
            bind_group_layouts: &[Some(&disposition)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("champ"),
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
                    format,
                    // Additif : les rubans qui se croisent s'additionnent, comme de la lumière.
                    // Un mélange classique les ferait se masquer, ce qui écraserait la profondeur.
                    blend: Some(wgpu::BlendState {
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
                    }),
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
        let uniforme = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("champ · cadre"),
            size: std::mem::size_of::<CadreGpu>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Le tampon garde toujours sa taille maximale : les rubans absents sont éteints, pas
        // retirés, et le groupe de liaison ne se reconstruit jamais.
        let stockage = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("champ · rubans"),
            contents: bytemuck::cast_slice(&[CourantGpu::zeroed(); RUBANS_MAX]),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });
        let groupe = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("champ"),
            layout: &disposition,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforme.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: stockage.as_entire_binding(),
                },
            ],
        });
        Self {
            pipeline,
            groupe,
            uniforme,
            stockage,
            rubans: Vec::new(),
            temps: 0.0,
            attenuation: 1.0,
            accent: Accent::defaut().champ,
            poussiere: if logiciel {
                POUSSIERE_LOGICIEL
            } else {
                POUSSIERE
            },
            par_courant: if logiciel {
                PAR_COURANT_LOGICIEL
            } else {
                PAR_COURANT
            },
        }
    }

    /// Le nombre de particules d'une image : ce que le GPU trace réellement.
    pub(crate) fn particules(&self) -> u32 {
        self.poussiere + self.rubans.len() as u32 * self.par_courant + GRILLE
    }

    /// Prépare le champ pour une image : quelles missions deviennent des rubans, à quel instant.
    ///
    /// `temps` est l'horloge de l'interface ; sous mouvement réduit, elle est ignorée et le
    /// champ se fige. La mission choisie reçoit l'accent.
    pub(crate) fn preparer(
        &mut self,
        scene: &Scene,
        selection: Option<&str>,
        temps: f64,
        mouvement_reduit: bool,
        accent: Accent,
    ) {
        self.rubans = rubans(&scene.courants, selection);
        self.attenuation = scene.attenuation_du_champ();
        self.temps = if mouvement_reduit { 0.0 } else { temps as f32 };
        self.accent = accent.champ;
    }

    /// Vrai si une image suivante différerait de celle-ci : quelque chose avance.
    pub(crate) fn vivant(&self, mouvement_reduit: bool) -> bool {
        !mouvement_reduit
            && self
                .rubans
                .iter()
                .any(|r| r.vitesse > 0.0 && r.clarte > 0.0)
    }

    /// Dessine le champ dans la passe ouverte, avant l'interface.
    pub(crate) fn dessiner(
        &self,
        queue: &wgpu::Queue,
        passe: &mut wgpu::RenderPass<'_>,
        largeur: u32,
        hauteur: u32,
    ) {
        let alerte = crate::theme::palette::ATTENTE;
        let cadre = CadreGpu {
            resolution: [largeur as f32, hauteur as f32],
            temps: self.temps,
            attenuation: self.attenuation,
            poussiere: self.poussiere,
            par_courant: self.par_courant,
            grille: GRILLE,
            rubans: self.rubans.len() as u32,
            accent: self.accent,
            _r0: 0.0,
            alerte: [
                f32::from(alerte.r()) / 255.0,
                f32::from(alerte.g()) / 255.0,
                f32::from(alerte.b()) / 255.0,
            ],
            _r1: 0.0,
        };
        queue.write_buffer(&self.uniforme, 0, bytemuck::bytes_of(&cadre));
        let mut rubans = [CourantGpu::zeroed(); RUBANS_MAX];
        rubans[..self.rubans.len()].copy_from_slice(&self.rubans);
        queue.write_buffer(&self.stockage, 0, bytemuck::cast_slice(&rubans));
        passe.set_pipeline(&self.pipeline);
        passe.set_bind_group(0, &self.groupe, &[]);
        passe.draw(0..6, 0..self.particules());
    }
}

/// Les rubans d'une scène : au plus [`RUBANS_MAX`], ce qui réclame et ce qui vit d'abord.
fn rubans(courants: &[Courant], selection: Option<&str>) -> Vec<CourantGpu> {
    let mut ordre: Vec<usize> = (0..courants.len()).collect();
    let cle = |i: &usize| {
        let c = &courants[*i];
        (
            std::cmp::Reverse(Some(c.tache.as_str()) == selection),
            std::cmp::Reverse(c.reclame()),
            std::cmp::Reverse(ordered(c.vitesse())),
            c.tache.as_str(),
        )
    };
    if ordre.len() > RUBANS_MAX {
        ordre.select_nth_unstable_by_key(RUBANS_MAX, cle);
        ordre.truncate(RUBANS_MAX);
    }
    ordre.sort_by_key(cle);
    ordre
        .iter()
        .enumerate()
        .map(|(rang, &i)| {
            let c = &courants[i];
            let rang = rang as f32;
            CourantGpu {
                // Les rubans se répartissent dans la houle du champ sans jamais se superposer
                // exactement : deux courants confondus ne se distingueraient plus.
                base: 0.66 - 0.055 * rang - 0.03 * (rang % 2.0),
                amplitude: 0.09 + 0.016 * (rang % 3.0),
                vitesse: c.vitesse(),
                clarte: c.clarte(),
                phase: rang * 2.39,
                teinte: if c.reclame() { 1.0 } else { 0.0 },
                accent: if Some(c.tache.as_str()) == selection {
                    1.0
                } else {
                    0.0
                },
                largeur: 0.04,
            }
        })
        .collect()
}

/// Un flottant ordonnable pour le tri ; les vitesses sont finies et positives.
fn ordered(v: f32) -> u32 {
    (v.clamp(0.0, 1.0) * 1_000_000.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Etat;

    fn courant(tache: &str, etat: Etat, debit: f32) -> Courant {
        Courant {
            tache: tache.to_owned(),
            intitule: "quelque chose".to_owned(),
            agent: "claude-code".to_owned(),
            etat,
            debit,
            budget_consomme: 0.2,
            etapes: 12,
            task_state: None,
            task_revision: 0,
        }
    }

    #[test]
    fn mille_missions_ne_font_pas_mille_rubans_et_ce_qui_reclame_passe_devant() {
        let mut courants: Vec<Courant> = (0..1000)
            .map(|n| courant(&format!("{n:04}"), Etat::Court, 5.0))
            .collect();
        courants.push(courant("bloquee", Etat::Bloque, 0.0));
        courants.push(courant("vive", Etat::Court, 40.0));
        let rubans = rubans(&courants, Some("0500"));
        assert_eq!(rubans.len(), RUBANS_MAX);
        assert_eq!(
            rubans[0].accent, 1.0,
            "la mission choisie est toujours dessinée"
        );
        assert_eq!(
            rubans[1].teinte, 1.0,
            "ce qui réclame l'humain vient ensuite"
        );
        assert!(
            rubans[2].vitesse > rubans[3].vitesse,
            "puis la plus vive avant les autres"
        );
        assert!(
            rubans.iter().filter(|r| r.accent == 1.0).count() == 1,
            "un seul accent"
        );
    }

    #[test]
    fn deux_rubans_ne_partagent_ni_hauteur_ni_phase() {
        let courants: Vec<Courant> = (0..RUBANS_MAX)
            .map(|n| courant(&n.to_string(), Etat::Court, 10.0))
            .collect();
        let rubans = rubans(&courants, None);
        for (i, a) in rubans.iter().enumerate() {
            for b in rubans.iter().skip(i + 1) {
                assert!((a.base - b.base).abs() > 1e-3 || (a.phase - b.phase).abs() > 1e-3);
            }
            assert!(
                a.base > 0.05 && a.base < 0.95,
                "un ruban reste dans l'écran"
            );
        }
    }

    #[test]
    fn un_champ_sans_mission_active_n_est_pas_vivant() {
        let statique = rubans(
            &[
                courant("a", Etat::Bloque, 0.0),
                courant("b", Etat::Fini, 0.0),
            ],
            None,
        );
        assert!(statique.iter().all(|r| r.vitesse == 0.0));
        let vivant = rubans(&[courant("c", Etat::Court, 3.0)], None);
        assert!(vivant.iter().any(|r| r.vitesse > 0.0));
    }
}
