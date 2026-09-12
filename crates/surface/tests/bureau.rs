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
        },
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
fn le_mouvement_reduit_fige_la_sculpture_sans_supprimer_le_dessin() {
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
    assert!(
        first
            .iter()
            .zip(next.iter())
            .filter(|(a, b)| a != b)
            .count()
            > 1000,
        "l'animation doit modifier la sculpture rendue"
    );
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
    let events = click_widget(&bureau, "intention");
    frame(&mut bureau, &context, &target, events);
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
                    "intention",
                    "Explorer une idée",
                    "Comprendre un sujet",
                    "Affiner un texte",
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
            assert_eq!(
                target.pixels(&context).unwrap().len(),
                (width * height * 4) as usize
            );
        }
    }
}

#[test]
#[ignore = "needs_gpu"]
fn une_decision_capturee_est_lisible_et_fait_reculer_le_fond() {
    let context = Contexte::hors_ecran().unwrap();
    let target = Cible::nouvelle(&context, 1920, 1080);
    let mut bureau = Bureau::nouveau(&context, "http://127.0.0.1:1/v1".into(), false);
    bureau.figer_transitions();
    for _ in 0..3 {
        frame(&mut bureau, &context, &target, vec![]);
    }
    let before = target.pixels(&context).unwrap();
    let mut scene = scene();
    scene.decision = Some(surface::scene::Decision {
        question: "Autoriser cette action sur les fichiers sélectionnés ?".into(),
        consequence: "Les fichiers sélectionnés seront modifiés après votre accord.".into(),
        tache: "test".into(),
        depuis_secondes: 12,
        irreversible: false,
    });
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1920.0, 1080.0),
            )),
            time: Some(8.0),
            ..Default::default()
        };
        let (mut output, decision) = bureau.composer(input, &scene);
        assert!(decision.is_none());
        bureau.rendre(&context, &target, &mut output);
    }
    let after = target.pixels(&context).unwrap();
    let pixel = (160 * 1920 + 100) * 4;
    assert!(
        u16::from(after[pixel]) * 10 < u16::from(before[pixel]) * 8,
        "le fond doit être atténué"
    );
    let bright = |pixels: &[u8]| {
        (370..710)
            .flat_map(|y| (630..1290).map(move |x| (y * 1920 + x) * 4))
            .filter(|&i| pixels[i] > 180 && pixels[i + 1] > 150)
            .count()
    };
    assert!(
        bright(&after) > bright(&before) + 300,
        "le texte de la décision doit être lisible à instant constant"
    );
}
