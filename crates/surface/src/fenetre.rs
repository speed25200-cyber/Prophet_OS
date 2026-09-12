//! Fenêtre native. La saisie et le rendu n'attendent jamais le moteur d'inférence.

use crate::atelier::Page;
use crate::bureau::Bureau;
use crate::gpu::{Cible, Contexte, ErreurGpu};
use crate::scene::Scene;
use egui_winit::accesskit_winit::{
    Event as AccessibilityEvent, WindowEvent as AccessibilityWindowEvent,
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

/// Réponse humaine à l'action précise montrée par la surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reponse {
    /// Autorisation.
    Accepte,
    /// Refus.
    Refuse,
}

/// Source des tâches, de l'isolation et des décisions du système.
pub trait Source {
    /// État disponible immédiatement, sans requête bloquante.
    fn scene(&mut self) -> Scene;
    /// Décision explicite sur l'action montrée.
    fn repond(&mut self, reponse: Reponse);
}

/// Configuration de la fenêtre et de son moteur local.
#[derive(Debug, Clone)]
pub struct Options {
    /// Base HTTP du moteur sur la boucle locale.
    pub endpoint: String,
    /// Marque les exemples et désactive les requêtes au moteur.
    pub demonstration: bool,
    /// Fenêtre redimensionnable plutôt que plein écran.
    pub fenetree: bool,
    /// Désactive les animations décoratives dès le démarrage.
    pub mouvement_reduit: bool,
    /// Page initiale.
    pub page: Page,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            endpoint: std::env::var("PROPHET_MODEL_ENDPOINT")
                .unwrap_or_else(|_| "http://127.0.0.1:8080/v1".to_owned()),
            demonstration: false,
            fenetree: false,
            mouvement_reduit: false,
            page: Page::Accueil,
        }
    }
}

/// Ouvre l'espace natif avec la configuration habituelle.
///
/// # Errors
/// Boucle de fenêtre ou GPU indisponible.
pub fn tenir(source: Box<dyn Source>) -> Result<(), ErreurFenetre> {
    tenir_avec(source, Options::default())
}

/// Ouvre l'espace natif avec une configuration explicite.
///
/// # Errors
/// Boucle de fenêtre ou GPU indisponible.
pub fn tenir_avec(source: Box<dyn Source>, options: Options) -> Result<(), ErreurFenetre> {
    let boucle = EventLoop::<Evenement>::with_user_event()
        .build()
        .map_err(|e| ErreurFenetre::Boucle(e.to_string()))?;
    let mut application = Application {
        source,
        options,
        etat: None,
        prochain: Instant::now(),
        erreur: None,
        proxy: boucle.create_proxy(),
    };
    boucle
        .run_app(&mut application)
        .map_err(|e| ErreurFenetre::Boucle(e.to_string()))?;
    application.erreur.map_or(Ok(()), Err)
}

/// Échec réel d'installation de la fenêtre ou du moteur graphique.
#[derive(Debug, thiserror::Error)]
pub enum ErreurFenetre {
    /// Échec de la boucle d'événements.
    #[error("boucle d'évènements : {0}")]
    Boucle(String),
    /// Échec du contexte graphique.
    #[error(transparent)]
    Gpu(#[from] ErreurGpu),
}

struct Etat {
    fenetre: Arc<Window>,
    surface: wgpu::Surface<'static>,
    contexte: Contexte,
    bureau: Bureau,
    entrees: egui_winit::State,
    cachee: bool,
    premiere_image: bool,
}

struct Application {
    source: Box<dyn Source>,
    options: Options,
    etat: Option<Etat>,
    prochain: Instant,
    erreur: Option<ErreurFenetre>,
    proxy: EventLoopProxy<Evenement>,
}

enum Evenement {
    Accessibilite(AccessibilityEvent),
    Repeindre(Instant),
}

impl From<AccessibilityEvent> for Evenement {
    fn from(value: AccessibilityEvent) -> Self {
        Self::Accessibilite(value)
    }
}

impl ApplicationHandler<Evenement> for Application {
    fn resumed(&mut self, boucle: &ActiveEventLoop) {
        if self.etat.is_some() {
            return;
        }
        match installer(boucle, &self.options, self.proxy.clone()) {
            Ok(etat) => self.etat = Some(etat),
            Err(erreur) => {
                self.erreur = Some(erreur);
                boucle.exit();
            }
        }
    }

    fn user_event(&mut self, _: &ActiveEventLoop, event: Evenement) {
        let event = match event {
            Evenement::Repeindre(instant) => {
                self.prochain = self.prochain.min(instant);
                return;
            }
            Evenement::Accessibilite(event) => event,
        };
        let Some(etat) = &mut self.etat else {
            return;
        };
        if event.window_id != etat.fenetre.id() {
            return;
        }
        match event.window_event {
            AccessibilityWindowEvent::InitialTreeRequested => etat.bureau.ctx.enable_accesskit(),
            AccessibilityWindowEvent::ActionRequested(request) => {
                etat.entrees.on_accesskit_action_request(request)
            }
            AccessibilityWindowEvent::AccessibilityDeactivated => {
                etat.bureau.ctx.disable_accesskit()
            }
        }
        etat.fenetre.request_redraw();
    }

