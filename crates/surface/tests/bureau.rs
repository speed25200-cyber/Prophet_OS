//! Parcours de saisie et navigation dans les vrais widgets, avec rendu GPU hors écran.

use egui::{Event, Modifiers, PointerButton, pos2};
use surface::atelier::Page;
use surface::bureau::Bureau;
use surface::gpu::{Cible, Contexte};
use surface::scene::{Isolation, Scene};

fn scene() -> Scene {
    Scene {
        heure: "12:30".into(),
        date: "samedi".into(),
        courants: vec![],
        decision: None,
        isolation: Isolation {
            niveau_max: 0,
            manque: Some("services absents dans ce test".into()),
            reserve: None,
        },
    }
}

fn capture(context: &Contexte, target: &Cible, name: &str) {
    if let Ok(dir) = std::env::var("PROPHET_CAPTURE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(
            std::path::Path::new(&dir)
                .join(format!("surface-atelier-{name}-{}.png", target.largeur)),
        )
        .unwrap();
        let mut encoder = png::Encoder::new(file, target.largeur, target.hauteur);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&target.pixels(context).unwrap())
            .unwrap();
    }
}

fn frame(bureau: &mut Bureau, context: &Contexte, target: &Cible, events: Vec<Event>) {
    frame_at(bureau, context, target, events, 8.0);
}

fn frame_at(
    bureau: &mut Bureau,
    context: &Contexte,
    target: &Cible,
    events: Vec<Event>,
    time: f64,
) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(target.largeur as f32, target.hauteur as f32),
        )),
        time: Some(time),
        events,
        focused: true,
        ..Default::default()
    };
    let (mut output, decision) = bureau.composer(input, &scene());
    assert!(decision.is_none());
    bureau.rendre(context, target, &mut output);
}

#[test]
#[ignore = "needs_gpu"]
fn la_supervision_au_repos_ne_produit_pas_d_animation_decorative() {
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1280, 720);
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.figer_transitions();
    bureau.atelier.mouvement_reduit = true;
    for _ in 0..3 {
        frame_at(&mut bureau, &context, &target, vec![], 8.0);
    }
    let fixed = target.pixels(&context).unwrap();
    frame_at(&mut bureau, &context, &target, vec![], 32.0);
    assert_eq!(
        fixed,
        target.pixels(&context).unwrap(),
        "le décor doit rester fixe"
    );
    bureau.atelier.mouvement_reduit = false;
    frame_at(&mut bureau, &context, &target, vec![], 40.0);
    let first = target.pixels(&context).unwrap();
    frame_at(&mut bureau, &context, &target, vec![], 48.0);
    let next = target.pixels(&context).unwrap();
    assert_eq!(first, next, "un état inchangé reste visuellement stable");
}

#[test]
#[ignore = "needs_gpu: mélange des couleurs prémultipliées du bureau"]
fn le_blanc_translucide_ne_grise_pas_un_fond_blanc() {
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 128, 128);
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    let mut output = bureau.ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(128., 128.),
            )),
            ..Default::default()
        },
        |ui| {
            let r = ui.max_rect();
            ui.painter().rect_filled(r, 0, egui::Color32::WHITE);
            ui.painter()
                .rect_filled(r.shrink(16.), 12, egui::Color32::from_white_alpha(128));
        },
    );
    bureau.rendre(&context, &target, &mut output);
    let pixels = target.pixels(&context).unwrap();
    let at = (64 * 128 + 64) * 4;
    let center = &pixels[at..at + 4];
    assert!(
        center[..3].iter().all(|&c| c >= 253),
        "blanc sur blanc assombri : {center:?}"
    );
    assert_eq!(center[3], 255);
}

fn click(x: f32, y: f32) -> Vec<Event> {
    let pos = pos2(x, y);
    vec![
        Event::PointerMoved(pos),
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::default(),
        },
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::default(),
        },
    ]
}

fn click_widget(bureau: &Bureau, id: &str) -> Vec<Event> {
    let response = bureau
        .ctx
        .read_response(egui::Id::new(id))
        .expect("contrôle rendu");
    let center = response.rect.center();
    click(center.x, center.y)
}

#[test]
#[ignore = "needs_gpu"]
fn une_action_irreversible_ne_s_autorise_pas_pour_toute_la_mission() {
    // capd n'en fait jamais une règle de mission (ADR 0054) : la surface ne le propose pas.
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1280, 800);
    for irreversible in [false, true] {
        let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
        bureau.figer_transitions();
        let mut scene = scene();
        scene.decision = Some(surface::scene::Decision {
            question: "Envoyer le paiement ?".into(),
            consequence: "L'argent part.".into(),
            motif: None,
            tache: "test".into(),
            depuis_secondes: 3,
            irreversible,
        });
        for _ in 0..3 {
            avec_scene(&mut bureau, &context, &target, &scene, vec![]);
        }
        let events = click_widget(&bureau, "examiner-decision");
        avec_scene(&mut bureau, &context, &target, &scene, events);
        for _ in 0..3 {
            avec_scene(&mut bureau, &context, &target, &scene, vec![]);
        }
        assert!(
            bureau
                .ctx
                .read_response(egui::Id::new("decision-autoriser"))
                .is_some()
        );
        assert_eq!(
            bureau
                .ctx
                .read_response(egui::Id::new("decision-autoriser-mission"))
                .is_some(),
            !irreversible,
            "irréversible : {irreversible}"
        );
    }
}

#[test]
#[ignore = "needs_gpu"]
fn les_grands_ecrans_se_dessinent_en_1440p_et_en_4k() {
    // Les limites basses de wgpu plafonnaient les textures à 2048 points : une capture ou une
    // fenêtre en 2560 × 1440 faisait tomber la surface.
    let context = Contexte::hors_ecran().unwrap();
    assert!(
        context.dimension_max() >= 3840,
        "{}",
        context.dimension_max()
    );
    for (largeur, hauteur) in [(2560, 1440), (3840, 2160)] {
        let target = Cible::nouvelle(&context, largeur, hauteur);
        assert_eq!((target.largeur, target.hauteur), (largeur, hauteur));
        let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
        for _ in 0..3 {
            frame(&mut bureau, &context, &target, vec![]);
        }
        capture(&context, &target, "grand-ecran");
        let pixels = target.pixels(&context).unwrap();
        assert_eq!(pixels.len(), (largeur * hauteur * 4) as usize);
    }
    // Au-delà de ce que le périphérique accepte, la cible est bornée au lieu de tomber.
    let max = context.dimension_max();
    let (l, h) = context.borner(max + 1000, 0);
    assert_eq!((l, h), (max, 1));
}

#[test]
#[ignore = "needs_gpu"]
fn un_dialogue_vide_propose_des_departs_qui_remplissent_le_brouillon_sans_rien_envoyer() {
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1440, 1000);
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    for _ in 0..3 {
        frame(&mut bureau, &context, &target, vec![]);
    }
    let events = click_widget(&bureau, "nav-conversation");
    frame(&mut bureau, &context, &target, events);
    for _ in 0..3 {
        frame(&mut bureau, &context, &target, vec![]);
    }
    capture(&context, &target, "dialogue-vide");
    let events = click_widget(&bureau, "suggestion-corriger");
    frame(&mut bureau, &context, &target, events);
    frame(&mut bureau, &context, &target, vec![]);
    assert!(
        bureau.atelier.brouillon.contains("corriger un document"),
        "{}",
        bureau.atelier.brouillon
    );
    assert!(!bureau.atelier.generation, "une suggestion n'envoie rien");
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("intention"))
            .is_some_and(|r| r.has_focus()),
        "la saisie reçoit le focus pour compléter"
    );
}

#[test]
#[ignore = "needs_gpu"]
fn la_navigation_et_la_saisie_unicode_fonctionnent_dans_les_widgets_rendus() {
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1280, 720);
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    for _ in 0..3 {
        frame(&mut bureau, &context, &target, vec![]);
    }
    let events = click_widget(&bureau, "nav-modeles");
    frame(&mut bureau, &context, &target, events);
    assert_eq!(bureau.atelier.page, Page::Modeles);
    let events = click_widget(&bureau, "nav-accueil");
    frame(&mut bureau, &context, &target, events);
    assert_eq!(bureau.atelier.page, Page::Accueil);
    let events = click_widget(&bureau, "preparer-mission");
    frame(&mut bureau, &context, &target, events);
    assert_eq!(bureau.atelier.page, Page::Accueil);
    frame(&mut bureau, &context, &target, vec![]);
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("mission-intent"))
            .is_some()
    );
    capture(&context, &target, "preparation");
    let events = click_widget(&bureau, "mission-prepare-back");
    frame(&mut bureau, &context, &target, events);
    let events = click_widget(&bureau, "nav-conversation");
    frame(&mut bureau, &context, &target, events);
    assert_eq!(bureau.atelier.page, Page::Conversation);
    for _ in 0..3 {
        frame(&mut bureau, &context, &target, vec![]);
    }
    let events = click_widget(&bureau, "intention");
    frame(&mut bureau, &context, &target, events);
    let input = bureau
        .ctx
        .read_response(egui::Id::new("intention"))
        .unwrap();
    assert!(
        input.has_focus(),
        "saisie sans focus : {:?}, visible={:?}",
        input.rect,
        input.interact_rect
    );
    frame(
        &mut bureau,
        &context,
        &target,
        vec![
            Event::Text("Bonjour, été".into()),
            Event::Paste(" — texte collé".into()),
        ],
    );
    assert_eq!(bureau.atelier.brouillon, "Bonjour, été — texte collé");
    frame(
        &mut bureau,
        &context,
        &target,
        vec![Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        }],
    );
    assert!(bureau.atelier.brouillon.ends_with('\n'));
    assert!(
        !bureau.atelier.generation,
        "Entrée seule ne doit pas envoyer"
    );
    let pixels = target.pixels(&context).unwrap();
    assert!(
        pixels.chunks_exact(4).any(|p| p[0] > 200 && p[1] > 150),
        "les widgets doivent être visibles"
    );
}

