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

/// Hôtes d'interrogation : ceux dont un `POST` est une question, pas un effet.
///
/// La règle « `POST` modifie un état distant » est vraie pour envoyer, payer, poster ou
/// supprimer ; elle est fausse pour une API de décision comme Jev, qui répond à un `POST` sans
/// rien retenir ni rien faire. Demander une décision humaine à chaque question rendrait une
/// boucle de décision inutilisable, et une approbation donnée cent fois par minute n'en serait
/// plus une. L'administrateur nomme donc explicitement ces hôtes ; pour eux, et pour `POST`
/// seulement, la requête est contrôlée comme une lecture : jeton, grant `net.egress` sur
/// l'hôte, détection d'exfiltration sur le corps et journal restent entiers. `PUT`, `PATCH` et
/// `DELETE` restent des modifications partout. Le motif `*` est refusé : il transformerait tous
/// les `POST` de la machine en lectures.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryHosts {
    patterns: Vec<String>,
}

impl QueryHosts {
    /// Lit une liste séparée par des virgules, telle qu'une configuration la donne.
    ///
    /// # Errors
    /// Un motif n'est pas un nom de domaine valide, ou vaut `*`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut patterns = Vec::new();
        for raw in text.split(',') {
            let pattern = raw.trim();
            if pattern.is_empty() {
                continue;
            }
            if pattern == "*" || pattern.starts_with("driver:") {
                return Err(format!(
                    "hôte d'interrogation refusé : {pattern} (un nom de domaine est requis)"
                ));
            }
            prophet_types::pattern::validate(Family::Domain, pattern)
                .map_err(|e| format!("hôte d'interrogation invalide : {e}"))?;
            patterns.push(pattern.to_owned());
        }
        Ok(Self { patterns })
    }

    /// Aucun hôte déclaré.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Les motifs déclarés.
    #[must_use]
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    /// Vrai si cet hôte est un hôte d'interrogation.
    #[must_use]
    pub fn covers(&self, host: &str) -> bool {
        self.patterns
            .iter()
            .any(|p| matches(Family::Domain, p, host, ""))
    }
}

/// Cette requête modifie-t-elle un état distant ?
///
/// C'est la question que le proxy pose à capd sous les drapeaux `external` et `irreversible`.
#[must_use]
pub fn is_mutating(method: &str, host: &str, query_hosts: &QueryHosts) -> bool {
    let method = method.to_ascii_uppercase();
    if !MUTATING_METHODS.contains(&method.as_str()) {
        return false;
    }
    !(method == "POST" && query_hosts.covers(host))
}

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

    #[test]
    fn un_post_vers_un_hote_d_interrogation_est_une_lecture_et_rien_d_autre() {
        let hotes = QueryHosts::parse(" api.typesafe.ai, *.decisions.exemple.fr ,").unwrap();
        assert_eq!(hotes.patterns().len(), 2);
        assert!(!is_mutating("POST", "api.typesafe.ai", &hotes));
        assert!(!is_mutating("post", "jev.decisions.exemple.fr", &hotes));
        assert!(!is_mutating("GET", "api.typesafe.ai", &hotes));
        // Seul POST est une interrogation ; les autres méthodes modifiantes le restent.
        assert!(is_mutating("PUT", "api.typesafe.ai", &hotes));
        assert!(is_mutating("DELETE", "api.typesafe.ai", &hotes));
        assert!(is_mutating("PATCH", "api.typesafe.ai", &hotes));
        // Un hôte non déclaré garde la règle ordinaire, même s'il ressemble.
        assert!(is_mutating("POST", "typesafe.ai", &hotes));
        assert!(is_mutating("POST", "api.typesafe.ai.evil.com", &hotes));
        assert!(is_mutating(
            "POST",
            "api.typesafe.ai",
            &QueryHosts::default()
        ));
    }

    #[test]
    fn les_hotes_d_interrogation_sont_des_domaines_jamais_tout() {
        assert!(QueryHosts::parse("*").is_err());
        assert!(QueryHosts::parse("api.typesafe.ai,*").is_err());
        assert!(QueryHosts::parse("driver:claude-code").is_err());
        assert!(QueryHosts::parse("pas un domaine").is_err());
        assert!(QueryHosts::parse("localhost").is_err());
        assert!(QueryHosts::parse("").unwrap().is_empty());
        assert!(QueryHosts::parse("127.0.0.1").is_ok());
    }
}
