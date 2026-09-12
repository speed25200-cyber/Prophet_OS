//! Identité d'agent sur le réseau.
//!
//! Un service distant qui reçoit une requête d'un agent devrait pouvoir le savoir, et savoir pour
//! qui il agit. Aujourd'hui il ne le peut pas : une requête d'agent ressemble à une requête
//! d'humain, ce qui empêche tout traitement différencié, qu'il s'agisse de quotas, de journaux ou
//! de refus.
//!
//! L'en-tête ajouté ici est **signé** : il ne peut pas être fabriqué par la page qui le reçoit, ni
//! par l'agent lui-même, puisque la clé appartient au proxy. Il ne contient aucune donnée
//! personnelle : l'utilisateur y figure sous une empreinte stable, qui permet de reconnaître le
//! même utilisateur sans savoir qui il est.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use serde::{Deserialize, Serialize};

/// Nom de l'en-tête ajouté aux requêtes sortantes.
pub const HEADER: &str = "X-Prophet-Agent";

/// Identité annoncée.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentIdentity {
    /// Identifiant de l'agent, tel qu'il figure dans son manifeste.
    pub agent: String,
    /// Identifiant de la tâche.
    pub task: String,
    /// Empreinte stable de l'utilisateur, sans rien révéler de lui.
    pub user_digest: String,
    /// Version du format.
    pub v: u8,
}

impl AgentIdentity {
    /// Construit une identité à partir d'une tâche.
    ///
    /// `salt` est propre à la machine : deux machines différentes produisent des empreintes
    /// différentes pour le même utilisateur, ce qui empêche de le suivre d'un système à l'autre.
    #[must_use]
    pub fn new(agent: &str, task: &str, user: &str, salt: &[u8]) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(salt);
        hasher.update(user.as_bytes());
        Self {
            agent: agent.to_owned(),
            task: task.to_owned(),
            user_digest: hasher
                .finalize()
                .to_hex()
                .to_string()
                .chars()
                .take(16)
                .collect(),
            v: 0,
        }
    }

    /// Valeur de l'en-tête, signée.
    ///
    /// # Errors
    /// Si la sérialisation échoue.
    pub fn to_header(&self, key: &ed25519_dalek::SigningKey) -> Result<String, String> {
        use ed25519_dalek::Signer as _;
        let charge = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        let signature = key.sign(&charge);
        Ok(format!(
            "{}.{}",
            B64.encode(charge),
            B64.encode(signature.to_bytes())
        ))
    }

    /// Vérifie et décode un en-tête.
    ///
    /// # Errors
    /// Format invalide ou signature incorrecte.
    pub fn from_header(header: &str, key: &ed25519_dalek::VerifyingKey) -> Result<Self, String> {
        use ed25519_dalek::Verifier as _;
        let (charge_b64, signature_b64) = header
            .split_once('.')
            .ok_or_else(|| "en-tête mal formé".to_owned())?;
        let charge = B64.decode(charge_b64).map_err(|e| e.to_string())?;
        let bytes = B64.decode(signature_b64).map_err(|e| e.to_string())?;
        let array: [u8; 64] = bytes
            .try_into()
            .map_err(|_| "signature de taille inattendue".to_owned())?;
        key.verify(&charge, &ed25519_dalek::Signature::from_bytes(&array))
            .map_err(|_| "signature invalide".to_owned())?;
        serde_json::from_slice(&charge).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn cle() -> SigningKey {
        SigningKey::generate(&mut rand::rngs::OsRng)
    }

    #[test]
    fn aller_retour_signe() {
        let k = cle();
        let identite = AgentIdentity::new("org.test.agent", "task:01", "hakik", b"sel");
        let entete = identite.to_header(&k).unwrap();
        assert_eq!(
            AgentIdentity::from_header(&entete, &k.verifying_key()).unwrap(),
            identite
        );
    }

    #[test]
    fn l_utilisateur_n_apparait_jamais_en_clair() {
        let identite = AgentIdentity::new("org.test.agent", "task:01", "hakik", b"sel");
        let rendu = serde_json::to_string(&identite).unwrap();
        assert!(!rendu.contains("hakik"), "{rendu}");
        assert_eq!(identite.user_digest.len(), 16);
    }

    #[test]
    fn le_sel_de_la_machine_empeche_le_suivi_entre_systemes() {
        let a = AgentIdentity::new("org.test.agent", "task:01", "hakik", b"machine-a");
        let b = AgentIdentity::new("org.test.agent", "task:01", "hakik", b"machine-b");
        assert_ne!(a.user_digest, b.user_digest);

        // Sur la même machine, l'empreinte est stable : un service peut reconnaître un habitué
        // sans savoir qui il est.
        let encore = AgentIdentity::new("org.test.agent", "task:02", "hakik", b"machine-a");
        assert_eq!(a.user_digest, encore.user_digest);
    }

    #[test]
    fn un_en_tete_fabrique_est_refuse() {
        let k = cle();
        let autre = cle();
        let identite = AgentIdentity::new("org.test.agent", "task:01", "hakik", b"sel");
        let entete = identite.to_header(&autre).unwrap();
        assert!(AgentIdentity::from_header(&entete, &k.verifying_key()).is_err());
    }

    #[test]
    fn un_en_tete_altere_est_refuse() {
        let k = cle();
        let identite = AgentIdentity::new("org.test.agent", "task:01", "hakik", b"sel");
        let entete = identite.to_header(&k).unwrap();
        let mut forge = AgentIdentity::new("org.autre.agent", "task:01", "hakik", b"sel")
            .to_header(&k)
            .unwrap();
        // On recolle la signature de l'original sur une charge différente.
        forge = format!(
            "{}.{}",
            forge.split('.').next().unwrap(),
            entete.split('.').nth(1).unwrap()
        );
        assert!(AgentIdentity::from_header(&forge, &k.verifying_key()).is_err());
    }

    #[test]
    fn un_en_tete_mal_forme_est_refuse() {
        let k = cle();
        for mauvais in ["", "sans-point", "a.b", "...."] {
            assert!(AgentIdentity::from_header(mauvais, &k.verifying_key()).is_err());
        }
    }
}
