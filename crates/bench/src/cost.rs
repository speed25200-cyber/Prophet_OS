//! Coût d'une observation.
//!
//! La comparaison honnête n'est pas « JSON contre PNG » : c'est le coût d'un **tour complet** de
//! boucle agentique. Un tour par capture d'écran doit réobserver entièrement pour vérifier ce
//! qu'il vient de faire ; un tour sémantique reçoit le résultat avec l'action.

use serde::{Deserialize, Serialize};

/// Coût d'une observation, en octets et en tokens estimés.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Measurement {
    /// Octets transmis au modèle.
    pub bytes: usize,
    /// Tokens estimés.
    pub tokens: usize,
}

/// Tokens par octet de texte, estimation courante pour du JSON en français.
pub const BYTES_PER_TEXT_TOKEN: usize = 3;

/// Tokens d'une capture d'écran, ordre de grandeur pour une image de bureau.
///
/// Une image de 1440 sur 900 coûte environ 1500 tokens chez les fournisseurs qui la découpent en
/// tuiles ; on retient cette valeur plutôt que le poids du fichier, qui ne dit rien du coût réel.
pub const SCREENSHOT_TOKENS: usize = 1_500;

/// Poids typique d'une capture d'écran en PNG, en octets.
pub const SCREENSHOT_BYTES: usize = 900_000;

impl Measurement {
    /// Mesure d'une observation textuelle.
    #[must_use]
    pub const fn text(bytes: usize) -> Self {
        Self {
            bytes,
            tokens: bytes / BYTES_PER_TEXT_TOKEN,
        }
    }

    /// Mesure d'une capture d'écran.
    #[must_use]
    pub const fn screenshot() -> Self {
        Self {
            bytes: SCREENSHOT_BYTES,
            tokens: SCREENSHOT_TOKENS,
        }
    }
}

/// Comparaison d'une boucle sémantique et d'une boucle par capture d'écran.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    /// Nombre d'étapes de la tâche.
    pub steps: usize,
    /// Coût total de la boucle sémantique.
    pub semantic: Measurement,
    /// Coût total de la boucle par capture.
    pub pixels: Measurement,
    /// Rapport des tokens, capture sur sémantique.
    pub token_ratio: f64,
}

/// Compare les deux boucles sur une tâche.
///
/// `observation_bytes` est la première observation, `diff_bytes` le coût moyen d'une réobservation
/// différentielle. La boucle par capture paie une image par observation **et** une seconde pour
/// vérifier le résultat de chaque action ; la boucle sémantique paie l'arbre une fois, puis des
/// différentiels, et rien pour la vérification puisque le résultat la porte.
#[must_use]
pub fn compare(steps: usize, observation_bytes: usize, diff_bytes: usize) -> Comparison {
    let steps = steps.max(1);
    let semantic_bytes = observation_bytes + diff_bytes * steps.saturating_sub(1);
    let semantic = Measurement::text(semantic_bytes);
    let pixels = Measurement {
        bytes: SCREENSHOT_BYTES * steps * 2,
        tokens: SCREENSHOT_TOKENS * steps * 2,
    };
    let token_ratio = if semantic.tokens == 0 {
        f64::INFINITY
    } else {
        pixels.tokens as f64 / semantic.tokens as f64
    };
    Comparison {
        steps,
        semantic,
        pixels,
        token_ratio,
    }
}

/// Rendu lisible d'une comparaison.
#[must_use]
pub fn render(comparison: &Comparison) -> String {
    format!(
        "Tâche de {} étapes\n  sémantique : {:>9} octets, {:>7} tokens\n  captures   : {:>9} octets, {:>7} tokens\n  rapport    : {:.1} fois moins de tokens\n",
        comparison.steps,
        comparison.semantic.bytes,
        comparison.semantic.tokens,
        comparison.pixels.bytes,
        comparison.pixels.tokens,
        comparison.token_ratio
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_tache_courte_est_deja_bien_moins_chere() {
        // Chiffres mesurés sur la page de réservation du test M10.
        let c = compare(5, 1_929, 300);
        assert!(
            c.token_ratio > 3.0,
            "rapport insuffisant : {:.1}",
            c.token_ratio
        );
        assert!(render(&c).contains("fois moins"));
    }

    #[test]
    fn l_ecart_se_creuse_avec_la_longueur_de_la_tache() {
        let court = compare(3, 1_929, 300);
        let long = compare(30, 1_929, 300);
        assert!(
            long.token_ratio > court.token_ratio,
            "court {:.1}, long {:.1}",
            court.token_ratio,
            long.token_ratio
        );
    }

    #[test]
    fn une_seule_etape_reste_favorable() {
        let c = compare(1, 1_929, 300);
        assert_eq!(c.semantic.bytes, 1_929);
        assert!(c.token_ratio > 1.0);
    }

    #[test]
    fn zero_etape_est_ramene_a_une() {
        assert_eq!(compare(0, 100, 10).steps, 1);
    }

    #[test]
    fn une_capture_coute_plus_qu_un_arbre() {
        assert!(Measurement::screenshot().tokens > Measurement::text(2_000).tokens);
    }
}
