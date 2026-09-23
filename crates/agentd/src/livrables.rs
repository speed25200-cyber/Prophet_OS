//! Les livrables qu'un objectif nomme, et le rappel quand le modèle conclut sans eux (ADR 0049).
//!
//! Un petit modèle conclut souvent par du texte (« le total est 330 ») sans écrire le fichier
//! que l'objectif demande : la mission réussit, rien n'est produit. Quand l'objectif nomme un
//! chemin `~/…` que la portée de la mission couvre et qui n'existe pas au départ, le service
//! vérifie à chaque conclusion qu'il existe dans l'espace de travail ; sinon, il le rappelle au
//! modèle et le laisse continuer, au plus [`RAPPELS`] fois par mission : un petit modèle
//! annonce souvent l'écriture (« je vais créer le fichier ») au lieu de la faire, et le second
//! rappel le lui redit plus nettement.
//! Le rappel demande d'abord d'écrire — un petit modèle suit une consigne, il décline une
//! condition — puis laisse une issue : un chemin nommé comme une entrée absente se dit au lieu
//! de s'écrire. Il ne donne aucun droit : l'écriture passe par le même outil, le même jeton et
//! la même politique.
use std::path::{Component, Path, PathBuf};

use std::sync::Mutex;

use providers::DriverError;
use providers::native::{ModelClient, ModelTurn, ToolExecutor, Usage};
use serde_json::{Value, json};

/// Rappels au plus par mission : au-delà, la conclusion du modèle est rendue telle quelle.
pub const RAPPELS: u8 = 2;

/// Un chemin que l'objectif nomme, tel qu'il l'écrit (`~/ventes/out/total.txt`), et le chemin
/// réel qu'il désigne.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nomme {
    /// Tel que l'objectif l'écrit, sans ponctuation finale.
    pub tel_quel: String,
    /// Chemin réel sous le répertoire personnel.
    pub reel: PathBuf,
}

/// Les chemins `~/…` qu'un objectif nomme, dans l'ordre, sans doublon. Une ponctuation collée
/// (virgule, point, guillemet, parenthèse) ne fait pas partie du chemin ; un chemin qui remonte
/// (`..`) n'est pas retenu.
#[must_use]
pub fn nommes(intent: &str, home: &Path) -> Vec<Nomme> {
    let mut vus: Vec<Nomme> = Vec::new();
    for mot in intent.split_whitespace() {
        let Some(debut) = mot.find("~/") else {
            continue;
        };
        let brut = mot[debut..].trim_end_matches(|c: char| {
            matches!(
                c,
                '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '»' | '"' | '\'' | '`' | '’'
            )
        });
        let reste = brut["~/".len()..].trim_end_matches('/');
        if reste.is_empty() {
            continue;
        }
        let relatif = Path::new(reste);
        if !relatif
            .components()
            .all(|composant| matches!(composant, Component::Normal(_)))
        {
            continue;
        }
        let nomme = Nomme {
            tel_quel: format!("~/{reste}"),
            reel: home.join(relatif),
        };
        if !vus.contains(&nomme) {
            vus.push(nomme);
        }
    }
    vus
}

/// Un livrable attendu : ce que l'objectif nomme et où l'espace de travail le recevra.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attendu {
    /// Tel que l'objectif l'écrit.
    pub tel_quel: String,
    /// Chemin dans l'espace de travail de la mission.
    pub travail: PathBuf,
}

/// Les livrables attendus : les chemins nommés que `vers_travail` traduit (la portée les
/// couvre) et qui n'existent pas encore dans l'espace de travail. Un fichier qui existe déjà
/// est une entrée, pas un livrable.
pub fn attendus(nommes: &[Nomme], vers_travail: impl Fn(&Path) -> Option<PathBuf>) -> Vec<Attendu> {
    nommes
        .iter()
        .filter_map(|nomme| {
            let travail = vers_travail(&nomme.reel)?;
            (!travail.exists()).then(|| Attendu {
                tel_quel: nomme.tel_quel.clone(),
                travail,
            })
        })
        .collect()
}

