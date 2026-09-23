//! Suite de tâches mesurables.
//!
//! Une suite de tâches n'a de valeur que si chacune se **vérifie automatiquement** : sans
//! vérificateur, on mesure la vitesse d'un agent, pas sa réussite. Chaque tâche porte donc, avec
//! son énoncé, la fonction qui dit si le résultat est bon.
//!
//! Les tâches sont volontairement ordinaires. Le but n'est pas de piéger un agent, mais de
//! mesurer ce qu'un utilisateur fait vraiment de sa machine.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Famille d'une tâche, pour équilibrer la suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    /// Lecture, recherche, résumé de fichiers.
    Files,
    /// Production d'un document.
    Authoring,
    /// Extraction et calcul sur des données.
    Data,
    /// Navigation et formulaires.
    Web,
    /// Enchaînement de plusieurs des précédentes.
    Composite,
}

/// Ce que la tâche exige du système pour être seulement tentée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Requires {
    /// Rien d'autre que le système de fichiers.
    Nothing,
    /// Un navigateur.
    Browser,
    /// Une sortie réseau.
    Network,
}

/// Une tâche de la suite.
pub struct Task {
    /// Identifiant court.
    pub id: &'static str,
    /// Famille.
    pub family: Family,
    /// Ce qu'il faut pour la tenter.
    pub requires: Requires,
    /// Énoncé, tel qu'un utilisateur l'écrirait.
    pub intent: &'static str,
    /// Le dossier du répertoire personnel où la tâche travaille (`notes` pour `~/notes`) : la
    /// portée qu'une mission reçoit, rien au-delà.
    pub root: &'static str,
    /// Prépare l'environnement de la tâche dans un répertoire personnel neuf.
    pub setup: fn(&Path) -> std::io::Result<()>,
    /// Dit si le résultat est correct.
    pub verify: fn(&Path) -> Result<(), String>,
}

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Task")
            .field("id", &self.id)
            .field("family", &self.family)
            .finish_non_exhaustive()
    }
}

fn ecrire(home: &Path, relatif: &str, contenu: &str) -> std::io::Result<()> {
    let chemin = home.join(relatif);
    if let Some(parent) = chemin.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(chemin, contenu)
}

fn lire(home: &Path, relatif: &str) -> Result<String, String> {
    std::fs::read_to_string(home.join(relatif))
        .map_err(|_| format!("fichier attendu absent : {relatif}"))
}

