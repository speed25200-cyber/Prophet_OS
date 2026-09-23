//! Ce que l'image doit contenir.
//!
//! Vérifier une interface par une capture est facile à mal faire : on compare des octets, une
//! version de pilote change, et tout devient rouge sans qu'aucun défaut n'existe. Ces tests
//! n'affirment donc rien sur des pixels précis. Ils affirment des **propriétés** — il y a de la
//! lumière, elle est dorée, et l'écran change quand l'état du système change — qui sont exactement
//! ce que cette surface promet.
//!
//! Ils exigent un adaptateur graphique, fût-il logiciel, et portent donc le marqueur `needs_gpu`.

use surface::disposition::{Rect, disposer};
use surface::gpu::{Cible, Contexte};
use surface::rendu::Rendu;
use surface::scene::{Courant, Decision, Etat, Isolation, Scene};

fn scene(avec_decision: bool) -> Scene {
    let mut scene = Scene {
        heure: "14:37".to_owned(),
        date: "jeudi 12 septembre".to_owned(),
        courants: vec![
            Courant {
                tache: "t1".to_owned(),
                intitule: "Relire les changements".to_owned(),
                agent: "claude-code".to_owned(),
                etat: Etat::Court,
                debit: 30.0,
                budget_consomme: 0.2,
                etapes: 40,
                task_state: None,
                task_revision: 0,
            },
            Courant {
                tache: "t2".to_owned(),
                intitule: "Réserver un billet".to_owned(),
                agent: "codex".to_owned(),
                etat: if avec_decision {
                    Etat::Attend
                } else {
                    Etat::Court
                },
                debit: 8.0,
                budget_consomme: 0.4,
                etapes: 20,
                task_state: None,
                task_revision: 0,
            },
        ],
        decision: avec_decision.then(|| Decision {
            question: "Envoyer le paiement ?".to_owned(),
            consequence: "L'argent part.".to_owned(),
            motif: None,
            tache: "t2".to_owned(),
            depuis_secondes: 14,
            irreversible: true,
        }),
        isolation: Isolation {
            niveau_max: 1,
            manque: None,
            reserve: None,
        },
    };
    scene.ordonner();
    scene
}

fn rendre(avec_decision: bool, temps: f32) -> Vec<u8> {
    let contexte = Contexte::hors_ecran().expect("un adaptateur est exigé par ce test");
    let mut rendu = Rendu::nouveau(&contexte).expect("les pipelines doivent se construire");
    let cible = Cible::nouvelle(&contexte, 960, 540);
    rendu
        .dessiner(&contexte, &cible, &scene(avec_decision), temps)
        .expect("le rendu doit aboutir");
    cible
        .pixels(&contexte)
        .expect("l'image doit être relisible")
}

/// Fraction de pixels dont la luminosité dépasse un seuil.
fn part_eclairee(pixels: &[u8], seuil: u8) -> f32 {
    let total = pixels.len() / 4;
    let clairs = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0].max(p[1]).max(p[2]) > seuil)
        .count();
    clairs as f32 / total as f32
}

#[test]
#[ignore = "needs_gpu"]
fn l_image_n_est_pas_noire() {
    // Le défaut le plus probable d'un rendu qui « marche » : un écran vide qu'aucun test
    // n'interroge. C'est ici qu'on l'attrape.
    let pixels = rendre(false, 8.0);
    let part = part_eclairee(&pixels, 24);
    assert!(
        part > 0.01,
        "seulement {:.3} % de l'image porte de la lumière : l'écran est vide",
        part * 100.0
    );
}

#[test]
#[ignore = "needs_gpu"]
fn la_lumiere_est_doree() {
    // La promesse du thème : ce qui brille est or, jamais gris. Un rendu qui perdrait la teinte
    // — mauvais espace de couleur, mélange raté — passerait le test précédent sans être correct.
    let pixels = rendre(false, 8.0);
    let mut dores = 0usize;
    let mut clairs = 0usize;
    for p in pixels.as_chunks::<4>().0 {
        let (r, v, b) = (u32::from(p[0]), u32::from(p[1]), u32::from(p[2]));
        if r.max(v).max(b) > 60 {
            clairs += 1;
            if r > b + 20 && v > b {
                dores += 1;
            }
        }
    }
    assert!(clairs > 0, "aucun pixel clair");
    let proportion = dores as f32 / clairs as f32;
    assert!(
        proportion > 0.6,
        "seulement {:.0} % des pixels clairs sont dorés : la teinte s'est perdue",
        proportion * 100.0
    );
}

#[test]
#[ignore = "needs_gpu"]
fn une_decision_change_l_ecran() {
    // La propriété qui fait de cette surface autre chose qu'un fond d'écran : l'état du système
    // se voit. Si les deux images étaient identiques, la décision ne serait pas signalée.
    let sans = rendre(false, 8.0);
    let avec = rendre(true, 8.0);
    assert_eq!(sans.len(), avec.len());
    let differents = sans
        .as_chunks::<4>()
        .0
        .iter()
        .zip(avec.as_chunks::<4>().0.iter())
        .filter(|(a, b)| {
            a[0].abs_diff(b[0]) > 8 || a[1].abs_diff(b[1]) > 8 || a[2].abs_diff(b[2]) > 8
        })
        .count();
    let part = differents as f32 / (sans.len() / 4) as f32;
    assert!(
        part > 0.02,
        "les deux états ne diffèrent que sur {:.2} % de l'écran : une décision passerait inaperçue",
        part * 100.0
    );
}