#[test]
#[ignore = "needs_gpu"]
fn chaque_page_se_rend_aux_tailles_annoncees() {
    let context = Contexte::hors_ecran().unwrap();
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    for (width, height) in [(640, 480), (1280, 720), (1920, 1080)] {
        let target = Cible::nouvelle(&context, width, height);
        for page in [
            Page::Accueil,
            Page::Conversation,
            Page::Modeles,
            Page::Activite,
        ] {
            bureau.atelier.page = page;
            for _ in 0..3 {
                frame(&mut bureau, &context, &target, vec![]);
            }
            if page == Page::Accueil {
                for id in [
                    "preparer-mission",
                    "filter-all",
                    "filter-attention",
                    "filter-active",
                    "filter-done",
                    "mission-search",
                ] {
                    let control = bureau
                        .ctx
                        .read_response(egui::Id::new(id))
                        .expect("contrôle de l'accueil");
                    assert!(
                        control
                            .interact_rect
                            .contains_rect(control.rect.shrink(1.0)),
                        "contrôle coupé à {width}×{height} : {id}"
                    );
                }
            }
            for id in [
                "nav-accueil",
                "nav-conversation",
                "nav-modeles",
                "nav-activite",
            ] {
                let control = bureau
                    .ctx
                    .read_response(egui::Id::new(id))
                    .expect("navigation présente");
                assert!(
                    control
                        .interact_rect
                        .contains_rect(control.rect.shrink(1.0)),
                    "navigation coupée à {width}×{height} : {id}"
                );
            }
            assert_eq!(
                target.pixels(&context).unwrap().len(),
                (width * height * 4) as usize
            );
        }
    }
}

fn avec_scene(
    bureau: &mut Bureau,
    context: &Contexte,
    target: &Cible,
    scene: &Scene,
    events: Vec<Event>,
) -> (egui::FullOutput, Option<surface::fenetre::Reponse>) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(target.largeur as f32, target.hauteur as f32),
        )),
        time: Some(8.0),
        events,
        focused: true,
        ..Default::default()
    };
    let (mut output, decision) = bureau.composer(input, scene);
    bureau.rendre(context, target, &mut output);
    (output, decision)
}

/// capd demande le code d'approbation pour accorder (ADR 0057) : la surface ouvre un champ
/// masqué, n'accepte pas un code trop court, rend le code tapé à la source, et « Renoncer »
/// ferme sans rien accorder. Sans code défini, elle propose d'en choisir un.
#[test]
#[ignore = "needs_gpu: la preuve de présence dans la surface"]
fn accorder_demande_le_code_d_approbation_dans_un_champ_masque() {
    use surface::fenetre::Reponse;
    let context = Contexte::hors_ecran().unwrap();
    for (largeur, hauteur) in [(1280, 800), (640, 900)] {
        let target = Cible::nouvelle(&context, largeur, hauteur);
        let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
        bureau.figer_transitions();
        let scene = scene();
        bureau.presence = Some(surface::presence::Demande {
            id: "apr-1".into(),
            portee: "once".into(),
            message: "code d'approbation faux ; encore 4 essais avant le verrou".into(),
            definir: false,
        });
        for _ in 0..3 {
            avec_scene(&mut bureau, &context, &target, &scene, vec![]);
        }
        capture(&context, &target, &format!("code-approbation-{largeur}"));
        let champ = bureau
            .ctx
            .read_response(egui::Id::new("code-approbation"))
            .expect("le champ du code");
        assert!(champ.has_focus(), "le champ reçoit le clavier d'emblée");
        // Trop court : « Accorder » ne rend rien.
        avec_scene(
            &mut bureau,
            &context,
            &target,
            &scene,
            vec![Event::Text("pivo".into())],
        );
        let clic = click_widget(&bureau, "code-confirmer");

        let (_, reponse) = avec_scene(&mut bureau, &context, &target, &scene, clic);
        assert_eq!(reponse, None, "un code trop court ne part pas");
        avec_scene(
            &mut bureau,
            &context,
            &target,
            &scene,
            vec![Event::Text("ine-42".into())],
        );
        let mut rendu = None;
        for _ in 0..2 {
            let clic = click_widget(&bureau, "code-confirmer");

            let (_, reponse) = avec_scene(&mut bureau, &context, &target, &scene, clic);
            if reponse.is_some() {
                rendu = reponse;
                break;
            }
        }
        assert_eq!(rendu, Some(Reponse::Code("pivoine-42".into())));
        // Renoncer ferme sans accorder.
        let clic = click_widget(&bureau, "code-renoncer");

        let (_, reponse) = avec_scene(&mut bureau, &context, &target, &scene, clic);
        assert_eq!(reponse, Some(Reponse::RenoncerAuCode));
    }
    // Sans code défini, la surface propose d'en choisir un et le rend comme tel.
    let target = Cible::nouvelle(&context, 1280, 800);
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.figer_transitions();
    let scene = scene();
    bureau.presence = Some(surface::presence::Demande {
        id: "apr-1".into(),
        portee: "task".into(),
        message: String::new(),
        definir: true,
    });
    for _ in 0..3 {
        avec_scene(&mut bureau, &context, &target, &scene, vec![]);
    }
    capture(&context, &target, "code-approbation-definir-1280");
    avec_scene(
        &mut bureau,
        &context,
        &target,
        &scene,
        vec![Event::Text("glycine-7".into())],
    );
    let mut rendu = None;
    for _ in 0..2 {
        let clic = click_widget(&bureau, "code-confirmer");

        let (_, reponse) = avec_scene(&mut bureau, &context, &target, &scene, clic);
        if reponse.is_some() {
            rendu = reponse;
            break;
        }
    }
    assert_eq!(rendu, Some(Reponse::DefinirCode("glycine-7".into())));
}

/// La page Système dit où en est le code d'approbation (ADR 0057) : à choisir, avec
/// « Définir maintenant » qui ouvre le champ sans décision en attente ; défini ; verrouillé.
#[test]
#[ignore = "needs_gpu: rendu wgpu hors écran"]
fn la_page_systeme_dit_le_code_d_approbation_et_le_fait_choisir() {
    use surface::fenetre::Reponse;
    use surface::presence::EtatDuCode;
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1280, 1400);
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.figer_transitions();
    bureau.atelier.page = Page::Activite;
    let scene = scene();
    bureau.code = Some(EtatDuCode {
        defini: false,
        verrou_s: None,
    });
    for _ in 0..3 {
        avec_scene(&mut bureau, &context, &target, &scene, vec![]);
    }
    capture(&context, &target, "systeme-code-a-choisir");
    let mut rendu = None;
    for _ in 0..2 {
        let clic = click_widget(&bureau, "code-definir-maintenant");
        let (_, reponse) = avec_scene(&mut bureau, &context, &target, &scene, clic);
        if reponse.is_some() {
            rendu = reponse;
            break;
        }
    }
    assert_eq!(rendu, Some(Reponse::DemanderLeCode));
    // La source ouvre alors le champ, sans décision : « Définir » et « Plus tard ».
    bureau.presence = Some(surface::presence::Demande::definir_seulement());
    for _ in 0..3 {
        avec_scene(&mut bureau, &context, &target, &scene, vec![]);
    }
    capture(&context, &target, "systeme-code-definir-seulement");
    avec_scene(
        &mut bureau,
        &context,
        &target,
        &scene,
        vec![Event::Text("glycine-7".into())],
    );
    let mut rendu = None;
    for _ in 0..2 {
        let clic = click_widget(&bureau, "code-confirmer");
        let (_, reponse) = avec_scene(&mut bureau, &context, &target, &scene, clic);
        if reponse.is_some() {
            rendu = reponse;
            break;
        }
    }
    assert_eq!(rendu, Some(Reponse::DefinirCode("glycine-7".into())));
    bureau.presence = None;
    // Défini : plus de bouton. Verrouillé : dit, en minutes.
    bureau.code = Some(EtatDuCode {
        defini: true,
        verrou_s: None,
    });
    for _ in 0..3 {
        avec_scene(&mut bureau, &context, &target, &scene, vec![]);
    }
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("code-definir-maintenant"))
            .is_none(),
        "un code défini ne se propose plus"
    );
    bureau.code = Some(EtatDuCode {
        defini: true,
        verrou_s: Some(240),
    });
    // Le journal du service ne répond plus : trois événements attendent (ADR 0059).
    bureau.journal = Some(surface::scene::AttenteDuJournal {
        nombre: 3,
        depuis: Some("2026-09-24T07:05:00Z".into()),
    });
    let mut textes = String::new();
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(target.largeur as f32, target.hauteur as f32),
            )),
            time: Some(8.0),
            focused: true,
            ..Default::default()
        };
        let (mut sortie, _) = bureau.composer(input, &scene);
        textes = sortie
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        bureau.rendre(&context, &target, &mut sortie);
    }
    capture(&context, &target, "systeme-code-verrouille");
    assert!(
        textes.contains("Verrouillé après trop de codes faux : encore 4 min."),
        "{textes}"
    );
    assert!(textes.contains("JOURNAL DU SERVICE"), "{textes}");
    assert!(
        textes.contains("3 événements attendent le journal depuis 07:05 UTC."),
        "{textes}"
    );
    assert!(textes.contains("Rien n'est perdu"), "{textes}");
}

