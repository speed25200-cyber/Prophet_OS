//! La fenêtre, et la boucle qui la tient vivante.
//!
//! Prophet OS n'a pas de bureau : cette fenêtre occupe l'écran entier, sans décoration, sans
//! barre, sans rien à déplacer. Elle n'est pas une application parmi d'autres — elle est ce que
//! l'écran montre quand la machine est allumée.
//!
//! Le clavier n'y sert qu'à une chose : trancher une décision. Il n'y a rien d'autre à commander,
//! et c'est le propos.

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::gpu::{Contexte, ErreurGpu, FORMAT};
use crate::rendu::{ErreurRendu, Rendu};
use crate::scene::Scene;

/// Ce que la personne devant l'écran a répondu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reponse {
    /// Elle accepte.
    Accepte,
    /// Elle refuse.
    Refuse,
}

/// Ce qui alimente la surface, et reçoit ce qu'elle décide.
///
/// La surface ne sait rien du système : elle demande une scène, et rend une réponse quand une
/// décision est tranchée. C'est ce qui permet de la dessiner sans machine en marche, et de la
/// brancher ensuite sur le ledger sans la rouvrir.
pub trait Source {
    /// L'état à montrer maintenant.
    fn scene(&mut self) -> Scene;
    /// La personne a tranché la décision en attente.
    fn repond(&mut self, reponse: Reponse);
}

/// Ouvre la fenêtre et ne rend la main qu'à sa fermeture.
///
/// # Errors
/// Si la boucle d'évènements ne peut pas être créée, ou si le rendu échoue à s'installer.
pub fn tenir(source: Box<dyn Source>) -> Result<(), ErreurFenetre> {
    let boucle = EventLoop::new().map_err(|e| ErreurFenetre::Boucle(e.to_string()))?;
    // `Poll` plutôt que `Wait` : les courants avancent même quand personne ne touche à rien, et
    // c'est précisément ce qu'on veut voir.
    boucle.set_control_flow(ControlFlow::Poll);
    let mut application = Application {
        source,
        etat: None,
        debut: std::time::Instant::now(),
    };
    boucle
        .run_app(&mut application)
        .map_err(|e| ErreurFenetre::Boucle(e.to_string()))
}

/// Ce qui peut empêcher la surface de s'afficher.
#[derive(Debug, thiserror::Error)]
pub enum ErreurFenetre {
    /// La boucle d'évènements.
    #[error("boucle d'évènements : {0}")]
    Boucle(String),
    /// Le contexte graphique.
    #[error(transparent)]
    Gpu(#[from] ErreurGpu),
    /// Le rendu.
    #[error(transparent)]
    Rendu(#[from] ErreurRendu),
}

struct Etat {
    fenetre: Arc<Window>,
    surface: wgpu::Surface<'static>,
    contexte: Contexte,
    rendu: Rendu,
}

struct Application {
    source: Box<dyn Source>,
    etat: Option<Etat>,
    debut: std::time::Instant,
}

impl ApplicationHandler for Application {
    fn resumed(&mut self, boucle: &ActiveEventLoop) {
        if self.etat.is_some() {
            return;
        }
        match installer(boucle) {
            Ok(etat) => self.etat = Some(etat),
            Err(erreur) => {
                // Sans surface, il n'y a rien à montrer et rien à attendre : on le dit et on
                // s'arrête, plutôt que de tourner devant un écran noir.
                eprintln!("surface impossible à ouvrir : {erreur}");
                boucle.exit();
            }
        }
    }

    fn window_event(&mut self, boucle: &ActiveEventLoop, _: WindowId, evenement: WindowEvent) {
        let Some(etat) = &mut self.etat else { return };
        match evenement {
            WindowEvent::CloseRequested => boucle.exit(),
            WindowEvent::Resized(taille) => {
                reconfigurer(etat, taille.width.max(1), taille.height.max(1));
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                // Les deux seules touches qui font quelque chose. Tout le reste est ignoré sans
                // bruit : il n'y a rien d'autre à commander.
                match event.logical_key {
                    Key::Named(NamedKey::Enter) => self.source.repond(Reponse::Accepte),
                    Key::Named(NamedKey::Escape) => self.source.repond(Reponse::Refuse),
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                let mut scene = self.source.scene();
                scene.ordonner();
                let temps = self.debut.elapsed().as_secs_f32();
                if let Err(erreur) = dessiner(etat, &scene, temps) {
                    eprintln!("image perdue : {erreur}");
                }
                etat.fenetre.request_redraw();
            }
            _ => {}
        }
    }
}

fn installer(boucle: &ActiveEventLoop) -> Result<Etat, ErreurFenetre> {
    let attributs = Window::default_attributes()
        .with_title("Prophet OS")
        .with_decorations(false)
        .with_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
    let fenetre = Arc::new(
        boucle
            .create_window(attributs)
            .map_err(|e| ErreurFenetre::Boucle(e.to_string()))?,
    );

    let (contexte, surface) = Contexte::avec_surface(fenetre.clone().into())?;
    let rendu = Rendu::nouveau(&contexte)?;

    let taille = fenetre.inner_size();
    let etat = Etat {
        fenetre,
        surface,
        contexte,
        rendu,
    };
    reconfigurer(&etat, taille.width.max(1), taille.height.max(1));
    Ok(etat)
}

fn reconfigurer(etat: &Etat, largeur: u32, hauteur: u32) {
    etat.surface.configure(
        &etat.contexte.device,
        &wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: FORMAT,
            width: largeur,
            height: hauteur,
            present_mode: wgpu::PresentMode::AutoVsync,
            color_space: wgpu::SurfaceColorSpace::Srgb,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
        },
    );
}

fn dessiner(etat: &mut Etat, scene: &Scene, temps: f32) -> Result<(), ErreurFenetre> {
    // wgpu distingue six issues là où un `Result` n'en donnerait que deux, et la distinction
    // porte : une image sautée parce que la fenêtre est masquée n'appelle pas le même geste
    // qu'une surface périmée, qu'il faut reconfigurer.
    let image = match etat.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(image)
        | wgpu::CurrentSurfaceTexture::Suboptimal(image) => image,
        wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
            return Ok(());
        }
        wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
            let taille = etat.fenetre.inner_size();
            reconfigurer(etat, taille.width.max(1), taille.height.max(1));
            return Ok(());
        }
        autre => {
            eprintln!("image indisponible : {autre:?}");
            return Ok(());
        }
    };
    let vue = image
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let cible = crate::gpu::Cible {
        texture: image.texture.clone(),
        vue,
        largeur: image.texture.width(),
        hauteur: image.texture.height(),
    };
    etat.rendu.dessiner(&etat.contexte, &cible, scene, temps)?;
    // La présentation appartient à la file de commandes : elle suit donc le travail déjà soumis,
    // au lieu de courir après lui.
    etat.contexte.queue.present(image);
    Ok(())
}
