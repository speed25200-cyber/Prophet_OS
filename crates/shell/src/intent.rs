//! De l'intention au plan.
//!
//! L'humain écrit une phrase ; le système doit en déduire **le moins de droits possible** pour
//! l'accomplir. C'est le sens de la barre d'intentions : elle ne devine pas ce que l'utilisateur
//! veut dire, elle propose un périmètre et le lui montre avant d'agir.
//!
//! Le principe de conception est l'inverse du réflexe habituel : en cas d'ambiguïté, on propose
//! **moins** de droits et on laisse l'agent en redemander, plutôt que d'accorder large « au cas
//! où ». Un agent qui manque d'un droit le dit ; un agent qui en a trop ne le dit jamais.

use std::collections::BTreeSet;

use prophet_types::cap::{Act, Grant, Res};
use serde::{Deserialize, Serialize};

/// Ce qu'une intention laisse supposer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    /// Périmètre du système de fichiers, en chemins relatifs au répertoire personnel.
    pub scopes: Vec<String>,
    /// Capacités proposées.
    pub grants: Vec<Grant>,
    /// Ce qui reste incertain et mérite une question à l'humain.
    pub questions: Vec<String>,
}

impl Proposal {
    /// Vrai si l'intention n'a rien donné d'exploitable.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.grants.is_empty()
    }
}

/// Verbes qui trahissent une écriture.
const VERBES_ECRITURE: &[&str] = &[
    "écris",
    "ecris",
    "écrire",
    "ecrire",
    "crée",
    "cree",
    "créer",
    "creer",
    "génère",
    "genere",
    "générer",
    "generer",
    "modifie",
    "modifier",
    "enregistre",
    "enregistrer",
    "range",
    "ranger",
    "renomme",
    "renommer",
    "prépare",
    "prepare",
    "préparer",
    "preparer",
    "rédige",
    "redige",
];

/// Verbes qui trahissent une sortie réseau.
const VERBES_RESEAU: &[&str] = &[
    "télécharge",
    "telecharge",
    "récupère",
    "recupere",
    "consulte",
    "consulter",
    "cherche en ligne",
    "navigue",
    "naviguer",
    "ouvre le site",
    "vérifie sur",
    "verifie sur",
];

/// Verbes qui trahissent une action engageante.
const VERBES_ENGAGEANTS: &[&str] = &[
    "envoie",
    "envoyer",
    "publie",
    "publier",
    "poste",
    "poster",
    "commande",
    "commander",
    "réserve",
    "reserve",
    "réserver",
    "reserver",
    "paie",
    "payer",
    "supprime",
    "supprimer",
    "achète",
    "achete",
    "acheter",
];

/// Extrait les chemins mentionnés, sous la forme `~/...` ou `dossier/`.
#[must_use]
pub fn mentioned_paths(intent: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    for mot in intent.split_whitespace() {
        let nettoye = mot.trim_matches(|c: char| {
            !c.is_alphanumeric() && c != '~' && c != '/' && c != '.' && c != '-' && c != '_'
        });
        if nettoye.starts_with("~/") && nettoye.len() > 2 {
            out.insert(nettoye.trim_end_matches('/').to_owned());
        }
    }
    out.into_iter().collect()
}

/// Propose un périmètre et des capacités à partir d'une intention.
///
/// La proposition est volontairement étroite. Chaque élargissement possible devient une
/// **question**, pas un droit accordé en silence.
#[must_use]
pub fn propose(intent: &str) -> Proposal {
    let bas = intent.to_lowercase();
    let contient = |liste: &[&str]| liste.iter().any(|v| bas.contains(v));

    let chemins = mentioned_paths(intent);
    let mut grants = Vec::new();
    let mut questions = Vec::new();
    let mut scopes = Vec::new();

    if chemins.is_empty() {
        questions.push(
            "Aucun dossier n'est nommé dans l'intention. Sur quels fichiers la tâche doit-elle travailler ?"
                .to_owned(),
        );
    } else {
        for chemin in &chemins {
            scopes.push(chemin.clone());
            grants.push(Grant::new(Res::Fs, Act::Read, format!("{chemin}/**")));
            grants.push(Grant::new(Res::Fs, Act::List, format!("{chemin}/**")));
        }
    }

    if contient(VERBES_ECRITURE)
        && let Some(premier) = chemins.first()
    {
        // L'écriture est cantonnée à un sous-dossier de sortie : une tâche qui produit un
        // résultat n'a pas besoin de réécrire ses sources.
        grants.push(Grant::new(Res::Fs, Act::Write, format!("{premier}/out/**")));
        questions.push(format!(
            "L'écriture est limitée à {premier}/out. La tâche doit-elle modifier les fichiers d'origine ?"
        ));
    }

    if contient(VERBES_RESEAU) {
        questions.push(
            "L'intention suppose un accès réseau. Quels domaines la tâche a-t-elle le droit de joindre ?"
                .to_owned(),
        );
    }

    if contient(VERBES_ENGAGEANTS) {
        questions.push(
            "L'intention contient une action engageante. Elle demandera votre approbation au moment de l'exécuter."
                .to_owned(),
        );
    }

    if !grants.is_empty() {
        grants.push(Grant::new(Res::Tool, Act::Call, "fs.*"));
        grants.push(Grant::new(Res::Tool, Act::Call, "task.*"));
    }

    Proposal {
        scopes,
        grants,
        questions,
    }
}