#[test]
#[ignore = "needs_gpu"]
fn la_decision_exige_un_examen_puis_un_choix_explicite_aux_trois_tailles() {
    let context = Contexte::hors_ecran().unwrap();
    for (width, height) in [(640, 480), (1280, 720), (1920, 1080)] {
        let target = Cible::nouvelle(&context, width, height);
        let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
        bureau.figer_transitions();
        let mut scene = scene();
        scene.decision = Some(surface::scene::Decision {
            question: "Autoriser cette action sur les fichiers sélectionnés ?".into(),
            consequence: "Les fichiers sélectionnés seront modifiés après votre accord.".into(),
            motif: None,
            tache: "test".into(),
            depuis_secondes: 12,
            irreversible: false,
        });
        for _ in 0..3 {
            assert!(
                avec_scene(&mut bureau, &context, &target, &scene, vec![])
                    .1
                    .is_none()
            );
        }
        assert!(
            bureau
                .ctx
                .read_response(egui::Id::new("decision-autoriser"))
                .is_none()
        );
        let before = target.pixels(&context).unwrap();
        let events = click_widget(&bureau, "examiner-decision");
        avec_scene(&mut bureau, &context, &target, &scene, events);
        for _ in 0..3 {
            assert!(
                avec_scene(&mut bureau, &context, &target, &scene, vec![])
                    .1
                    .is_none()
            );
        }
        let after = target.pixels(&context).unwrap();
        let pixel = (2 * width as usize + 2) * 4;
        assert!(
            u16::from(after[pixel]) * 100 < u16::from(before[pixel]) * 95,
            "le fond recule pendant l'examen à {width}×{height} : {} -> {}",
            before[pixel],
            after[pixel]
        );
        for id in ["decision-retour", "decision-refuser", "decision-autoriser"] {
            let control = bureau
                .ctx
                .read_response(egui::Id::new(id))
                .expect("choix visible");
            assert!(
                control
                    .interact_rect
                    .contains_rect(control.rect.shrink(1.0)),
                "choix coupé à {width}×{height} : {id}"
            );
        }
        let events = click_widget(&bureau, "decision-refuser");
        assert_eq!(
            avec_scene(&mut bureau, &context, &target, &scene, events).1,
            Some(surface::fenetre::Reponse::Refuse)
        );
        for _ in 0..3 {
            avec_scene(&mut bureau, &context, &target, &scene, vec![]);
        }
        let events = click_widget(&bureau, "examiner-decision");
        avec_scene(&mut bureau, &context, &target, &scene, events);
        for _ in 0..3 {
            avec_scene(&mut bureau, &context, &target, &scene, vec![]);
        }
        scene.decision.as_mut().unwrap().consequence =
            "Une autre conséquence, qui doit être relue.".into();
        for _ in 0..3 {
            assert!(
                avec_scene(&mut bureau, &context, &target, &scene, vec![])
                    .1
                    .is_none()
            );
        }
        assert!(
            bureau
                .ctx
                .read_response(egui::Id::new("decision-autoriser"))
                .is_none(),
            "une conséquence modifiée referme l'examen"
        );
    }
}

#[test]
#[ignore = "needs_gpu"]
fn choisir_et_filtrer_une_mission_preserve_son_identite() {
    use surface::scene::{Courant, Etat};
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1440, 900);
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.figer_transitions();
    let mut scene = scene();
    scene.courants = [("a", Etat::Court), ("b", Etat::Bloque)]
        .into_iter()
        .map(|(id, etat)| Courant {
            tache: id.into(),
            intitule: format!("Mission {id}"),
            agent: "local".into(),
            etat,
            debit: 1.0,
            budget_consomme: 0.2,
            etapes: 12,
            task_state: None,
            task_revision: 0,
        })
        .collect();
    for _ in 0..3 {
        avec_scene(&mut bureau, &context, &target, &scene, vec![]);
    }
    let events = click_widget(&bureau, "mission-a");
    avec_scene(&mut bureau, &context, &target, &scene, events);
    for _ in 0..3 {
        avec_scene(&mut bureau, &context, &target, &scene, vec![]);
    }
    let events = click_widget(&bureau, "copier-reference");
    let (output, _) = avec_scene(&mut bureau, &context, &target, &scene, events);
    assert!(
        output
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::CopyText(value) if value == "a"))
    );
    let events = click_widget(&bureau, "filter-attention");
    avec_scene(&mut bureau, &context, &target, &scene, events);
    for _ in 0..3 {
        avec_scene(&mut bureau, &context, &target, &scene, vec![]);
    }
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("mission-a"))
            .is_none()
    );
    let events = click_widget(&bureau, "copier-reference");
    let (output, _) = avec_scene(&mut bureau, &context, &target, &scene, events);
    assert!(
        output
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::CopyText(value) if value == "b"))
    );
    scene.courants.clear();
    for _ in 0..3 {
        avec_scene(&mut bureau, &context, &target, &scene, vec![]);
    }
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("copier-reference"))
            .is_none(),
        "une mission disparue ne reste pas dans l'inspecteur"
    );
}

#[test]
#[ignore = "needs_gpu: le mode Focale change la composition sans changer la mission"]
fn la_focale_garde_la_mission_et_le_filtre_retablit_le_panorama() {
    use surface::scene::{Courant, Etat};
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1440, 1000);
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.figer_transitions();
    let mut scene = scene();
    scene.courants = [("a", Etat::Court), ("b", Etat::Bloque)]
        .into_iter()
        .map(|(id, etat)| Courant {
            tache: id.into(),
            intitule: format!("Mission {id} avec un contexte à préserver"),
            agent: "local:test".into(),
            etat,
            debit: 1.0,
            budget_consomme: 0.2,
            etapes: 3,
            task_state: None,
            task_revision: 0,
        })
        .collect();
    for _ in 0..3 {
        avec_scene(&mut bureau, &context, &target, &scene, vec![]);
    }
    let events = click_widget(&bureau, "mission-a");
    avec_scene(&mut bureau, &context, &target, &scene, events);
    let events = click_widget(&bureau, "workspace-focus");
    avec_scene(&mut bureau, &context, &target, &scene, events);
    for _ in 0..3 {
        avec_scene(&mut bureau, &context, &target, &scene, vec![]);
    }
    capture(&context, &target, "focale");
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("mission-a"))
            .is_none()
    );
    let events = click_widget(&bureau, "copier-reference");
    let (out, choice) = avec_scene(&mut bureau, &context, &target, &scene, events);
    assert!(choice.is_none());
    assert!(
        out.platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::CopyText(s) if s=="a"))
    );
    let events = click_widget(&bureau, "filter-attention");
    avec_scene(&mut bureau, &context, &target, &scene, events);
    for _ in 0..3 {
        avec_scene(&mut bureau, &context, &target, &scene, vec![]);
    }
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("mission-b"))
            .is_some()
    );
    let events = click_widget(&bureau, "copier-reference");
    let (out, _) = avec_scene(&mut bureau, &context, &target, &scene, events);
    assert!(
        out.platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::CopyText(s) if s=="b"))
    );
}

#[test]
#[ignore = "needs_gpu: galerie bornée et recherche clavier dans mille missions"]
fn mille_missions_restent_retrouvables_sans_dessiner_toute_la_galerie() {
    use surface::scene::{Courant, Etat};
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1440, 1000);
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.figer_transitions();
    let mut scene = scene();
    scene.courants = (0..1000)
        .map(|n| Courant {
            tache: n.to_string(),
            intitule: format!("Travail {n} à superviser"),
            agent: "local:test".into(),
            etat: Etat::Court,
            debit: 1.,
            budget_consomme: 0.1,
            etapes: 2,
            task_state: None,
            task_revision: 0,
        })
        .collect();
    for _ in 0..3 {
        avec_scene(&mut bureau, &context, &target, &scene, vec![]);
    }
    capture(&context, &target, "mille-missions");
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("mission-0"))
            .is_some()
    );
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("mission-999"))
            .is_none()
    );
    let mut samples = Vec::new();
    for _ in 0..60 {
        let start = std::time::Instant::now();
        avec_scene(&mut bureau, &context, &target, &scene, vec![]);
        samples.push(start.elapsed().as_secs_f64() * 1000.);
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "1000 missions, composition + soumission : p50={:.3} ms, p95={:.3} ms",
        samples[30], samples[57]
    );
    avec_scene(
        &mut bureau,
        &context,
        &target,
        &scene,
        vec![Event::Key {
            key: egui::Key::K,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::CTRL,
        }],
    );
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("mission-search"))
            .unwrap()
            .has_focus()
    );
    avec_scene(
        &mut bureau,
        &context,
        &target,
        &scene,
        vec![Event::Text("750".into())],
    );
    for _ in 0..3 {
        avec_scene(&mut bureau, &context, &target, &scene, vec![]);
    }
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("mission-750"))
            .is_some()
    );
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("mission-0"))
            .is_none()
    );
    capture(&context, &target, "recherche");
    let events = click_widget(&bureau, "copier-reference");
    let (out, choice) = avec_scene(&mut bureau, &context, &target, &scene, events);
    assert!(choice.is_none());
    assert!(
        out.platform_output
            .commands
            .iter()
            .any(|c| matches!(c,egui::OutputCommand::CopyText(s) if s=="750"))
    );
}

