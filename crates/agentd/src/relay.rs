//! Relais de modèles par rôle : la réflexion profonde à un modèle, l'exécution économe à un
//! autre, le code à un troisième, et le compte exact de ce que chacun a coûté (ADR 0034).
//!
//! Rien ici ne donne de droit : un rôle choisit un modèle parmi ceux que le profil admet déjà,
//! capd tranche la délégation comme avant, et le briefing remis au modèle est une consigne,
//! pas une autorité. Ce module ne fait que deux choses vérifiables : résoudre un rôle en un
//! modèle réellement disponible, et dire à l'humain ce que le relais a économisé.

use std::collections::BTreeMap;

use prophet_types::manifest::MODEL_ROLES;

use crate::budget::{ModelUsage, UsageByModel, share_outside};

/// Rôles d'un contexte que la mission peut confier, tels que le briefing les annonce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    /// Identifiant du profil dans le catalogue.
    pub profile: String,
    /// Rôles que ce profil sait jouer, avec leurs modèles admis.
    pub roles: BTreeMap<String, Vec<String>>,
}

/// Le modèle qu'un rôle désigne dans ces `roles`, parmi ceux qui sont réellement disponibles :
/// les modèles locaux que le moteur sert (`available`, sans préfixe) et les pilotes officiels
/// que le lanceur de la session dit connectés (`drivers`, sans préfixe).
///
/// Le résultat garde le préfixe du manifeste (`local:` ou `driver:`). Le premier modèle admis
/// et présent l'emporte : l'ordre du profil est une préférence, pas une garantie.
///
/// # Errors
/// Rôle inconnu du profil, ou aucun de ses modèles disponible.
pub fn resolve(
    roles: &BTreeMap<String, Vec<String>>,
    role: &str,
    available: &[String],
    drivers: &[String],
) -> Result<String, String> {
    let role = role.trim().to_lowercase();
    if !MODEL_ROLES.contains(&role.as_str()) {
        return Err(format!(
            "rôle inconnu : {role} (attendu : {})",
            MODEL_ROLES.join(", ")
        ));
    }
    let models = roles
        .get(&role)
        .filter(|m| !m.is_empty())
        .ok_or_else(|| format!("ce contexte ne définit pas le rôle {role}"))?;
    models
        .iter()
        .find(|reference| {
            reference
                .strip_prefix("local:")
                .is_some_and(|name| available.iter().any(|a| a == name))
                || reference
                    .strip_prefix("driver:")
                    .is_some_and(|name| drivers.iter().any(|d| d == name))
        })
        .cloned()
        .ok_or_else(|| {
            format!(
                "aucun modèle du rôle {role} n'est disponible : ni servi par le moteur, ni client connecté (attendus : {})",
                models.join(", ")
            )
        })
}

/// La consigne remise au modèle d'une mission qui participe à un relais.
///
/// `None` quand la mission n'a pas de rôle et ne peut rien confier : un seul modèle fait tout,
/// et la boucle native reste ce qu'elle était. Sinon, la consigne dit au modèle ce qu'on attend
/// de son rôle et ce qu'il peut confier, avec les rôles exacts que chaque contexte sait jouer.
#[must_use]
pub fn briefing(role: Option<&str>, contexts: &[Context]) -> Option<String> {
    let delegable: Vec<&Context> = contexts
        .iter()
        .filter(|c| c.roles.values().any(|m| !m.is_empty()))
        .collect();
    if role.is_none() && delegable.is_empty() {
        return None;
    }
    let mut text = String::from(
        "Vous travaillez dans Prophet OS, au sein d'un relais de modèles où chaque rôle coûte différemment. ",
    );
    match role {
        Some("reflect") => text.push_str(
            "Votre rôle est la réflexion : vous êtes le modèle le plus capable et le plus coûteux de la mission. Réfléchissez, découpez l'objectif en étapes précises et autonomes, puis confiez chaque étape d'exécution par l'outil task.delegate en nommant son rôle (role) plutôt que son modèle ; faites relire tout code ou document important par le rôle review, qui porte un autre regard que celui qui l'a produit ; chaque sous-mission part de votre espace de travail et ce qu'elle y change vous revient à sa fin (carried) : relisez ce travail avec vos outils, puis concluez. Gardez vos propres tours rares et courts : lire, décider, déléguer, vérifier.",
        ),
        Some("execute") => text.push_str(
            "Votre rôle est l'exécution : accomplissez exactement l'objectif confié, sans digression ni reformulation, avec le moins de tours et d'appels d'outils possible, puis rendez un résultat bref et vérifiable.",
        ),
        Some("code") => text.push_str(
            "Votre rôle est le code : écrivez ou modifiez précisément ce que l'objectif demande, vérifiez ce que vous produisez avec les outils disponibles, et rendez un résultat qui dit ce qui a changé.",
        ),
        Some("review") => text.push_str(
            "Votre rôle est la relecture : vous jugez un travail qu'un autre modèle a rendu, sans le refaire. Lisez ce qui a changé avec les outils disponibles, cherchez ce qui est faux, manquant, dangereux ou non vérifié, et rendez un verdict court et argumenté : accepté tel quel, ou la liste précise de ce qu'il faut corriger, chaque point cité. Ne modifiez rien.",
        ),
        _ => text.push_str("Votre mission n'a pas de rôle assigné ; elle peut en confier."),
    }
    if !delegable.is_empty() {
        text.push_str(" Contextes que vous pouvez confier par task.delegate, avec les rôles qu'ils savent jouer : ");
        let parts: Vec<String> = delegable
            .iter()
            .map(|c| {
                let roles: Vec<&str> = MODEL_ROLES
                    .into_iter()
                    .filter(|r| c.roles.get(*r).is_some_and(|m| !m.is_empty()))
                    .collect();
                format!("{} ({})", c.profile, roles.join(", "))
            })
            .collect();
        text.push_str(&parts.join(" ; "));
        text.push_str(". Formulez pour chaque délégation un objectif complet : la sous-mission ne voit pas votre conversation.");
    }
    Some(text)
}

