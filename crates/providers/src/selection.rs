//! Choix du pilote pour une tâche.
//!
//! Le manifeste exprime une préférence ; la machine, l'état des sessions et la politique de
//! confidentialité décident du possible. La sélection est explicite et traçable : on doit pouvoir
//! dire *pourquoi* telle tâche a tourné sur tel pilote.

use prophet_types::manifest::{Manifest, Privacy};
use serde::{Deserialize, Serialize};

/// Ce qui est disponible au moment du choix.
///
/// `#[serde(default)]` porte un sens, pas une commodité : ne pas mentionner une catégorie veut
/// dire qu'elle est vide. Un appelant qui ne connaît que les modèles locaux n'a pas à énumérer
/// trois listes vides pour le dire, et exiger qu'il le fasse ferait échouer sa description sur une
/// question de forme plutôt que de fond.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Availability {
    /// Pilotes de clients officiels connectés.
    pub logged_in_drivers: Vec<String>,
    /// Modèles locaux chargeables.
    pub local_models: Vec<String>,
    /// Pilotes dont le quota d'abonnement est épuisé.
    pub exhausted_drivers: Vec<String>,
    /// Fournisseurs d'API configurés.
    pub api_providers: Vec<String>,
}

/// Résultat du choix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    /// Référence retenue, telle qu'écrite dans le manifeste.
    pub reference: String,
    /// Pourquoi ce choix, en une phrase destinée à l'humain.
    pub reason: String,
}

/// Pourquoi aucun pilote ne convient.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoChoice {
    /// La confidentialité exige le local, mais aucun modèle local n'est disponible.
    LocalRequiredButAbsent,
    /// Toutes les préférences sont indisponibles.
    NothingAvailable {
        /// Ce qui a été essayé, dans l'ordre.
        tried: Vec<String>,
    },
}

/// Choisit un pilote.
///
/// # Errors
/// Si aucune préférence n'est satisfiable.
pub fn choose(manifest: &Manifest, availability: &Availability) -> Result<Choice, NoChoice> {
    let mut tried = Vec::new();
    for reference in &manifest.model.preferred {
        tried.push(reference.clone());
        let Some((kind, rest)) = reference.split_once(':') else {
            continue;
        };
        match kind {
            "local" => {
                if availability.local_models.iter().any(|m| m == rest) {
                    return Ok(Choice {
                        reference: reference.clone(),
                        reason: format!("modèle local {rest} disponible"),
                    });
                }
            }
            "driver" => {
                if manifest.model.privacy == Privacy::LocalOnly {
                    continue;
                }
                // `client@palier` : le client seul décide de la disponibilité (ADR 0040).
                let client = rest.split('@').next().unwrap_or(rest);
                if availability.exhausted_drivers.iter().any(|d| d == client) {
                    continue;
                }
                if availability.logged_in_drivers.iter().any(|d| d == client) {
                    return Ok(Choice {
                        reference: reference.clone(),
                        reason: format!("abonnement {client} connecté et quota disponible"),
                    });
                }
            }
            "api" => {
                if manifest.model.privacy == Privacy::LocalOnly {
                    continue;
                }
                let provider = rest.split(':').next().unwrap_or_default();
                if availability.api_providers.iter().any(|p| p == provider) {
                    return Ok(Choice {
                        reference: reference.clone(),
                        reason: format!("fournisseur d'API {provider} configuré"),
                    });
                }
            }
            _ => {}
        }
    }
    if manifest.model.privacy == Privacy::LocalOnly && availability.local_models.is_empty() {
        return Err(NoChoice::LocalRequiredButAbsent);
    }
    Err(NoChoice::NothingAvailable { tried })
}

