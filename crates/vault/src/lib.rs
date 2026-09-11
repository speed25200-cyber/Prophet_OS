//! Coffre à secrets.
//!
//! Règle unique, dont tout le reste découle : **un modèle ne voit jamais la valeur d'un secret**.
//! Un agent demande « appelle GitHub avec mon identité » et reçoit une *référence*. C'est le proxy
//! de sortie qui substitue la valeur au dernier moment, hors de portée du modèle.
//!
//! Le coffre chiffre au repos (ChaCha20-Poly1305). La clé vit dans un fichier en mode 0600, scellé
//! par le TPM là où il y en a un.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng, rand_core::RngCore as _};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use serde::{Deserialize, Serialize};

/// Erreur du coffre.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    /// Erreur d'entrée-sortie.
    #[error("erreur d'entrée-sortie : {0}")]
    Io(#[from] std::io::Error),
    /// Format de stockage illisible.
    #[error("coffre illisible : {0}")]
    Format(#[from] serde_json::Error),
    /// Déchiffrement impossible : clé fausse ou contenu altéré.
    #[error("déchiffrement impossible : clé incorrecte ou contenu altéré")]
    Decrypt,
    /// Clé de coffre absente ou mal formée.
    #[error("clé de coffre invalide")]
    BadKey,
    /// Secret inconnu.
    #[error("secret inconnu : {0}")]
    Unknown(String),
}

/// Référence opaque à un secret, seule chose qu'un agent ou un modèle peut voir.
///
/// Elle ne contient aucune information sur la valeur : ni longueur, ni empreinte, ni fragment.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretRef(String);

impl SecretRef {
    /// Construit une référence à partir d'un nom.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self(format!("prophet-secret:{name}"))
    }

    /// Nom du secret référencé.
    #[must_use]
    pub fn name(&self) -> &str {
        self.0.strip_prefix("prophet-secret:").unwrap_or(&self.0)
    }

    /// Vue textuelle, telle qu'elle apparaît dans un en-tête à substituer.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Reconnaît une référence dans un texte.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        text.strip_prefix("prophet-secret:").map(Self::new)
    }
}

impl std::fmt::Display for SecretRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Description d'un secret, sans sa valeur. C'est ce que `secrets.list_refs` renvoie.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretInfo {
    /// Nom.
    pub name: String,
    /// Domaines auxquels ce secret peut être présenté.
    pub domains: Vec<String>,
    /// En-tête dans lequel il est substitué.
    pub header: String,
    /// Description libre.
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    info: SecretInfo,
    nonce: String,
    ciphertext: String,
}

/// Coffre chiffré sur disque.
pub struct Vault {
    path: PathBuf,
    cipher: ChaCha20Poly1305,
    entries: BTreeMap<String, Entry>,
}