/// Une ligne lisible du compte par modèle, avec la part prise en charge hors du modèle
/// `reference` (celui de la mission) quand elle a un sens.
#[must_use]
pub fn render_usage(usage: &UsageByModel, reference: &str) -> String {
    if usage.is_empty() {
        return String::new();
    }
    let mut parts: Vec<String> = usage
        .iter()
        .map(|(model, u): (&String, &ModelUsage)| {
            format!(
                "{} : {} tour(s), {} tokens ({} entrée, {} sortie)",
                model.strip_prefix("local:").unwrap_or(model),
                u.turns,
                u.tokens(),
                u.tokens_in,
                u.tokens_out
            )
        })
        .collect();
    if usage.len() > 1
        && let Some(share) = share_outside(usage, reference)
    {
        parts.push(format!(
            "{share} % des tokens pris en charge hors de {}",
            reference.strip_prefix("local:").unwrap_or(reference)
        ));
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roles() -> BTreeMap<String, Vec<String>> {
        BTreeMap::from([
            ("reflect".to_owned(), vec!["local:grand".to_owned()]),
            (
                "execute".to_owned(),
                vec!["local:absent".to_owned(), "local:petit".to_owned()],
            ),
        ])
    }

    #[test]
    fn un_role_designe_le_premier_modele_servi() {
        let servis = vec!["grand".to_owned(), "petit".to_owned()];
        assert_eq!(
            resolve(&roles(), "execute", &servis, &[]).unwrap(),
            "local:petit"
        );
        assert_eq!(
            resolve(&roles(), "Reflect ", &servis, &[]).unwrap(),
            "local:grand"
        );
    }

    #[test]
    fn un_role_peut_designer_un_client_officiel_connecte() {
        let mut roles = roles();
        roles.insert(
            "code".to_owned(),
            vec!["driver:codex".to_owned(), "local:petit".to_owned()],
        );
        let servis = vec!["petit".to_owned()];
        // Codex connecté : il l'emporte, dans l'ordre du profil.
        assert_eq!(
            resolve(&roles, "code", &servis, &["codex".to_owned()]).unwrap(),
            "driver:codex"
        );
        // Codex absent : le modèle local suivant.
        assert_eq!(
            resolve(&roles, "code", &servis, &[]).unwrap(),
            "local:petit"
        );
        // Rien de disponible : l'erreur nomme les deux voies.
        let erreur = resolve(&roles, "code", &[], &[]).unwrap_err();
        assert!(erreur.contains("ni client connecté"), "{erreur}");
    }

    #[test]
    fn un_role_inconnu_ou_sans_modele_servi_est_une_erreur_nommee() {
        let servis = vec!["grand".to_owned()];
        assert!(
            resolve(&roles(), "muse", &servis, &[])
                .unwrap_err()
                .contains("rôle inconnu")
        );
        assert!(
            resolve(&roles(), "code", &servis, &[])
                .unwrap_err()
                .contains("ne définit pas le rôle code")
        );
        let erreur = resolve(&roles(), "execute", &servis, &[]).unwrap_err();
        assert!(erreur.contains("aucun modèle du rôle execute"), "{erreur}");
        assert!(erreur.contains("local:petit"), "{erreur}");
    }

    #[test]
    fn sans_role_ni_contexte_delegable_il_n_y_a_pas_de_briefing() {
        assert_eq!(briefing(None, &[]), None);
        let muet = Context {
            profile: "scribe".into(),
            roles: BTreeMap::new(),
        };
        assert_eq!(briefing(None, &[muet]), None);
    }

    #[test]
    fn le_briefing_nomme_le_role_et_les_contextes_confiables() {
        let scribe = Context {
            profile: "scribe".into(),
            roles: roles(),
        };
        let texte = briefing(Some("reflect"), &[scribe]).unwrap();
        assert!(texte.contains("réflexion"), "{texte}");
        assert!(texte.contains("scribe (reflect, execute)"), "{texte}");
        assert!(texte.contains("task.delegate"), "{texte}");
        let exec = briefing(Some("execute"), &[]).unwrap();
        assert!(exec.contains("exécution") && !exec.contains("task.delegate"));
        // La réflexion est invitée à faire relire ; la relecture juge sans refaire ni modifier.
        assert!(texte.contains("rôle review"), "{texte}");
        let relecture = briefing(Some("review"), &[]).unwrap();
        assert!(relecture.contains("relecture") && relecture.contains("Ne modifiez rien"));
        assert!(!relecture.contains("task.delegate"));
    }

    #[test]
    fn le_compte_dit_la_part_hors_du_modele_de_la_mission() {
        let mut usage = UsageByModel::new();
        usage.entry("local:grand".into()).or_default().add(100, 20);
        usage.entry("local:petit".into()).or_default().add(300, 60);
        let ligne = render_usage(&usage, "local:grand");
        assert!(ligne.contains("grand : 1 tour(s), 120 tokens"), "{ligne}");
        assert!(
            ligne.contains("75 % des tokens pris en charge hors de grand"),
            "{ligne}"
        );
        let seul = UsageByModel::from([("local:grand".to_owned(), ModelUsage::default())]);
        assert!(!render_usage(&seul, "local:grand").contains('%'));
        assert_eq!(render_usage(&UsageByModel::new(), "local:grand"), "");
    }
}
