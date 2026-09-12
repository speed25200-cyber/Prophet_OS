//! Rend la surface d'observation, dans une image.
//!
//! La fenêtre viendra ensuite ; ce binaire existe d'abord pour que la surface soit *regardable*
//! sans écran — en intégration continue, dans une revue, dans un rapport. Une interface qu'on ne
//! peut montrer qu'en la faisant tourner ne se discute pas et ne se vérifie pas.

use std::process::ExitCode;

use surface::fenetre::{Reponse, Source};
use surface::gpu::{Cible, Contexte};
use surface::rendu::Rendu;
use surface::scene::{Courant, Decision, Etat, Isolation, Scene};

fn usage() {
    eprintln!(
        "Usage : prophet-surface [--capture FICHIER.png] [options]

Sans --capture, ouvre la surface en plein écran sur l'état réel du système. C'est ainsi que
Prophet OS l'affiche. Les tâches viennent d'agentd, les décisions de capd, l'isolation de
sandboxd ; si l'un ne répond pas, sa part reste vide plutôt qu'inventée.

  --capture FICHIER    écrire une image au lieu d'ouvrir la fenêtre
  --largeur N          défaut 1920
  --hauteur N          défaut 1080
  --temps SECONDES     instant du champ ; la même valeur donne toujours la même image (défaut 8)
  --decision           montrer l'état où une décision attend un humain
  --demonstration      montrer une scène d'exemple au lieu du système réel
  --aide               ce message"
    );
}

fn main() -> ExitCode {
    let mut fichier = None;
    let mut largeur = 1920u32;
    let mut hauteur = 1080u32;
    let mut temps = 8.0f32;
    let mut decision = false;
    let mut demonstration_demandee = false;

    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--capture" => fichier = arguments.next(),
            "--largeur" => {
                largeur = arguments
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1920)
            }
            "--hauteur" => {
                hauteur = arguments
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1080)
            }
            "--temps" => temps = arguments.next().and_then(|v| v.parse().ok()).unwrap_or(8.0),
            "--decision" => decision = true,
            "--demonstration" => demonstration_demandee = true,
            "--aide" | "-h" => {
                usage();
                return ExitCode::SUCCESS;
            }
            autre => {
                eprintln!("argument inconnu : {autre}");
                usage();
                return ExitCode::FAILURE;
            }
        }
    }

    let Some(fichier) = fichier else {
        // Le mode ordinaire : la surface occupe l'écran, et n'en sort pas.
        //
        // Elle montre le système, pas une démonstration. Si les daemons ne répondent pas, le champ
        // reste vide et la ligne d'isolation dit pourquoi — parce qu'une interface d'observation
        // qui invente ce qu'elle affiche est pire qu'une interface absente : elle a l'air de dire
        // quelque chose. La démonstration reste accessible par `--demonstration`, pour une capture
        // ou une revue.
        let source: Box<dyn Source> = if demonstration_demandee {
            Box::new(Demonstration {
                avec_decision: decision,
            })
        } else {
            Box::new(surface::reel::Reel::demarrer(
                surface::reel::Sockets::default(),
            ))
        };
        return match surface::fenetre::tenir(source) {
            Ok(()) => ExitCode::SUCCESS,
            Err(erreur) => {
                eprintln!("surface impossible à tenir : {erreur}");
                ExitCode::FAILURE
            }
        };
    };

    match capturer(&fichier, largeur, hauteur, temps, decision) {
        Ok(adaptateur) => {
            println!("{fichier} — {largeur}×{hauteur}, rendu par {adaptateur}");
            ExitCode::SUCCESS
        }
        Err(erreur) => {
            eprintln!("rendu impossible : {erreur}");
            ExitCode::FAILURE
        }
    }
}

fn capturer(
    fichier: &str,
    largeur: u32,
    hauteur: u32,
    temps: f32,
    avec_decision: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let contexte = Contexte::hors_ecran()?;
    let mut rendu = Rendu::nouveau(&contexte)?;
    let cible = Cible::nouvelle(&contexte, largeur, hauteur);

    let mut scene = demonstration(avec_decision);
    scene.ordonner();
    rendu.dessiner(&contexte, &cible, &scene, temps)?;

    let pixels = cible.pixels(&contexte)?;
    ecrire_png(fichier, largeur, hauteur, &pixels)?;
    Ok(contexte.adaptateur.clone())
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
