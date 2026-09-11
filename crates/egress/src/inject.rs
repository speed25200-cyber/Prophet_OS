//! Substitution des secrets au moment de la sortie.
//!
//! L'agent écrit `Authorization: prophet-secret:github`. Le modèle n'a donc jamais vu la valeur,
//! et ne peut pas la divulguer, ni par erreur ni sous injection. Le proxy la substitue juste avant
//! d'émettre, après avoir vérifié que ce secret a le droit d'être présenté à cet hôte.

use vault::{SecretRef, Vault, VaultError};

/// Erreur de substitution.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InjectionError {
    /// Le secret n'existe pas.
    #[error("secret inconnu : {0}")]
    Unknown(String),
    /// Le secret n'a pas le droit d'être présenté à cet hôte.
    #[error("le secret {secret} n'est pas autorisé pour {host}")]
    WrongHost {
        /// Nom du secret.
        secret: String,
        /// Hôte visé.
        host: String,
    },
    /// Déchiffrement impossible.
    #[error("secret illisible")]
    Unreadable,
}

impl From<VaultError> for InjectionError {
    fn from(error: VaultError) -> Self {
        match error {
            VaultError::Unknown(name) => Self::Unknown(name),
            _ => Self::Unreadable,
        }
    }
}

/// Substitue les références de secrets dans les en-têtes sortants.
#[derive(Debug)]
pub struct Injector<'a> {
    vault: &'a Vault,
}

impl<'a> Injector<'a> {
    /// Construit un substituteur adossé à un coffre.
    #[must_use]
    pub const fn new(vault: &'a Vault) -> Self {
        Self { vault }
    }

    /// Remplace toute référence par sa valeur, après vérification du domaine.
    ///
    /// Les en-têtes rendus ne doivent **jamais** être journalisés : c'est le seul endroit du
    /// système où une valeur de secret existe en clair, et elle n'en sort que vers la socket.
    ///
    /// # Erreurs
    /// Secret inconnu, ou non autorisé pour cet hôte.
    pub fn substitute(
        &self,
        host: &str,
        headers: &[(String, String)],
    ) -> Result<Vec<(String, String)>, InjectionError> {
        let mut out = Vec::with_capacity(headers.len());
        for (name, value) in headers {
            out.push((name.clone(), self.substitute_value(host, value)?));
        }
        Ok(out)
    }

    fn substitute_value(&self, host: &str, value: &str) -> Result<String, InjectionError> {
        // La référence peut être seule ou précédée d'un schéma, par exemple
        // `Bearer prophet-secret:github`.
        let Some(position) = value.find("prophet-secret:") else {
            return Ok(value.to_owned());
        };
        let (prefix, rest) = value.split_at(position);
        // Le nom s'arrête au premier caractère qui ne peut pas en faire partie. S'arrêter au
        // premier espace avalerait la ponctuation d'un en-tête composite, par exemple le
        // point-virgule d'un `Cookie`.
        let reference_text: String = rest
            .char_indices()
            .take_while(|(index, c)| {
                *index < "prophet-secret:".len()
                    || c.is_ascii_alphanumeric()
                    || *c == '-'
                    || *c == '_'
                    || *c == '.'
            })
            .map(|(_, c)| c)
            .collect();
        let Some(reference) = SecretRef::parse(&reference_text) else {
            return Ok(value.to_owned());
        };
        let name = reference.name();
        if self.vault.info(name).is_none() {
            return Err(InjectionError::Unknown(name.to_owned()));
        }
        if !self.vault.allowed_for(name, host) {
            return Err(InjectionError::WrongHost {
                secret: name.to_owned(),
                host: host.to_owned(),
            });
        }
        let secret = self.vault.reveal(name)?;
        let suffix = &rest[reference_text.len()..];
        Ok(format!("{prefix}{secret}{suffix}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault::SecretInfo;

    fn coffre(dir: &std::path::Path) -> Vault {
        let mut v = Vault::open(dir.join("vault.json"), dir.join("key")).unwrap();
        v.put(
            SecretInfo {
                name: "github".to_owned(),
                domains: vec!["api.github.com".to_owned()],
                header: "Authorization".to_owned(),
                description: String::new(),
            },
            "ghp_valeur_reelle",
        )
        .unwrap();
        v
    }

    #[test]
    fn substitution_apres_un_schema() {
        let dir = tempfile::tempdir().unwrap();
        let v = coffre(dir.path());
        let headers = vec![(
            "Authorization".to_owned(),
            "Bearer prophet-secret:github".to_owned(),
        )];
        let out = Injector::new(&v)
            .substitute("api.github.com", &headers)
            .unwrap();
        assert_eq!(out[0].1, "Bearer ghp_valeur_reelle");
    }

    #[test]
    fn substitution_seule() {
        let dir = tempfile::tempdir().unwrap();
        let v = coffre(dir.path());
        let headers = vec![("X-Token".to_owned(), "prophet-secret:github".to_owned())];
        let out = Injector::new(&v)
            .substitute("api.github.com", &headers)
            .unwrap();
        assert_eq!(out[0].1, "ghp_valeur_reelle");
    }

    #[test]
    fn mauvais_hote_refuse() {
        let dir = tempfile::tempdir().unwrap();
        let v = coffre(dir.path());
        let headers = vec![(
            "Authorization".to_owned(),
            "Bearer prophet-secret:github".to_owned(),
        )];
        let err = Injector::new(&v)
            .substitute("evil.com", &headers)
            .unwrap_err();
        assert_eq!(
            err,
            InjectionError::WrongHost {
                secret: "github".to_owned(),
                host: "evil.com".to_owned()
            },
            "un secret ne doit jamais partir vers un hôte non déclaré"
        );
    }

    #[test]
    fn secret_inconnu_refuse() {
        let dir = tempfile::tempdir().unwrap();
        let v = coffre(dir.path());
        let headers = vec![("X".to_owned(), "prophet-secret:inexistant".to_owned())];
        assert!(matches!(
            Injector::new(&v).substitute("api.github.com", &headers),
            Err(InjectionError::Unknown(_))
        ));
    }

    #[test]
    fn en_tete_ordinaire_inchange() {
        let dir = tempfile::tempdir().unwrap();
        let v = coffre(dir.path());
        let headers = vec![("Accept".to_owned(), "application/json".to_owned())];
        let out = Injector::new(&v)
            .substitute("api.github.com", &headers)
            .unwrap();
        assert_eq!(out, headers);
    }

    #[test]
    fn le_suffixe_est_conserve() {
        let dir = tempfile::tempdir().unwrap();
        let v = coffre(dir.path());
        let headers = vec![(
            "Cookie".to_owned(),
            "prophet-secret:github; Path=/".to_owned(),
        )];
        let out = Injector::new(&v)
            .substitute("api.github.com", &headers)
            .unwrap();
        assert_eq!(out[0].1, "ghp_valeur_reelle; Path=/");
    }
}
