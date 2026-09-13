//! L'espace de travail Prophet OS, en fenêtre ou en capture reproductible.

use clap::{Parser, ValueEnum};
use std::process::ExitCode;
use std::time::{Duration, Instant};
use surface::atelier::Page;
use surface::bureau::Bureau;
use surface::fenetre::{Options, Reponse, Source};
use surface::gpu::{Cible, Contexte};
use surface::rendu::Rendu;
use surface::scene::{Courant, Decision, Etat, Isolation, Scene};

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
enum Vue {
    #[default]
    Accueil,
    Conversation,
    Modeles,
    Activite,
}

#[derive(Debug, Parser)]
#[command(about = "Espace de travail natif pour les modèles locaux de Prophet OS")]
struct Args {
    /// Écrit une capture PNG au lieu d'ouvrir la fenêtre.
    #[arg(long)]
    capture: Option<String>,
    /// Largeur de la capture.
    #[arg(long, default_value_t=1920, value_parser=clap::value_parser!(u32).range(640..=7680))]
    largeur: u32,
    /// Hauteur de la capture.
    #[arg(long, default_value_t=1080, value_parser=clap::value_parser!(u32).range(480..=4320))]
    hauteur: u32,
    /// Instant de l'animation dans la capture.
    #[arg(long, default_value_t = 8.0)]
    temps: f32,
    /// Utilise des tâches d'exemple clairement identifiées.
    #[arg(long)]
    demonstration: bool,
    /// Ajoute une décision à la scène d'exemple.
    #[arg(long, requires = "demonstration")]
    decision: bool,
    /// Ouvre le panneau d'examen dans une capture de démonstration, sans répondre.
    #[arg(long, requires_all = ["capture", "decision"])]
    examen: bool,
    /// Capture l'ancienne surface de courants pour ses tests visuels.
    #[arg(long, requires = "capture")]
    observation: bool,
    /// Ouvre une fenêtre redimensionnable.
    #[arg(long)]
    fenetree: bool,
    /// Adresse HTTP locale, sinon PROPHET_MODEL_ENDPOINT ou 127.0.0.1:8080/v1.
    #[arg(long)]
    endpoint: Option<String>,
    /// Page à ouvrir ou capturer.
    #[arg(long, value_enum, default_value_t=Vue::Accueil)]
    page: Vue,
    /// Modèle réel pour une capture de conversation.
    #[arg(long, requires = "capture", conflicts_with = "demonstration")]
    modele: Option<String>,
    /// Demande réelle à exécuter avant de capturer la conversation.
    #[arg(long, requires_all=["capture","modele"], conflicts_with_all=["demonstration","observation"])]
    prompt: Option<String>,
    /// Fige les animations décoratives.
    #[arg(long)]
    mouvement_reduit: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match executer(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("surface : {error}");
            ExitCode::FAILURE
        }
    }
}

