//! Ce que tout daemon Prophet OS fait de la même façon.
//!
//! Sept services ouvrent un socket, chargent peut-être une clé, et décident à qui ils acceptent de
//! parler. Recopier cette dernière règle sept fois serait l'endroit idéal pour qu'une des sept
//! copies soit fausse — et une règle d'autorisation fausse ne se voit pas : elle laisse simplement
//! passer. Elle est donc écrite ici, une fois, avec ses tests.
//!
//! Rien dans ce crate ne décide d'un droit. `capd` reste le seul à le faire. Ce qui est décidé ici
//! est plus modeste et plus ancien : *à qui le noyau dit que je parle*, et *où sont mes fichiers*.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(feature = "essai")]
pub mod essai;

use std::path::{Path, PathBuf};

use ed25519_dalek::SigningKey;
use prophet_ipc::{Error, ErrorCode, PeerIdentity};
use serde_json::Value;

/// Groupe dont les membres peuvent appeler les méthodes système.
pub const GROUPE_SYSTEME: &str = "prophet-system";

/// Qui a le droit de parler à ce daemon.
///
/// Deux réponses acceptables, et une seule règle : soit le pair appartient au groupe système, soit
/// il *est* le service lui-même. Rien d'autre — et surtout pas « root passe partout » : `root` qui
/// parle à `capd` reste un pair comme un autre, parce que le jour où un programme tourne en root
/// sans qu'on l'ait voulu, on préfère qu'il soit refusé.
#[derive(Debug, Clone, Copy)]
pub struct Pairs {
    gid_systeme: Option<u32>,
    uid_propre: u32,
}

impl Pairs {
    /// Lit l'identité du service et celle du groupe système.
    ///
    /// # Erreurs
    /// Si `/proc/self` est illisible, c'est-à-dire si le service ne peut pas savoir qui il est.
    /// Servir sans le savoir serait pire que ne pas servir.
    pub fn detecter() -> std::io::Result<Self> {
        let gid_systeme = gid_du_groupe(GROUPE_SYSTEME);
        if gid_systeme.is_none() {
            tracing::warn!(
                groupe = GROUPE_SYSTEME,
                "groupe absent : seul l'utilisateur du service sera servi"
            );
        }
        Ok(Self {
            gid_systeme,
            uid_propre: uid_propre()?,
        })
    }

    /// Construit une règle explicite. Réservé aux tests, qui doivent pouvoir décrire une machine
    /// qu'ils n'ont pas.
    #[must_use]
    pub const fn explicite(gid_systeme: Option<u32>, uid_propre: u32) -> Self {
        Self {
            gid_systeme,
            uid_propre,
        }
    }

    /// Ce pair peut-il appeler une méthode système ?
    #[must_use]
    pub fn autorise(&self, pair: PeerIdentity) -> bool {
        match self.gid_systeme {
            Some(gid) => pair.gid == gid || pair.uid == self.uid_propre,
            None => pair.uid == self.uid_propre,
        }
    }

    /// Le refus, formulé. Un pair refusé mérite de savoir pourquoi ; il n'apprend rien qu'il ne
    /// sache déjà sur lui-même.
    #[must_use]
    pub fn refus(&self) -> Error {
        Error::new(
            ErrorCode::Unauthorized,
            format!("ce pair n'appartient pas au groupe {GROUPE_SYSTEME}"),
        )
    }
}

/// L'identifiant d'utilisateur de ce processus, sans passer par `unsafe`.
///
/// `/proc/self` appartient au propriétaire du processus : le noyau le dit, et le lire évite un
/// appel à `libc` pour une information que le système de fichiers porte déjà.
///
/// # Erreurs
/// Si `/proc` n'est pas monté.
pub fn uid_propre() -> std::io::Result<u32> {
    use std::os::unix::fs::MetadataExt as _;
    Ok(std::fs::metadata("/proc/self")?.uid())
}

/// Identifiant numérique d'un groupe, lu dans `/etc/group`.
#[must_use]
pub fn gid_du_groupe(nom: &str) -> Option<u32> {
    let contenu = std::fs::read_to_string("/etc/group").ok()?;
    contenu.lines().find_map(|ligne| {
        let mut champs = ligne.split(':');
        (champs.next()? == nom).then(|| champs.nth(1)?.parse().ok())?
    })
}

/// Charge une clé de signature, ou en crée une au premier démarrage.
///
/// Le mode `0600` est vérifié **après** écriture, et non supposé : un `umask` hostile ferait mentir
/// la création seule. C'est la même règle que partout ailleurs dans ce dépôt — on essaie, puis on
/// regarde ce qui s'est réellement produit.
///
/// # Erreurs
/// Si le fichier existe mais ne fait pas 32 octets, s'il ne peut être écrit, ou si son mode n'est
/// pas celui qu'on vient de poser.
pub fn clef(etat: &Path, nom: &str) -> std::io::Result<SigningKey> {
    use std::os::unix::fs::PermissionsExt as _;

    let chemin = etat.join(nom);
    if chemin.exists() {
        let octets = std::fs::read(&chemin)?;
        let tableau: [u8; 32] = octets.as_slice().try_into().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "{} fait {} octets, attendu 32 — refus de deviner une clé",
                    chemin.display(),
                    octets.len()
                ),
            )
        })?;
        return Ok(SigningKey::from_bytes(&tableau));
    }

    std::fs::create_dir_all(etat)?;
    let clef = SigningKey::generate(&mut rand::rngs::OsRng);
    std::fs::write(&chemin, clef.to_bytes())?;
    std::fs::set_permissions(&chemin, std::fs::Permissions::from_mode(0o600))?;
    let mode = std::fs::metadata(&chemin)?.permissions().mode() & 0o777;
    if mode != 0o600 {
        return Err(std::io::Error::other(format!(
            "{} est en mode {mode:o} après écriture, attendu 600",
            chemin.display()
        )));
    }
    tracing::info!(chemin = %chemin.display(), "clé créée");
    Ok(clef)
}