/// La suite.
#[must_use]
pub fn suite() -> Vec<Task> {
    vec![
        Task {
            id: "compter-lignes",
            root: "notes",
            family: Family::Files,
            requires: Requires::Nothing,
            intent: "combien de lignes contiennent les fichiers de ~/notes ? écris le total dans ~/notes/out/total.txt",
            setup: |home| {
                ecrire(home, "notes/a.txt", "une\ndeux\ntrois\n")?;
                ecrire(home, "notes/b.txt", "quatre\ncinq\n")
            },
            verify: |home| {
                let contenu = lire(home, "notes/out/total.txt")?;
                if contenu.contains('5') {
                    Ok(())
                } else {
                    Err(format!("total attendu 5, trouvé : {}", contenu.trim()))
                }
            },
        },
        Task {
            id: "trouver-le-contrat",
            root: "documents",
            family: Family::Files,
            requires: Requires::Nothing,
            intent: "retrouve le fichier qui mentionne la référence ZX-99417 dans ~/documents et note son nom dans ~/documents/out/trouve.txt",
            setup: |home| {
                for i in 0..12 {
                    ecrire(
                        home,
                        &format!("documents/note{i}.md"),
                        &format!("note ordinaire {i}"),
                    )?;
                }
                ecrire(
                    home,
                    "documents/contrat-mars.md",
                    "référence ZX-99417, signé en mars",
                )
            },
            verify: |home| {
                let contenu = lire(home, "documents/out/trouve.txt")?;
                if contenu.contains("contrat-mars") {
                    Ok(())
                } else {
                    Err(format!("fichier attendu non nommé : {}", contenu.trim()))
                }
            },
        },
        Task {
            id: "total-des-ventes",
            root: "ventes",
            family: Family::Data,
            requires: Requires::Nothing,
            intent: "calcule le total de la colonne montant de ~/ventes/q3.csv et écris-le dans ~/ventes/out/total.txt",
            setup: |home| {
                ecrire(
                    home,
                    "ventes/q3.csv",
                    "produit,montant\nA,100\nB,250\nC,75\n",
                )
            },
            verify: |home| {
                let contenu = lire(home, "ventes/out/total.txt")?;
                if contenu.contains("425") {
                    Ok(())
                } else {
                    Err(format!("total attendu 425, trouvé : {}", contenu.trim()))
                }
            },
        },
        Task {
            id: "rapport-trimestriel",
            root: "ventes",
            family: Family::Authoring,
            requires: Requires::Nothing,
            intent: "rédige un résumé des ventes de ~/ventes dans ~/ventes/out/resume.md, avec un titre et le total",
            setup: |home| ecrire(home, "ventes/q3.csv", "produit,montant\nA,100\nB,250\n"),
            verify: |home| {
                let contenu = lire(home, "ventes/out/resume.md")?;
                if !contenu.starts_with('#') {
                    return Err("le résumé doit commencer par un titre".to_owned());
                }
                if !contenu.contains("350") {
                    return Err("le total 350 n'apparaît pas".to_owned());
                }
                Ok(())
            },
        },
        Task {
            id: "ranger-par-annee",
            root: "compta",
            family: Family::Files,
            requires: Requires::Nothing,
            intent: "range les factures de ~/compta dans des sous-dossiers par année, sous ~/compta/out",
            setup: |home| {
                ecrire(home, "compta/facture-2025-03.pdf", "x")?;
                ecrire(home, "compta/facture-2025-09.pdf", "x")?;
                ecrire(home, "compta/facture-2026-01.pdf", "x")
            },
            verify: |home| {
                for (annee, attendu) in [("2025", 2), ("2026", 1)] {
                    let dossier = home.join(format!("compta/out/{annee}"));
                    let trouve = std::fs::read_dir(&dossier)
                        .map_err(|_| format!("dossier attendu absent : compta/out/{annee}"))?
                        .count();
                    if trouve != attendu {
                        return Err(format!(
                            "{annee} : {attendu} fichiers attendus, {trouve} trouvés"
                        ));
                    }
                }
                Ok(())
            },
        },
        Task {
            id: "reserver-un-billet",
            root: "out",
            family: Family::Web,
            requires: Requires::Browser,
            intent: "réserve un billet de Paris à Lyon pour demain, en première classe",
            setup: |_home| Ok(()),
            verify: |home| {
                // Le vérificateur lit la trace laissée par le serveur de test.
                let contenu = lire(home, "out/reservation.txt")?;
                if contenu.contains("Paris")
                    && contenu.contains("Lyon")
                    && contenu.contains("premiere")
                {
                    Ok(())
                } else {
                    Err(format!("réservation incomplète : {contenu}"))
                }
            },
        },
        Task {
            id: "extraire-et-resumer",
            root: "projet",
            family: Family::Composite,
            requires: Requires::Nothing,
            intent: "lis les notes de ~/projet, extrais les décisions et écris-les dans ~/projet/out/decisions.md",
            setup: |home| {
                ecrire(
                    home,
                    "projet/reunion-1.md",
                    "Discussion.\nDÉCISION : reporter la sortie à mars.\n",
                )?;
                ecrire(
                    home,
                    "projet/reunion-2.md",
                    "Points divers.\nDÉCISION : embaucher un designer.\n",
                )?;
                ecrire(home, "projet/reunion-3.md", "Rien de notable.\n")
            },
            verify: |home| {
                let contenu = lire(home, "projet/out/decisions.md")?;
                for attendu in ["mars", "designer"] {
                    if !contenu.contains(attendu) {
                        return Err(format!("décision manquante : {attendu}"));
                    }
                }
                Ok(())
            },
        },
        Task {
            id: "ne-pas-toucher-au-reste",
            root: "ventes",
            family: Family::Composite,
            requires: Requires::Nothing,
            intent: "résume ~/ventes dans ~/ventes/out/resume.md sans rien modifier d'autre",
            setup: |home| {
                ecrire(home, "ventes/q3.csv", "produit,montant\nA,100\n")?;
                ecrire(home, "prive/journal.txt", "contenu à ne pas toucher")
            },
            verify: |home| {
                lire(home, "ventes/out/resume.md")?;
                // La tâche est réussie seulement si rien d'autre n'a bougé : une tâche qui
                // atteint son but en abîmant le reste est un échec.
                if lire(home, "prive/journal.txt")? != "contenu à ne pas toucher" {
                    return Err("un fichier hors périmètre a été modifié".to_owned());
                }
                if lire(home, "ventes/q3.csv")? != "produit,montant\nA,100\n" {
                    return Err("le fichier source a été modifié".to_owned());
                }
                Ok(())
            },
        },
        Task {
            id: "compter-les-erreurs",
            root: "journaux",
            family: Family::Data,
            requires: Requires::Nothing,
            intent: "combien de lignes de ~/journaux/app.log sont des erreurs (ERROR) ? écris le nombre dans ~/journaux/out/erreurs.txt",
            setup: |home| {
                ecrire(
                    home,
                    "journaux/app.log",
                    "INFO démarrage\nERROR base injoignable\nINFO nouvel essai\nWARN lent\nERROR délai dépassé\nINFO reprise\nERROR disque plein\nINFO arrêt\n",
                )
            },
            verify: |home| {
                let contenu = lire(home, "journaux/out/erreurs.txt")?;
                match premier_nombre(&contenu) {
                    Some(3) => Ok(()),
                    autre => Err(format!("3 erreurs attendues, trouvé : {autre:?}")),
                }
            },
        },
        Task {
            id: "le-plus-gros-achat",
            root: "achats",
            family: Family::Data,
            requires: Requires::Nothing,
            intent: "quel fournisseur a reçu le plus gros achat dans ~/achats/2026.csv ? écris son nom dans ~/achats/out/fournisseur.txt",
            setup: |home| {
                ecrire(
                    home,
                    "achats/2026.csv",
                    "date,fournisseur,montant\n2026-01-04,Lumen,1200\n2026-02-11,Brillant,4800\n2026-03-02,Lumen,950\n2026-04-19,Caravelle,3100\n",
                )
            },
            verify: |home| {
                let contenu = lire(home, "achats/out/fournisseur.txt")?;
                if contenu.contains("Brillant") && !contenu.contains("Caravelle") {
                    Ok(())
                } else {
                    Err(format!("Brillant attendu, trouvé : {}", contenu.trim()))
                }
            },
        },
        Task {
            id: "corriger-une-faute",
            root: "lettres",
            family: Family::Authoring,
            requires: Requires::Nothing,
            intent: "corrige la faute « sincèrment » en « sincèrement » dans ~/lettres/candidature.txt, sans rien changer d'autre",
            setup: |home| {
                ecrire(
                    home,
                    "lettres/candidature.txt",
                    "Madame, Monsieur,\n\nJe vous adresse ma candidature au poste proposé.\n\nJe vous prie d'agréer, sincèrment, mes salutations.\n",
                )
            },
            verify: |home| {
                let attendu = "Madame, Monsieur,\n\nJe vous adresse ma candidature au poste proposé.\n\nJe vous prie d'agréer, sincèrement, mes salutations.\n";
                let contenu = lire(home, "lettres/candidature.txt")?;
                if contenu.trim_end() == attendu.trim_end() {
                    Ok(())
                } else if contenu.contains("sincèrment") {
                    Err("la faute est toujours là".to_owned())
                } else {
                    Err("la lettre a changé au-delà de la faute".to_owned())
                }
            },
        },
        Task {
            id: "tableau-de-l-equipe",
            root: "equipe",
            family: Family::Authoring,
            requires: Requires::Nothing,
            intent: "transforme ~/equipe/membres.csv en tableau Markdown dans ~/equipe/out/membres.md",
            setup: |home| {
                ecrire(
                    home,
                    "equipe/membres.csv",
                    "nom,rôle\nAda,développement\nLin,design\nSam,support\n",
                )
            },
            verify: |home| {
                let contenu = lire(home, "equipe/out/membres.md")?;
                let lignes: Vec<&str> = contenu.lines().filter(|l| l.contains('|')).collect();
                if !lignes.iter().any(|l| l.contains("---")) {
                    return Err("pas de ligne de séparation d'en-tête".to_owned());
                }
                for (nom, role) in [
                    ("Ada", "développement"),
                    ("Lin", "design"),
                    ("Sam", "support"),
                ] {
                    if !lignes.iter().any(|l| l.contains(nom) && l.contains(role)) {
                        return Err(format!("{nom} et son rôle ne sont pas sur une même ligne"));
                    }
                }
                Ok(())
            },
        },
        Task {
            id: "changer-le-port",
            root: "service",
            family: Family::Files,
            requires: Requires::Nothing,
            intent: "passe le port de ~/service/config.toml à 9090, sans toucher au reste du fichier",
            setup: |home| {
                ecrire(
                    home,
                    "service/config.toml",
                    "host = \"0.0.0.0\"\nport = 8080\nworkers = 4\n",
                )
            },
            verify: |home| {
                let contenu = lire(home, "service/config.toml")?;
                if contenu.trim_end() == "host = \"0.0.0.0\"\nport = 9090\nworkers = 4" {
                    Ok(())
                } else if contenu.contains("8080") {
                    Err("le port n'a pas changé".to_owned())
                } else {
                    Err(format!("le fichier a changé au-delà du port : {contenu}"))
                }
            },
        },
        Task {
            id: "fusionner-les-listes",
            root: "courses",
            family: Family::Composite,
            requires: Requires::Nothing,
            intent: "fusionne les listes de ~/courses en une seule, sans doublons, par ordre alphabétique, un article par ligne, dans ~/courses/out/liste.txt",
            setup: |home| {
                ecrire(home, "courses/lundi.txt", "pain\nlait\noeufs\n")?;
                ecrire(home, "courses/mardi.txt", "lait\npommes\npain\n")
            },
            verify: |home| {
                let contenu = lire(home, "courses/out/liste.txt")?;
                let articles: Vec<String> = contenu
                    .lines()
                    .map(|l| {
                        l.trim()
                            .trim_start_matches(['-', '*', ' '])
                            .trim()
                            .to_lowercase()
                    })
                    .filter(|l| !l.is_empty())
                    .collect();
                if articles == ["lait", "oeufs", "pain", "pommes"] {
                    Ok(())
                } else {
                    Err(format!(
                        "lait, oeufs, pain, pommes attendus, trouvé : {articles:?}"
                    ))
                }
            },
        },
        Task {
            id: "extraire-les-adresses",
            root: "contacts",
            family: Family::Data,
            requires: Requires::Nothing,
            intent: "extrais les adresses électroniques de ~/contacts/notes.txt, une par ligne, dans ~/contacts/out/adresses.txt",
            setup: |home| {
                ecrire(
                    home,
                    "contacts/notes.txt",
                    "Réunion avec Claire (claire.martin@exemple.fr) et Paul.\nPaul préfère paul@atelier.test pour les devis.\nLe support répond à aide@service.example, pas au standard.\n",
                )
            },
            verify: |home| {
                let contenu = lire(home, "contacts/out/adresses.txt")?;
                let lignes: Vec<&str> = contenu
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .collect();
                for adresse in [
                    "claire.martin@exemple.fr",
                    "paul@atelier.test",
                    "aide@service.example",
                ] {
                    if !lignes.iter().any(|l| l.contains(adresse)) {
                        return Err(format!("adresse manquante : {adresse}"));
                    }
                }
                if lignes.len() == 3 {
                    Ok(())
                } else {
                    Err(format!("3 lignes attendues, {} trouvées", lignes.len()))
                }
            },
        },
        Task {
            id: "la-derniere-sauvegarde",
            root: "sauvegardes",
            family: Family::Files,
            requires: Requires::Nothing,
            intent: "quelle est la sauvegarde la plus récente dans ~/sauvegardes ? écris le nom de son fichier dans ~/sauvegardes/out/derniere.txt",
            setup: |home| {
                for date in ["2026-03-01", "2026-09-12", "2025-12-24", "2026-06-30"] {
                    ecrire(home, &format!("sauvegardes/sauvegarde-{date}.tar"), "x")?;
                }
                Ok(())
            },
            verify: |home| {
                let contenu = lire(home, "sauvegardes/out/derniere.txt")?;
                if contenu.contains("2026-09-12") && !contenu.contains("2026-06-30") {
                    Ok(())
                } else {
                    Err(format!(
                        "sauvegarde-2026-09-12.tar attendue, trouvé : {}",
                        contenu.trim()
                    ))
                }
            },
        },
    ]
}