#[test]
#[ignore = "needs_gpu"]
fn une_petite_fenetre_ouvre_le_contexte_sans_le_cacher_sous_la_liste() {
    use surface::scene::{Courant, Etat};
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 640, 480);
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.figer_transitions();
    let mut scene = scene();
    scene.courants.push(Courant {
        tache: "a".into(),
        intitule: "Examiner une mission".into(),
        agent: "local".into(),
        etat: Etat::Court,
        debit: 2.0,
        budget_consomme: 0.2,
        etapes: 3,
        task_state: None,
        task_revision: 0,
    });
    for _ in 0..3 {
        avec_scene(&mut bureau, &context, &target, &scene, vec![]);
    }
    let events = click_widget(&bureau, "mission-a");
    avec_scene(&mut bureau, &context, &target, &scene, events);
    for _ in 0..3 {
        avec_scene(&mut bureau, &context, &target, &scene, vec![]);
    }
    for id in ["retour-missions", "copier-reference"] {
        let control = bureau
            .ctx
            .read_response(egui::Id::new(id))
            .expect("contexte accessible");
        assert!(
            control
                .interact_rect
                .contains_rect(control.rect.shrink(1.0)),
            "contrôle coupé : {id}"
        );
    }
    let events = click_widget(&bureau, "copier-reference");
    let (output, _) = avec_scene(&mut bureau, &context, &target, &scene, events);
    assert!(
        output
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::CopyText(value) if value == "a"))
    );
    let events = click_widget(&bureau, "retour-missions");
    avec_scene(&mut bureau, &context, &target, &scene, events);
    for _ in 0..3 {
        avec_scene(&mut bureau, &context, &target, &scene, vec![]);
    }
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("mission-a"))
            .is_some()
    );
}

#[test]
#[ignore = "needs_gpu: le champ avance avec une mission active et se fige au repos"]
fn le_champ_avance_avec_une_mission_active_et_se_fige_au_repos_ou_sous_mouvement_reduit() {
    use surface::scene::{Courant, Etat};
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1280, 720);
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.figer_transitions();
    let mission = |etat, debit| Courant {
        tache: "a".into(),
        intitule: "Une mission qui avance".into(),
        agent: "local".into(),
        etat,
        debit,
        budget_consomme: 0.2,
        etapes: 12,
        task_state: None,
        task_revision: 0,
    };
    let rendre = |bureau: &mut Bureau, scene: &Scene, time: f64| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280., 720.),
            )),
            time: Some(time),
            focused: true,
            ..Default::default()
        };
        let (mut output, _) = bureau.composer(input, scene);
        bureau.rendre(&context, &target, &mut output);
        target.pixels(&context).unwrap()
    };
    let mut scene = scene();
    scene.courants.push(mission(Etat::Court, 20.0));
    for _ in 0..3 {
        rendre(&mut bureau, &scene, 8.0);
    }
    assert!(
        bureau.champ_vivant(),
        "une mission en cours fait vivre le champ"
    );
    let tot = rendre(&mut bureau, &scene, 8.0);
    let tard = rendre(&mut bureau, &scene, 9.0);
    assert_ne!(
        tot, tard,
        "le ruban d'une mission en cours avance avec le temps"
    );
    capture(&context, &target, "champ");

    bureau.atelier.mouvement_reduit = true;
    let un = rendre(&mut bureau, &scene, 8.0);
    let deux = rendre(&mut bureau, &scene, 9.0);
    assert!(!bureau.champ_vivant(), "le mouvement réduit fige le champ");
    assert_eq!(un, deux, "sous mouvement réduit, rien n'avance");

    bureau.atelier.mouvement_reduit = false;
    scene.courants[0] = mission(Etat::Bloque, 0.0);
    let un = rendre(&mut bureau, &scene, 8.0);
    let deux = rendre(&mut bureau, &scene, 9.0);
    assert!(
        !bureau.champ_vivant(),
        "une mission arrêtée ne fait rien bouger"
    );
    assert_eq!(
        un, deux,
        "un ruban arrêté est immobile : l'arrêt se voit, il ne se lit pas"
    );
}

/// Teinte d'un pixel en degrés, et sa saturation de 0 à 1.
fn teinte(p: &[u8]) -> (f32, f32) {
    let (r, g, b) = (
        f32::from(p[0]) / 255.0,
        f32::from(p[1]) / 255.0,
        f32::from(p[2]) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    if d < 1e-6 {
        return (0.0, 0.0);
    }
    let h = if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    ((h + 360.0) % 360.0, d / max)
}

fn ecart_de_teinte(a: f32, b: f32) -> f32 {
    let d = (a - b).abs() % 360.0;
    d.min(360.0 - d)
}

#[test]
#[ignore = "needs_gpu: la nuit, et la lumière dans la couleur de l'accent choisi"]
fn l_atelier_est_une_nuit_dont_la_lumiere_prend_la_couleur_de_l_accent() {
    use surface::scene::{Courant, Etat};
    use surface::theme::{Accent, palette};
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1440, 1000);
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.figer_transitions();
    let mut scene = scene();
    scene.courants = [
        ("a", Etat::Court, 30.0),
        ("b", Etat::Court, 6.0),
        ("c", Etat::Fini, 0.0),
    ]
    .into_iter()
    .map(|(id, etat, debit)| Courant {
        tache: id.into(),
        intitule: format!("Mission {id}"),
        agent: "local:test".into(),
        etat,
        debit,
        budget_consomme: 0.3,
        etapes: 9,
        task_state: None,
        task_revision: 0,
    })
    .collect();
    let (alerte, _) = teinte(&[
        palette::ATTENTE.r(),
        palette::ATTENTE.g(),
        palette::ATTENTE.b(),
    ]);
    let mut images = Vec::new();
    for nom in ["arc", "or"] {
        let accent = Accent::par_nom(nom).unwrap();
        bureau.choisir_accent(accent);
        for _ in 0..3 {
            avec_scene(&mut bureau, &context, &target, &scene, vec![]);
        }
        capture(&context, &target, &format!("nuit-{nom}"));
        let pixels = target.pixels(&context).unwrap();
        let (attendu, _) = teinte(&[accent.vif.r(), accent.vif.g(), accent.vif.b()]);
        let mut sombres = 0usize;
        let mut colores = 0usize;
        let mut fideles = 0usize;
        for p in pixels.chunks_exact(4) {
            let max = p[0].max(p[1]).max(p[2]);
            if max < 40 {
                sombres += 1;
            }
            if max > 90 {
                let (h, sat) = teinte(p);
                if sat > 0.3 {
                    colores += 1;
                    if ecart_de_teinte(h, attendu) < 35.0 || ecart_de_teinte(h, alerte) < 25.0 {
                        fideles += 1;
                    }
                }
            }
        }
        let total = pixels.len() / 4;
        assert!(
            sombres as f32 / total as f32 > 0.6,
            "{nom} : le fond est une nuit, {:.0} % de pixels sombres seulement",
            sombres as f32 / total as f32 * 100.0
        );
        assert!(colores > 0, "{nom} : aucun pixel coloré");
        assert!(
            fideles as f32 / colores as f32 > 0.7,
            "{nom} : seulement {:.0} % des pixels colorés sont de l'accent ou de l'alerte",
            fideles as f32 / colores as f32 * 100.0
        );
        let coin = &pixels[(2 * 1440 + 2) * 4..(2 * 1440 + 2) * 4 + 3];
        assert!(
            coin.iter().all(|&c| c > 0) && coin.iter().all(|&c| c < 40),
            "{nom} : le coin de l'écran est une nuit sans être un noir absolu : {coin:?}"
        );
        images.push(pixels);
    }
    assert_ne!(images[0], images[1], "changer d'accent change l'écran");
}

/// Deux missions reçues, pour que la liste et l'espace de mission se partagent la largeur.
fn deux_missions() -> Scene {
    use surface::scene::{Courant, Etat};
    let mut scene = scene();
    scene.courants = [("a", Etat::Court), ("b", Etat::Bloque)]
        .into_iter()
        .map(|(id, etat)| Courant {
            tache: id.into(),
            intitule: format!("Mission {id} dont le cadre doit tenir dans sa colonne"),
            agent: "local:test".into(),
            etat,
            debit: 1.0,
            budget_consomme: 0.4,
            etapes: 12,
            task_state: None,
            task_revision: 0,
        })
        .collect();
    scene
}

