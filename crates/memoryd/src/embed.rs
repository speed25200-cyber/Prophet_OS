//! Représentation vectorielle du texte.
//!
//! En service, les vecteurs viennent du petit modèle toujours résident. Pour que la mémoire soit
//! testable et utilisable sans modèle chargé, l'interface est abstraite et une implémentation de
//! repli, déterministe, fournit des vecteurs de sacs de mots.

/// Produit un vecteur à partir d'un texte.
pub trait Embedder: Send + Sync {
    /// Vecteur du texte.
    fn embed(&self, text: &str) -> Vec<f32>;

    /// Nombre de dimensions.
    fn dimensions(&self) -> usize;

    /// Nom, enregistré avec l'entrée : un changement de modèle invalide les vecteurs, et il faut
    /// pouvoir le détecter.
    fn name(&self) -> String;
}

/// Vecteur de sac de mots par hachage.
///
/// Déterministe, sans dépendance, sans modèle. Il ne capte pas la synonymie, mais il retrouve ce
/// qui partage des mots, ce qui suffit à rendre la mémoire utile dès le premier démarrage.
#[derive(Debug, Clone)]
pub struct HashEmbedder {
    dimensions: usize,
}

impl Default for HashEmbedder {
    fn default() -> Self {
        Self { dimensions: 256 }
    }
}

impl HashEmbedder {
    /// Vecteur de la dimension donnée.
    #[must_use]
    pub const fn new(dimensions: usize) -> Self {
        Self { dimensions }
    }
}

/// Découpe un texte en mots normalisés.
#[must_use]
pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.chars().count() > 2)
        .map(ToOwned::to_owned)
        .collect()
}

impl Embedder for HashEmbedder {
    fn embed(&self, text: &str) -> Vec<f32> {
        let mut vector = vec![0.0_f32; self.dimensions];
        for word in tokenize(text) {
            let hash = blake3::hash(word.as_bytes());
            let bytes = hash.as_bytes();
            let index = (u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
                % self.dimensions;
            // Le signe issu du hachage limite les collisions destructrices entre mots différents.
            let signe = if bytes[4].is_multiple_of(2) {
                1.0
            } else {
                -1.0
            };
            vector[index] += signe;
        }
        let norme = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norme > 0.0 {
            for value in &mut vector {
                *value /= norme;
            }
        }
        vector
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn name(&self) -> String {
        format!("hash-{}", self.dimensions)
    }
}

/// Similarité cosinus de deux vecteurs. Rend 0 si les dimensions diffèrent.
#[must_use]
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let produit: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = a.iter().map(|v| v * v).sum::<f32>().sqrt();
    let nb = b.iter().map(|v| v * v).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        produit / (na * nb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministe() {
        let e = HashEmbedder::default();
        assert_eq!(
            e.embed("les ventes du trimestre"),
            e.embed("les ventes du trimestre")
        );
    }

    #[test]
    fn textes_proches_plus_similaires_que_textes_eloignes() {
        let e = HashEmbedder::default();
        let reference = e.embed("le rapport des ventes du troisième trimestre");
        let proche = e.embed("rapport ventes trimestre");
        let loin = e.embed("recette de la tarte aux pommes");
        assert!(
            cosine(&reference, &proche) > cosine(&reference, &loin),
            "proche {} contre loin {}",
            cosine(&reference, &proche),
            cosine(&reference, &loin)
        );
    }

    #[test]
    fn similarite_bornee() {
        let e = HashEmbedder::default();
        let v = e.embed("quelque chose");
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-5);
        assert_eq!(cosine(&v, &[]), 0.0);
        assert_eq!(cosine(&v, &[0.0; 10]), 0.0);
    }

    #[test]
    fn les_mots_courts_sont_ignores() {
        assert_eq!(tokenize("le de la un à"), Vec::<String>::new());
        assert_eq!(
            tokenize("ventes 2026 trimestre"),
            vec!["ventes", "2026", "trimestre"]
        );
    }

    #[test]
    fn le_nom_du_modele_est_enregistrable() {
        assert_eq!(HashEmbedder::new(128).name(), "hash-128");
        assert_eq!(HashEmbedder::new(128).dimensions(), 128);
    }
}
