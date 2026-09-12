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
    ]
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
        assert!(suite.len() >= 8);
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