/// Où ce daemon écoute. `PROPHET_SOCKET` l'emporte, pour les tests et les développements.
#[must_use]
pub fn socket(daemon: &str) -> PathBuf {
    std::env::var("PROPHET_SOCKET").map_or_else(|_| prophet_ipc::socket_path(daemon), PathBuf::from)
}

/// Où ce daemon range son état. `STATE_DIRECTORY` est posé par systemd ; le repli sert au
/// développement.
#[must_use]
pub fn etat(daemon: &str) -> PathBuf {
    std::env::var("STATE_DIRECTORY").map_or_else(
        |_| PathBuf::from(format!("/var/lib/prophet/{daemon}")),
        PathBuf::from,
    )
}

/// Installe la journalisation. `RUST_LOG` la règle ; à défaut, `info`.
pub fn journaliser() {
    let filtre = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    // Deux daemons lancés dans le même test installeraient deux fois le même abonné global ; la
    // seconde échoue, et ce n'est pas une raison de ne pas servir.
    let _ = tracing_subscriber::fmt().with_env_filter(filtre).try_init();
}

/// Un paramètre texte obligatoire.
///
/// # Erreurs
/// S'il est absent ou n'est pas une chaîne.
pub fn texte(params: &Value, nom: &str) -> Result<String, Error> {
    params
        .get(nom)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            Error::new(
                ErrorCode::InvalidParams,
                format!("paramètre « {nom} » attendu, de type chaîne"),
            )
        })
}

/// Sérialise une réponse.
///
/// # Erreurs
/// Si la valeur ne peut pas devenir du JSON, ce qui est une faute du daemon et non de l'appelant.
pub fn repondre<T: serde::Serialize>(valeur: &T) -> Result<Value, Error> {
    serde_json::to_value(valeur).map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))
}

/// Une méthode qui n'existe pas, nommée telle qu'elle a été demandée.
#[must_use]
pub fn methode_inconnue(methode: &str) -> Error {
    Error::new(
        ErrorCode::MethodNotFound,
        format!("méthode inconnue : {methode}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(uid: u32, gid: u32) -> PeerIdentity {
        PeerIdentity {
            uid,
            gid,
            pid: None,
        }
    }

    #[test]
    fn le_groupe_systeme_ouvre_la_porte() {
        let regle = Pairs::explicite(Some(900), 42);
        assert!(regle.autorise(pair(1000, 900)), "membre du groupe système");
        assert!(regle.autorise(pair(42, 1)), "le service lui-même");
    }

    #[test]
    fn root_ne_passe_pas_par_faveur() {
        // Le jour où un programme tourne en root sans qu'on l'ait voulu, on préfère qu'il soit
        // refusé comme n'importe qui d'autre.
        let regle = Pairs::explicite(Some(900), 42);
        assert!(!regle.autorise(pair(0, 0)));
    }

    #[test]
    fn sans_groupe_seul_le_service_est_servi() {
        // Sur une machine de développement, le groupe n'existe pas. Servir tout le monde « parce
        // qu'on ne sait pas » serait exactement la faute que ce module existe pour éviter.
        let regle = Pairs::explicite(None, 42);
        assert!(regle.autorise(pair(42, 7)));
        assert!(!regle.autorise(pair(43, 900)));
    }

    #[test]
    fn une_cle_relue_est_la_meme() {
        let temp = tempfile::tempdir().unwrap();
        let premiere = clef(temp.path(), "essai.key").unwrap();
        let seconde = clef(temp.path(), "essai.key").unwrap();
        assert_eq!(premiere.to_bytes(), seconde.to_bytes());
    }

    #[test]
    fn une_cle_tronquee_est_refusee_et_non_devinee() {
        // Le cas qui compte : compléter silencieusement un fichier abîmé donnerait une clé
        // valide mais fausse, et tous les jetons déjà émis deviendraient invérifiables sans que
        // rien ne le signale.
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("courte.key"), [1u8; 16]).unwrap();
        let erreur = clef(temp.path(), "courte.key").expect_err("16 octets ne font pas une clé");
        assert!(erreur.to_string().contains("16 octets"), "{erreur}");
    }

    #[test]
    fn une_cle_neuve_n_est_lisible_que_par_le_service() {
        use std::os::unix::fs::PermissionsExt as _;
        let temp = tempfile::tempdir().unwrap();
        clef(temp.path(), "neuve.key").unwrap();
        let mode = std::fs::metadata(temp.path().join("neuve.key"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn un_parametre_absent_le_dit_par_son_nom() {
        let erreur = texte(&serde_json::json!({}), "id").expect_err("absent");
        assert_eq!(erreur.code, ErrorCode::InvalidParams);
        assert!(erreur.message.contains("id"), "{}", erreur.message);
    }
}