fn executer(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    if !args.temps.is_finite() || args.temps < 0.0 {
        return Err("instant de capture invalide".into());
    }
    let options = Options {
        endpoint: args
            .endpoint
            .clone()
            .unwrap_or_else(|| Options::default().endpoint),
        demonstration: args.demonstration,
        fenetree: args.fenetree,
        mouvement_reduit: args.mouvement_reduit,
        page: match args.page {
            Vue::Accueil => Page::Accueil,
            Vue::Conversation => Page::Conversation,
            Vue::Modeles => Page::Modeles,
            Vue::Activite => Page::Activite,
        },
    };
    let mut source: Box<dyn Source> = if args.demonstration {
        Box::new(Demonstration {
            avec_decision: args.decision,
        })
    } else {
        Box::new(surface::reel::Reel::demarrer(
            surface::reel::Sockets::default(),
        ))
    };
    let Some(path) = &args.capture else {
        return surface::fenetre::tenir_avec(source, options).map_err(Into::into);
    };
    let context = Contexte::hors_ecran()?;
    let target = Cible::nouvelle(&context, args.largeur, args.hauteur);
    if args.observation {
        let mut renderer = Rendu::nouveau(&context)?;
        let mut scene = source.scene();
        scene.ordonner();
        renderer.dessiner(&context, &target, &scene, args.temps)?;
    } else {
        let mut bureau = Bureau::nouveau(&context, options.endpoint, args.demonstration);
        bureau.brancher_missions(surface::reel::Sockets::default().agentd);
        bureau.brancher_journal(surface::reel::socket_du_journal());
        bureau.figer_transitions();
        bureau.atelier.mouvement_reduit = args.mouvement_reduit;
        bureau.atelier.decouvrir(&bureau.ctx);
        attendre(&mut bureau, Duration::from_secs(7), |b| {
            b.atelier.decouverte
        })?;
        if let Some(model) = args.modele {
            if !bureau.atelier.modeles.contains(&model) {
                return Err(format!("modèle indisponible : {model}").into());
            }
            bureau.atelier.choisi = model;
        }
        if let Some(prompt) = args.prompt {
            bureau.atelier.brouillon = prompt;
            bureau.atelier.envoyer(&bureau.ctx);
            attendre(&mut bureau, Duration::from_secs(185), |b| {
                b.atelier.generation
            })?;
            let tour = bureau
                .atelier
                .tours
                .last()
                .ok_or("la demande n'a pas démarré")?;
            if let Some(error) = &tour.erreur {
                return Err(error.clone().into());
            }
            println!("réponse du moteur : {}", tour.reponse);
        } else {
            bureau.atelier.page = match args.page {
                Vue::Accueil => Page::Accueil,
                Vue::Conversation => Page::Conversation,
                Vue::Modeles => Page::Modeles,
                Vue::Activite => Page::Activite,
            };
        }
        // Stabiliser le rendu et attendre une réponse de l'inspecteur réellement affiché.
        let capture_started = Instant::now();
        let mut frame = 0;
        loop {
            let events = if args.examen && frame == 3 {
                let response = bureau
                    .ctx
                    .read_response(egui::Id::new("examiner-decision"))
                    .ok_or("contrôle d'examen absent")?;
                let pos = response.rect.center();
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::default(),
                    },
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::default(),
                    },
                ]
            } else {
                vec![]
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(args.largeur as f32, args.hauteur as f32),
                )),
                time: Some(f64::from(args.temps)),
                events,
                ..Default::default()
            };
            let scene = source.scene();
            let (mut output, _) = bureau.composer(input, &scene);
            bureau.rendre(&context, &target, &mut output);
            frame += 1;
            if frame >= if args.examen { 6 } else { 3 } {
                if args.demonstration
                    || bureau.atelier.page != Page::Accueil
                    || scene.courants.is_empty()
                    || bureau.missions().snapshot().is_some()
                    || bureau.missions().error().is_some()
                {
                    break;
                }
                if capture_started.elapsed() > Duration::from_secs(6) {
                    return Err("détail de mission non reçu pour la capture".into());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    ecrire_png(path, args.largeur, args.hauteur, &target.pixels(&context)?)?;
    println!(
        "{path} — {}×{}, rendu par {}",
        args.largeur, args.hauteur, context.adaptateur
    );
    Ok(())
}

fn attendre(
    bureau: &mut Bureau,
    maximum: Duration,
    en_cours: impl Fn(&Bureau) -> bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    while en_cours(bureau) {
        if started.elapsed() > maximum {
            return Err("délai de capture dépassé".into());
        }
        std::thread::sleep(Duration::from_millis(10));
        bureau.atelier.actualiser();
    }
    Ok(())
}

fn ecrire_png(
    chemin: &str,
    largeur: u32,
    hauteur: u32,
    pixels: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let fichier = std::fs::File::create(chemin)?;
    let mut encodeur = png::Encoder::new(std::io::BufWriter::new(fichier), largeur, hauteur);
    encodeur.set_color(png::ColorType::Rgba);
    encodeur.set_depth(png::BitDepth::Eight);
    encodeur.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    encodeur.write_header()?.write_image_data(pixels)?;
    Ok(())
}

/// La source de démonstration : elle rend toujours la même scène.
///
/// Elle ne sert plus au démarrage ordinaire — `surface::reel::Reel` a pris sa place — mais à
/// produire des captures et à montrer la surface sans machine en marche, ce qui reste utile pour
/// une revue. Elle n'est atteignable que par `--demonstration` ou `--capture`, jamais par défaut.
struct Demonstration {
    avec_decision: bool,
}

impl Source for Demonstration {
    fn scene(&mut self) -> Scene {
        demonstration(self.avec_decision)
    }

    fn repond(&mut self, reponse: Reponse) {
        // Une démonstration n'a rien à trancher ; on le dit plutôt que de faire semblant.
        eprintln!("réponse ignorée en démonstration : {reponse:?}");
    }
}

/// Une scène représentative, pour montrer la surface sans machine en marche.
///
/// Les chiffres sont ceux d'une journée ordinaire : des tâches de rythmes différents, une
/// terminée, et selon le cas une décision qui attend. Rien d'idéalisé — une capture qui ne
/// montrerait que des courants vifs cacherait justement ce que cette surface sert à voir.
fn demonstration(avec_decision: bool) -> Scene {
    let courant = |tache: &str, intitule: &str, agent: &str, etat, debit, budget, etapes| Courant {
        tache: tache.to_owned(),
        intitule: intitule.to_owned(),
        agent: agent.to_owned(),
        etat,
        debit,
        budget_consomme: budget,
        etapes,
        task_state: None,
        task_revision: 0,
    };

    Scene {
        heure: "14:37".to_owned(),
        date: "jeudi 12 septembre".to_owned(),
        courants: vec![
            courant(
                "t-4812",
                "Relire les changements de la branche",
                "claude-code",
                Etat::Court,
                34.0,
                0.18,
                71,
            ),
            courant(
                "t-4813",
                "Réserver un billet Paris–Lyon",
                "codex",
                if avec_decision {
                    Etat::Attend
                } else {
                    Etat::Court
                },
                9.0,
                0.41,
                23,
            ),
            courant(
                "t-4809",
                "Indexer la documentation interne",
                "prophet-agent",
                Etat::Court,
                6.0,
                0.77,
                318,
            ),
            courant(
                "t-4801",
                "Comparer les offres d'hébergement",
                "gemini",
                Etat::Bloque,
                0.0,
                0.52,
                44,
            ),
            courant(
                "t-4795",
                "Résumer les messages de la nuit",
                "claude-code",
                Etat::Fini,
                0.0,
                0.09,
                12,
            ),
        ],
        decision: avec_decision.then(|| Decision {
            question: "Envoyer le paiement de 87,40 € à SNCF Connect ?".to_owned(),
            consequence: "L'argent part. Aucune annulation n'est possible depuis Prophet OS."
                .to_owned(),
            tache: "t-4813".to_owned(),
            depuis_secondes: 14,
            irreversible: true,
        }),
        isolation: Isolation {
            niveau_max: 1,
            manque: Some("les images d'invité pour le niveau 2".to_owned()),
        },
    }
}
