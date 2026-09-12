//! Où les choses se placent.
//!
//! Pas de barre de navigation. Une barre de navigation suppose qu'on navigue, donc qu'on clique,
//! donc qu'on interagit — l'exact contraire de ce que cette surface cherche. Il n'y a rien à
//! visiter : ce qui compte est déjà à l'écran, et ce qui n'y est pas ne compte pas maintenant.
//!
//! Trois zones, et le champ qui traverse tout :
//!
//! - **à gauche**, les courants, un par tâche, le plus urgent en haut ;
//! - **à droite**, l'état de la machine : son isolation, l'heure ;
//! - **au centre**, rien. Le champ passe, et on le voit.
//!
//! Quand une décision attend, un panneau vient au centre et prend la place du vide. C'est le seul
//! moment où quelque chose s'interpose.

use crate::scene::Scene;
use crate::theme::PAS;

/// Un rectangle, en pixels, origine en haut à gauche.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Abscisse du coin haut-gauche.
    pub x: f32,
    /// Ordonnée du coin haut-gauche.
    pub y: f32,
    /// Largeur.
    pub l: f32,
    /// Hauteur.
    pub h: f32,
}

impl Rect {
    /// Le bord droit.
    #[must_use]
    pub fn droite(&self) -> f32 {
        self.x + self.l
    }
    /// Le bord bas.
    #[must_use]
    pub fn bas(&self) -> f32 {
        self.y + self.h
    }
    /// Vrai si les deux rectangles se chevauchent.
    #[must_use]
    pub fn chevauche(&self, autre: &Self) -> bool {
        self.x < autre.droite()
            && autre.x < self.droite()
            && self.y < autre.bas()
            && autre.y < self.bas()
    }
    /// Le même rectangle, rétréci de `marge` sur chaque bord.
    #[must_use]
    pub fn retreci(&self, marge: f32) -> Self {
        Self {
            x: self.x + marge,
            y: self.y + marge,
            l: (self.l - 2.0 * marge).max(0.0),
            h: (self.h - 2.0 * marge).max(0.0),
        }
    }
}

/// Ce qu'un panneau contient, pour que le rendu sache quoi écrire dedans.
#[derive(Debug, Clone)]
pub enum Contenu {
    /// Un courant, désigné par son rang dans la scène.
    Courant(usize),
    /// L'isolation de la machine.
    Isolation,
    /// L'heure et la date.
    Horloge,
    /// La décision en attente.
    Decision,
}

/// Un panneau placé.
#[derive(Debug, Clone)]
pub struct Place {
    /// Sa géométrie.
    pub rect: Rect,
    /// Ce qu'il porte.
    pub contenu: Contenu,
}

/// Marge extérieure, en pas.
const MARGE: f32 = 2.5;
/// Hauteur d'un panneau de courant.
const HAUTEUR_COURANT: f32 = 96.0;

