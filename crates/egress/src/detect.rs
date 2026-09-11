//! Détection d'exfiltration.
//!
//! Une injection de prompt réussie ne se voit pas dans le raisonnement du modèle : elle se voit
//! dans ce qui sort. Ce module regarde le trafic, pas le texte du modèle, parce que c'est la seule
//! couche qu'un modèle compromis ne peut pas influencer.
//!
//! Les heuristiques sont volontairement simples et explicables. Chacune produit un signal nommé,
//! pour qu'un refus puisse toujours se justifier auprès de l'utilisateur.

use serde::{Deserialize, Serialize};

/// Signal de suspicion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    /// Un motif de secret connu apparaît dans la requête.
    SecretPattern {
        /// Famille de secret reconnue.
        family: String,
    },
    /// Le volume sortant dépasse nettement ce qu'une requête d'API justifie.
    UnusualVolume {
        /// Octets observés.
        bytes: u64,
    },
    /// Le corps ressemble à des données encodées plutôt qu'à une requête.
    HighEntropyPayload {
        /// Proportion de caractères d'alphabet base64.
        ratio: f64,
    },
    /// Des données sortent par l'adresse plutôt que par le corps.
    OversizedUrl {
        /// Longueur observée.
        length: usize,
    },
}

impl Signal {
    /// Explication destinée à l'humain qui arbitre.
    #[must_use]
    pub fn explain(&self) -> String {
        match self {
            Self::SecretPattern { family } => {
                format!("la requête contient ce qui ressemble à un secret ({family})")
            }
            Self::UnusualVolume { bytes } => {
                format!("{bytes} octets sortants, bien au-delà d'un appel d'API ordinaire")
            }
            Self::HighEntropyPayload { ratio } => format!(
                "le corps est à {:.0} % composé de caractères d'encodage : données encodées probables",
                ratio * 100.0
            ),
            Self::OversizedUrl { length } => {
                format!("adresse de {length} caractères : données probablement passées dans l'URL")
            }
        }
    }
}

/// Motifs de secrets reconnus, par famille.
const SECRET_PATTERNS: &[(&str, &str)] = &[
    ("clé Anthropic", "sk-ant-"),
    ("clé OpenAI", "sk-proj-"),
    ("jeton GitHub", "ghp_"),
    ("jeton GitHub (app)", "github_pat_"),
    ("clé AWS", "AKIA"),
    ("clé privée", "-----BEGIN"),
    ("jeton Slack", "xoxb-"),
    ("jeton Google", "ya29."),
];

/// Seuils de la détection.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Thresholds {
    /// Volume au-delà duquel une requête devient suspecte, en octets.
    pub volume_bytes: u64,
    /// Longueur d'URL au-delà de laquelle des données y sont probablement cachées.
    pub url_length: usize,
    /// Proportion de caractères d'encodage au-delà de laquelle le corps est suspect.
    pub entropy_ratio: f64,
    /// Taille minimale d'un corps avant d'évaluer son entropie.
    pub entropy_min_bytes: usize,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            volume_bytes: 1_000_000,
            url_length: 2_000,
            entropy_ratio: 0.95,
            entropy_min_bytes: 512,
        }
    }
}

/// Requête sortante examinée.
#[derive(Debug, Clone)]
pub struct Outbound<'a> {
    /// Hôte visé.
    pub host: &'a str,
    /// Adresse complète.
    pub url: &'a str,
    /// En-têtes, déjà débarrassés des références de secrets substituées par le proxy.
    pub headers: &'a [(String, String)],
    /// Corps.
    pub body: &'a [u8],
}

/// Détecteur.
#[derive(Debug, Clone, Default)]
pub struct Detector {
    thresholds: Thresholds,
}

impl Detector {
    /// Détecteur aux seuils par défaut.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Détecteur aux seuils donnés.
    #[must_use]
    pub const fn with_thresholds(thresholds: Thresholds) -> Self {
        Self { thresholds }
    }