/// Les entrées que l'objectif nomme : les chemins nommés que la portée couvre et qui existent
/// au départ dans l'espace de travail — ce que la mission doit lire. Rendus relatifs au
/// répertoire personnel (`ventes`, `ventes/q3.csv`).
pub fn entrees(nommes: &[Nomme], vers_travail: impl Fn(&Path) -> Option<PathBuf>) -> Vec<String> {
    nommes
        .iter()
        .filter(|nomme| vers_travail(&nomme.reel).is_some_and(|travail| travail.exists()))
        .map(|nomme| nomme.tel_quel.trim_start_matches("~/").to_owned())
        .collect()
}

/// Le chemin qu'un appel d'outil vise, relatif au répertoire personnel, s'il en nomme un.
fn visee(arguments: &Value) -> Option<String> {
    let brut = arguments
        .get("path")
        .or_else(|| arguments.get("root"))?
        .as_str()?;
    Some(
        brut.trim_start_matches("~/")
            .trim_end_matches('/')
            .to_owned(),
    )
}

/// Vrai si `chemin` est `entree` ou lui appartient (composant par composant).
fn sous(chemin: &str, entree: &str) -> bool {
    Path::new(chemin).starts_with(Path::new(entree))
}

/// L'exécuteur d'une mission, vu à travers les entrées que l'objectif nomme.
///
/// Un petit modèle écrit souvent le livrable sans avoir lu ce qu'il devait résumer (« Total des
/// ventes : 0 € »). Quand la mission écrit pour la première fois alors qu'aucune lecture n'a
/// encore porté sur une entrée nommée, le résultat de l'écriture — réussie, elle — porte une
/// note qui le lui dit, une fois par mission. Rien n'est refusé ni modifié d'autre.
pub struct Lecture {
    inner: Box<dyn ToolExecutor>,
    entrees: Vec<String>,
    /// Une entrée a été lue ; la note a été donnée.
    etat: Mutex<(bool, bool)>,
}

impl Lecture {
    /// Enveloppe l'exécuteur ; sans entrée nommée, les appels passent tels quels.
    #[must_use]
    pub fn new(inner: Box<dyn ToolExecutor>, entrees: Vec<String>) -> Self {
        Self {
            inner,
            entrees,
            etat: Mutex::new((false, false)),
        }
    }
}

impl ToolExecutor for Lecture {
    fn call(&self, tool: &str, arguments: &Value) -> (bool, Value) {
        let (ok, mut result) = self.inner.call(tool, arguments);
        if !ok || self.entrees.is_empty() {
            return (ok, result);
        }
        let Ok(mut etat) = self.etat.lock() else {
            return (ok, result);
        };
        let cible = visee(arguments);
        let sur_entree = cible
            .as_deref()
            .is_some_and(|c| self.entrees.iter().any(|e| sous(c, e)));
        let lit = match tool {
            "fs.read" | "doc.read" | "fs.edit" => true,
            // Une recherche par contenu lit ; une recherche par nom ne fait que lister.
            "fs.search" => arguments.get("content_contains").is_some(),
            _ => false,
        };
        if lit && sur_entree {
            etat.0 = true;
        }
        if matches!(tool, "fs.write" | "fs.edit") && !etat.0 && !etat.1 {
            etat.1 = true;
            let liste = self
                .entrees
                .iter()
                .map(|e| format!("~/{e}"))
                .collect::<Vec<_>>()
                .join(", ");
            if let Some(objet) = result.as_object_mut() {
                objet.insert(
                    "note".into(),
                    json!(format!(
                        "Vous n'avez encore lu aucun des fichiers que l'objectif nomme ({liste}). \
                         Lisez-les avec fs.read, puis corrigez ce que vous venez d'écrire si besoin."
                    )),
                );
            }
        }
        (ok, result)
    }
}

/// Ce que la mission n'a pas encore produit.
#[must_use]
pub fn manquants(attendus: &[Attendu]) -> Vec<String> {
    attendus
        .iter()
        .filter(|attendu| !attendu.travail.exists())
        .map(|attendu| attendu.tel_quel.clone())
        .collect()
}

