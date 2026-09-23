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
    let mut out = b"GGUF".to_vec();
    out.extend(3u32.to_le_bytes());
    out.extend(0u64.to_le_bytes());
    out.extend(3u64.to_le_bytes());
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