    /// Examine une requête et renvoie les signaux relevés.
    #[must_use]
    pub fn inspect(&self, outbound: &Outbound<'_>) -> Vec<Signal> {
        let mut signals = Vec::new();

        // Un secret peut voyager dans l'adresse, dans un en-tête ou dans le corps : on regarde
        // les trois, car choisir un seul endroit revient à indiquer par où passer.
        let body_text = String::from_utf8_lossy(outbound.body);
        let header_text = outbound
            .headers
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n");
        for (family, pattern) in SECRET_PATTERNS {
            if outbound.url.contains(pattern)
                || header_text.contains(pattern)
                || body_text.contains(pattern)
            {
                signals.push(Signal::SecretPattern {
                    family: (*family).to_owned(),
                });
            }
        }

        let bytes = outbound.body.len() as u64;
        if bytes > self.thresholds.volume_bytes {
            signals.push(Signal::UnusualVolume { bytes });
        }
        if outbound.url.len() > self.thresholds.url_length {
            signals.push(Signal::OversizedUrl {
                length: outbound.url.len(),
            });
        }
        if outbound.body.len() >= self.thresholds.entropy_min_bytes {
            let ratio = encoded_ratio(&body_text);
            // La proportion seule ne suffit pas : un corps fait d'un seul caractère répété la
            // maximise sans être des données encodées. On exige aussi une réelle variété, ce qui
            // distingue un encodage d'un remplissage.
            let variety = body_text
                .chars()
                .take(4096)
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            if ratio >= self.thresholds.entropy_ratio && variety >= 16 {
                signals.push(Signal::HighEntropyPayload { ratio });
            }
        }
        signals
    }

    /// Vrai si les signaux relevés justifient de **bloquer** la requête.
    ///
    /// Seul un motif de secret bloque. C'est le cas qui compte, et il ne souffre pas d'arbitrage :
    /// laisser partir une clé est irréparable.
    #[must_use]
    pub fn should_block(signals: &[Signal]) -> bool {
        signals
            .iter()
            .any(|s| matches!(s, Signal::SecretPattern { .. }))
    }

    /// Vrai si les signaux justifient de **soumettre à l'humain** sans bloquer d'office.
    ///
    /// Volume inhabituel, corps encodé, adresse démesurée : chacun a des usages légitimes
    /// (téléverser un document, envoyer une image). Refuser d'office produirait des faux positifs
    /// coûteux ; les porter à l'humain coûte une approbation.
    #[must_use]
    pub fn should_escalate(signals: &[Signal]) -> bool {
        !signals.is_empty() && !Self::should_block(signals)
    }
}

