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
    let events = click_widget(&bureau, "preparer-mission");
    frame(&mut bureau, &context, &target, events);
    assert_eq!(bureau.atelier.page, Page::Conversation);
    frame(&mut bureau, &context, &target, vec![]);
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
                for id in ["preparer-mission", "filter-all", "filter-attention"] {
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
