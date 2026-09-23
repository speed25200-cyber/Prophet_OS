//! Le catalogue des poids que le système sait télécharger (M8-T7, ADR 0046).
//!
//! Une entrée dit d'où vient un fichier de poids et ce qu'il doit être : son adresse, épinglée
//! sur une révision, son empreinte SHA-256 et les hôtes par lesquels le téléchargement peut
//! passer, redirections comprises. Le catalogue est écrit dans le dépôt et compilé dans les
//! binaires : il fait partie du système, comme le reste de `/nix/store`, et rien sur la machine
//! ne le réécrit. `PROPHET_MODEL_CATALOG` le remplace — une variable du service, posée par la
//! configuration du système, jamais par une tâche.
//!
//! N'y entre qu'un fichier dont l'empreinte publiée a été relevée : une entrée sans empreinte
//! serait un téléchargement que rien ne vérifie.

use std::path::PathBuf;

use prophet_types::pattern::{Family, matches, validate};
use serde::{Deserialize, Serialize};

/// Le catalogue compilé dans les binaires.
pub const BUILTIN: &str = include_str!("../catalogue.json");

/// Sous-dossier du dossier des poids où vont les fichiers téléchargés : le seul que le
/// téléchargement écrit et que la suppression touche.
pub const PULLED_DIR: &str = "catalogue";

/// Une entrée du catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Identifiant (`qwen3-1.7b-q8`), celui que `prophet model pull` prend.
    pub id: String,
    /// Nom pour l'humain.
    pub name: String,
    /// Nom du fichier posé dans le dossier des téléchargements.
    pub file: String,
    /// Adresse du fichier, épinglée sur une révision.
    pub url: String,
    /// Empreinte SHA-256 publiée, en hexadécimal minuscule.
    pub sha256: String,
    /// Taille exacte en octets, quand elle est connue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    /// Hôtes par lesquels le téléchargement peut passer (motifs de domaine, `*.hf.co`).
    pub hosts: Vec<String>,
    /// Quantification annoncée.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantization: Option<String>,
    /// Licence des poids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licence: Option<String>,
    /// Une phrase pour l'humain : à quoi sert ce modèle ici.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Cache KV par token, relevé dans l'en-tête du fichier (voir
    /// [`crate::weights::Weights::kv_bytes_per_token`]) : la mémoire se dit avant de télécharger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kv_bytes_per_token: Option<u64>,
    /// Taille du vocabulaire, relevée dans l'en-tête du fichier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocabulary: Option<u64>,
}

/// Le catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalogue {
    /// Version du format.
    pub version: u32,
    /// Les entrées, dans l'ordre où l'humain les voit.
    pub entries: Vec<Entry>,
}

impl Catalogue {
    /// Lit et valide un catalogue.
    ///
    /// # Errors
    /// JSON illisible, version inconnue, identifiant en double ou entrée invalide.
    pub fn parse(text: &str) -> Result<Self, String> {
        let catalogue: Self =
            serde_json::from_str(text).map_err(|e| format!("catalogue illisible : {e}"))?;
        if catalogue.version != 1 {
            return Err(format!(
                "version de catalogue inconnue : {}",
                catalogue.version
            ));
        }
        let mut vus = std::collections::BTreeSet::new();
        for entry in &catalogue.entries {
            entry.validate()?;
            if !vus.insert(entry.id.as_str()) {
                return Err(format!("identifiant en double : {}", entry.id));
            }
        }
        Ok(catalogue)
    }

    /// Le catalogue compilé dans les binaires.
    ///
    /// # Panics
    /// Jamais sur l'arbre du dépôt : un essai lit ce catalogue à chaque `just check`.
    #[must_use]
    pub fn builtin() -> Self {
        Self::parse(BUILTIN).expect("le catalogue du dépôt est valide")
    }

    /// Le catalogue de cette machine : le fichier que `PROPHET_MODEL_CATALOG` nomme, sinon
    /// celui des binaires.
    ///
    /// # Errors
    /// Fichier nommé illisible ou invalide : on ne retombe pas sur un autre catalogue en
    /// silence.
    pub fn load() -> Result<Self, String> {
        match std::env::var_os("PROPHET_MODEL_CATALOG") {
            Some(chemin) => {
                let texte = std::fs::read_to_string(&chemin).map_err(|e| {
                    format!(
                        "catalogue {} illisible : {e}",
                        PathBuf::from(&chemin).display()
                    )
                })?;
                Self::parse(&texte)
            }
            None => Ok(Self::builtin()),
        }
    }

    /// L'entrée qui porte cet identifiant.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }
}

