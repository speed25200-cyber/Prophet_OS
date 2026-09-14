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
    /// Accent de couleur : arc, or, plasma, jade ou nacre. Sinon PROPHET_SURFACE_ACCENT,
    /// puis le choix conservé dans la configuration.
    #[arg(long)]
    accent: Option<String>,
    /// Mesure le rendu hors écran sur ce nombre d'images, GPU attendu à chaque image, et
    /// imprime les temps, la mémoire résidente et l'adaptateur. Sans fenêtre ni capture.
    #[arg(long, conflicts_with_all = ["capture", "fenetree"], value_parser = clap::value_parser!(u32).range(10..=100_000))]
    mesure: Option<u32>,
    /// Champ complet même sur un rastériseur logiciel, qui le reçoit allégé par défaut :
    /// pour des captures et des mesures comparables à celles d'une carte graphique.
    #[arg(long)]
    champ_complet: bool,
    /// Mesure la consommation au repos : rejoue pendant ce nombre de secondes la politique de
    /// la fenêtre (relecture des services quatre fois par seconde, redessin seulement si la
    /// scène a changé ou si l'interface l'a demandé) et imprime les images rendues et le temps
    /// processeur consommé.
    #[arg(long, conflicts_with_all = ["capture", "fenetree", "mesure"], value_parser = clap::value_parser!(u32).range(1..=3600))]
    repos: Option<u32>,
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
    let accent = match &args.accent {
        Some(nom) => Some(surface::theme::Accent::par_nom(nom).ok_or_else(|| {
            format!(
                "accent inconnu : {nom} ; accents proposés : {}",
                surface::theme::ACCENTS
                    .iter()
                    .map(|a| a.nom)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?),
        None => None,
    };
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
        accent,
        champ_complet: args.champ_complet,
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
    if let Some(images) = args.mesure {
        return mesurer(
            source.as_mut(),
            &options,
            images,
            args.largeur,
            args.hauteur,
        );
    }
    if let Some(secondes) = args.repos {
        return reposer(
            source.as_mut(),
            &options,
            secondes,
            args.largeur,
            args.hauteur,
        );
    }
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
        if let Some(accent) = options.accent {
            bureau.choisir_accent(accent);
        }
        if options.champ_complet {
            bureau.forcer_champ_complet(&context);
        }
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

/// Mesure ce que coûte une image : composition, soumission et travail du GPU, attendu.
///
/// C'est l'instrument du critère d'interface de FRONTIER : le même binaire, la même scène,
/// sur n'importe quelle machine, donne des temps par image et une mémoire résidente
/// comparables. Sur un rastériseur logiciel, il mesure le processeur ; sur une carte
/// graphique, il mesure la carte. Il le dit en nommant l'adaptateur.
fn mesurer(
    source: &mut dyn Source,
    options: &Options,
    images: u32,
    largeur: u32,
    hauteur: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let context = Contexte::hors_ecran()?;
    let target = Cible::nouvelle(&context, largeur, hauteur);
    let mut bureau = Bureau::nouveau(&context, options.endpoint.clone(), options.demonstration);
    if let Some(accent) = options.accent {
        bureau.choisir_accent(accent);
    }
    if options.champ_complet {
        bureau.forcer_champ_complet(&context);
    }
    bureau.brancher_missions(surface::reel::Sockets::default().agentd);
    bureau.atelier.mouvement_reduit = options.mouvement_reduit;
    bureau.atelier.page = options.page;
    let avant = memoire_residente_kio();
    let mut durees = Vec::with_capacity(images as usize);
    // Cinq images de mise en route : chargement des glyphes, premières allocations.
    for i in 0..images + 5 {
        let mut scene = source.scene();
        scene.ordonner();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(largeur as f32, hauteur as f32),
            )),
            time: Some(f64::from(i) / 60.0),
            ..Default::default()
        };
        let depart = Instant::now();
        let (mut output, _) = bureau.composer(input, &scene);
        bureau.rendre(&context, &target, &mut output);
        context
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|e| format!("attente du GPU : {e}"))?;
        if i >= 5 {
            durees.push(depart.elapsed().as_secs_f64() * 1000.0);
        }
    }
    let apres = memoire_residente_kio();
    durees.sort_by(f64::total_cmp);
    let centile = |p: f64| durees[((durees.len() - 1) as f64 * p).round() as usize];
    let scene = source.scene();
    println!(
        "adaptateur : {}{}",
        context.adaptateur,
        if context.logiciel && !options.champ_complet {
            " — rastériseur logiciel, champ allégé"
        } else if context.logiciel {
            " — rastériseur logiciel, champ complet imposé"
        } else {
            ""
        }
    );
    println!(
        "scène : {} mission{} ({} active{}), {}×{}, champ {}",
        scene.courants.len(),
        if scene.courants.len() == 1 { "" } else { "s" },
        scene.actives(),
        if scene.actives() == 1 { "" } else { "s" },
        largeur,
        hauteur,
        if bureau.champ_vivant() {
            "vivant"
        } else {
            "immobile"
        }
    );
    println!("particules par image : {}", bureau.particules_du_champ());
    println!(
        "{} images : médiane {:.2} ms, p95 {:.2} ms, maximum {:.2} ms (composition + soumission + GPU attendu)",
        durees.len(),
        centile(0.5),
        centile(0.95),
        durees[durees.len() - 1]
    );
    match (avant, apres) {
        (Some(a), Some(b)) => println!(
            "mémoire résidente : {:.1} Mio avant, {:.1} Mio après",
            a as f64 / 1024.0,
            b as f64 / 1024.0
        ),
        _ => println!("mémoire résidente : indisponible sur ce système"),
    }
    Ok(())
}