#[test]
#[ignore = "needs_gpu"]
fn l_espace_de_mission_tient_dans_la_colonne_des_commandes() {
    let context = Contexte::hors_ecran().unwrap();
    let scene = deux_missions();
    for (width, height) in [(1280, 720), (1440, 1000), (1920, 1080)] {
        let target = Cible::nouvelle(&context, width, height);
        let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
        bureau.figer_transitions();
        for focale in [false, true] {
            if focale {
                let events = click_widget(&bureau, "workspace-focus");
                avec_scene(&mut bureau, &context, &target, &scene, events);
            }
            for _ in 0..3 {
                avec_scene(&mut bureau, &context, &target, &scene, vec![]);
            }
            let rect = |id: &str| {
                bureau
                    .ctx
                    .read_response(egui::Id::new(id))
                    .unwrap_or_else(|| panic!("{id} rendu à {width}×{height}"))
                    .rect
            };
            // La plaque, telle qu'elle est finalement dessinée, s'arrête au bord de la colonne
            // que les commandes de la page définissent : ni au-delà, ni en retrait.
            let plaque = bureau
                .plaque("espace-de-mission")
                .expect("espace de mission dessiné");
            let colonne = rect("preparer-mission").right();
            assert!(
                (plaque.right() - colonne).abs() <= 1.5,
                "la plaque s'arrête à {:.1} px de la colonne à {width}×{height} (focale : {focale})",
                plaque.right() - colonne
            );
            // Le bouton de l'inspecteur est aligné à droite dans la plaque, à sa marge.
            assert!(rect("copier-reference").right() <= plaque.right() - 22.0);
        }
    }
}

fn touche(key: egui::Key, modifiers: Modifiers) -> Vec<Event> {
    vec![Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }]
}

#[test]
#[ignore = "needs_gpu"]
fn l_espace_vide_recoit_l_objectif_et_ouvre_sa_preparation() {
    let context = Contexte::hors_ecran().unwrap();
    for (width, height) in [(1440, 1000), (640, 900)] {
        let target = Cible::nouvelle(&context, width, height);
        let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
        bureau.figer_transitions();
        for _ in 0..3 {
            frame(&mut bureau, &context, &target, vec![]);
        }
        capture(&context, &target, "vide");
        if width >= 1280 {
            // Sur un grand écran, la plaque entière se lit sans défiler : la barre d'état
            // (28 px) et la marge de la page (30 px) restent libres sous elle.
            let plaque = bureau.plaque("espace-vide").expect("espace vide dessiné");
            assert!(
                plaque.bottom() <= height as f32 - 28.0 - 30.0 + 0.5,
                "la plaque vide finit à {:.0} sur {height}",
                plaque.bottom()
            );
        }
        let events = click_widget(&bureau, "intention-accueil");
        frame(&mut bureau, &context, &target, events);
        assert!(
            bureau
                .ctx
                .read_response(egui::Id::new("intention-accueil"))
                .unwrap()
                .has_focus(),
            "le champ de l'accueil reçoit la saisie à {width}×{height}"
        );
        frame(
            &mut bureau,
            &context,
            &target,
            vec![Event::Text("Classer mes factures de septembre".into())],
        );
        // Rien n'est préparé en tapant : l'objectif reste un brouillon de l'accueil.
        assert!(
            bureau
                .ctx
                .read_response(egui::Id::new("mission-intent"))
                .is_none()
        );
        frame(
            &mut bureau,
            &context,
            &target,
            touche(egui::Key::Enter, Modifiers::default()),
        );
        for _ in 0..2 {
            frame(&mut bureau, &context, &target, vec![]);
        }
        assert!(
            bureau
                .ctx
                .read_response(egui::Id::new("mission-intent"))
                .is_some(),
            "Entrée ouvre la préparation à {width}×{height}"
        );
        assert_eq!(
            bureau.preparation().intent,
            "Classer mes factures de septembre",
            "la préparation reprend l'objectif saisi, sans rien soumettre"
        );
        assert!(bureau.preparation().attempted_id().is_none());
    }
}

#[test]
#[ignore = "needs_gpu"]
fn le_clavier_ouvre_les_pages_et_la_preparation_sans_souris() {
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1280, 720);
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.figer_transitions();
    for _ in 0..3 {
        frame(&mut bureau, &context, &target, vec![]);
    }
    for (key, page) in [
        (egui::Key::Num2, Page::Conversation),
        (egui::Key::Num3, Page::Modeles),
        (egui::Key::Num4, Page::Activite),
        (egui::Key::Num1, Page::Accueil),
    ] {
        frame(&mut bureau, &context, &target, touche(key, Modifiers::CTRL));
        assert_eq!(bureau.atelier.page, page, "Ctrl+{key:?}");
    }
    let preparation_ouverte = |bureau: &Bureau| {
        bureau
            .ctx
            .read_response(egui::Id::new("mission-intent"))
            .is_some()
    };
    frame(
        &mut bureau,
        &context,
        &target,
        touche(egui::Key::N, Modifiers::CTRL),
    );
    frame(&mut bureau, &context, &target, vec![]);
    assert!(preparation_ouverte(&bureau), "Ctrl+N ouvre la préparation");
    let champ = bureau
        .ctx
        .read_response(egui::Id::new("mission-intent"))
        .unwrap();
    assert!(champ.has_focus(), "l'objectif se tape aussitôt");
    frame(
        &mut bureau,
        &context,
        &target,
        vec![Event::Text("Résumer le rapport".into())],
    );
    assert_eq!(bureau.preparation().intent, "Résumer le rapport");
    // Échap quitte d'abord le champ, puis la préparation ; le brouillon est conservé.
    frame(
        &mut bureau,
        &context,
        &target,
        touche(egui::Key::Escape, Modifiers::default()),
    );
    frame(&mut bureau, &context, &target, vec![]);
    assert!(
        preparation_ouverte(&bureau),
        "le premier Échap rend le focus"
    );
    frame(
        &mut bureau,
        &context,
        &target,
        touche(egui::Key::Escape, Modifiers::default()),
    );
    frame(&mut bureau, &context, &target, vec![]);
    assert!(!preparation_ouverte(&bureau), "le second Échap referme");
    assert_eq!(bureau.preparation().intent, "Résumer le rapport");
    assert_eq!(bureau.atelier.page, Page::Accueil);
}

/// Un en-tête GGUF v3 minimal : architecture, quantification Q8_0 et fenêtre de contexte.
fn gguf(architecture: &str, contexte: u32) -> Vec<u8> {
    gguf_avec(architecture, contexte, &[])
}

/// Ajoute un gabarit de conversation à un en-tête : une paire de plus, comptée.
fn avec_gabarit(mut gguf: Vec<u8>, gabarit: &str) -> Vec<u8> {
    let n = u64::from_le_bytes(gguf[16..24].try_into().unwrap()) + 1;
    gguf[16..24].copy_from_slice(&n.to_le_bytes());
    let cle = "tokenizer.chat_template";
    gguf.extend((cle.len() as u64).to_le_bytes());
    gguf.extend(cle.as_bytes());
    gguf.extend(8u32.to_le_bytes());
    gguf.extend((gabarit.len() as u64).to_le_bytes());
    gguf.extend(gabarit.as_bytes());
    gguf
}

/// Le même, avec des nombres de plus sous l'architecture (couches, têtes…).
fn gguf_avec(architecture: &str, contexte: u32, en_plus: &[(&str, u32)]) -> Vec<u8> {
    let mut kv = Vec::new();
    let texte = |kv: &mut Vec<u8>, k: &str, v: &str| {
        kv.extend((k.len() as u64).to_le_bytes());
        kv.extend(k.as_bytes());
        kv.extend(8u32.to_le_bytes());
        kv.extend((v.len() as u64).to_le_bytes());
        kv.extend(v.as_bytes());
    };
    let nombre = |kv: &mut Vec<u8>, k: &str, v: u32| {
        kv.extend((k.len() as u64).to_le_bytes());
        kv.extend(k.as_bytes());
        kv.extend(4u32.to_le_bytes());
        kv.extend(v.to_le_bytes());
    };
    texte(&mut kv, "general.architecture", architecture);
    nombre(&mut kv, "general.file_type", 7);
    nombre(&mut kv, &format!("{architecture}.context_length"), contexte);
    for (k, v) in en_plus {
        nombre(&mut kv, &format!("{architecture}.{k}"), *v);
    }
    let mut out = b"GGUF".to_vec();
    out.extend(3u32.to_le_bytes());
    out.extend(0u64.to_le_bytes());
    out.extend((3 + en_plus.len() as u64).to_le_bytes());
    out.extend(kv);
    out
}

#[test]
#[ignore = "needs_gpu"]
fn les_poids_installes_se_lisent_sur_la_page_modeles() {
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1440, 1000);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("qwen3-1.7b.gguf"), gguf("qwen3", 40_960)).unwrap();
    std::fs::write(dir.path().join("abime.gguf"), b"pas un modele").unwrap();
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.figer_transitions();
    bureau.atelier.dossier_des_poids = dir.path().to_owned();
    bureau.atelier.fichiers_de_poids.clear();
    bureau.atelier.page = Page::Modeles;
    let limite = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !bureau.atelier.poids_lus {
        frame(&mut bureau, &context, &target, vec![]);
        assert!(std::time::Instant::now() < limite, "catalogue jamais lu");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    for _ in 0..3 {
        frame(&mut bureau, &context, &target, vec![]);
    }
    capture(&context, &target, "poids");
    assert_eq!(bureau.atelier.poids.len(), 2);
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("poids-qwen3-1.7b.gguf"))
            .is_some(),
        "le poids lu doit être affiché"
    );
    assert!(
        bureau.atelier.poids[0].as_ref().is_err(),
        "le fichier abîmé est dit refusé"
    );
}

