//! Scellement : signature périodique de la tête de chaîne.
//!
//! Le chaînage par hachage détecte toute modification locale, mais pas une réécriture complète et
//! cohérente par un attaquant qui aurait les droits d'écriture. Le sceau signé ferme cette porte :
//! reproduire un sceau exige la clé, détenue par le daemon (et scellée par le TPM quand il y en a
//! un).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _, VerifyingKey};
use serde::{Deserialize, Serialize};

/// Sceau : signature de la tête de chaîne à un instant donné.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seal {
    /// Dernière séquence couverte.
    pub up_to_seq: u64,
    /// Empreinte de cette dernière entrée.
    pub head: String,
    /// Signature, `ed25519:<base64>`.
    pub signature: String,
    /// Clé publique du signataire, `ed25519:<base64>`.
    pub signer: String,
}

/// Signataire de sceaux.
#[derive(Debug)]
pub struct Sealer {
    key: SigningKey,
}

impl Sealer {
    /// Construit un signataire à partir d'une clé.
    #[must_use]
    pub const fn new(key: SigningKey) -> Self {
        Self { key }
    }

    /// Génère un signataire avec une clé neuve.
    #[must_use]
    pub fn generate() -> Self {
        Self::new(SigningKey::generate(&mut rand::rngs::OsRng))
    }

    /// Clé publique, encodée.
    #[must_use]
    pub fn public(&self) -> String {
        format!(
            "ed25519:{}",
            B64.encode(self.key.verifying_key().to_bytes())
        )
    }

    /// Produit un sceau pour la tête de chaîne donnée.
    #[must_use]
    pub fn seal(&self, up_to_seq: u64, head: &str) -> Seal {
        let message = format!("{up_to_seq}:{head}");
        let signature: Signature = self.key.sign(message.as_bytes());
        Seal {
            up_to_seq,
            head: head.to_owned(),
            signature: format!("ed25519:{}", B64.encode(signature.to_bytes())),
            signer: self.public(),
        }
    }

    /// Vérifie un sceau contre la clé publique qu'il déclare.
    #[must_use]
    pub fn verify(&self, seal: &Seal) -> bool {
        verify_seal(seal)
    }
}

/// Vérifie un sceau de façon autonome, à partir de la clé publique qu'il porte.
#[must_use]
pub fn verify_seal(seal: &Seal) -> bool {
    let Some(key_raw) = seal.signer.strip_prefix("ed25519:") else {
        return false;
    };
    let Ok(key_bytes) = B64.decode(key_raw) else {
        return false;
    };
    let Ok(key_array): Result<[u8; 32], _> = key_bytes.try_into() else {
        return false;
    };
    let Ok(key) = VerifyingKey::from_bytes(&key_array) else {
        return false;
    };
    let Some(sig_raw) = seal.signature.strip_prefix("ed25519:") else {
        return false;
    };
    let Ok(sig_bytes) = B64.decode(sig_raw) else {
        return false;
    };
    let Ok(sig_array): Result<[u8; 64], _> = sig_bytes.try_into() else {
        return false;
    };
    let message = format!("{}:{}", seal.up_to_seq, seal.head);
    key.verify(message.as_bytes(), &Signature::from_bytes(&sig_array))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sceau_valide() {
        let sealer = Sealer::generate();
        let seal = sealer.seal(42, "blake3:abc");
        assert!(verify_seal(&seal));
    }

    #[test]
    fn sceau_altere_refuse() {
        let sealer = Sealer::generate();
        let mut seal = sealer.seal(42, "blake3:abc");
        seal.head = "blake3:def".into();
        assert!(!verify_seal(&seal));

        let mut seal2 = sealer.seal(42, "blake3:abc");
        seal2.up_to_seq = 43;
        assert!(!verify_seal(&seal2));
    }

    #[test]
    fn sceau_d_une_autre_cle_refuse() {
        let a = Sealer::generate();
        let b = Sealer::generate();
        let mut seal = a.seal(1, "blake3:x");
        // L'attaquant remplace la clé déclarée par la sienne sans pouvoir resigner.
        seal.signer = b.public();
        assert!(!verify_seal(&seal));
    }

    #[test]
    fn sceau_mal_forme_refuse() {
        let sealer = Sealer::generate();
        let mut seal = sealer.seal(1, "blake3:x");
        seal.signature = "pas-un-prefixe".into();
        assert!(!verify_seal(&seal));
    }
}