/// Fraction de pixels éclairés partout où aucun panneau ne se pose.
///
/// Une bande étroite choisie à la main s'était révélée presque vide : les filaments ont leur base
/// aux trois dixièmes et aux quatre dixièmes de la hauteur, et la bande n'en attrapait qu'une
/// lisière de quelques pixels — assez pour que l'atténuation la ramène à zéro et fasse échouer
/// l'assertion qui vérifie que le champ ne s'éteint pas.
///
/// On masque donc les panneaux et on mesure tout le reste, ce qui couvre le champ là où il est.
/// Les mêmes rectangles sont masqués dans les deux images, décision comprise, de sorte que la
/// comparaison porte sur des surfaces identiques.
fn part_eclairee_hors_panneaux(
    pixels: &[u8],
    largeur: u32,
    hauteur: u32,
    panneaux: &[Rect],
    seuil: u8,
) -> f32 {
    let mut clairs = 0usize;
    let mut regardes = 0usize;
    for y in 0..hauteur {
        for x in 0..largeur {
            let px = x as f32;
            let py = y as f32;
            if panneaux.iter().any(|r| {
                px >= r.x - 2.0 && px <= r.droite() + 2.0 && py >= r.y - 2.0 && py <= r.bas() + 2.0
            }) {
                continue;
            }
            regardes += 1;
            let i = ((y * largeur + x) * 4) as usize;
            let p = &pixels[i..i + 3];
            if p[0].max(p[1]).max(p[2]) > seuil {
                clairs += 1;
            }
        }
    }
    assert!(
        regardes > 0,
        "les panneaux ne peuvent pas couvrir tout l'écran"
    );
    clairs as f32 / regardes as f32
}

/// Deux scènes dont **seule** la présence d'une décision diffère.
///
/// La scène ordinaire change davantage : la tâche concernée passe en « attend », elle réclame donc
/// le regard, elle passe en tête, et les filaments échangent leurs hauteurs. Comparer ces deux
/// scènes-là revient à comparer deux champs différents — ce que le premier correctif faisait
/// encore, une fois la mesure pourtant recentrée sur le champ.
fn rendre_a_champ_constant(avec_decision: bool) -> Vec<u8> {
    let contexte = Contexte::hors_ecran().expect("un adaptateur est exigé par ce test");
    let mut rendu = Rendu::nouveau(&contexte).expect("les pipelines doivent se construire");
    let cible = Cible::nouvelle(&contexte, 960, 540);

    let mut scene = scene(false);
    if avec_decision {
        scene.decision = Some(Decision {
            question: "Envoyer le paiement ?".to_owned(),
            consequence: "L'argent part.".to_owned(),
            motif: None,
            tache: "t2".to_owned(),
            depuis_secondes: 14,
            irreversible: true,
        });
    }
    // Pas de `ordonner` ici : l'ordre est déjà fixé par `scene(false)`, et le refaire
    // réintroduirait précisément ce qu'on cherche à écarter.
    rendu
        .dessiner(&contexte, &cible, &scene, 8.0)
        .expect("le rendu doit aboutir");
    cible
        .pixels(&contexte)
        .expect("l'image doit être relisible")
}

#[test]
#[ignore = "needs_gpu"]
fn une_decision_fait_reculer_le_champ() {
    // Reculer, pas disparaître. Les deux erreurs symétriques — un champ qui continue de concourir
    // avec la question, ou un champ éteint qui masque ce qui tourne — sont également fausses.
    //
    // La mesure porte sur une bande où seul le champ se dessine. Prise sur l'écran entier, elle
    // confondait le recul du champ avec l'apparition du panneau de décision, qui ajoute au centre
    // plus de lumière que le champ n'en perd : le premier passage a ainsi declaré en échec un
    // comportement parfaitement correct.
    // Les rectangles de l'état le plus charge : masquer aussi le panneau de décision dans l'image
    // qui ne l'a pas garantit que les deux mesures portent sur la même surface.
    let mut avec_decision = scene(false);
    avec_decision.decision = Some(Decision {
        question: "Envoyer le paiement ?".to_owned(),
        consequence: "L'argent part.".to_owned(),
        motif: None,
        tache: "t2".to_owned(),
        depuis_secondes: 14,
        irreversible: true,
    });
    let panneaux: Vec<Rect> = disposer(&avec_decision, 960.0, 540.0)
        .into_iter()
        .map(|p| p.rect)
        .collect();

    let sans =
        part_eclairee_hors_panneaux(&rendre_a_champ_constant(false), 960, 540, &panneaux, 24);
    let avec = part_eclairee_hors_panneaux(&rendre_a_champ_constant(true), 960, 540, &panneaux, 24);
    eprintln!("champ éclairé — sans décision {sans:.4}, avec {avec:.4}");
    assert!(
        avec < sans,
        "le champ doit reculer devant une question : {avec:.4} contre {sans:.4}"
    );
    assert!(
        avec > 0.0,
        "le champ ne doit pas s'éteindre : ce qui tourne reste visible"
    );
}

#[test]
#[ignore = "needs_gpu"]
fn le_meme_instant_donne_la_meme_image() {
    // Sans cette propriété, aucune des comparaisons ci-dessus ne voudrait rien dire.
    let une = rendre(false, 5.0);
    let deux = rendre(false, 5.0);
    assert_eq!(une, deux, "deux rendus du même instant doivent coïncider");
}

#[test]
#[ignore = "needs_gpu"]
fn le_champ_avance_avec_le_temps() {
    let tot = rendre(false, 1.0);
    let tard = rendre(false, 9.0);
    assert_ne!(tot, tard, "les courants doivent avancer");
}
