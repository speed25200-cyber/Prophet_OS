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
/// Quatre réponses acceptables :
///
/// - le pair **est** le service lui-même ;
/// - son groupe principal *est* le groupe système — c'est le cas des sept daemons ;
/// - l'administrateur l'a **déclaré** membre du groupe système dans `/etc/group` ;
/// - il est `root`.
///
/// ## Pourquoi la troisième règle existe
///
/// `SO_PEERCRED` ne rend que le groupe **principal** du pair. Un processus que l'administrateur a
/// mis dans `prophet-system` par `extraGroups` y appartient réellement — le noyau le sait, et les
/// droits du socket le respectent — mais son `gid` attesté reste celui de son groupe principal.
/// Comparer ce seul `gid` revient donc à refuser des membres véritables du groupe.
///
/// Ce n'était pas théorique : la surface, dont tout le travail est d'afficher ce que font les
/// daemons, est déclarée ainsi. Chacun d'eux la refusait, et l'écran serait resté vide sur une
/// machine parfaitement saine. Le kernel rend la liste complète par `SO_PEERGROUPS` ; l'obtenir
/// changerait le type porté par `prophet-ipc` jusque dans les sept daemons. On lit donc la
/// déclaration de l'administrateur, dans le fichier même que `initgroups` consulte.
///
/// Ce que cela concède : un processus qui aurait *abandonné* le groupe par `setgroups` reste
/// accepté. Il appartient toujours au groupe au sens où l'administrateur l'entend, et il pourrait
/// de toute façon le reprendre par `newgrp`.
///
/// La liste n'est **pas** mise en cache, et c'est un choix. Une première version la lisait au
/// démarrage ; le test en machine virtuelle a aussitôt montré ce que cela coûte — un compte créé
/// après le démarrage des daemons est refusé jusqu'au prochain redémarrage de chacun d'eux, ce
/// qu'un `nixos-rebuild switch` ne fait pas. Un refus qui dépend de l'heure à laquelle un service
/// a démarré est exactement le genre de comportement qu'on ne diagnostique jamais.
///
/// Le fichier n'est lu que lorsque le pair serait sinon refusé. Les sept daemons se reconnaissent
/// par leur groupe principal et `root` par son `uid` : sur le chemin fréquent, rien n'est ouvert.
/// Restent la surface et l'humain, qui parlent peu et lentement.
///
/// ## Pourquoi `root` passe
///
/// La règle disait l'inverse, et le disait bien : « le jour où un programme tourne en root sans
/// qu'on l'ait voulu, on préfère qu'il soit refusé ». Elle ne protégeait rien. `root` lit déjà les
/// clés de signature dans `/var/lib/prophet`, et peut donc émettre les jetons qu'il veut sans
/// jamais toucher à ce socket ; il peut aussi arrêter les services et prendre leur place. La seule
/// chose que ce refus produisait était un `prophet status` inutilisable pour le propriétaire de la
/// machine — ce que le test en machine virtuelle a fini par montrer.
///
/// Une tâche isolée ne peut pas s'en servir : `SO_PEERCRED` traduit les identifiants dans l'espace
/// de noms du destinataire, et un `uid 0` qui n'y est pas projeté arrive en `overflowuid`, pas en
/// zéro.
#[derive(Debug, Clone)]
pub struct Pairs {
    gid_systeme: Option<u32>,
    uid_propre: u32,
    /// Membres figés. `None` — le cas d'une vraie machine — veut dire « relire `/etc/group` au
    /// moment où la question se pose ». Les tests s'en servent pour décrire une machine qu'ils
    /// n'ont pas, sans dépendre du `/etc/group` de celle qui les exécute.
    membres_figes: Option<Vec<u32>>,
}

/// L'identifiant de l'administrateur.
const ROOT: u32 = 0;

/// Ce qu'est un pair admis, pour les méthodes qui ne s'ouvrent pas à tous.
///
/// Appartenir au groupe système ouvre la porte ; cela ne dit pas qui l'on est. Les sept daemons
/// ont ce groupe pour groupe principal, que systemd leur donne (`Group=`) et que `SO_PEERCRED`
/// atteste ; l'humain, sa session et la surface n'en sont que membres déclarés
/// (`extraGroups`). La différence suffit à séparer ce qui émet des droits ou écrit le journal de
/// ce qui les lit et tranche ce qui revient à l'humain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classe {
    /// Le service lui-même, ou `root` : ils peuvent déjà tout par leurs fichiers.
    Soi,
    /// Un daemon de l'OS : son groupe principal est le groupe système.
    Service,
    /// Un membre déclaré du groupe système : l'humain, sa session, la surface.
    Humain,
}