/// Le message que le modèle reçoit quand il conclut alors qu'un chemin que l'objectif nomme
/// n'existe toujours pas. La consigne vient d'abord : au banc, un rappel qui commençait par une
/// condition (« s'il vous revient de le produire ») a été décliné treize fois sur vingt-neuf,
/// un rappel impératif jamais (ADR 0049). L'issue vient ensuite : le service ne sait pas si
/// l'objectif demandait ce chemin ou le nommait comme une entrée absente.
#[must_use]
pub fn rappel(manquants: &[String], rang: u8) -> String {
    if rang > 1 {
        let liste = manquants.join(", ");
        return format!(
            "Vous n'avez toujours pas écrit {liste} : appelez maintenant l'outil d'écriture de \
             fichiers avec son contenu, au lieu de l'annoncer. S'il s'agissait d'un fichier à \
             lire, répondez seulement qu'il est absent."
        );
    }
    match manquants {
        [seul] => format!(
            "Vous n'avez pas encore écrit {seul}. Écrivez-le avec l'outil d'écriture de \
             fichiers, puis concluez. Si l'objectif le nommait comme un fichier à lire, dites \
             qu'il est absent au lieu de l'écrire."
        ),
        _ => format!(
            "Vous n'avez pas encore écrit {}. Écrivez-les avec l'outil d'écriture de \
             fichiers, puis concluez. Si l'objectif les nommait comme des fichiers à lire, \
             dites qu'ils sont absents au lieu de les écrire.",
            manquants.join(", ")
        ),
    }
}

/// Ce que le service fait d'un rappel avant de l'envoyer : le journaliser. Une erreur arrête
/// la mission.
pub type Consigner = Box<dyn Fn(&[String], u8) -> Result<(), String> + Send + Sync>;

/// Le modèle, vu à travers les livrables de l'objectif.
///
/// Quand le modèle conclut alors qu'un livrable manque, sa conclusion et le rappel s'insèrent
/// dans l'historique qu'il reçoit, et il est interrogé de nouveau ; chaque interrogation passe
/// par le client enveloppé, donc est comptée comme une étape et en tokens. L'historique de la
/// boucle n'est pas modifié : les insertions sont rejouées à leur place à chaque tour suivant.
pub struct Rappel {
    inner: Box<dyn ModelClient>,
    attendus: Vec<Attendu>,
    consigner: Consigner,
    /// Insertions déjà faites : position dans l'historique de la boucle, messages insérés.
    inserts: Vec<(usize, [Value; 2])>,
    restants: u8,
}

impl Rappel {
    /// Enveloppe `inner` ; sans livrable attendu, les tours passent tels quels.
    #[must_use]
    pub fn new(inner: Box<dyn ModelClient>, attendus: Vec<Attendu>, consigner: Consigner) -> Self {
        Self {
            inner,
            attendus,
            consigner,
            inserts: Vec::new(),
            restants: RAPPELS,
        }
    }

    /// Rappels déjà faits.
    #[must_use]
    pub fn faits(&self) -> usize {
        self.inserts.len()
    }

    fn vue(&self, history: &[Value]) -> Vec<Value> {
        let mut vue = Vec::with_capacity(history.len() + 2 * self.inserts.len());
        let mut depuis = 0;
        for (position, messages) in &self.inserts {
            let position = (*position).min(history.len());
            vue.extend_from_slice(&history[depuis..position]);
            vue.extend(messages.iter().cloned());
            depuis = position;
        }
        vue.extend_from_slice(&history[depuis..]);
        vue
    }
}

impl ModelClient for Rappel {
    fn model_name(&self) -> String {
        self.inner.model_name()
    }

