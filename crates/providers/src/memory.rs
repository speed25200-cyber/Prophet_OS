//! La mémoire qu'un poids demande au moteur local, et celle que la machine lui offre.
//!
//! Charger un modèle qui ne tient pas en mémoire ne fait pas d'erreur : le noyau pagine, le
//! bureau gèle, puis l'OOM tue quelqu'un. On le dit avant : l'en-tête GGUF donne de quoi
//! estimer ce que llama.cpp réservera (voir [`need`]), `/proc/meminfo` ce que la machine a.
//! L'estimation est vérifiée contre la mémoire résidente du vrai moteur en CI
//! (`crates/agentd/tests/poids.rs`).

use serde::{Deserialize, Serialize};

use crate::weights::Weights;

/// Fenêtre du moteur de l'image quand la configuration ne la dit pas (`contextSize` du module
/// du moteur local).
pub const DEFAULT_CONTEXT: u64 = 4096;

/// Tokens d'un micro-lot du moteur (`n_ubatch` de llama.cpp) : c'est pour eux qu'il réserve
/// les logits de son tampon de calcul.
const MICRO_BATCH: u64 = 512;

/// Ce que le moteur porte en plus des tenseurs : binaire et bibliothèques, tokeniseur, états
/// intermédiaires du calcul, serveur HTTP.
const RUNTIME_BYTES: u64 = 192 << 20;

/// Tampon de calcul supposé quand l'en-tête ne dit pas le vocabulaire.
const UNKNOWN_COMPUTE_BYTES: u64 = 256 << 20;

/// Ce que le système garde pour lui : bureau, services, cache. Un poids qui ne laisse pas cela
/// libre ne tient pas, même sur une machine vide.
pub const SYSTEM_RESERVE: u64 = 1536 << 20;

/// La fenêtre avec laquelle le moteur de cette machine charge un poids : `PROPHET_LOCAL_CONTEXT`,
/// que le module du moteur local pose, sinon [`DEFAULT_CONTEXT`].
#[must_use]
pub fn context() -> u64 {
    std::env::var("PROPHET_LOCAL_CONTEXT")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|&n: &u64| n > 0)
        .unwrap_or(DEFAULT_CONTEXT)
}

/// La mémoire qu'un poids demande, par poste.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Need {
    /// Fenêtre pour laquelle l'estimation vaut, en tokens.
    pub context: u64,
    /// Les tenseurs : la taille du fichier.
    pub weights: u64,
    /// Le cache KV, pour toute la fenêtre.
    pub kv_cache: u64,
    /// Tampon de calcul et moteur.
    pub compute: u64,
    /// Le tout.
    pub total: u64,
}

/// Ce que le moteur local réservera pour servir `weights` avec une fenêtre de `context` tokens :
/// le fichier (llama.cpp le projette ou le recopie, il est résident une fois le premier token
/// produit), le cache KV de toute la fenêtre (alloué et mis à zéro au chargement), les logits
/// d'un micro-lot et le moteur lui-même. `None` si l'en-tête ne permet pas de calculer le cache
/// KV : on ne devine pas.
#[must_use]
pub fn need(weights: &Weights, context: u64) -> Option<Need> {
    need_from(
        weights.bytes,
        weights.kv_bytes_per_token,
        weights.vocabulary,
        context,
    )
}

/// [`need`] à partir des seuls nombres : taille du fichier, cache KV par token, vocabulaire.
/// C'est ainsi qu'une entrée du catalogue se dit avant d'être téléchargée.
#[must_use]
pub fn need_from(
    bytes: u64,
    kv_bytes_per_token: Option<u64>,
    vocabulary: Option<u64>,
    context: u64,
) -> Option<Need> {
    let kv_cache = kv_bytes_per_token?.checked_mul(context)?;
    let compute = vocabulary
        .and_then(|v| v.checked_mul(MICRO_BATCH * 4))
        .unwrap_or(UNKNOWN_COMPUTE_BYTES)
        .checked_add(RUNTIME_BYTES)?;
    let total = bytes.checked_add(kv_cache)?.checked_add(compute)?;
    Some(Need {
        context,
        weights: bytes,
        kv_cache,
        compute,
        total,
    })
}

/// La mémoire de la machine, lue dans `/proc/meminfo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct System {
    /// Mémoire vive totale.
    pub total: u64,
    /// Mémoire disponible sans paginer (`MemAvailable`).
    pub available: u64,
}

/// Ce qu'une demande devient face à la mémoire de la machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// La mémoire disponible suffit.
    Fits,
    /// La machine la tiendrait, mais pas en ce moment : d'autres programmes, ou un autre
    /// modèle, occupent ce qui manque.
    Tight,
    /// Même vide, la machine ne la tient pas en gardant [`SYSTEM_RESERVE`] au système.
    TooLarge,
}

impl System {
    /// Où `need` octets tombent sur cette machine.
    #[must_use]
    pub fn fit(&self, need: u64) -> Fit {
        if need.saturating_add(SYSTEM_RESERVE) > self.total {
            Fit::TooLarge
        } else if need > self.available {
            Fit::Tight
        } else {
            Fit::Fits
        }
    }
}

