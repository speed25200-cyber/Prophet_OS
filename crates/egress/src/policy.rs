//! Politique de sortie : ce qu'une tâche a le droit de joindre, et comment.

use prophet_types::pattern::{Family, matches};
use serde::{Deserialize, Serialize};

/// Décision sur une requête sortante.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Verdict {
    /// Requête autorisée.
    Allow,
    /// Requête refusée, avec le motif.
    Deny {
        /// Motif stable.
        reason: DenyReason,
        /// Détail lisible.
        detail: String,
    },
    /// Requête suspendue en attente d'approbation humaine.
    NeedsApproval {
        /// Pourquoi.
        detail: String,
    },
}

/// Motif de refus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DenyReason {
    /// L'hôte n'est couvert par aucun grant.
    HostNotAllowed,
    /// La méthode n'est pas autorisée.
    MethodNotAllowed,
    /// Le volume sortant dépasse le budget.
    VolumeExceeded,
    /// Exfiltration suspectée.
    ExfiltrationSuspected,
    /// Adresse littérale, refusée en v0.
    LiteralAddress,
}

/// Méthodes considérées comme modifiant l'état distant, donc irréversibles.
pub const MUTATING_METHODS: &[&str] = &["POST", "PUT", "PATCH", "DELETE"];

/// Politique appliquée à une tâche.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Policy {
    /// Motifs d'hôtes autorisés, issus des grants `net.egress` du jeton.
    pub allowed_hosts: Vec<String>,
    /// Méthodes autorisées. Vide signifie « toutes les méthodes de lecture ».
    #[serde(default)]
    pub allowed_methods: Vec<String>,
    /// Volume sortant maximal, en octets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes_out: Option<u64>,
    /// Les méthodes modifiantes exigent-elles une approbation ?
    #[serde(default = "default_true")]
    pub approval_for_mutations: bool,
}

const fn default_true() -> bool {
    true
}

impl Policy {
    /// Politique n'autorisant que les hôtes donnés, en lecture.
    #[must_use]
    pub fn allowing(hosts: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            allowed_hosts: hosts.into_iter().map(Into::into).collect(),
            allowed_methods: Vec::new(),
            max_bytes_out: None,
            approval_for_mutations: true,
        }
    }

    /// Vrai si l'hôte est couvert par un motif autorisé.
    ///
    /// Les motifs `driver:<pilote>` ne correspondent à aucun hôte ici : ils sont traduits en
    /// domaines concrets par le pilote lui-même, jamais devinés par le proxy.
    #[must_use]
    pub fn host_allowed(&self, host: &str) -> bool {
        self.allowed_hosts.iter().any(|pattern| {
            !pattern.starts_with("driver:") && matches(Family::Domain, pattern, host, "")
        })
    }

    /// Évalue une requête.
    #[must_use]
    pub fn evaluate(&self, host: &str, method: &str, bytes_out: u64) -> Verdict {
        if is_literal_address(host) {
            return Verdict::Deny {
                reason: DenyReason::LiteralAddress,
                detail: format!(
                    "adresse littérale refusée : {host}. Les politiques s'expriment en noms de domaine."
                ),
            };
        }
        if !self.host_allowed(host) {
            return Verdict::Deny {
                reason: DenyReason::HostNotAllowed,
                detail: format!("aucun grant net.egress ne couvre {host}"),
            };
        }
        let method_upper = method.to_ascii_uppercase();
        if !self.allowed_methods.is_empty()
            && !self
                .allowed_methods
                .iter()
                .any(|m| m.eq_ignore_ascii_case(&method_upper))
        {
            return Verdict::Deny {
                reason: DenyReason::MethodNotAllowed,
                detail: format!("méthode {method_upper} non autorisée"),
            };
        }
        if let Some(max) = self.max_bytes_out
            && bytes_out > max
        {
            return Verdict::Deny {
                reason: DenyReason::VolumeExceeded,
                detail: format!("{bytes_out} octets sortants, plafond {max}"),
            };
        }
        if self.approval_for_mutations && MUTATING_METHODS.contains(&method_upper.as_str()) {
            return Verdict::NeedsApproval {
                detail: format!("{method_upper} vers {host} modifie un état distant"),
            };
        }
        Verdict::Allow
    }
}