/// Calcule la place de chaque panneau.
///
/// Les colonnes sont proportionnelles mais bornées : sur un écran très large, des panneaux qui
/// s'étireraient indéfiniment deviendraient illisibles ; sur un écran étroit, des panneaux trop
/// fins le deviendraient aussi.
#[must_use]
pub fn disposer(scene: &Scene, largeur: f32, hauteur: f32) -> Vec<Place> {
    let marge = MARGE * PAS;
    let gouttiere = PAS;

    let colonne_gauche = (largeur * 0.26).clamp(300.0, 460.0);
    let colonne_droite = (largeur * 0.22).clamp(260.0, 400.0);

    let mut places = Vec::new();

    // --- Les courants, à gauche ---
    // Ce qui ne tient pas n'est pas dessiné plus petit : il n'est pas dessiné. Entasser douze
    // tâches illisibles vaut moins que d'en montrer six qu'on lit d'un coup d'œil.
    let hauteur_utile = hauteur - 2.0 * marge;
    let tiennent = ((hauteur_utile + gouttiere) / (HAUTEUR_COURANT + gouttiere)).floor() as usize;
    let a_montrer = scene.courants.len().min(tiennent.max(1));

    for rang in 0..a_montrer {
        places.push(Place {
            rect: Rect {
                x: marge,
                y: marge + rang as f32 * (HAUTEUR_COURANT + gouttiere),
                l: colonne_gauche,
                h: HAUTEUR_COURANT,
            },
            contenu: Contenu::Courant(rang),
        });
    }

    // --- L'état de la machine, à droite ---
    let x_droite = largeur - marge - colonne_droite;
    let hauteur_isolation = 132.0;
    let hauteur_horloge = 118.0;

    places.push(Place {
        rect: Rect {
            x: x_droite,
            y: marge,
            l: colonne_droite,
            h: hauteur_isolation,
        },
        contenu: Contenu::Isolation,
    });
    places.push(Place {
        rect: Rect {
            x: x_droite,
            y: hauteur - marge - hauteur_horloge,
            l: colonne_droite,
            h: hauteur_horloge,
        },
        contenu: Contenu::Horloge,
    });

    // --- La décision, au centre, quand il y en a une ---
    if scene.decision.is_some() {
        let l = (largeur * 0.46).clamp(420.0, 760.0);
        let h = 260.0;
        places.push(Place {
            rect: Rect {
                x: (largeur - l) * 0.5,
                y: (hauteur - h) * 0.5,
                l,
                h,
            },
            contenu: Contenu::Decision,
        });
    }

    places
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Courant, Decision, Etat, Isolation};

    fn scene_avec(nombre: usize, decision: bool) -> Scene {
        Scene {
            heure: "14:37".to_owned(),
            date: "jeudi 12 septembre".to_owned(),
            courants: (0..nombre)
                .map(|i| Courant {
                    tache: format!("t{i}"),
                    intitule: "faire quelque chose".to_owned(),
                    agent: "claude-code".to_owned(),
                    etat: Etat::Court,
                    debit: 12.0,
                    budget_consomme: 0.2,
                    etapes: 7,
                })
                .collect(),
            decision: decision.then(|| Decision {
                question: "Envoyer ?".to_owned(),
                consequence: "Le message part.".to_owned(),
                tache: "t0".to_owned(),
                depuis_secondes: 8,
                irreversible: true,
            }),
            isolation: Isolation {
                niveau_max: 1,
                manque: None,
            },
        }
    }

    #[test]
    fn rien_ne_deborde_de_l_ecran() {
        for (l, h) in [(1920.0, 1080.0), (1280.0, 720.0), (3840.0, 2160.0)] {
            let places = disposer(&scene_avec(4, true), l, h);
            for place in &places {
                assert!(place.rect.x >= 0.0, "{:?} déborde à gauche", place.contenu);
                assert!(place.rect.y >= 0.0, "{:?} déborde en haut", place.contenu);
                assert!(
                    place.rect.droite() <= l + 0.01,
                    "{:?} déborde à droite en {l}x{h}",
                    place.contenu
                );
                assert!(
                    place.rect.bas() <= h + 0.01,
                    "{:?} déborde en bas en {l}x{h}",
                    place.contenu
                );
            }
        }
    }

    #[test]
    fn les_colonnes_ne_se_chevauchent_pas() {
        let places = disposer(&scene_avec(6, false), 1280.0, 720.0);
        let gauche: Vec<_> = places
            .iter()
            .filter(|p| matches!(p.contenu, Contenu::Courant(_)))
            .collect();
        let droite: Vec<_> = places
            .iter()
            .filter(|p| !matches!(p.contenu, Contenu::Courant(_)))
            .collect();
        for g in &gauche {
            for d in &droite {
                assert!(
                    !g.rect.chevauche(&d.rect),
                    "un courant recouvre l'état de la machine : {:?} et {:?}",
                    g.rect,
                    d.rect
                );
            }
        }
    }

    #[test]
    fn deux_courants_ne_se_recouvrent_jamais() {
        let places = disposer(&scene_avec(5, false), 1920.0, 1080.0);
        let courants: Vec<_> = places
            .iter()
            .filter(|p| matches!(p.contenu, Contenu::Courant(_)))
            .collect();
        for (i, a) in courants.iter().enumerate() {
            for b in courants.iter().skip(i + 1) {
                assert!(!a.rect.chevauche(&b.rect));
            }
        }
    }

    #[test]
    fn ce_qui_ne_tient_pas_n_est_pas_entasse() {
        // Sur un écran court, on montre moins de tâches plutôt que des tâches illisibles.
        let places = disposer(&scene_avec(40, false), 1280.0, 400.0);
        let montres = places
            .iter()
            .filter(|p| matches!(p.contenu, Contenu::Courant(_)))
            .count();
        assert!(montres < 40, "40 tâches ne tiennent pas sur 400 pixels");
        assert!(montres >= 1, "il faut en montrer au moins une");
    }

    #[test]
    fn la_decision_est_centree() {
        let (l, h) = (1920.0, 1080.0);
        let places = disposer(&scene_avec(3, true), l, h);
        let decision = places
            .iter()
            .find(|p| matches!(p.contenu, Contenu::Decision))
            .expect("une décision attend, elle doit avoir sa place");
        let centre_x = decision.rect.x + decision.rect.l * 0.5;
        let centre_y = decision.rect.y + decision.rect.h * 0.5;
        assert!((centre_x - l * 0.5).abs() < 1.0);
        assert!((centre_y - h * 0.5).abs() < 1.0);
    }

    #[test]
    fn sans_decision_rien_ne_s_interpose_au_centre() {
        let places = disposer(&scene_avec(3, false), 1920.0, 1080.0);
        assert!(
            !places
                .iter()
                .any(|p| matches!(p.contenu, Contenu::Decision)),
            "le centre reste au champ tant qu'aucune décision n'attend"
        );
    }
}