/// Mesure ce que coûte le repos : la fenêtre relit les services quatre fois par seconde et
/// ne redessine que si la scène a changé ou si l'interface l'a demandé. Ici, sans fenêtre,
/// la même politique tourne pendant `secondes` et l'on compte les images réellement rendues
/// et le temps processeur consommé. Une scène immobile doit coûter une image, puis rien.
fn reposer(
    source: &mut dyn Source,
    options: &Options,
    secondes: u32,
    largeur: u32,
    hauteur: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let context = Contexte::hors_ecran()?;
    let target = Cible::nouvelle(&context, largeur, hauteur);
    let mut bureau = Bureau::nouveau(&context, options.endpoint.clone(), options.demonstration);
    if let Some(accent) = options.accent {
        bureau.choisir_accent(accent);
    }
    if options.champ_complet {
        bureau.forcer_champ_complet(&context);
    }
    bureau.brancher_missions(surface::reel::Sockets::default().agentd);
    bureau.atelier.mouvement_reduit = options.mouvement_reduit;
    bureau.atelier.page = options.page;
    // Les demandes de redessin de l'interface arrivent par ce canal, comme dans la fenêtre.
    let (tx, rx) = std::sync::mpsc::channel::<Duration>();
    bureau.ctx.set_request_repaint_callback(move |request| {
        let _ = tx.send(request.delay);
    });
    let debut = Instant::now();
    let cpu_debut = temps_processeur_secondes();
    let mut empreinte = None;
    let mut repeindre = true;
    let mut prochaine_lecture = Instant::now();
    let mut prochain_redessin: Option<Instant> = None;
    let mut images = 0u32;
    while debut.elapsed() < Duration::from_secs(u64::from(secondes)) {
        while let Ok(delai) = rx.try_recv() {
            let quand = Instant::now() + delai;
            prochain_redessin = Some(prochain_redessin.map_or(quand, |p| p.min(quand)));
        }
        let maintenant = Instant::now();
        if prochain_redessin.is_some_and(|p| maintenant >= p) {
            prochain_redessin = None;
            repeindre = true;
        }
        let mut redessiner = false;
        if maintenant >= prochaine_lecture {
            prochaine_lecture = maintenant + Duration::from_millis(250);
            let scene = source.scene();
            redessiner = surface::fenetre::doit_redessiner(repeindre, empreinte, &scene);
        } else if repeindre {
            redessiner = true;
        }
        if redessiner {
            let mut scene = source.scene();
            scene.ordonner();
            empreinte = Some(scene.empreinte());
            repeindre = false;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(largeur as f32, hauteur as f32),
                )),
                time: Some(debut.elapsed().as_secs_f64()),
                ..Default::default()
            };
            let (mut output, _) = bureau.composer(input, &scene);
            bureau.rendre(&context, &target, &mut output);
            context
                .device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: None,
                })
                .map_err(|e| format!("attente du GPU : {e}"))?;
            images += 1;
        } else {
            let attente = [Some(prochaine_lecture), prochain_redessin]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(prochaine_lecture);
            std::thread::sleep(
                attente
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(250)),
            );
        }
    }
    let ecoule = debut.elapsed().as_secs_f64();
    let cpu = temps_processeur_secondes()
        .zip(cpu_debut)
        .map(|(fin, debut)| fin - debut);
    let scene = source.scene();
    println!("adaptateur : {}", context.adaptateur);
    println!(
        "scène : {} mission{} ({} active{}), {}×{}, champ {}",
        scene.courants.len(),
        if scene.courants.len() == 1 { "" } else { "s" },
        scene.actives(),
        if scene.actives() == 1 { "" } else { "s" },
        largeur,
        hauteur,
        if bureau.champ_vivant() {
            "vivant"
        } else {
            "immobile"
        }
    );
    println!(
        "{ecoule:.1} s : {images} image{} rendue{}, {:.1} par seconde",
        if images == 1 { "" } else { "s" },
        if images == 1 { "" } else { "s" },
        f64::from(images) / ecoule
    );
    match cpu {
        Some(cpu) => println!(
            "temps processeur : {cpu:.3} s, soit {:.1} % d'un cœur",
            cpu / ecoule * 100.0
        ),
        None => println!("temps processeur : indisponible sur ce système"),
    }
    match memoire_residente_kio() {
        Some(kio) => println!("mémoire résidente : {:.1} Mio", kio as f64 / 1024.0),
        None => println!("mémoire résidente : indisponible sur ce système"),
    }
    Ok(())
}

/// Le temps processeur du processus (utilisateur et système), en secondes, lu dans `/proc`.
fn temps_processeur_secondes() -> Option<f64> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    // Le nom du programme, entre parenthèses, peut contenir des espaces : on lit après.
    let apres = &stat[stat.rfind(')')? + 2..];
    let champs: Vec<&str> = apres.split_whitespace().collect();
    let utime: f64 = champs.get(11)?.parse().ok()?;
    let stime: f64 = champs.get(12)?.parse().ok()?;
    let tics = 100.0;
    Some((utime + stime) / tics)
}

/// La mémoire résidente du processus, en Kio, lue dans `/proc` ; absente ailleurs.
fn memoire_residente_kio() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
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
            motif: Some("Le tarif de 87,40 € expire ce soir ; demain il sera de 112 €.".to_owned()),
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
