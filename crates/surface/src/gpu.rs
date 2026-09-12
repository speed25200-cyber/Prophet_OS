//! Le contexte graphique, et le rendu hors écran.
//!
//! Le rendu hors écran n'est pas un mode dégradé : c'est le mode qui se teste. Une interface qui
//! ne s'observe qu'en la regardant tourner ne se vérifie pas, et une surface d'observation qui ne
//! se vérifie pas est la dernière chose qu'on veut sur un système dont le métier est justement de
//! rendre observable ce que font les agents.
//!
//! La fenêtre viendra se brancher sur le même rendu.

use std::sync::Arc;

/// Ce qui n'a pas pu être mis en place.
#[derive(Debug, thiserror::Error)]
pub enum ErreurGpu {
    /// Aucun adaptateur graphique utilisable.
    #[error(
        "aucun adaptateur graphique : {0}. Sur une machine sans GPU, le rendu logiciel exige \
         `lavapipe` ou `llvmpipe` ; l'absence est dite ici plutôt que rendue par une image noire."
    )]
    AucunAdaptateur(String),
    /// Le pilote a refusé de fournir un périphérique.
    #[error("périphérique refusé par le pilote : {0}")]
    Peripherique(String),
    /// La lecture de l'image rendue a échoué.
    #[error("l'image rendue n'a pas pu être relue : {0}")]
    Lecture(String),
    /// La surface d'affichage n'a pas pu être créée.
    #[error("surface d'affichage impossible : {0}")]
    Surface(String),
}

/// L'instance, configurée de la même façon pour tous les usages.
fn instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        flags: wgpu::InstanceFlags::default(),
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        backend_options: wgpu::BackendOptions::default(),
        display: None,
    })
}

/// Le contexte graphique, partagé par tous les rendus.
pub struct Contexte {
    /// L'instance, retenue parce qu'une surface d'affichage en naît.
    pub instance: wgpu::Instance,
    /// Périphérique logique.
    pub device: Arc<wgpu::Device>,
    /// File de commandes.
    pub queue: Arc<wgpu::Queue>,
    /// Nom de l'adaptateur retenu, pour le journal et le diagnostic.
    pub adaptateur: String,
}

impl std::fmt::Debug for Contexte {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Contexte")
            .field("adaptateur", &self.adaptateur)
            .finish_non_exhaustive()
    }
}

/// Le format de couleur employé partout.
///
/// `Rgba8UnormSrgb` : les mélanges se font en linéaire et l'écriture finale reconvertit. C'est ce
/// qui évite les bords grisâtres autour du doré sur fond noir.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

impl Contexte {
    /// Ouvre un contexte, sans surface d'affichage.
    ///
    /// # Errors
    /// Si aucun adaptateur n'est disponible, ou si le pilote refuse un périphérique.
    pub fn hors_ecran() -> Result<Self, ErreurGpu> {
        pollster::block_on(Self::ouvrir(None))
    }

    /// Ouvre un contexte et la surface d'affichage qui va avec.
    ///
    /// La cible est passée telle que wgpu l'attend : ce module ignore donc quelle bibliothèque de
    /// fenêtrage l'a produite, et reste compilable sans elle.
    ///
    /// # Errors
    /// Si aucun adaptateur ne convient à cette surface, ou si le pilote refuse un périphérique.
    pub fn avec_surface(
        cible: wgpu::SurfaceTarget<'static>,
    ) -> Result<(Self, wgpu::Surface<'static>), ErreurGpu> {
        pollster::block_on(async {
            // L'instance doit exister avant la surface, et la surface avant le choix de
            // l'adaptateur : c'est elle qui dit lesquels savent dessiner sur cet écran.
            let instance = instance();
            let surface = instance
                .create_surface(cible)
                .map_err(|e| ErreurGpu::Surface(e.to_string()))?;
            let contexte = Self::depuis(instance, Some(&surface)).await?;
            Ok((contexte, surface))
        })
    }

    async fn ouvrir(surface: Option<&wgpu::Surface<'static>>) -> Result<Self, ErreurGpu> {
        let instance = instance();
        Self::depuis(instance, surface).await
    }

    async fn depuis(
        instance: wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, ErreurGpu> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: surface,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|e| ErreurGpu::AucunAdaptateur(e.to_string()))?;

        let info = adapter.get_info();
        let adaptateur = format!("{} ({:?})", info.name, info.backend);

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("prophet-surface"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| ErreurGpu::Peripherique(e.to_string()))?;

        Ok(Self {
            instance,
            device: Arc::new(device),
            queue: Arc::new(queue),
            adaptateur,
        })
    }
}

/// Une cible de rendu hors écran, relisible en pixels.
pub struct Cible {
    /// Texture dans laquelle on dessine.
    pub texture: wgpu::Texture,
    /// Vue de cette texture.
    pub vue: wgpu::TextureView,
    /// Largeur en pixels.
    pub largeur: u32,
    /// Hauteur en pixels.
    pub hauteur: u32,
}

impl std::fmt::Debug for Cible {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cible")
            .field("largeur", &self.largeur)
            .field("hauteur", &self.hauteur)
            .finish_non_exhaustive()
    }
}

impl Cible {
    /// Crée une cible de la taille demandée.
    #[must_use]
    pub fn nouvelle(contexte: &Contexte, largeur: u32, hauteur: u32) -> Self {
        let texture = contexte.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cible hors écran"),
            size: wgpu::Extent3d {
                width: largeur,
                height: hauteur,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let vue = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            vue,
            largeur,
            hauteur,
        }
    }

    /// Relit les pixels rendus, en RGBA sur huit bits.
    ///
    /// # Errors
    /// Si la carte ne rend pas la mémoire demandée.
    pub fn pixels(&self, contexte: &Contexte) -> Result<Vec<u8>, ErreurGpu> {
        // La copie depuis une texture exige des lignes alignées sur 256 octets ; l'image finale
        // les retire. Oublier cet alignement produit une image oblique, défaut spectaculaire et
        // facile à diagnostiquer — ce qui n'est pas une raison pour le laisser passer.
        let octets_par_pixel = 4;
        let brut = self.largeur * octets_par_pixel;
        let aligne = brut.div_ceil(256) * 256;

        let tampon = contexte.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("relecture"),
            size: u64::from(aligne) * u64::from(self.hauteur),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encodeur =
            contexte
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("relecture"),
                });
        encodeur.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &tampon,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(aligne),
                    rows_per_image: Some(self.hauteur),
                },
            },
            wgpu::Extent3d {
                width: self.largeur,
                height: self.hauteur,
                depth_or_array_layers: 1,
            },
        );
        contexte.queue.submit(Some(encodeur.finish()));

        let tranche = tampon.slice(..);
        let (envoi, reception) = std::sync::mpsc::channel();
        tranche.map_async(wgpu::MapMode::Read, move |r| {
            let _ = envoi.send(r);
        });
        contexte
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|e| ErreurGpu::Lecture(e.to_string()))?;
        reception
            .recv()
            .map_err(|e| ErreurGpu::Lecture(e.to_string()))?
            .map_err(|e| ErreurGpu::Lecture(e.to_string()))?;

        let donnees = tranche
            .get_mapped_range()
            .map_err(|e| ErreurGpu::Lecture(e.to_string()))?;
        let mut image = Vec::with_capacity((self.largeur * self.hauteur * 4) as usize);
        for ligne in 0..self.hauteur {
            let debut = (ligne * aligne) as usize;
            let fin = debut + (brut as usize);
            image.extend_from_slice(&donnees[debut..fin]);
        }
        drop(donnees);
        tampon.unmap();
        Ok(image)
    }
}