    fn window_event(&mut self, boucle: &ActiveEventLoop, _: WindowId, evenement: WindowEvent) {
        let Some(etat) = &mut self.etat else {
            return;
        };
        let response = etat.entrees.on_window_event(&etat.fenetre, &evenement);
        if response.repaint {
            etat.fenetre.request_redraw();
        }
        match evenement {
            WindowEvent::CloseRequested => boucle.exit(),
            WindowEvent::Occluded(cachee) => etat.cachee = cachee,
            WindowEvent::Resized(taille) => {
                etat.cachee = taille.width == 0 || taille.height == 0;
                if !etat.cachee {
                    reconfigurer(etat, taille.width, taille.height);
                }
            }
            WindowEvent::RedrawRequested if !etat.cachee => {
                let mut scene = self.source.scene();
                scene.ordonner();
                if let Some(reponse) = dessiner(etat, &scene) {
                    self.source.repond(reponse);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, boucle: &ActiveEventLoop) {
        if let Some(etat) = &self.etat {
            let maintenant = Instant::now();
            if maintenant >= self.prochain {
                let anime = etat.bureau.atelier.page == Page::Accueil
                    && !etat.bureau.atelier.mouvement_reduit;
                self.prochain = maintenant
                    + Duration::from_millis(if etat.cachee {
                        1000
                    } else if anime {
                        33
                    } else {
                        250
                    });
                if !etat.cachee {
                    etat.fenetre.request_redraw();
                }
            }
            boucle.set_control_flow(ControlFlow::WaitUntil(self.prochain));
        }
    }
}

fn installer(
    boucle: &ActiveEventLoop,
    options: &Options,
    proxy: EventLoopProxy<Evenement>,
) -> Result<Etat, ErreurFenetre> {
    let mut attributs = Window::default_attributes()
        .with_title("Prophet OS")
        .with_min_inner_size(winit::dpi::LogicalSize::new(640.0, 480.0))
        .with_inner_size(winit::dpi::LogicalSize::new(1440.0, 900.0));
    if !options.fenetree {
        attributs = attributs
            .with_decorations(false)
            .with_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
    }
    let fenetre = Arc::new(
        boucle
            .create_window(attributs)
            .map_err(|e| ErreurFenetre::Boucle(e.to_string()))?,
    );
    let (contexte, surface) = Contexte::avec_surface(fenetre.clone().into())?;
    let mut bureau = Bureau::nouveau(&contexte, options.endpoint.clone(), options.demonstration);
    bureau.atelier.mouvement_reduit = options.mouvement_reduit;
    bureau.atelier.page = options.page;
    let mut entrees = egui_winit::State::new(
        bureau.ctx.clone(),
        egui::ViewportId::ROOT,
        fenetre.as_ref(),
        Some(fenetre.scale_factor() as f32),
        None,
        None,
    );
    entrees.init_accesskit(boucle, &fenetre, proxy.clone());
    bureau.ctx.set_request_repaint_callback(move |request| {
        if let Some(instant) = Instant::now().checked_add(request.delay) {
            let _ = proxy.send_event(Evenement::Repeindre(instant));
        }
    });
    bureau.atelier.decouvrir(&bureau.ctx);
    let taille = fenetre.inner_size();
    let etat = Etat {
        fenetre,
        surface,
        contexte,
        bureau,
        entrees,
        cachee: false,
        premiere_image: false,
    };
    reconfigurer(&etat, taille.width.max(1), taille.height.max(1));
    Ok(etat)
}

fn reconfigurer(etat: &Etat, largeur: u32, hauteur: u32) {
    let mut configuration = etat
        .contexte
        .configuration_surface
        .clone()
        .expect("avec_surface fournit une configuration compatible");
    configuration.width = largeur;
    configuration.height = hauteur;
    etat.surface
        .configure(&etat.contexte.device, &configuration);
}

fn dessiner(etat: &mut Etat, scene: &Scene) -> Option<Reponse> {
    let image = match etat.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(image)
        | wgpu::CurrentSurfaceTexture::Suboptimal(image) => image,
        wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
            let taille = etat.fenetre.inner_size();
            reconfigurer(etat, taille.width.max(1), taille.height.max(1));
            return None;
        }
        _ => return None,
    };
    let vue = image
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let cible = Cible {
        texture: image.texture.clone(),
        vue,
        largeur: image.texture.width(),
        hauteur: image.texture.height(),
    };
    let input = etat.entrees.take_egui_input(&etat.fenetre);
    let (mut output, decision) = etat.bureau.composer(input, scene);
    etat.bureau.rendre(&etat.contexte, &cible, &mut output);
    etat.entrees
        .handle_platform_output(&etat.fenetre, output.platform_output);
    etat.contexte.queue.present(image);
    if !etat.premiere_image {
        eprintln!(
            "surface : première image soumise à Wayland ({})",
            etat.contexte.adaptateur
        );
        etat.premiere_image = true;
    }
    decision
}
