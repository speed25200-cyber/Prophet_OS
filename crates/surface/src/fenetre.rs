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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reponse {
    /// Autorisation, cette fois seulement.
    Accepte,
    /// Autorisation pour toute la mission : la même action ne redemandera pas (ADR 0041).
    AccepteMission,
    /// Refus.
    Refuse,
    /// Le code d'approbation que capd demande pour accorder (ADR 0057).
    Code(String),
    /// Le premier code d'approbation de cette machine, choisi par l'humain.
    DefinirCode(String),
    /// L'humain renonce à donner son code : l'accord n'est pas fait.
    RenoncerAuCode,
}

/// Source des tâches, de l'isolation et des décisions du système.
pub trait Source {
    /// État disponible immédiatement, sans requête bloquante.
    fn scene(&mut self) -> Scene;
    /// Décision explicite sur l'action montrée.
    fn repond(&mut self, reponse: Reponse);
    /// La demande de code d'approbation en cours, s'il y en a une (ADR 0057).
    fn presence(&self) -> Option<crate::presence::Demande> {
        None
    }
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
    /// Accent imposé pour cette session, sinon celui configuré.
    pub accent: Option<crate::theme::Accent>,
    /// Champ complet même sur un rastériseur logiciel.
    pub champ_complet: bool,
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
            accent: None,
            champ_complet: false,
        }
    }
}

/// Faut-il redessiner ? Oui si l'interface l'a demandé (champ vivant, saisie, transition) ou
/// si la scène ne ressemble plus à la dernière image. Sinon, l'écran reste tel quel et le GPU
/// dort : c'est toute la consommation au repos.
#[must_use]
pub fn doit_redessiner(repeindre: bool, derniere: Option<u64>, scene: &Scene) -> bool {
    repeindre || derniere != Some(scene.empreinte())
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
        repeindre: true,
        empreinte: None,
        presence: None,
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
    /// Un redessin a été demandé par l'interface elle-même (champ vivant, transition, saisie).
    repeindre: bool,
    /// L'empreinte de la dernière scène dessinée : un écran inchangé n'est pas redessiné.
    empreinte: Option<u64>,
    /// La demande de code d'approbation montrée à la dernière image (ADR 0057).
    presence: Option<crate::presence::Demande>,
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
                self.repeindre = true;
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
                self.empreinte = Some(scene.empreinte());
                self.repeindre = false;
                self.presence = self.source.presence();
                etat.bureau.presence.clone_from(&self.presence);
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
                self.prochain =
                    maintenant + Duration::from_millis(if etat.cachee { 1000 } else { 250 });
                // Un écran au repos n'est pas redessiné : on relit les services, et si rien
                // de visible n'a changé et que l'interface ne demande rien, le GPU dort.
                // Une demande de code d'approbation qui paraît, change ou se clôt se redessine.
                if !etat.cachee
                    && (doit_redessiner(self.repeindre, self.empreinte, &self.source.scene())
                        || self.source.presence() != self.presence)
                {
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
    #[cfg(target_os = "linux")]
    {
        use winit::platform::wayland::WindowAttributesExtWayland as _;
        attributs = attributs.with_name("org.prophet.Supervision", "prophet-surface");
    }
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
    bureau.brancher_missions(crate::reel::Sockets::default().agentd);
    bureau.atelier.mouvement_reduit = options.mouvement_reduit;
    bureau.atelier.page = options.page;
    if let Some(accent) = options.accent {
        bureau.choisir_accent(accent);
    }
    if options.champ_complet {
        bureau.forcer_champ_complet(&contexte);
    }
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
    // Un écran plus grand que ce que le périphérique accepte ne doit pas faire tomber la
    // surface : elle se dessine à la taille maximale, que le compositeur étire.
    let (largeur, hauteur) = etat.contexte.borner(largeur, hauteur);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Courant, Etat, Isolation};

    fn scene(etapes: u32) -> Scene {
        Scene {
            heure: "14:37".to_owned(),
            date: "jeudi".to_owned(),
            courants: vec![Courant {
                tache: "t1".to_owned(),
                intitule: "quelque chose".to_owned(),
                agent: "local".to_owned(),
                etat: Etat::Court,
                debit: 3.0,
                budget_consomme: 0.1,
                etapes,
                task_state: None,
                task_revision: 0,
            }],
            decision: None,
            isolation: Isolation {
                niveau_max: 1,
                manque: None,
                reserve: None,
            },
        }
    }

    #[test]
    fn un_ecran_inchange_n_est_pas_redessine_mais_une_etape_ou_une_demande_le_sont() {
        let derniere = Some(scene(4).empreinte());
        assert!(
            !doit_redessiner(false, derniere, &scene(4)),
            "rien n'a changé"
        );
        assert!(
            doit_redessiner(false, derniere, &scene(5)),
            "une étape s'est franchie"
        );
        assert!(
            doit_redessiner(true, derniere, &scene(4)),
            "l'interface l'a demandé"
        );
        assert!(
            doit_redessiner(false, None, &scene(4)),
            "la première image se dessine"
        );
    }
}