/// Rendu volontairement muet : un coffre ne s'imprime pas dans un journal ni dans un message
/// d'erreur, même par inadvertance.
impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("path", &self.path)
            .field("secrets", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl Vault {
    /// Ouvre ou crée un coffre, avec une clé stockée dans `key_path` (créée si absente).
    ///
    /// # Erreurs
    /// Si la clé ou le coffre sont illisibles.
    pub fn open(path: impl AsRef<Path>, key_path: impl AsRef<Path>) -> Result<Self, VaultError> {
        let path = path.as_ref().to_path_buf();
        let key = load_or_create_key(key_path.as_ref())?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
        let entries = if path.exists() {
            serde_json::from_str(&std::fs::read_to_string(&path)?)?
        } else {
            BTreeMap::new()
        };
        Ok(Self {
            path,
            cipher,
            entries,
        })
    }

    /// Enregistre un secret et renvoie sa référence.
    ///
    /// # Erreurs
    /// Si le chiffrement ou l'écriture échouent.
    pub fn put(&mut self, info: SecretInfo, value: &str) -> Result<SecretRef, VaultError> {
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(nonce, value.as_bytes())
            .map_err(|_| VaultError::Decrypt)?;
        let name = info.name.clone();
        self.entries.insert(
            name.clone(),
            Entry {
                info,
                nonce: B64.encode(nonce_bytes),
                ciphertext: B64.encode(ciphertext),
            },
        );
        self.save()?;
        Ok(SecretRef::new(&name))
    }

    /// Liste les secrets, **sans** leurs valeurs. C'est tout ce qu'un agent obtient.
    #[must_use]
    pub fn list(&self) -> Vec<SecretInfo> {
        self.entries.values().map(|e| e.info.clone()).collect()
    }

    /// Informations d'un secret, sans sa valeur.
    #[must_use]
    pub fn info(&self, name: &str) -> Option<&SecretInfo> {
        self.entries.get(name).map(|e| &e.info)
    }

    /// Révèle la valeur d'un secret.
    ///
    /// **Réservé au proxy de sortie**, au moment de la substitution. Aucun chemin de code menant à
    /// un modèle, à un journal ou à un résultat d'outil n'appelle cette fonction.
    ///
    /// # Erreurs
    /// Secret inconnu, ou contenu altéré.
    pub fn reveal(&self, name: &str) -> Result<String, VaultError> {
        let entry = self
            .entries
            .get(name)
            .ok_or_else(|| VaultError::Unknown(name.to_owned()))?;
        let nonce_bytes = B64.decode(&entry.nonce).map_err(|_| VaultError::Decrypt)?;
        let ciphertext = B64
            .decode(&entry.ciphertext)
            .map_err(|_| VaultError::Decrypt)?;
        let plaintext = self
            .cipher
            .decrypt(Nonce::from_slice(&nonce_bytes), ciphertext.as_ref())
            .map_err(|_| VaultError::Decrypt)?;
        String::from_utf8(plaintext).map_err(|_| VaultError::Decrypt)
    }

    /// Supprime un secret.
    ///
    /// # Erreurs
    /// Si l'écriture échoue.
    pub fn delete(&mut self, name: &str) -> Result<bool, VaultError> {
        let removed = self.entries.remove(name).is_some();
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    /// Remplace la valeur d'un secret existant en conservant ses métadonnées.
    ///
    /// # Erreurs
    /// Secret inconnu, ou écriture en échec.
    pub fn rotate(&mut self, name: &str, value: &str) -> Result<(), VaultError> {
        let info = self
            .entries
            .get(name)
            .map(|e| e.info.clone())
            .ok_or_else(|| VaultError::Unknown(name.to_owned()))?;
        self.put(info, value)?;
        Ok(())
    }

    /// Vrai si le secret peut être présenté à cet hôte.
    #[must_use]
    pub fn allowed_for(&self, name: &str, host: &str) -> bool {
        self.entries.get(name).is_some_and(|entry| {
            entry.info.domains.iter().any(|pattern| {
                pattern == host
                    || pattern.strip_prefix("*.").is_some_and(|suffix| {
                        host == suffix || host.ends_with(&format!(".{suffix}"))
                    })
            })
        })
    }

    fn save(&self) -> Result<(), VaultError> {
        use std::os::unix::fs::PermissionsExt as _;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.entries)?;
        let temp = self.path.with_extension("tmp");
        std::fs::write(&temp, text)?;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600))?;
        std::fs::rename(temp, &self.path)?;
        Ok(())
    }
}