/// Rendu de la proposition, tel que la barre d'intentions l'affiche avant de démarrer.
#[must_use]
pub fn render_proposal(intent: &str, proposal: &Proposal) -> String {
    let mut out = format!("Intention : {intent}\n\n");
    if proposal.is_empty() {
        out.push_str("  Aucune capacité ne peut être déduite de cette intention.\n\n");
    } else {
        out.push_str("  Périmètre :\n");
        for scope in &proposal.scopes {
            out.push_str(&format!("    {scope}\n"));
        }
        out.push_str("  Capacités proposées :\n");
        for grant in &proposal.grants {
            out.push_str(&format!(
                "    {}.{} sur {}\n",
                format!("{:?}", grant.res).to_lowercase(),
                format!("{:?}", grant.act).to_lowercase(),
                grant.pattern
            ));
        }
    }
    if !proposal.questions.is_empty() {
        out.push_str("\n  À confirmer :\n");
        for question in &proposal.questions {
            out.push_str(&format!("    - {question}\n"));
        }
    }
    out.push_str("\n  Entrée pour démarrer · e pour élargir · Échap pour annuler\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_chemins_mentionnes_sont_extraits() {
        assert_eq!(
            mentioned_paths("lis les fichiers de ~/ventes et écris dans ~/rapports/"),
            vec!["~/rapports", "~/ventes"]
        );
        assert!(mentioned_paths("fais quelque chose d'utile").is_empty());
    }

    #[test]
    fn une_lecture_simple_ne_donne_pas_l_ecriture() {
        let proposal = propose("résume les fichiers de ~/ventes");
        assert!(
            !proposal
                .grants
                .iter()
                .any(|g| g.res == Res::Fs && g.act == Act::Write),
            "aucune écriture ne doit être proposée pour un simple résumé"
        );
        assert!(
            proposal
                .grants
                .iter()
                .any(|g| g.res == Res::Fs && g.act == Act::Read)
        );
    }

    #[test]
    fn l_ecriture_est_cantonnee_a_un_dossier_de_sortie() {
        let proposal = propose("écris un rapport à partir de ~/ventes");
        let ecriture: Vec<&Grant> = proposal
            .grants
            .iter()
            .filter(|g| g.act == Act::Write)
            .collect();
        assert_eq!(ecriture.len(), 1);
        assert_eq!(ecriture[0].pattern, "~/ventes/out/**");
        assert!(
            proposal
                .questions
                .iter()
                .any(|q| q.contains("fichiers d'origine")),
            "l'élargissement doit être une question, pas un droit silencieux"
        );
    }

    #[test]
    fn une_intention_sans_dossier_pose_la_question() {
        let proposal = propose("range mes affaires");
        assert!(proposal.is_empty());
        assert!(
            proposal
                .questions
                .iter()
                .any(|q| q.contains("Sur quels fichiers"))
        );
    }

    #[test]
    fn le_reseau_n_est_jamais_accorde_d_office() {
        let proposal = propose("télécharge le rapport et enregistre-le dans ~/ventes");
        assert!(
            !proposal.grants.iter().any(|g| g.res == Res::Net),
            "aucun domaine ne peut être deviné : la question est posée"
        );
        assert!(proposal.questions.iter().any(|q| q.contains("domaines")));
    }

    #[test]
    fn une_action_engageante_est_annoncee_avant_de_commencer() {
        let proposal = propose("envoie le rapport de ~/ventes à Marie");
        assert!(
            proposal.questions.iter().any(|q| q.contains("approbation")),
            "l'humain doit savoir dès le plan qu'on lui redemandera : {:?}",
            proposal.questions
        );
    }

    #[test]
    fn plusieurs_dossiers_donnent_plusieurs_perimetres() {
        let proposal = propose("compare ~/ventes et ~/budget");
        assert_eq!(proposal.scopes.len(), 2);
    }

    #[test]
    fn le_rendu_montre_le_perimetre_et_les_questions() {
        let intention = "écris un rapport à partir de ~/ventes";
        let rendu = render_proposal(intention, &propose(intention));
        assert!(rendu.contains("Périmètre"), "{rendu}");
        assert!(rendu.contains("~/ventes/out/**"), "{rendu}");
        assert!(rendu.contains("À confirmer"), "{rendu}");
        assert!(rendu.contains("Entrée pour démarrer"), "{rendu}");
    }

    #[test]
    fn le_rendu_d_une_intention_vide_reste_lisible() {
        let rendu = render_proposal("fais un truc", &propose("fais un truc"));
        assert!(rendu.contains("Aucune capacité"), "{rendu}");
    }

    #[test]
    fn les_accents_ne_changent_pas_la_detection() {
        for variante in ["écris dans ~/x", "ecris dans ~/x", "ÉCRIS dans ~/x"] {
            let proposal = propose(variante);
            assert!(
                proposal.grants.iter().any(|g| g.act == Act::Write),
                "variante non reconnue : {variante}"
            );
        }
    }
}