impl Entry {
    /// Ce que le moteur réservera pour servir cette entrée à la fenêtre `context`, d'après la
    /// taille et l'en-tête relevés au catalogue ; `None` s'ils n'y sont pas.
    #[must_use]
    pub fn memory(&self, context: u64) -> Option<crate::memory::Need> {
        crate::memory::need_from(
            self.bytes?,
            self.kv_bytes_per_token,
            self.vocabulary,
            context,
        )
    }

    /// Vérifie une entrée : identifiant et nom de fichier sûrs, empreinte bien formée, adresse
    /// HTTP(S) dont l'hôte est parmi les hôtes permis.
    ///
    /// # Errors
    /// La première règle violée, avec l'entrée.
    pub fn validate(&self) -> Result<(), String> {
        let id_ok = !self.id.is_empty()
            && self.id.len() <= 64
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '.'));
        if !id_ok {
            return Err(format!("identifiant invalide : {:?}", self.id));
        }
        let fichier_ok = !self.file.is_empty()
            && self.file.len() <= 200
            && !self.file.starts_with('.')
            && self
                .file
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            && self.file.to_ascii_lowercase().ends_with(".gguf");
        if !fichier_ok {
            return Err(format!(
                "{} : nom de fichier invalide : {:?}",
                self.id, self.file
            ));
        }
        let empreinte_ok = self.sha256.len() == 64
            && self
                .sha256
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
        if !empreinte_ok {
            return Err(format!("{} : empreinte SHA-256 invalide", self.id));
        }
        if self.hosts.is_empty() {
            return Err(format!("{} : aucun hôte permis", self.id));
        }
        for hote in &self.hosts {
            if hote == "*" || validate(Family::Domain, hote).is_err() {
                return Err(format!("{} : hôte permis invalide : {hote:?}", self.id));
            }
        }
        let adresse = Url::parse(&self.url).map_err(|e| format!("{} : {e}", self.id))?;
        if !self.permits(&adresse.authority) {
            return Err(format!(
                "{} : l'hôte de l'adresse ({}) n'est pas parmi les hôtes permis",
                self.id, adresse.authority
            ));
        }
        Ok(())
    }

    /// Vrai si le téléchargement peut passer par cet hôte (`hôte` ou `hôte:port`).
    #[must_use]
    pub fn permits(&self, authority: &str) -> bool {
        self.hosts
            .iter()
            .any(|motif| matches(Family::Domain, motif, authority, ""))
    }
}

/// Une adresse `http://` ou `https://`, découpée pour une requête en forme absolue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    /// `http` ou `https`.
    pub scheme: String,
    /// Hôte, avec son port s'il est donné.
    pub authority: String,
    /// Chemin et requête, commençant par `/`.
    pub path: String,
}

impl Url {
    /// Découpe une adresse.
    ///
    /// # Errors
    /// Schéma autre que HTTP(S), hôte vide, identifiants dans l'adresse, caractères de contrôle.
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.chars().any(|c| c.is_control() || c == ' ') {
            return Err(format!("adresse invalide : {text:?}"));
        }
        let (scheme, reste) = text
            .split_once("://")
            .ok_or_else(|| format!("adresse sans schéma : {text:?}"))?;
        if scheme != "http" && scheme != "https" {
            return Err(format!("schéma refusé : {scheme}"));
        }
        let (authority, path) = reste
            .find(['/', '?'])
            .map_or((reste, "/"), |i| (&reste[..i], &reste[i..]));
        if authority.is_empty() || authority.contains('@') {
            return Err(format!("hôte invalide dans {text:?}"));
        }
        let path = if path.starts_with('?') {
            format!("/{path}")
        } else {
            path.to_owned()
        };
        Ok(Self {
            scheme: scheme.to_owned(),
            authority: authority.to_ascii_lowercase(),
            path,
        })
    }

    /// Résout une redirection (`Location`) : absolue, ou relative à la racine de l'hôte.
    ///
    /// # Errors
    /// Adresse relative sans `/` initial, ou adresse absolue invalide.
    pub fn join(&self, location: &str) -> Result<Self, String> {
        if location.starts_with("//") {
            return Self::parse(&format!("{}:{location}", self.scheme));
        }
        if location.starts_with('/') {
            if location.chars().any(|c| c.is_control() || c == ' ') {
                return Err(format!("redirection invalide : {location:?}"));
            }
            return Ok(Self {
                path: location.to_owned(),
                ..self.clone()
            });
        }
        if location.contains("://") {
            return Self::parse(location);
        }
        Err(format!("redirection relative refusée : {location:?}"))
    }

    /// L'adresse entière.
    #[must_use]
    pub fn full(&self) -> String {
        format!("{}://{}{}", self.scheme, self.authority, self.path)
    }
}