#[test]
#[ignore = "needs_gpu"]
fn la_page_modeles_dit_la_memoire_que_chaque_poids_demande() {
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1440, 1400);
    let dir = tempfile::tempdir().unwrap();
    let tetes = |couches| {
        [
            ("block_count", couches),
            ("attention.head_count", 16),
            ("attention.head_count_kv", 8),
            ("attention.key_length", 128),
            ("attention.value_length", 128),
        ]
    };
    std::fs::write(
        dir.path().join("qwen3-1.7b.gguf"),
        avec_gabarit(
            gguf_avec("qwen3", 40_960, &tetes(28)),
            "{%- if tools %}{{ tools }}{%- endif %}<tool_call></tool_call><think></think>",
        ),
    )
    .unwrap();
    // Des couches par centaines de milliers : un cache KV qu'aucune machine ne tient.
    std::fs::write(
        dir.path().join("demesure.gguf"),
        gguf_avec("qwen3", 40_960, &tetes(400_000)),
    )
    .unwrap();
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.figer_transitions();
    bureau.atelier.dossier_des_poids = dir.path().to_owned();
    bureau.atelier.fichiers_de_poids.clear();
    bureau.atelier.contexte_local = 4096;
    bureau.atelier.page = Page::Modeles;
    let limite = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !bureau.atelier.poids_lus {
        frame(&mut bureau, &context, &target, vec![]);
        assert!(std::time::Instant::now() < limite, "catalogue jamais lu");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    for _ in 0..3 {
        frame(&mut bureau, &context, &target, vec![]);
    }
    capture(&context, &target, "poids-memoire");
    let machine = bureau
        .atelier
        .memoire
        .expect("la mémoire de la machine est lue");
    assert!(machine.total > 0 && machine.available <= machine.total);
    for fichier in ["qwen3-1.7b.gguf", "demesure.gguf"] {
        assert!(
            bureau
                .ctx
                .read_response(egui::Id::new(format!("poids-memoire-{fichier}")))
                .is_some(),
            "la jauge de {fichier} doit être affichée"
        );
    }
    let estimations: Vec<_> = bureau
        .atelier
        .poids
        .iter()
        .flatten()
        .map(|w| providers::memory::assess(w, 4096, Some(&machine)).unwrap())
        .collect();
    assert_eq!(
        estimations[0].fit,
        Some(providers::memory::Fit::TooLarge),
        "{estimations:?}"
    );
    assert_eq!(estimations[1].need.kv_cache, 28 * 8 * 256 * 2 * 4096);
    // Le gabarit de conversation dit ce que le modèle sait faire ; sans gabarit, rien.
    let qwen = bureau.atelier.poids[1].as_ref().unwrap();
    assert_eq!(
        qwen.template.map(|t| (t.tool_calls, t.reasoning)),
        Some((true, true))
    );
    assert_eq!(bureau.atelier.poids[0].as_ref().unwrap().template, None);
    // Le seul qui tient et déclare les outils est celui qu'on recommande aux agents.
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("poids-recommande-qwen3-1.7b.gguf"))
            .is_some()
    );
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("poids-recommande-demesure.gguf"))
            .is_none()
    );
    // Servi : le repère de ce que l'instance du moteur tient vraiment.
    let chemin = qwen.path.clone();
    bureau.atelier.instances = vec![(
        chemin.clone(),
        providers::memory::Resident {
            pid: 1,
            rss: 2_300_000_000,
            anonymous: 1_100_000_000,
            file: 1_200_000_000,
        },
    )];
    for _ in 0..2 {
        frame(&mut bureau, &context, &target, vec![]);
    }
    capture(&context, &target, "poids-memoire-servi");
    assert_eq!(
        providers::memory::resident_for(&chemin, &bureau.atelier.instances).map(|r| r.rss),
        Some(2_300_000_000)
    );
}

/// Un moteur simulé : `/props` comme llama-server, un 404 pour le reste, le temps de l'essai.
fn moteur_qui_sert(props: serde_json::Value) -> String {
    use std::io::{BufRead as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
            let mut lecture = std::io::BufReader::new(stream);
            let mut premiere = String::new();
            if lecture.read_line(&mut premiere).is_err() {
                continue;
            }
            loop {
                let mut ligne = String::new();
                if lecture.read_line(&mut ligne).unwrap_or(0) == 0 || ligne == "\r\n" {
                    break;
                }
            }
            let (etat, corps) = if premiere.starts_with("GET /props ") {
                ("200 OK", props.to_string())
            } else {
                ("404 Not Found", "{}".to_owned())
            };
            let _ = write!(
                lecture.get_mut(),
                "HTTP/1.1 {etat}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{corps}",
                corps.len()
            );
        }
    });
    endpoint
}

#[test]
#[ignore = "needs_gpu: rendu wgpu hors écran"]
fn la_page_modeles_marque_le_poids_servi_et_sa_fenetre() {
    // Le fichier annonce 40 960 tokens ; le moteur en accorde 4 096 par requête. La page dit
    // les deux, et marque le fichier réellement chargé.
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1440, 1000);
    let dir = tempfile::tempdir().unwrap();
    let servi = dir.path().join("qwen3-1.7b.gguf");
    std::fs::write(&servi, gguf("qwen3", 40_960)).unwrap();
    std::fs::write(dir.path().join("gemma.gguf"), gguf("gemma3", 131_072)).unwrap();
    let endpoint = moteur_qui_sert(serde_json::json!({"model_path": servi,
        "default_generation_settings": {"n_ctx": 4096}}));
    let mut bureau = Bureau::nouveau(&context, endpoint, false);
    bureau.figer_transitions();
    bureau.atelier.dossier_des_poids = dir.path().to_owned();
    bureau.atelier.fichiers_de_poids.clear();
    bureau.atelier.page = Page::Modeles;
    let limite = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !bureau.atelier.poids_lus {
        frame(&mut bureau, &context, &target, vec![]);
        assert!(std::time::Instant::now() < limite, "catalogue jamais lu");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    for _ in 0..3 {
        frame(&mut bureau, &context, &target, vec![]);
    }
    capture(&context, &target, "poids-servi");
    let marque = bureau
        .ctx
        .read_response(egui::Id::new("poids-servi-qwen3-1.7b.gguf"))
        .expect("le poids servi est marqué");
    assert!(marque.rect.width() > 40.0);
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("poids-servi-gemma.gguf"))
            .is_none(),
        "seul le poids chargé est marqué"
    );
}

#[test]
#[ignore = "needs_gpu: rendu wgpu hors écran"]
fn la_page_systeme_dit_la_reserve_de_microvm() {
    // Niveau 2 et deux microVM prêtes : la page Système le dit, en une ligne lisible et
    // nommée pour l'accessibilité ; sans réserve, la ligne n'existe pas (ADR 0045).
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1440, 1000);
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.figer_transitions();
    bureau.atelier.page = Page::Activite;
    let mut avec = scene();
    avec.isolation = Isolation {
        niveau_max: 2,
        manque: None,
        reserve: Some(surface::scene::Reserve {
            pretes: 2,
            cible: 2,
            erreur: None,
        }),
    };
    let composer = |bureau: &mut Bureau, scene: &Scene| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1440.0, 1000.0),
            )),
            time: Some(8.0),
            focused: true,
            ..Default::default()
        };
        let (mut output, _) = bureau.composer(input, scene);
        bureau.rendre(&context, &target, &mut output);
    };
    for _ in 0..3 {
        composer(&mut bureau, &avec);
    }
    capture(&context, &target, "systeme-reserve");
    let ligne = bureau
        .ctx
        .read_response(egui::Id::new("isolation-reserve"))
        .expect("la réserve est dite");
    assert!(ligne.rect.width() > 200.0, "{:?}", ligne.rect);
    for _ in 0..3 {
        composer(&mut bureau, &scene());
    }
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("isolation-reserve"))
            .is_none(),
        "sans réserve, rien n'est inventé"
    );
}

/// Les méthodes qu'un faux service a reçues, avec leurs paramètres.
type Recues = std::sync::Arc<std::sync::Mutex<Vec<(String, serde_json::Value)>>>;

/// Un faux agentd : répond au catalogue par ce que `catalogue` rend, et note chaque méthode
/// reçue avec ses paramètres.
fn faux_agentd(
    catalogue: impl Fn() -> serde_json::Value + Send + 'static,
) -> (tempfile::TempDir, std::path::PathBuf, Recues) {
    use std::io::{BufRead as _, Write as _};
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("agentd.sock");
    let ecoute = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let recues = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let notees = recues.clone();
    std::thread::spawn(move || {
        for flux in ecoute.incoming() {
            let Ok(flux) = flux else { return };
            let mut lecteur = std::io::BufReader::new(flux);
            let mut ligne = String::new();
            while lecteur.read_line(&mut ligne).unwrap_or(0) > 0 {
                let requete: serde_json::Value = serde_json::from_str(&ligne).unwrap();
                ligne.clear();
                let methode = requete["method"].as_str().unwrap_or_default().to_owned();
                notees
                    .lock()
                    .unwrap()
                    .push((methode.clone(), requete["params"].clone()));
                let resultat = if methode == "model.catalog" {
                    catalogue()
                } else {
                    serde_json::json!({"id": requete["params"]["id"], "state": "running", "received": 0})
                };
                let reponse =
                    serde_json::json!({"jsonrpc": "2.0", "id": requete["id"], "result": resultat});
                let _ = writeln!(lecteur.get_mut(), "{reponse}");
            }
        }
    });
    (dir, socket, recues)
}