/// Proportion de caractères appartenant à l'alphabet base64, indice d'un contenu encodé.
fn encoded_ratio(text: &str) -> f64 {
    let total = text.chars().count();
    if total == 0 {
        return 0.0;
    }
    let encoded = text
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '/' || *c == '=')
        .count();
    encoded as f64 / total as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requete<'a>(url: &'a str, body: &'a [u8], headers: &'a [(String, String)]) -> Outbound<'a> {
        Outbound {
            host: "exemple.fr",
            url,
            headers,
            body,
        }
    }

    #[test]
    fn cle_dans_le_corps_detectee() {
        let signals = Detector::new().inspect(&requete(
            "https://exemple.fr/collect",
            b"data=sk-ant-api03-abcdef",
            &[],
        ));
        assert!(matches!(signals[0], Signal::SecretPattern { .. }));
        assert!(Detector::should_block(&signals));
    }

    #[test]
    fn cle_dans_l_adresse_detectee() {
        let signals = Detector::new().inspect(&requete(
            "https://exemple.fr/?q=ghp_0123456789abcdef",
            b"",
            &[],
        ));
        assert!(Detector::should_block(&signals));
    }

    #[test]
    fn cle_dans_un_en_tete_detectee() {
        let headers = vec![("X-Note".to_owned(), "AKIAIOSFODNN7EXAMPLE".to_owned())];
        let signals = Detector::new().inspect(&requete("https://exemple.fr/", b"", &headers));
        assert!(Detector::should_block(&signals));
    }

    #[test]
    fn cle_privee_detectee() {
        let signals = Detector::new().inspect(&requete(
            "https://exemple.fr/",
            b"-----BEGIN OPENSSH PRIVATE KEY-----",
            &[],
        ));
        assert!(Detector::should_block(&signals));
    }

    #[test]
    fn requete_ordinaire_non_signalee() {
        let signals = Detector::new().inspect(&requete(
            "https://api.exemple.fr/v1/ventes?trimestre=3",
            br#"{"format":"pdf","destinataire":"marie"}"#,
            &[("Accept".to_owned(), "application/json".to_owned())],
        ));
        assert!(signals.is_empty(), "{signals:?}");
        assert!(!Detector::should_block(&signals));
    }

    #[test]
    fn corps_encode_volumineux_signale() {
        let corps = "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=".repeat(40);
        let signals = Detector::new().inspect(&requete(
            "https://exemple.fr/collect",
            corps.as_bytes(),
            &[],
        ));
        assert!(
            signals
                .iter()
                .any(|s| matches!(s, Signal::HighEntropyPayload { .. })),
            "{signals:?}"
        );
    }

    #[test]
    fn adresse_demesuree_signalee() {
        let url = format!("https://exemple.fr/?d={}", "a".repeat(3000));
        let signals = Detector::new().inspect(&requete(&url, b"", &[]));
        assert!(
            signals
                .iter()
                .any(|s| matches!(s, Signal::OversizedUrl { .. }))
        );
    }

    #[test]
    fn volume_anormal_signale() {
        let corps = vec![b'x'; 2_000_000];
        let signals = Detector::new().inspect(&requete("https://exemple.fr/", &corps, &[]));
        assert!(
            signals
                .iter()
                .any(|s| matches!(s, Signal::UnusualVolume { .. }))
        );
    }

    #[test]
    fn un_televersement_volumineux_remonte_a_l_humain_sans_etre_bloque() {
        let corps = vec![b'x'; 2_000_000];
        let signals = Detector::new().inspect(&requete("https://exemple.fr/", &corps, &[]));
        assert_eq!(signals.len(), 1, "{signals:?}");
        assert!(!Detector::should_block(&signals));
        assert!(Detector::should_escalate(&signals));
    }

    #[test]
    fn un_remplissage_monotone_n_est_pas_pris_pour_un_encodage() {
        // Un corps fait d'un seul caractère répété maximise la proportion sans être des données
        // encodées : la variété doit le distinguer.
        let corps = "a".repeat(4096);
        let signals =
            Detector::new().inspect(&requete("https://exemple.fr/", corps.as_bytes(), &[]));
        assert!(
            !signals
                .iter()
                .any(|s| matches!(s, Signal::HighEntropyPayload { .. })),
            "{signals:?}"
        );
    }

    #[test]
    fn deux_signaux_faibles_ne_bloquent_toujours_pas() {
        // Téléverser une image encodée déclenche volume et entropie : c'est un usage légitime,
        // qui doit passer par une approbation et non par un refus.
        let corps = "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVphYmNkZWZnaGlqa2xtbm9w".repeat(40_000);
        let url = format!("https://exemple.fr/upload?{}", "n".repeat(3000));
        let signals = Detector::new().inspect(&requete(&url, corps.as_bytes(), &[]));
        assert!(signals.len() >= 2, "{signals:?}");
        assert!(!Detector::should_block(&signals));
        assert!(Detector::should_escalate(&signals));
    }

    #[test]
    fn chaque_signal_s_explique() {
        for signal in [
            Signal::SecretPattern {
                family: "clé AWS".into(),
            },
            Signal::UnusualVolume { bytes: 10 },
            Signal::HighEntropyPayload { ratio: 0.99 },
            Signal::OversizedUrl { length: 5000 },
        ] {
            assert!(!signal.explain().is_empty());
        }
    }
}