/// La mémoire de cette machine ; `None` hors de Linux ou si `/proc` est illisible.
#[must_use]
pub fn system() -> Option<System> {
    parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

/// Lit `MemTotal` et `MemAvailable` (en kio) d'un texte au format de `/proc/meminfo`.
#[must_use]
pub fn parse_meminfo(text: &str) -> Option<System> {
    let field = |name: &str| {
        text.lines().find_map(|line| {
            let rest = line.strip_prefix(name)?.strip_prefix(':')?;
            let kib: u64 = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
            kib.checked_mul(1024)
        })
    };
    Some(System {
        total: field("MemTotal")?,
        available: field("MemAvailable")?,
    })
}

/// Une estimation et ce qu'elle devient sur cette machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assessment {
    /// Ce que le poids demande.
    #[serde(flatten)]
    pub need: Need,
    /// Où la demande tombe ; absent si la mémoire de la machine est inconnue.
    pub fit: Option<Fit>,
}

/// Estime `weights` à la fenêtre `context` et le confronte à `system`.
#[must_use]
pub fn assess(weights: &Weights, context: u64, system: Option<&System>) -> Option<Assessment> {
    Some(assess_need(need(weights, context)?, system))
}

/// Une demande confrontée à la mémoire de la machine.
#[must_use]
pub fn assess_need(need: Need, system: Option<&System>) -> Assessment {
    Assessment {
        need,
        fit: system.map(|s| s.fit(need.total)),
    }
}

impl Fit {
    /// Le verdict, dit à l'humain.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Fits => "tient en mémoire",
            Self::Tight => "tient, mais la mémoire libre manque en ce moment",
            Self::TooLarge => "ne tient pas en mémoire sur cette machine",
        }
    }
}

/// Des octets en gigaoctets au dixième, pour l'humain : `5,9 Go` ; en téraoctets au-delà.
#[must_use]
pub fn gigabytes(bytes: u64) -> String {
    let (valeur, unite) = if bytes >= 1_000_000_000_000 {
        (bytes as f64 / 1e12, "To")
    } else {
        (bytes as f64 / 1e9, "Go")
    };
    format!("{valeur:.1} {unite}").replace('.', ",")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poids(bytes: u64, kv: Option<u64>, vocabulary: Option<u64>) -> Weights {
        Weights {
            path: "m.gguf".into(),
            bytes,
            version: 3,
            architecture: Some("qwen3".into()),
            name: None,
            size_label: None,
            quantization: None,
            context_length: Some(40_960),
            layers: Some(36),
            tensors: 0,
            kv_bytes_per_token: kv,
            vocabulary,
            template: None,
        }
    }

    #[test]
    fn la_demande_compte_le_fichier_le_cache_de_la_fenetre_et_les_logits() {
        // Qwen3 8B en Q4_K_M : 36 couches, 8 têtes KV de 128 ; 151 936 tokens de vocabulaire.
        let kv = 36 * 8 * (128 + 128) * 2;
        let w = poids(5_027_783_488, Some(kv), Some(151_936));
        let n = need(&w, 4096).unwrap();
        assert_eq!(n.kv_cache, 4096 * 147_456);
        assert_eq!(n.kv_cache, 576 << 20);
        assert_eq!(n.compute, 151_936 * 512 * 4 + (192 << 20));
        assert_eq!(n.total, n.weights + n.kv_cache + n.compute);
        // La fenêtre double, le cache aussi ; le reste ne bouge pas.
        let deux = need(&w, 8192).unwrap();
        assert_eq!(deux.kv_cache, 2 * n.kv_cache);
        assert_eq!(deux.total - n.total, n.kv_cache);
        // Sans de quoi calculer le cache, on ne devine pas ; sans vocabulaire, un tampon supposé.
        assert_eq!(need(&poids(1, None, Some(10)), 4096), None);
        assert_eq!(
            need(&poids(1, Some(1), None), 1).unwrap().compute,
            UNKNOWN_COMPUTE_BYTES + RUNTIME_BYTES
        );
        // Des nombres hostiles ne débordent pas.
        assert_eq!(need(&poids(u64::MAX, Some(1), None), 1), None);
        assert_eq!(need(&poids(1, Some(u64::MAX), None), 2), None);
    }

    #[test]
    fn la_memoire_de_la_machine_se_lit_et_dit_ce_qui_tient() {
        let m = parse_meminfo(
            "MemTotal:       16303132 kB\nMemFree:  1000 kB\nMemAvailable:    9123456 kB\n",
        )
        .unwrap();
        assert_eq!(m.total, 16_303_132 * 1024);
        assert_eq!(m.available, 9_123_456 * 1024);
        assert_eq!(m.fit(4_000_000_000), Fit::Fits);
        assert_eq!(m.fit(12_000_000_000), Fit::Tight);
        assert_eq!(m.fit(m.total - SYSTEM_RESERVE + 1), Fit::TooLarge);
        assert_eq!(parse_meminfo("MemTotal: 1 kB\n"), None);
        assert_eq!(parse_meminfo("MemTotalX: 1 kB\nMemAvailable: 1 kB"), None);
        assert_eq!(gigabytes(5_872_025_600), "5,9 Go");
        assert_eq!(gigabytes(6_711_400_000_000), "6,7 To");
        let w = poids(4_000_000_000, Some(1000), Some(1000));
        let a = assess(&w, 4096, Some(&m)).unwrap();
        assert_eq!(a.fit, Some(Fit::Fits));
        assert_eq!(assess(&w, 4096, None).unwrap().fit, None);
        let json = serde_json::to_value(a).unwrap();
        assert_eq!(json["total"], a.need.total);
        assert_eq!(json["context"], 4096);
        assert_eq!(json["fit"], "fits");
        assert_eq!(
            serde_json::to_value(Fit::TooLarge).unwrap(),
            serde_json::json!("too_large")
        );
    }
}