/// Vrai si l'hôte est une adresse IP littérale plutôt qu'un nom.
#[must_use]
pub fn is_literal_address(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    bare.parse::<std::net::IpAddr>().is_ok()
        || bare
            .split(':')
            .next()
            .is_some_and(|h| h.parse::<std::net::IpAddr>().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn politique() -> Policy {
        Policy::allowing(["*.exemple.fr", "api.github.com"])
    }

    #[test]
    fn hote_autorise_et_refuse() {
        let p = politique();
        assert!(p.host_allowed("api.exemple.fr"));
        assert!(p.host_allowed("exemple.fr"));
        assert!(p.host_allowed("api.github.com"));
        assert!(!p.host_allowed("evil.com"));
        assert!(
            !p.host_allowed("exemple.fr.evil.com"),
            "un suffixe trompeur ne doit pas passer"
        );
    }

    #[test]
    fn lecture_autorisee_ecriture_soumise_a_approbation() {
        let p = politique();
        assert_eq!(p.evaluate("api.exemple.fr", "GET", 0), Verdict::Allow);
        assert!(matches!(
            p.evaluate("api.exemple.fr", "POST", 10),
            Verdict::NeedsApproval { .. }
        ));
        assert!(matches!(
            p.evaluate("api.exemple.fr", "DELETE", 0),
            Verdict::NeedsApproval { .. }
        ));
    }

    #[test]
    fn hote_non_couvert_refuse() {
        assert!(matches!(
            politique().evaluate("evil.com", "GET", 0),
            Verdict::Deny {
                reason: DenyReason::HostNotAllowed,
                ..
            }
        ));
    }

    #[test]
    fn adresse_litterale_refusee() {
        let p = Policy::allowing(["*"]);
        for host in ["10.0.0.1", "127.0.0.1:8080", "[::1]", "192.168.1.1"] {
            assert!(
                matches!(
                    p.evaluate(host, "GET", 0),
                    Verdict::Deny {
                        reason: DenyReason::LiteralAddress,
                        ..
                    }
                ),
                "{host} devrait être refusé"
            );
        }
    }

    #[test]
    fn plafond_de_volume() {
        let p = Policy {
            max_bytes_out: Some(1000),
            approval_for_mutations: false,
            ..politique()
        };
        assert_eq!(p.evaluate("api.exemple.fr", "POST", 999), Verdict::Allow);
        assert!(matches!(
            p.evaluate("api.exemple.fr", "POST", 1001),
            Verdict::Deny {
                reason: DenyReason::VolumeExceeded,
                ..
            }
        ));
    }

    #[test]
    fn liste_de_methodes() {
        let p = Policy {
            allowed_methods: vec!["GET".into(), "HEAD".into()],
            ..politique()
        };
        assert_eq!(p.evaluate("api.exemple.fr", "get", 0), Verdict::Allow);
        assert!(matches!(
            p.evaluate("api.exemple.fr", "POST", 0),
            Verdict::Deny {
                reason: DenyReason::MethodNotAllowed,
                ..
            }
        ));
    }

    #[test]
    fn motif_de_pilote_ne_couvre_aucun_hote() {
        let p = Policy::allowing(["driver:claude-code"]);
        assert!(!p.host_allowed("api.anthropic.com"));
        assert!(!p.host_allowed("driver:claude-code"));
    }

    #[test]
    fn politique_vide_refuse_tout() {
        let p = Policy::default();
        assert!(matches!(
            p.evaluate("exemple.fr", "GET", 0),
            Verdict::Deny { .. }
        ));
    }
}