/// Le dossier des poids téléchargés : `PROPHET_PULL_DIR`, sinon le sous-dossier
/// [`PULLED_DIR`] du dossier des poids.
#[must_use]
pub fn pulled_dir() -> PathBuf {
    std::env::var_os("PROPHET_PULL_DIR")
        .map_or_else(|| crate::weights::dir().join(PULLED_DIR), PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entree() -> Entry {
        Catalogue::builtin().entries[0].clone()
    }

    #[test]
    fn le_catalogue_du_depot_est_valide_et_epingle() {
        let catalogue = Catalogue::builtin();
        assert!(!catalogue.entries.is_empty());
        for entry in &catalogue.entries {
            // Une révision épinglée, pas une branche qui bouge sous l'empreinte.
            assert!(!entry.url.contains("/resolve/main/"), "{}", entry.url);
            assert!(entry.url.starts_with("https://"), "{}", entry.url);
        }
        assert!(catalogue.get("qwen3-1.7b-q8").is_some());
        assert!(catalogue.get("inconnu").is_none());
        // Chaque entrée dit la mémoire qu'elle demandera, avant d'être téléchargée.
        for entry in &catalogue.entries {
            let besoin = entry
                .memory(4096)
                .unwrap_or_else(|| panic!("{} : taille ou en-tête manquant", entry.id));
            assert!(besoin.total > besoin.weights, "{}", entry.id);
        }
        // Phi-3 mini, sans GQA, demande plus de cache que Llama 3.2 3B, plus gros fichier égal.
        let phi = catalogue
            .get("phi-3-mini-q4")
            .unwrap()
            .memory(4096)
            .unwrap();
        let llama = catalogue
            .get("llama-3.2-3b-q4")
            .unwrap()
            .memory(4096)
            .unwrap();
        assert!(phi.kv_cache > 3 * llama.kv_cache);
    }

    #[test]
    fn une_entree_invalide_est_refusee_avec_sa_raison() {
        let mut e = entree();
        e.file = "../../etc/passwd.gguf".into();
        assert!(e.validate().unwrap_err().contains("nom de fichier"));
        let mut e = entree();
        e.file = ".cache.gguf".into();
        assert!(e.validate().is_err());
        let mut e = entree();
        e.sha256 = "ABC".into();
        assert!(e.validate().unwrap_err().contains("SHA-256"));
        let mut e = entree();
        e.hosts = vec!["*".into()];
        assert!(e.validate().unwrap_err().contains("hôte permis"));
        let mut e = entree();
        e.url = "https://ailleurs.example/x.gguf".into();
        assert!(e.validate().unwrap_err().contains("pas parmi"));
        let mut e = entree();
        e.url = "ftp://huggingface.co/x.gguf".into();
        assert!(e.validate().unwrap_err().contains("schéma"));
        let mut e = entree();
        e.url = "https://moi@huggingface.co/x.gguf".into();
        assert!(e.validate().is_err());
        let texte = format!(
            "{{\"version\":1,\"entries\":[{0},{0}]}}",
            serde_json::to_string(&entree()).unwrap()
        );
        assert!(Catalogue::parse(&texte).unwrap_err().contains("double"));
        assert!(Catalogue::parse("{\"version\":2,\"entries\":[]}").is_err());
    }

    #[test]
    fn les_hotes_permis_couvrent_les_redirections_du_depot_de_poids() {
        let e = entree();
        assert!(e.permits("huggingface.co"));
        assert!(e.permits("cdn-lfs.huggingface.co"));
        assert!(e.permits("cas-bridge.xethub.hf.co"));
        assert!(e.permits("huggingface.co:443"));
        assert!(!e.permits("huggingface.co.evil.example"));
        assert!(!e.permits("evilhf.co"));
    }

    #[test]
    fn une_adresse_se_decoupe_et_une_redirection_se_resout() {
        let u = Url::parse("https://huggingface.co/Qwen/x/resolve/abc/f.gguf").unwrap();
        assert_eq!(u.authority, "huggingface.co");
        assert_eq!(u.path, "/Qwen/x/resolve/abc/f.gguf");
        let r = u.join("https://cas-bridge.xethub.hf.co/xet?sig=1").unwrap();
        assert_eq!(r.authority, "cas-bridge.xethub.hf.co");
        assert_eq!(r.path, "/xet?sig=1");
        let r = u.join("/api/resolve-cache/f.gguf").unwrap();
        assert_eq!(r.full(), "https://huggingface.co/api/resolve-cache/f.gguf");
        assert_eq!(
            u.join("//cdn.hf.co/f").unwrap().full(),
            "https://cdn.hf.co/f"
        );
        assert!(u.join("f.gguf").is_err());
        assert!(Url::parse("http://127.0.0.1:8099").unwrap().path == "/");
        assert!(Url::parse("https://h/a b").is_err());
    }
}