/// À qui une méthode s'ouvre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acces {
    /// Tout pair admis : lire, demander, révoquer.
    Tous,
    /// Les daemons seulement : émettre ou vérifier des droits, écrire le journal.
    Services,
    /// L'humain seulement : trancher une décision qui lui revient.
    Humains,
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
            membres_figes: None,
        })
    }

    /// Construit une règle explicite. Réservé aux tests, qui doivent pouvoir décrire une machine
    /// qu'ils n'ont pas.
    #[must_use]
    pub const fn explicite(gid_systeme: Option<u32>, uid_propre: u32) -> Self {
        Self {
            gid_systeme,
            uid_propre,
            membres_figes: Some(Vec::new()),
        }
    }

    /// La même, en nommant les membres déclarés du groupe système.
    #[must_use]
    pub fn avec_membres(gid_systeme: Option<u32>, uid_propre: u32, membres: Vec<u32>) -> Self {
        Self {
            gid_systeme,
            uid_propre,
            membres_figes: Some(membres),
        }
    }

    /// Ce pair peut-il appeler une méthode système ?
    ///
    /// Les trois premières réponses ne touchent à aucun fichier ; la quatrième seule ouvre
    /// `/etc/group`, et seulement pour un pair qui serait sinon refusé.
    #[must_use]
    pub fn autorise(&self, pair: PeerIdentity) -> bool {
        self.classe(pair).is_some()
    }

    /// La classe d'un pair admis, ou `None` s'il ne l'est pas.
    #[must_use]
    pub fn classe(&self, pair: PeerIdentity) -> Option<Classe> {
        if pair.uid == ROOT || pair.uid == self.uid_propre {
            return Some(Classe::Soi);
        }
        if self.gid_systeme.is_some_and(|gid| pair.gid == gid) {
            return Some(Classe::Service);
        }
        let declare = match &self.membres_figes {
            Some(membres) => membres.contains(&pair.uid),
            None => membres_du_groupe(GROUPE_SYSTEME).contains(&pair.uid),
        };
        declare.then_some(Classe::Humain)
    }

    /// Ce pair peut-il appeler une méthode ouverte à `acces` ? Le service lui-même et `root`
    /// peuvent tout ; un service n'ouvre pas ce qui revient à l'humain, ni l'humain ce qui
    /// revient aux services.
    #[must_use]
    pub fn permet(&self, pair: PeerIdentity, acces: Acces) -> bool {
        matches!(
            (self.classe(pair), acces),
            (Some(Classe::Soi), _)
                | (Some(_), Acces::Tous)
                | (Some(Classe::Service), Acces::Services)
                | (Some(Classe::Humain), Acces::Humains)
        )
    }

    /// Le refus d'une méthode réservée, formulé pour un pair admis.
    #[must_use]
    pub fn refus_pour(&self, methode: &str, acces: Acces) -> Error {
        Error::new(
            ErrorCode::Unauthorized,
            match acces {
                Acces::Services => {
                    format!("« {methode} » est réservée aux services de l'OS")
                }
                Acces::Humains => format!("« {methode} » revient à l'humain"),
                Acces::Tous => format!("« {methode} » n'est pas ouverte à ce pair"),
            },
        )
    }

    /// Le refus, formulé. Un pair refusé mérite de savoir pourquoi ; il n'apprend rien qu'il ne
    /// sache déjà sur lui-même.
    #[must_use]
    pub fn refus(&self) -> Error {
        Error::new(
            ErrorCode::Unauthorized,
            format!(
                "ce pair n'appartient pas au groupe {GROUPE_SYSTEME} : \
                 ni comme groupe principal, ni comme membre déclaré dans /etc/group"
            ),
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

/// Les `uid` des membres **déclarés** d'un groupe, d'après `/etc/group` et `/etc/passwd`.
///
/// Ceux dont c'est le groupe principal n'y figurent pas : `/etc/group` ne liste que les
/// appartenances supplémentaires. Ce n'est pas un manque — le groupe principal, lui, est attesté
/// par le noyau à chaque connexion, et `Pairs::autorise` le regarde séparément.
#[must_use]
pub fn membres_du_groupe(nom: &str) -> Vec<u32> {
    let groupes = std::fs::read_to_string("/etc/group").unwrap_or_default();
    let comptes = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
    membres_declares(nom, &groupes, &comptes)
}

/// La même règle, sur des fichiers donnés.
///
/// Séparée pour être vérifiable : un test qui dépendrait du `/etc/group` de la machine qui
/// l'exécute ne prouverait rien de stable.
#[must_use]
pub fn membres_declares(nom: &str, groupes: &str, comptes: &str) -> Vec<u32> {
    let Some(liste) = groupes.lines().find_map(|ligne| {
        let champs: Vec<&str> = ligne.split(':').collect();
        (champs.first() == Some(&nom)).then(|| champs.get(3).copied().unwrap_or_default())
    }) else {
        return Vec::new();
    };
    liste
        .split(',')
        .map(str::trim)
        .filter(|membre| !membre.is_empty())
        .filter_map(|membre| {
            comptes.lines().find_map(|ligne| {
                let champs: Vec<&str> = ligne.split(':').collect();
                (champs.first() == Some(&membre))
                    .then(|| champs.get(2)?.parse::<u32>().ok())
                    .flatten()
            })
        })
        .collect()
}

/// Identifiant numérique d'un utilisateur, lu dans `/etc/passwd`.
///
/// Sert aux daemons qui ne servent pas seulement un groupe mais une personne précise : le Vault
/// ne révèle une valeur qu'au proxy de sortie, et à personne d'autre.
#[must_use]
pub fn uid_de_l_utilisateur(nom: &str) -> Option<u32> {
    let contenu = std::fs::read_to_string("/etc/passwd").ok()?;
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
    fn un_daemon_est_un_service_et_un_membre_declare_un_humain() {
        // Les sept daemons ont le groupe système pour groupe principal (`Group=` de systemd) ;
        // l'humain et la surface n'en sont que membres déclarés (`extraGroups`).
        let regle = Pairs::avec_membres(Some(900), 42, vec![1000]);
        assert_eq!(regle.classe(pair(42, 1)), Some(Classe::Soi));
        assert_eq!(regle.classe(pair(0, 0)), Some(Classe::Soi));
        assert_eq!(regle.classe(pair(981, 900)), Some(Classe::Service));
        assert_eq!(regle.classe(pair(1000, 100)), Some(Classe::Humain));
        assert_eq!(regle.classe(pair(1001, 100)), None);
    }

    #[test]
    fn emettre_revient_aux_services_et_trancher_a_l_humain() {
        let regle = Pairs::avec_membres(Some(900), 42, vec![1000]);
        let (soi, service, humain, inconnu) = (
            pair(42, 1),
            pair(981, 900),
            pair(1000, 100),
            pair(1001, 100),
        );
        for p in [soi, service, humain] {
            assert!(regle.permet(p, Acces::Tous));
        }
        assert!(!regle.permet(inconnu, Acces::Tous));
        assert!(regle.permet(soi, Acces::Services) && regle.permet(service, Acces::Services));
        assert!(
            !regle.permet(humain, Acces::Services),
            "l'humain n'émet pas de droits"
        );
        assert!(regle.permet(soi, Acces::Humains) && regle.permet(humain, Acces::Humains));
        assert!(
            !regle.permet(service, Acces::Humains),
            "un service ne tranche pas"
        );
        assert!(!regle.permet(inconnu, Acces::Humains));
        // Le refus nomme la méthode et à qui elle revient.
        let refus = regle.refus_pour("cap.mint", Acces::Services);
        assert_eq!(refus.code, ErrorCode::Unauthorized);
        assert!(refus.message.contains("cap.mint"), "{}", refus.message);
    }

    #[test]
    fn root_administre_sa_machine() {
        // Le refus précédent ne protégeait rien : `root` lit les clés de signature dans
        // `/var/lib/prophet` et peut émettre ses jetons sans passer par ce socket. Il ne coûtait
        // qu'une chose, et une seule : `prophet status` inutilisable pour le propriétaire.
        let regle = Pairs::explicite(Some(900), 42);
        assert!(regle.autorise(pair(0, 0)));
    }

    #[test]
    fn un_membre_declare_passe_sans_avoir_le_groupe_en_principal() {
        // Le cas de la surface : `extraGroups = [ "prophet-system" ]`. Elle appartient au groupe,
        // mais `SO_PEERCRED` n'atteste que son groupe principal. Sans cette règle, chacun des sept
        // daemons la refuse et l'écran reste vide sur une machine saine.
        let regle = Pairs::avec_membres(Some(900), 42, vec![1001]);
        assert!(regle.autorise(pair(1001, 555)), "membre déclaré, autre gid");
        assert!(!regle.autorise(pair(1002, 555)), "et lui n'est pas déclaré");
    }

    #[test]
    fn sans_groupe_seul_le_service_et_root_sont_servis() {
        // Sur une machine de développement, le groupe n'existe pas. Servir tout le monde « parce
        // qu'on ne sait pas » serait exactement la faute que ce module existe pour éviter.
        let regle = Pairs::explicite(None, 42);
        assert!(regle.autorise(pair(42, 7)));
        assert!(!regle.autorise(pair(43, 900)));
    }

    #[test]
    fn les_membres_se_lisent_dans_le_fichier_de_groupes() {
        let groupes = "root:x:0:\nprophet-system:x:900:surface,prophet\nvideo:x:26:surface\n";
        let comptes = "root:x:0:0::/root:/bin/sh\n\
                       surface:x:998:998::/var/empty:/bin/false\n\
                       prophet:x:1000:100::/home/prophet:/bin/sh\n";
        let mut membres = membres_declares("prophet-system", groupes, comptes);
        membres.sort_unstable();
        assert_eq!(membres, vec![998, 1000]);
    }

    #[test]
    fn un_groupe_absent_ne_donne_aucun_membre() {
        // Le piège serait de rendre « tout le monde » quand on ne trouve rien, ou de se tromper de
        // colonne : `/etc/group` met le mot de passe en deuxième champ et les membres en
        // quatrième, et lire le mauvais accepterait des inconnus.
        assert!(
            membres_declares("absent", "autre:x:5:un,deux\n", "un:x:1:1::/:/bin/sh\n").is_empty()
        );
        assert!(
            membres_declares("vide", "vide:x:5:\n", "un:x:1:1::/:/bin/sh\n").is_empty(),
            "un groupe sans membre déclaré n'en a aucun, et surtout pas le mot de passe pour nom"
        );
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