#[test]
#[ignore = "needs_gpu: rendu wgpu hors écran"]
fn la_page_modeles_montre_le_catalogue_du_systeme_et_ce_qui_se_telecharge() {
    // Scène d'exemple : un poids posé, un autre à 62 %. La page les dit, chacun avec son geste.
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1440, 2000);
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), true);
    bureau.figer_transitions();
    bureau.atelier.page = Page::Modeles;
    for _ in 0..4 {
        frame(&mut bureau, &context, &target, vec![]);
    }
    capture(&context, &target, "catalogue");
    for id in [
        "catalogue-retirer-qwen3-1.7b-q8",
        "catalogue-arreter-qwen3-0.6b-q8",
        "catalogue-telecharger-qwen3-8b-q4",
    ] {
        assert!(
            bureau.ctx.read_response(egui::Id::new(id)).is_some(),
            "{id} absent"
        );
    }
    assert!(
        bureau
            .ctx
            .read_response(egui::Id::new("catalogue-telecharger-qwen3-1.7b-q8"))
            .is_none(),
        "un poids posé ne se télécharge pas une seconde fois"
    );
}

#[test]
#[ignore = "needs_gpu: rendu wgpu hors écran"]
fn telecharger_depuis_la_page_modeles_le_demande_a_agentd_et_suit_la_progression() {
    let clics = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let lance = clics.clone();
    let (_dir, socket, recues) = faux_agentd(move || {
        let pull = if lance.load(std::sync::atomic::Ordering::Relaxed) {
            serde_json::json!({"state": "running", "received": 250_000_000u64, "total": 1_000_000_000u64})
        } else {
            serde_json::Value::Null
        };
        serde_json::json!({"dir": "/var/lib/prophet/models/catalogue", "entries": [
            {"id": "essai", "name": "Poids d'essai", "quantization": "Q4_K_M", "installed": false, "pull": pull}
        ]})
    });
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1440, 2000);
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.figer_transitions();
    bureau.atelier.socket_agentd = socket;
    bureau.atelier.page = Page::Modeles;
    let attendre = |bureau: &mut Bureau, id: &str| {
        let limite = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while bureau.ctx.read_response(egui::Id::new(id)).is_none() {
            frame(bureau, &context, &target, vec![]);
            assert!(std::time::Instant::now() < limite, "{id} jamais rendu");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    };
    attendre(&mut bureau, "catalogue-telecharger-essai");
    clics.store(true, std::sync::atomic::Ordering::Relaxed);
    let clic = click_widget(&bureau, "catalogue-telecharger-essai");
    frame(&mut bureau, &context, &target, clic);
    attendre(&mut bureau, "catalogue-arreter-essai");
    for _ in 0..2 {
        frame(&mut bureau, &context, &target, vec![]);
    }
    capture(&context, &target, "catalogue-telechargement");
    let recues = recues.lock().unwrap().clone();
    assert!(
        recues
            .iter()
            .any(|(m, p)| m == "model.pull" && p["id"] == "essai"),
        "{recues:?}"
    );
}

/// Un faux routeur de llama-server : un modèle, déchargé tant qu'on ne demande pas de le
/// charger ; toute autre adresse répond 404. Rend l'adresse et les requêtes reçues.
fn routeur_qui_connait(chemin: std::path::PathBuf) -> (String, Recues) {
    use std::io::{BufRead as _, Read as _, Write as _};
    let ecoute = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let adresse = format!("http://{}/v1", ecoute.local_addr().unwrap());
    let recues: Recues = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let notees = recues.clone();
    std::thread::spawn(move || {
        let mut charge = false;
        for flux in ecoute.incoming() {
            let Ok(flux) = flux else { return };
            let _ = flux.set_read_timeout(Some(std::time::Duration::from_secs(5)));
            let mut lecteur = std::io::BufReader::new(flux);
            let mut premiere = String::new();
            if lecteur.read_line(&mut premiere).is_err() {
                continue;
            }
            let mut longueur = 0;
            loop {
                let mut ligne = String::new();
                if lecteur.read_line(&mut ligne).unwrap_or(0) == 0 || ligne == "\r\n" {
                    break;
                }
                if let Some(v) = ligne.to_ascii_lowercase().strip_prefix("content-length:") {
                    longueur = v.trim().parse().unwrap_or(0);
                }
            }
            let mut corps = vec![0; longueur];
            let _ = lecteur.read_exact(&mut corps);
            let corps: serde_json::Value =
                serde_json::from_slice(&corps).unwrap_or(serde_json::Value::Null);
            notees
                .lock()
                .unwrap()
                .push((premiere.trim().to_owned(), corps));
            let (etat, reponse) = if premiere.starts_with("POST /models/load ") {
                charge = true;
                ("200 OK", serde_json::json!({"success": true}))
            } else if premiere.starts_with("GET /models ") {
                (
                    "200 OK",
                    serde_json::json!({"data": [{"id": "essai",
                        "status": {"value": if charge { "loaded" } else { "unloaded" },
                                   "args": ["llama-server", "--model", chemin]}}]}),
                )
            } else {
                ("404 Not Found", serde_json::json!({}))
            };
            let reponse = reponse.to_string();
            let _ = write!(
                lecteur.get_mut(),
                "HTTP/1.1 {etat}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reponse}",
                reponse.len()
            );
        }
    });
    (adresse, recues)
}

#[test]
#[ignore = "needs_gpu: rendu wgpu hors écran"]
fn servir_depuis_la_page_modeles_fait_charger_le_poids_par_le_routeur() {
    let poids = tempfile::tempdir().unwrap();
    let chemin = poids.path().join("essai.gguf");
    std::fs::write(&chemin, b"GGUF").unwrap();
    let catalogue_chemin = chemin.clone();
    let (_dir, socket, _) = faux_agentd(move || {
        serde_json::json!({"dir": "/var/lib/prophet/models/catalogue", "entries": [
            {"id": "essai", "name": "Poids d'essai", "installed": true, "path": catalogue_chemin}
        ]})
    });
    let (moteur, recues) = routeur_qui_connait(chemin);
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1440, 2000);
    let mut bureau = Bureau::nouveau(&context, moteur, false);
    bureau.figer_transitions();
    bureau.atelier.socket_agentd = socket;
    bureau.atelier.page = Page::Modeles;
    let limite = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while bureau
        .ctx
        .read_response(egui::Id::new("catalogue-servir-essai"))
        .is_none()
    {
        frame(&mut bureau, &context, &target, vec![]);
        assert!(
            std::time::Instant::now() < limite,
            "« Servir » jamais proposé"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let clic = click_widget(&bureau, "catalogue-servir-essai");
    frame(&mut bureau, &context, &target, clic);
    // Chargé, le poids est dit servi et le geste disparaît.
    let limite = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while bureau
        .ctx
        .read_response(egui::Id::new("catalogue-servir-essai"))
        .is_some()
    {
        frame(&mut bureau, &context, &target, vec![]);
        assert!(
            std::time::Instant::now() < limite,
            "le poids n'est jamais dit servi"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    capture(&context, &target, "catalogue-servi");
    let limite = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let recues = loop {
        frame(&mut bureau, &context, &target, vec![]);
        let recues = recues.lock().unwrap().clone();
        let charge = recues.iter().position(|(r, corps)| {
            r.starts_with("POST /models/load ") && corps["model"] == "essai"
        });
        // Servi, le poids est un modèle de plus : la liste des modèles est relue.
        if charge.is_some_and(|i| {
            recues[i..]
                .iter()
                .any(|(r, _)| r.starts_with("GET /v1/models "))
        }) {
            break recues;
        }
        assert!(
            std::time::Instant::now() < limite,
            "chargement ou relecture des modèles absents : {recues:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert!(!recues.is_empty());
}

/// Un moteur qui répond en flux, un fragment toutes les `pas` millisecondes, comme llama-server
/// en SSE ; rend l'adresse.
fn moteur_en_flux(fragments: usize, pas: u64) -> String {
    use std::io::{BufRead as _, Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let mut lecture = std::io::BufReader::new(stream);
            let mut premiere = String::new();
            if lecture.read_line(&mut premiere).is_err() {
                continue;
            }
            let mut longueur = 0;
            loop {
                let mut ligne = String::new();
                if lecture.read_line(&mut ligne).unwrap_or(0) == 0 || ligne == "\r\n" {
                    break;
                }
                if let Some(v) = ligne.to_ascii_lowercase().strip_prefix("content-length:") {
                    longueur = v.trim().parse().unwrap_or(0);
                }
            }
            let mut corps = vec![0; longueur];
            let _ = lecture.read_exact(&mut corps);
            let flux = lecture.get_mut();
            let _ = write!(
                flux,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
            );
            for _ in 0..fragments {
                let fragment = serde_json::json!({"choices": [{"index": 0, "delta": {"content": "mot "}, "finish_reason": null}]});
                let _ = write!(flux, "data: {fragment}\n\n");
                let _ = flux.flush();
                std::thread::sleep(std::time::Duration::from_millis(pas));
            }
            let fin = serde_json::json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]});
            let usage = serde_json::json!({"choices": [], "usage": {"prompt_tokens": 5, "completion_tokens": fragments}});
            let _ = write!(flux, "data: {fin}\n\ndata: {usage}\n\ndata: [DONE]\n\n");
        }
    });
    endpoint
}