fn load_or_create_key(path: &Path) -> Result<[u8; 32], VaultError> {
    use std::os::unix::fs::PermissionsExt as _;
    if path.exists() {
        let raw = std::fs::read_to_string(path)?;
        let bytes = B64.decode(raw.trim()).map_err(|_| VaultError::BadKey)?;
        return bytes.try_into().map_err(|_| VaultError::BadKey);
    }
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, B64.encode(key))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coffre(dir: &Path) -> Vault {
        Vault::open(dir.join("vault.json"), dir.join("key")).unwrap()
    }

    fn info(name: &str, domains: &[&str]) -> SecretInfo {
        SecretInfo {
            name: name.to_owned(),
            domains: domains.iter().map(|d| (*d).to_owned()).collect(),
            header: "Authorization".to_owned(),
            description: String::new(),
        }
    }

    #[test]
    fn enregistrement_et_revelation() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = coffre(dir.path());
        let reference = v
            .put(info("github", &["api.github.com"]), "ghp_valeur_secrete")
            .unwrap();
        assert_eq!(reference.as_str(), "prophet-secret:github");
        assert_eq!(v.reveal("github").unwrap(), "ghp_valeur_secrete");
    }

    #[test]
    fn la_liste_ne_contient_aucune_valeur() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = coffre(dir.path());
        v.put(info("github", &["api.github.com"]), "ghp_valeur_secrete")
            .unwrap();
        let liste = v.list();
        let rendu = serde_json::to_string(&liste).unwrap();
        assert!(
            !rendu.contains("ghp_valeur_secrete"),
            "la valeur ne doit jamais apparaître : {rendu}"
        );
        assert_eq!(liste[0].name, "github");
    }

    #[test]
    fn le_fichier_sur_disque_ne_contient_pas_la_valeur_en_clair() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = coffre(dir.path());
        v.put(info("github", &["api.github.com"]), "ghp_valeur_secrete")
            .unwrap();
        let brut = std::fs::read_to_string(dir.path().join("vault.json")).unwrap();
        assert!(!brut.contains("ghp_valeur_secrete"), "{brut}");
    }

    #[test]
    fn la_reference_ne_revele_rien_de_la_valeur() {
        let reference = SecretRef::new("github");
        assert_eq!(reference.name(), "github");
        assert_eq!(SecretRef::parse("prophet-secret:github"), Some(reference));
        assert_eq!(SecretRef::parse("Bearer ghp_xxx"), None);
    }

    #[test]
    fn contenu_altere_refuse() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut v = coffre(dir.path());
            v.put(info("github", &["api.github.com"]), "valeur")
                .unwrap();
        }
        let path = dir.path().join("vault.json");
        let mut contenu: BTreeMap<String, Entry> =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let entry = contenu.get_mut("github").unwrap();
        let mut bytes = B64.decode(&entry.ciphertext).unwrap();
        bytes[0] ^= 0xff;
        entry.ciphertext = B64.encode(bytes);
        std::fs::write(&path, serde_json::to_string(&contenu).unwrap()).unwrap();

        let v = coffre(dir.path());
        assert!(matches!(v.reveal("github"), Err(VaultError::Decrypt)));
    }

    #[test]
    fn cle_differente_ne_dechiffre_pas() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut v = coffre(dir.path());
            v.put(info("github", &["api.github.com"]), "valeur")
                .unwrap();
        }
        let autre =
            Vault::open(dir.path().join("vault.json"), dir.path().join("autre-key")).unwrap();
        assert!(matches!(autre.reveal("github"), Err(VaultError::Decrypt)));
    }

    #[test]
    fn secret_limite_a_ses_domaines() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = coffre(dir.path());
        v.put(info("github", &["api.github.com", "*.exemple.fr"]), "x")
            .unwrap();
        assert!(v.allowed_for("github", "api.github.com"));
        assert!(v.allowed_for("github", "www.exemple.fr"));
        assert!(v.allowed_for("github", "exemple.fr"));
        assert!(!v.allowed_for("github", "evil.com"));
        assert!(
            !v.allowed_for("github", "api.github.com.evil.com"),
            "un suffixe trompeur ne doit pas correspondre"
        );
        assert!(!v.allowed_for("inconnu", "api.github.com"));
    }

    #[test]
    fn rotation_et_suppression() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = coffre(dir.path());
        v.put(info("github", &["api.github.com"]), "ancienne")
            .unwrap();
        v.rotate("github", "nouvelle").unwrap();
        assert_eq!(v.reveal("github").unwrap(), "nouvelle");
        assert_eq!(v.info("github").unwrap().domains, vec!["api.github.com"]);
        assert!(v.delete("github").unwrap());
        assert!(!v.delete("github").unwrap());
        assert!(v.reveal("github").is_err());
    }

    #[test]
    fn rotation_d_un_secret_inconnu() {
        let dir = tempfile::tempdir().unwrap();
        let mut v = coffre(dir.path());
        assert!(matches!(
            v.rotate("absent", "x"),
            Err(VaultError::Unknown(_))
        ));
    }

    #[test]
    fn persistance_entre_deux_ouvertures() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut v = coffre(dir.path());
            v.put(info("github", &["api.github.com"]), "valeur")
                .unwrap();
        }
        let v = coffre(dir.path());
        assert_eq!(v.reveal("github").unwrap(), "valeur");
    }

    #[test]
    fn droits_du_fichier_de_cle() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let _ = coffre(dir.path());
        let mode = std::fs::metadata(dir.path().join("key"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "la clé doit être illisible par les autres"
        );
    }
}