/// Que faire quand le quota d'un abonnement s'épuise en cours de tâche.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum QuotaPolicy {
    /// Mettre la tâche en file d'attente jusqu'à la fenêtre suivante.
    #[default]
    Queue,
    /// Basculer sur un modèle local.
    FallbackLocal,
    /// Échouer immédiatement.
    Fail,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifeste(preferred: &[&str], privacy: &str) -> Manifest {
        let liste = preferred
            .iter()
            .map(|p| format!("\"{p}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let text = format!(
            r#"
[agent]
id = "org.test.agent"
version = "1.0.0"
name = "Test"
publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
[model]
preferred = [{liste}]
privacy = "{privacy}"
[capabilities.max]
"fs.read" = ["~/x/**"]
"#
        );
        Manifest::from_toml(&text).unwrap()
    }

    #[test]
    fn un_palier_de_modele_ne_change_pas_le_client_choisi() {
        let m = manifeste(
            &["driver:claude-code@opus", "driver:codex"],
            "local-preferred",
        );
        let a = Availability {
            logged_in_drivers: vec!["claude-code".into()],
            ..Availability::default()
        };
        assert_eq!(choose(&m, &a).unwrap().reference, "driver:claude-code@opus");
        let a = Availability {
            logged_in_drivers: vec!["codex".into()],
            ..Availability::default()
        };
        assert_eq!(choose(&m, &a).unwrap().reference, "driver:codex");
    }

    #[test]
    fn le_local_est_prefere_quand_il_est_disponible() {
        let m = manifeste(&["local:qwen3-8b", "driver:claude-code"], "local-preferred");
        let a = Availability {
            local_models: vec!["qwen3-8b".into()],
            logged_in_drivers: vec!["claude-code".into()],
            ..Availability::default()
        };
        assert_eq!(choose(&m, &a).unwrap().reference, "local:qwen3-8b");
    }

    #[test]
    fn bascule_sur_l_abonnement_si_le_modele_local_manque() {
        let m = manifeste(&["local:qwen3-8b", "driver:claude-code"], "local-preferred");
        let a = Availability {
            logged_in_drivers: vec!["claude-code".into()],
            ..Availability::default()
        };
        let choix = choose(&m, &a).unwrap();
        assert_eq!(choix.reference, "driver:claude-code");
        assert!(choix.reason.contains("abonnement"), "{}", choix.reason);
    }

    #[test]
    fn un_quota_epuise_fait_passer_au_suivant() {
        let m = manifeste(&["driver:claude-code", "driver:codex"], "local-preferred");
        let a = Availability {
            logged_in_drivers: vec!["claude-code".into(), "codex".into()],
            exhausted_drivers: vec!["claude-code".into()],
            ..Availability::default()
        };
        assert_eq!(choose(&m, &a).unwrap().reference, "driver:codex");
    }

    #[test]
    fn la_confidentialite_locale_interdit_tout_distant() {
        let m = manifeste(&["local:qwen3-8b", "driver:claude-code"], "local-only");
        let a = Availability {
            logged_in_drivers: vec!["claude-code".into()],
            ..Availability::default()
        };
        assert_eq!(
            choose(&m, &a).unwrap_err(),
            NoChoice::LocalRequiredButAbsent,
            "une tâche marquée local-only ne doit jamais partir chez un éditeur"
        );
    }

    #[test]
    fn la_confidentialite_locale_reste_servie_par_un_modele_local() {
        let m = manifeste(&["local:qwen3-8b"], "local-only");
        let a = Availability {
            local_models: vec!["qwen3-8b".into()],
            ..Availability::default()
        };
        assert_eq!(choose(&m, &a).unwrap().reference, "local:qwen3-8b");
    }

    #[test]
    fn un_pilote_non_connecte_est_ignore() {
        let m = manifeste(&["driver:claude-code", "driver:codex"], "any");
        let a = Availability {
            logged_in_drivers: vec!["codex".into()],
            ..Availability::default()
        };
        assert_eq!(choose(&m, &a).unwrap().reference, "driver:codex");
    }

    #[test]
    fn api_retenue_en_dernier_recours() {
        let m = manifeste(&["local:qwen3-8b", "api:anthropic:claude-opus-5"], "any");
        let a = Availability {
            api_providers: vec!["anthropic".into()],
            ..Availability::default()
        };
        assert_eq!(
            choose(&m, &a).unwrap().reference,
            "api:anthropic:claude-opus-5"
        );
    }

    #[test]
    fn rien_de_disponible() {
        let m = manifeste(&["local:qwen3-8b", "driver:claude-code"], "any");
        let err = choose(&m, &Availability::default()).unwrap_err();
        match err {
            NoChoice::NothingAvailable { tried } => assert_eq!(tried.len(), 2),
            other => panic!("attendu NothingAvailable, obtenu {other:?}"),
        }
    }
}