/// FRONTIER, interface : « absence de blocage pendant l'inférence ». Le moteur répond en flux,
/// 30 fragments espacés de 60 ms ; la surface continue de composer ses images pendant ce temps,
/// affiche la réponse au fil des fragments, et aucune image n'attend le moteur. En build de
/// débogage, la première image d'une taille de police nouvelle coûte jusqu'à 150 ms : les
/// seuils sont relatifs au flux, pas à une carte. La CI relève les temps en release.
#[test]
#[ignore = "needs_gpu"]
fn la_surface_ne_bloque_pas_pendant_une_generation_en_flux() {
    const PAS_MS: u64 = 60;
    const FRAGMENTS: usize = 30;
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1440, 1000);
    let endpoint = moteur_en_flux(FRAGMENTS, PAS_MS);
    let mut bureau = Bureau::nouveau(&context, endpoint, false);
    bureau.figer_transitions();
    bureau.atelier.modeles = vec!["essai".into()];
    bureau.atelier.choisi = "essai".into();
    bureau.atelier.page = Page::Conversation;
    // Cinq images de mise en route : pipelines, glyphes, premières allocations.
    for _ in 0..5 {
        frame(&mut bureau, &context, &target, vec![]);
    }
    bureau.atelier.brouillon = "Dis trente mots.".into();
    let ctx = bureau.ctx.clone();
    bureau.atelier.envoyer(&ctx);
    assert!(bureau.atelier.generation);
    let mut durees = Vec::new();
    let mut longueurs = std::collections::BTreeSet::new();
    let debut = std::time::Instant::now();
    let limite = debut + std::time::Duration::from_secs(15);
    while bureau.atelier.generation {
        assert!(
            std::time::Instant::now() < limite,
            "génération jamais finie"
        );
        let depart = std::time::Instant::now();
        frame(&mut bureau, &context, &target, vec![]);
        durees.push(depart.elapsed().as_secs_f64() * 1000.0);
        if let Some(tour) = bureau.atelier.tours.last() {
            longueurs.insert(tour.reponse.len());
        }
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    let flux_ms = debut.elapsed().as_secs_f64() * 1000.0;
    let tour = bureau.atelier.tours.last().unwrap();
    assert!(tour.erreur.is_none(), "{:?}", tour.erreur);
    assert_eq!(tour.reponse, "mot ".repeat(FRAGMENTS));
    durees.sort_by(f64::total_cmp);
    let centile = |p: f64| durees[((durees.len() - 1) as f64 * p).round() as usize];
    let maximum = durees[durees.len() - 1];
    eprintln!(
        "mesure : {} images composées pendant une génération en flux de {:.0} ms ({} états de la réponse vus) : médiane {:.1} ms, p95 {:.1} ms, maximum {:.1} ms",
        durees.len(),
        flux_ms,
        longueurs.len(),
        centile(0.5),
        centile(0.95),
        maximum
    );
    // La réponse s'affiche au fil du flux, pas d'un bloc à la fin.
    assert!(longueurs.len() >= FRAGMENTS / 3, "{longueurs:?}");
    // Une surface qui attendrait le moteur composerait au rythme des fragments, ou gèlerait une
    // image le temps de toute la génération.
    assert!(
        centile(0.5) < PAS_MS as f64,
        "médiane {:.0} ms",
        centile(0.5)
    );
    assert!(
        maximum < flux_ms / 2.0,
        "une image a duré {maximum:.0} ms sur {flux_ms:.0}"
    );
}

/// L'arrêt d'urgence (FRONTIER, interface) : il n'apparaît qu'avec une mission à arrêter, un
/// premier geste n'ouvre que sa confirmation, Échap la referme sans rien arrêter, Ctrl+Maj+Échap
/// la rouvre, et la confirmation dit l'issue. Ici sans service : rien de réel ne s'arrête, et
/// la surface le dit.
#[test]
#[ignore = "needs_gpu"]
fn l_arret_d_urgence_demande_une_confirmation_avant_tout() {
    let context = Contexte::hors_ecran().unwrap();
    for (largeur, hauteur) in [(1440, 1000), (640, 900)] {
        let target = Cible::nouvelle(&context, largeur, hauteur);
        let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
        bureau.figer_transitions();
        let vide = scene();
        for _ in 0..3 {
            avec_scene(&mut bureau, &context, &target, &vide, vec![]);
        }
        assert!(
            bureau
                .ctx
                .read_response(egui::Id::new("arret-tout"))
                .is_none(),
            "rien à arrêter, pas de bouton"
        );
        let mut scene = scene();
        scene.courants.push(surface::scene::Courant {
            tache: "t-1".into(),
            intitule: "Relire la branche".into(),
            agent: "claude-code".into(),
            etat: surface::scene::Etat::Court,
            debit: 12.0,
            budget_consomme: 0.2,
            etapes: 8,
            task_state: None,
            task_revision: 0,
        });
        for _ in 0..3 {
            avec_scene(&mut bureau, &context, &target, &scene, vec![]);
        }
        let events = click_widget(&bureau, "arret-tout");
        avec_scene(&mut bureau, &context, &target, &scene, events);
        for _ in 0..2 {
            avec_scene(&mut bureau, &context, &target, &scene, vec![]);
        }
        assert!(
            bureau
                .ctx
                .read_response(egui::Id::new("arret-confirmer"))
                .is_some(),
            "la confirmation est ouverte"
        );
        capture(&context, &target, "arret-confirmation");
        avec_scene(
            &mut bureau,
            &context,
            &target,
            &scene,
            touche(egui::Key::Escape, Modifiers::NONE),
        );
        for _ in 0..2 {
            avec_scene(&mut bureau, &context, &target, &scene, vec![]);
        }
        assert!(
            bureau
                .ctx
                .read_response(egui::Id::new("arret-confirmer"))
                .is_none(),
            "Échap referme la confirmation"
        );
        assert!(
            bureau
                .ctx
                .read_response(egui::Id::new("arret-ecarter"))
                .is_none(),
            "rien n'est parti"
        );
        avec_scene(
            &mut bureau,
            &context,
            &target,
            &scene,
            touche(egui::Key::Escape, Modifiers::CTRL | Modifiers::SHIFT),
        );
        for _ in 0..2 {
            avec_scene(&mut bureau, &context, &target, &scene, vec![]);
        }
        let events = click_widget(&bureau, "arret-confirmer");
        avec_scene(&mut bureau, &context, &target, &scene, events);
        for _ in 0..2 {
            avec_scene(&mut bureau, &context, &target, &scene, vec![]);
        }
        assert!(
            bureau
                .ctx
                .read_response(egui::Id::new("arret-ecarter"))
                .is_some(),
            "l'issue se lit sous l'en-tête"
        );
        capture(&context, &target, "arret-issue");
    }
}

/// Ce que la surface peint — les cartes des clients, les relevés de l'en-tête, les cadrans d'une
/// mission, l'échelle d'isolation — un lecteur d'écran doit pouvoir le lire : l'arbre
/// d'accessibilité (AccessKit) le dit en phrases, page par page.
#[test]
#[ignore = "needs_gpu: arbre d'accessibilité des pages rendues"]
fn l_arbre_d_accessibilite_dit_ce_que_la_surface_peint() {
    let context = Contexte::hors_ecran().unwrap();
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), true);
    bureau.figer_transitions();
    bureau.ctx.enable_accesskit();
    let cible = Cible::nouvelle(&context, 1440, 1000);
    let mut s = scene();
    s.courants = vec![surface::scene::Courant {
        tache: "t-1".into(),
        intitule: "Relire les changements du dépôt".into(),
        agent: "claude-code".into(),
        etat: surface::scene::Etat::Court,
        debit: 3.0,
        budget_consomme: 0.4,
        etapes: 12,
        task_state: None,
        task_revision: 0,
    }];
    let lire = |bureau: &mut Bureau, page: Page| {
        bureau.atelier.page = page;
        let mut labels = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1440.0, 1000.0),
                )),
                time: Some(8.0),
                focused: true,
                ..Default::default()
            };
            let (mut output, _) = bureau.composer(input, &s);
            let maj = output.platform_output.accesskit_update.take();
            bureau.rendre(&context, &cible, &mut output);
            if let Some(update) = maj {
                labels = update
                    .nodes
                    .iter()
                    .filter_map(|(_, n)| n.label().or_else(|| n.value()).map(str::to_owned))
                    .collect();
            }
        }
        labels.join("\n")
    };
    let missions = lire(&mut bureau, Page::Accueil);
    for attendu in [
        "1 mission active, 0 à examiner, isolation jusqu'au niveau 0",
        "12:30, samedi, UTC",
        "12 étapes",
        "% du budget consommé",
    ] {
        assert!(
            missions.contains(attendu),
            "« {attendu} » absent :\n{missions}"
        );
    }
    let modeles = lire(&mut bureau, Page::Modeles);
    for attendu in [
        "claude-code, version exemple : Session ouverte.",
        "codex, version exemple : Installé, connexion requise. Connectez-vous dans sa fenêtre.",
        "gemini, version inconnue : Absent de cette machine.",
    ] {
        assert!(
            modeles.contains(attendu),
            "« {attendu} » absent :\n{modeles}"
        );
    }
    let systeme = lire(&mut bureau, Page::Activite);
    for attendu in [
        "Isolation : niveau 0, Confiné : disponible",
        "niveau 1, Noyau utilisateur : indisponible, il manque : services absents dans ce test",
        "niveau 2, MicroVM : indisponible",
    ] {
        assert!(
            systeme.contains(attendu),
            "« {attendu} » absent :\n{systeme}"
        );
    }
}