/// Le premier nombre entier d'un texte.
fn premier_nombre(texte: &str) -> Option<u64> {
    let debut = texte.find(|c: char| c.is_ascii_digit())?;
    texte[debut..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

/// Résultat de l'exécution d'une tâche.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    /// Tâche.
    pub task: String,
    /// Réussite selon le vérificateur.
    pub passed: bool,
    /// Explication en cas d'échec.
    pub detail: String,
    /// Étapes consommées.
    pub steps: u32,
    /// Tokens consommés.
    pub tokens: u64,
    /// Durée en millisecondes.
    pub duration_ms: u64,
}

/// Rendu d'une série d'exécutions.
#[must_use]
pub fn render(runs: &[Run]) -> String {
    let mut out = format!(
        "{:<26} {:<8} {:>7} {:>9} {:>9}  {}\n",
        "tâche", "issue", "étapes", "tokens", "ms", "détail"
    );
    for run in runs {
        out.push_str(&format!(
            "{:<26} {:<8} {:>7} {:>9} {:>9}  {}\n",
            run.task,
            if run.passed { "réussie" } else { "ÉCHEC" },
            run.steps,
            run.tokens,
            run.duration_ms,
            run.detail
        ));
    }
    let reussies = runs.iter().filter(|r| r.passed).count();
    out.push_str(&format!(
        "\n{reussies} réussies sur {}, soit {:.0} %\n",
        runs.len(),
        if runs.is_empty() {
            0.0
        } else {
            reussies as f64 / runs.len() as f64 * 100.0
        }
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_suite_est_equilibree() {
        let suite = suite();
        assert!(suite.len() >= 16);
        let familles: std::collections::BTreeSet<Family> = suite.iter().map(|t| t.family).collect();
        assert_eq!(
            familles.len(),
            5,
            "les cinq familles doivent être couvertes"
        );
    }

    #[test]
    fn les_identifiants_sont_uniques() {
        let suite = suite();
        let noms: std::collections::BTreeSet<&str> = suite.iter().map(|t| t.id).collect();
        assert_eq!(noms.len(), suite.len());
    }

    #[test]
    fn chaque_preparation_aboutit_et_chaque_verificateur_echoue_sur_un_travail_non_fait() {
        // Un vérificateur qui passerait sans que la tâche soit faite ne mesurerait rien.
        for task in suite() {
            let dir = tempfile::tempdir().unwrap();
            (task.setup)(dir.path()).unwrap_or_else(|e| panic!("{} : {e}", task.id));
            assert!(
                (task.verify)(dir.path()).is_err(),
                "{} : le vérificateur accepte un travail non fait",
                task.id
            );
        }
    }

    #[test]
    fn chaque_tache_ajoutee_accepte_une_solution_correcte() {
        // Un vérificateur qui refuserait le bon travail mesurerait l'échec du banc, pas celui
        // de l'agent.
        let solutions: [(&str, &[(&str, &str)]); 8] = [
            (
                "compter-les-erreurs",
                &[("journaux/out/erreurs.txt", "3 erreurs\n")],
            ),
            (
                "le-plus-gros-achat",
                &[("achats/out/fournisseur.txt", "Brillant\n")],
            ),
            (
                "corriger-une-faute",
                &[(
                    "lettres/candidature.txt",
                    "Madame, Monsieur,\n\nJe vous adresse ma candidature au poste proposé.\n\nJe vous prie d'agréer, sincèrement, mes salutations.\n",
                )],
            ),
            (
                "tableau-de-l-equipe",
                &[(
                    "equipe/out/membres.md",
                    "| nom | rôle |\n|---|---|\n| Ada | développement |\n| Lin | design |\n| Sam | support |\n",
                )],
            ),
            (
                "changer-le-port",
                &[(
                    "service/config.toml",
                    "host = \"0.0.0.0\"\nport = 9090\nworkers = 4\n",
                )],
            ),
            (
                "fusionner-les-listes",
                &[("courses/out/liste.txt", "lait\noeufs\npain\npommes\n")],
            ),
            (
                "extraire-les-adresses",
                &[(
                    "contacts/out/adresses.txt",
                    "claire.martin@exemple.fr\npaul@atelier.test\naide@service.example\n",
                )],
            ),
            (
                "la-derniere-sauvegarde",
                &[(
                    "sauvegardes/out/derniere.txt",
                    "sauvegarde-2026-09-12.tar\n",
                )],
            ),
        ];
        for (id, fichiers) in solutions {
            let task = suite().into_iter().find(|t| t.id == id).unwrap();
            let dir = tempfile::tempdir().unwrap();
            (task.setup)(dir.path()).unwrap();
            for (chemin, contenu) in fichiers {
                ecrire(dir.path(), chemin, contenu).unwrap();
            }
            (task.verify)(dir.path()).unwrap_or_else(|e| panic!("{id} : {e}"));
        }
        assert_eq!(premier_nombre("il y en a 13"), Some(13));
        assert_eq!(premier_nombre("aucune"), None);
    }

    #[test]
    fn un_travail_correct_est_accepte() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let task = suite()
            .into_iter()
            .find(|t| t.id == "total-des-ventes")
            .unwrap();
        (task.setup)(home).unwrap();
        std::fs::create_dir_all(home.join("ventes/out")).unwrap();
        std::fs::write(home.join("ventes/out/total.txt"), "425").unwrap();
        assert!((task.verify)(home).is_ok());
    }

    #[test]
    fn atteindre_le_but_en_abimant_le_reste_est_un_echec() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let task = suite()
            .into_iter()
            .find(|t| t.id == "ne-pas-toucher-au-reste")
            .unwrap();
        (task.setup)(home).unwrap();
        std::fs::create_dir_all(home.join("ventes/out")).unwrap();
        std::fs::write(home.join("ventes/out/resume.md"), "# Résumé").unwrap();
        assert!((task.verify)(home).is_ok());

        // Le but est atteint, mais un fichier hors périmètre a été touché.
        std::fs::write(home.join("prive/journal.txt"), "saccagé").unwrap();
        let erreur = (task.verify)(home).unwrap_err();
        assert!(erreur.contains("hors périmètre"), "{erreur}");
    }

    #[test]
    fn le_rendu_annonce_le_taux_de_reussite() {
        let runs = vec![
            Run {
                task: "a".into(),
                passed: true,
                detail: String::new(),
                steps: 4,
                tokens: 100,
                duration_ms: 500,
            },
            Run {
                task: "b".into(),
                passed: false,
                detail: "total incorrect".into(),
                steps: 9,
                tokens: 400,
                duration_ms: 1500,
            },
        ];
        let rendu = render(&runs);
        assert!(rendu.contains("50 %"), "{rendu}");
        assert!(rendu.contains("ÉCHEC"), "{rendu}");
        assert!(rendu.contains("total incorrect"), "{rendu}");
    }

    #[test]
    fn les_taches_declarent_ce_qu_elles_exigent() {
        let suite = suite();
        let navigateur = suite
            .iter()
            .filter(|t| t.requires == Requires::Browser)
            .count();
        assert!(
            navigateur >= 1,
            "la suite doit contenir au moins une tâche de navigation"
        );
        // Une tâche qui exige un navigateur doit pouvoir être écartée sur une machine qui n'en a
        // pas, plutôt que compter comme un échec de l'agent.
        let sans_dependance = suite
            .iter()
            .filter(|t| t.requires == Requires::Nothing)
            .count();
        assert!(sans_dependance >= 6);
    }
}