    fn next_turn(&mut self, history: &[Value]) -> Result<(ModelTurn, Usage), DriverError> {
        let mut total = Usage::default();
        loop {
            let vue = self.vue(history);
            let (turn, usage) = self.inner.next_turn(&vue)?;
            total.tokens_in = total.tokens_in.saturating_add(usage.tokens_in);
            total.tokens_out = total.tokens_out.saturating_add(usage.tokens_out);
            let ModelTurn::Final { text } = &turn else {
                return Ok((turn, total));
            };
            let manquants = manquants(&self.attendus);
            if manquants.is_empty() || self.restants == 0 {
                return Ok((turn, total));
            }
            self.restants -= 1;
            let rang = RAPPELS - self.restants;
            (self.consigner)(&manquants, rang).map_err(DriverError::Io)?;
            self.inserts.push((
                history.len(),
                [
                    json!({"role": "assistant", "content": text}),
                    json!({"role": "user", "content": rappel(&manquants, rang)}),
                ],
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use super::*;

    #[test]
    fn l_objectif_nomme_ses_chemins_sans_la_ponctuation() {
        let home = Path::new("/home/p");
        let vus = nommes(
            "calcule le total de ~/ventes/q3.csv et écris-le dans ~/ventes/out/total.txt, \
             puis range (~/compta/out/) « ~/a/b.md » ; ~/ventes/q3.csv encore, ~/../etc/passwd ~/",
            home,
        );
        let tels: Vec<&str> = vus.iter().map(|n| n.tel_quel.as_str()).collect();
        assert_eq!(
            tels,
            [
                "~/ventes/q3.csv",
                "~/ventes/out/total.txt",
                "~/compta/out",
                "~/a/b.md"
            ]
        );
        assert_eq!(vus[1].reel, Path::new("/home/p/ventes/out/total.txt"));
    }

    #[test]
    fn seul_ce_qui_manque_dans_la_portee_est_attendu() {
        let dossier = tempfile::tempdir().expect("dossier");
        let travail = dossier.path().join("work");
        std::fs::create_dir_all(travail.join("ventes")).expect("ventes");
        std::fs::write(travail.join("ventes/q3.csv"), "montant\n1\n").expect("csv");
        let home = Path::new("/home/p");
        let vus = nommes(
            "total de ~/ventes/q3.csv dans ~/ventes/out/total.txt, pas ~/ailleurs/x.txt",
            home,
        );
        let attendus = attendus(&vus, |reel| {
            let relatif = reel.strip_prefix(home).ok()?;
            relatif.starts_with("ventes").then(|| travail.join(relatif))
        });
        assert_eq!(attendus.len(), 1);
        assert_eq!(attendus[0].tel_quel, "~/ventes/out/total.txt");
        assert_eq!(manquants(&attendus), ["~/ventes/out/total.txt"]);
        std::fs::create_dir_all(travail.join("ventes/out")).expect("out");
        std::fs::write(travail.join("ventes/out/total.txt"), "2").expect("total");
        assert!(manquants(&attendus).is_empty());
    }

    /// Un modèle scripté qui garde les historiques reçus et, s'il le faut, écrit le livrable
    /// quand il appelle l'outil.
    struct Script {
        tours: VecDeque<ModelTurn>,
        vus: Arc<Mutex<Vec<Vec<Value>>>>,
    }
    impl ModelClient for Script {
        fn next_turn(&mut self, history: &[Value]) -> Result<(ModelTurn, Usage), DriverError> {
            self.vus.lock().expect("vus").push(history.to_vec());
            let tour = self.tours.pop_front().expect("tour prévu");
            Ok((
                tour,
                Usage {
                    tokens_in: 10,
                    tokens_out: 2,
                },
            ))
        }
        fn model_name(&self) -> String {
            "script".into()
        }
    }

    fn final_(text: &str) -> ModelTurn {
        ModelTurn::Final { text: text.into() }
    }

    #[test]
    fn une_conclusion_sans_livrable_est_rappelee_puis_rejouee_a_sa_place() {
        let dossier = tempfile::tempdir().expect("dossier");
        let livrable = dossier.path().join("total.txt");
        let vus = Arc::new(Mutex::new(Vec::new()));
        let consignes = Arc::new(Mutex::new(Vec::new()));
        let journal = consignes.clone();
        let mut modele = Rappel::new(
            Box::new(Script {
                tours: VecDeque::from([
                    final_("Le total est 330."),
                    ModelTurn::ToolCall {
                        tool: "fs.write".into(),
                        arguments: json!({"path": "~/ventes/out/total.txt"}),
                    },
                    final_("C'est écrit."),
                ]),
                vus: vus.clone(),
            }),
            vec![Attendu {
                tel_quel: "~/ventes/out/total.txt".into(),
                travail: livrable.clone(),
            }],
            Box::new(move |manquants, rang| {
                journal
                    .lock()
                    .expect("journal")
                    .push((manquants.to_vec(), rang));
                Ok(())
            }),
        );
        let mut historique = vec![json!({"role": "user", "content": "l'objectif"})];

        let (tour, usage) = modele.next_turn(&historique).expect("tour");
        assert!(matches!(tour, ModelTurn::ToolCall { .. }));
        assert_eq!(
            usage.tokens_in, 20,
            "deux interrogations, comptées ensemble"
        );
        assert_eq!(modele.faits(), 1);
        {
            let vus = vus.lock().expect("vus");
            assert_eq!(vus[1].len(), 3);
            assert_eq!(vus[1][1]["content"], "Le total est 330.");
            assert!(
                vus[1][2]["content"]
                    .as_str()
                    .is_some_and(|t| t.contains("~/ventes/out/total.txt"))
            );
        }
        assert_eq!(
            *consignes.lock().expect("consignes"),
            [(vec!["~/ventes/out/total.txt".to_owned()], 1)]
        );

        // La boucle ajoute l'appel et son résultat ; l'outil a écrit le livrable.
        historique.push(json!({"role": "assistant", "tool_call": {"tool": "fs.write"}}));
        historique.push(json!({"role": "tool", "ok": true, "result": {}}));
        std::fs::write(&livrable, "330").expect("livrable");
        let (tour, _) = modele.next_turn(&historique).expect("tour");
        assert!(matches!(tour, ModelTurn::Final { .. }));
        let vus = vus.lock().expect("vus");
        let dernier = &vus[2];
        assert_eq!(dernier.len(), 5, "l'insertion est rejouée à sa place");
        assert_eq!(dernier[0]["content"], "l'objectif");
        assert_eq!(dernier[1]["content"], "Le total est 330.");
        assert_eq!(dernier[2]["role"], "user");
        assert!(dernier[3].get("tool_call").is_some());
        assert_eq!(dernier[4]["role"], "tool");
    }

    #[test]
    fn un_rappel_decline_se_repete_une_fois_plus_nettement() {
        let dossier = tempfile::tempdir().expect("dossier");
        let vus = Arc::new(Mutex::new(Vec::new()));
        let mut modele = Rappel::new(
            Box::new(Script {
                tours: VecDeque::from([
                    final_("Je vais créer le fichier ~/x.txt."),
                    final_("Je vais maintenant l'écrire."),
                    final_("Il fallait le lire : il est absent."),
                ]),
                vus: vus.clone(),
            }),
            vec![Attendu {
                tel_quel: "~/x.txt".into(),
                travail: dossier.path().join("x.txt"),
            }],
            Box::new(|_, _| Ok(())),
        );
        let (tour, usage) = modele
            .next_turn(&[json!({"role": "user", "content": "écris ~/x.txt"})])
            .expect("tour");
        assert!(matches!(tour, ModelTurn::Final { ref text } if text.starts_with("Il fallait")));
        assert_eq!(usage.tokens_out, 6);
        assert_eq!(modele.faits(), 2);
        let vus = vus.lock().expect("vus");
        let second = vus[2].last().expect("rappel")["content"]
            .as_str()
            .expect("texte")
            .to_owned();
        assert!(second.contains("toujours pas écrit ~/x.txt"), "{second}");
    }

    #[test]
    fn les_rappels_sont_bornes() {
        let dossier = tempfile::tempdir().expect("dossier");
        let attendus: Vec<Attendu> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|nom| Attendu {
                tel_quel: format!("~/{nom}.txt"),
                travail: dossier.path().join(format!("{nom}.txt")),
            })
            .collect();
        // Chaque conclusion produit un fichier de plus : le modèle progresse, il en manque
        // toujours d'autres, et le plafond arrête les rappels.
        let chemins: Vec<PathBuf> = attendus.iter().map(|a| a.travail.clone()).collect();
        struct Producteur {
            chemins: Vec<PathBuf>,
            n: usize,
        }
        impl ModelClient for Producteur {
            fn next_turn(&mut self, _: &[Value]) -> Result<(ModelTurn, Usage), DriverError> {
                if let Some(chemin) = self.chemins.get(self.n) {
                    std::fs::write(chemin, "x").expect("écriture");
                }
                self.n += 1;
                Ok((final_(&format!("tour {}", self.n)), Usage::default()))
            }
            fn model_name(&self) -> String {
                "producteur".into()
            }
        }
        let mut modele = Rappel::new(
            Box::new(Producteur { chemins, n: 0 }),
            attendus,
            Box::new(|_, _| Ok(())),
        );
        let (tour, _) = modele
            .next_turn(&[json!({"role": "user", "content": "o"})])
            .expect("tour");
        assert!(matches!(tour, ModelTurn::Final { ref text } if text == "tour 3"));
        assert_eq!(modele.faits(), usize::from(RAPPELS));
    }

    struct Outils;
    impl ToolExecutor for Outils {
        fn call(&self, _: &str, _: &Value) -> (bool, Value) {
            (true, json!({"staged": true}))
        }
    }

    #[test]
    fn les_entrees_sont_les_chemins_nommes_qui_existent() {
        let dossier = tempfile::tempdir().expect("dossier");
        let travail = dossier.path().join("work");
        std::fs::create_dir_all(travail.join("ventes")).expect("ventes");
        let home = Path::new("/home/p");
        let vus = nommes("résume ~/ventes dans ~/ventes/out/resume.md", home);
        let traduire = |reel: &Path| Some(travail.join(reel.strip_prefix(home).ok()?));
        assert_eq!(entrees(&vus, traduire), ["ventes"]);
    }

    #[test]
    fn ecrire_sans_avoir_lu_les_entrees_porte_une_note_une_fois() {
        let lecture = Lecture::new(Box::new(Outils), vec!["ventes".into()]);
        let (_, liste) = lecture.call("fs.search", &json!({"root": "~/ventes"}));
        assert!(liste.get("note").is_none());
        let (ok, ecrit) = lecture.call(
            "fs.write",
            &json!({"path": "~/ventes/out/resume.md", "content": "0 €"}),
        );
        assert!(ok);
        assert!(
            ecrit["note"]
                .as_str()
                .is_some_and(|n| n.contains("~/ventes")),
            "{ecrit}"
        );
        let (_, encore) = lecture.call("fs.write", &json!({"path": "~/ventes/out/resume.md"}));
        assert!(encore.get("note").is_none(), "une fois par mission");
    }

    #[test]
    fn ecrire_apres_avoir_lu_une_entree_passe_tel_quel() {
        let lecture = Lecture::new(Box::new(Outils), vec!["ventes".into()]);
        lecture.call("fs.read", &json!({"path": "~/ventes/q3.csv"}));
        let (_, ecrit) = lecture.call("fs.write", &json!({"path": "~/ventes/out/resume.md"}));
        assert!(ecrit.get("note").is_none(), "{ecrit}");
        // Lire ailleurs ne compte pas ; chercher par contenu, si.
        let lecture = Lecture::new(Box::new(Outils), vec!["ventes".into()]);
        lecture.call("fs.read", &json!({"path": "~/ventesbis/a.txt"}));
        lecture.call(
            "fs.search",
            &json!({"root": "~/ventes", "content_contains": "total"}),
        );
        let (_, ecrit) = lecture.call("fs.write", &json!({"path": "~/ventes/out/resume.md"}));
        assert!(ecrit.get("note").is_none(), "{ecrit}");
    }

    #[test]
    fn sans_livrable_attendu_le_tour_passe_tel_quel() {
        let mut modele = Rappel::new(
            Box::new(Script {
                tours: VecDeque::from([final_("fini")]),
                vus: Arc::new(Mutex::new(Vec::new())),
            }),
            Vec::new(),
            Box::new(|_, _| Err("jamais appelé".into())),
        );
        let (tour, _) = modele.next_turn(&[]).expect("tour");
        assert!(matches!(tour, ModelTurn::Final { .. }));
        assert_eq!(modele.faits(), 0);
    }
}
