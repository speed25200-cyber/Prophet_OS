//! La preuve de présence de l'humain pour accorder une approbation (ADR 0057).
//!
//! Tout programme de la session de l'humain tourne sous son identité : `SO_PEERCRED` ne
//! distingue pas la surface d'un script. Accorder exige donc un secret que seul l'humain connaît,
//! son **code d'approbation**. capd n'en garde que l'empreinte, dans son état, que la session ne
//! peut pas lire ; un code juste rend un ticket aléatoire, valable dix minutes pour ce compte, que
//! la surface garde en mémoire. Cinq codes faux verrouillent la preuve cinq minutes.

use std::collections::HashMap;
use std::path::PathBuf;

use rand::RngCore as _;
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

/// Longueur minimale d'un code, en caractères.
pub const LONGUEUR_MIN: usize = 6;

/// Codes faux tolérés avant le verrou.
pub const ECHECS_MAX: u32 = 5;

/// Durée du verrou après trop d'échecs.
pub const VERROU: Duration = Duration::minutes(5);

/// Durée d'un ticket de présence.
pub const DUREE_DU_TICKET: Duration = Duration::minutes(10);

/// Contexte de dérivation : une empreinte ne vaut que pour cet usage.
const CONTEXTE: &str = "prophet-os 2026-09-24 code d'approbation v1";

/// Tours de dérivation : rendre chaque essai hors ligne coûteux, si le fichier fuyait malgré
/// son mode ; en ligne, c'est le verrou qui borne les essais.
const TOURS: u32 = 50_000;

/// Pourquoi la preuve est refusée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refus {
    /// Aucun code n'est défini sur cette machine.
    NonDefini,
    /// Trop d'échecs : la preuve est verrouillée encore tant de secondes.
    Verrouille {
        /// Secondes restantes.
        secondes: i64,
    },
    /// Code faux ; il reste tant d'essais avant le verrou.
    Faux {
        /// Essais restants.
        restants: u32,
    },
    /// Ticket inconnu, périmé ou d'un autre compte.
    TicketInvalide,
    /// Code trop court.
    TropCourt,
    /// Un code est déjà défini : le changer demande l'ancien.
    AncienRequis,
    /// Le code n'a pas pu être enregistré.
    Stockage(String),
}

impl std::fmt::Display for Refus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonDefini => write!(
                f,
                "aucun code d'approbation n'est défini : définissez-le (prophet cap code) avant d'accorder"
            ),
            Self::Verrouille { secondes } => {
                write!(f, "trop de codes faux : réessayez dans {secondes} s")
            }
            Self::Faux { restants } => write!(
                f,
                "code d'approbation faux ; {restants} essai(s) avant le verrou"
            ),
            Self::TicketInvalide => {
                write!(f, "présence non prouvée : donnez votre code d'approbation")
            }
            Self::TropCourt => write!(
                f,
                "le code d'approbation doit faire au moins {LONGUEUR_MIN} caractères"
            ),
            Self::AncienRequis => write!(
                f,
                "un code d'approbation est déjà défini : donnez l'ancien pour le changer"
            ),
            Self::Stockage(e) => write!(f, "code d'approbation non enregistré : {e}"),
        }
    }
}

/// L'empreinte gardée : un sel et le résultat de la dérivation, jamais le code.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Empreinte {
    v: u32,
    sel: String,
    empreinte: String,
}

impl Empreinte {
    fn nouvelle(code: &str) -> Self {
        let mut sel = [0_u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut sel);
        let sel = hex(&sel);
        let empreinte = deriver(&sel, code).to_hex().to_string();
        Self {
            v: 1,
            sel,
            empreinte,
        }
    }

    /// Comparaison en temps constant (celle de `blake3::Hash`).
    fn correspond(&self, code: &str) -> bool {
        blake3::Hash::from_hex(&self.empreinte)
            .is_ok_and(|attendue| deriver(&self.sel, code) == attendue)
    }
}

fn deriver(sel: &str, code: &str) -> blake3::Hash {
    let mut etat = blake3::derive_key(CONTEXTE, format!("{sel}:{code}").as_bytes());
    for _ in 0..TOURS {
        etat = blake3::derive_key(CONTEXTE, &etat);
    }
    blake3::Hash::from(etat)
}

fn hex(octets: &[u8]) -> String {
    octets.iter().map(|o| format!("{o:02x}")).collect()
}

/// La preuve de présence : le code, le verrou et les tickets en cours.
#[derive(Debug, Default)]
pub struct Presence {
    fichier: Option<PathBuf>,
    empreinte: Option<Empreinte>,
    echecs: u32,
    verrou: Option<OffsetDateTime>,
    tickets: HashMap<String, (u32, OffsetDateTime)>,
}

impl Presence {
    /// Relit le code gardé dans `fichier`, s'il y en a un.
    ///
    /// # Errors
    /// Fichier présent mais illisible : mieux vaut ne pas servir que servir sans le code.
    pub fn ouvrir(fichier: PathBuf) -> std::io::Result<Self> {
        let empreinte = match std::fs::read(&fichier) {
            Ok(octets) => Some(serde_json::from_slice(&octets).map_err(std::io::Error::other)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        Ok(Self {
            fichier: Some(fichier),
            empreinte,
            ..Self::default()
        })
    }

    /// Un code est-il défini ?
    #[must_use]
    pub const fn defini(&self) -> bool {
        self.empreinte.is_some()
    }

    /// Secondes de verrou restantes, s'il y en a.
    #[must_use]
    pub fn verrou_restant(&self, maintenant: OffsetDateTime) -> Option<i64> {
        self.verrou
            .filter(|fin| *fin > maintenant)
            .map(|fin| (fin - maintenant).whole_seconds().max(1))
    }

    /// Codes faux depuis le dernier juste.
    #[must_use]
    pub const fn echecs(&self) -> u32 {
        self.echecs
    }

    /// Définit ou change le code. Le changer demande l'ancien, sauf à l'administrateur.
    ///
    /// # Errors
    /// Code trop court, ancien absent ou faux, verrou, ou enregistrement impossible.
    pub fn definir(
        &mut self,
        code: &str,
        ancien: Option<&str>,
        administrateur: bool,
        maintenant: OffsetDateTime,
    ) -> Result<(), Refus> {
        if code.chars().count() < LONGUEUR_MIN {
            return Err(Refus::TropCourt);
        }
        if self.defini() && !administrateur {
            let ancien = ancien.ok_or(Refus::AncienRequis)?;
            self.comparer(ancien, maintenant)?;
        }
        let empreinte = Empreinte::nouvelle(code);
        if let Some(fichier) = &self.fichier {
            ecrire(fichier, &empreinte).map_err(|e| Refus::Stockage(e.to_string()))?;
        }
        self.empreinte = Some(empreinte);
        // Un nouveau code invalide les tickets obtenus avec l'ancien.
        self.tickets.clear();
        self.echecs = 0;
        self.verrou = None;
        Ok(())
    }

    /// Un code juste rend un ticket pour ce compte, et sa fin.
    ///
    /// # Errors
    /// Aucun code défini, verrou, ou code faux.
    pub fn prouver(
        &mut self,
        uid: u32,
        code: &str,
        maintenant: OffsetDateTime,
    ) -> Result<(String, OffsetDateTime), Refus> {
        self.comparer(code, maintenant)?;
        self.tickets.retain(|_, (_, fin)| *fin > maintenant);
        let mut octets = [0_u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut octets);
        let ticket = hex(&octets);
        let fin = maintenant + DUREE_DU_TICKET;
        self.tickets.insert(ticket.clone(), (uid, fin));
        Ok((ticket, fin))
    }

    /// Un ticket vaut-il pour ce compte, maintenant ?
    ///
    /// # Errors
    /// Ticket inconnu, périmé ou d'un autre compte.
    pub fn valider(
        &mut self,
        uid: u32,
        ticket: &str,
        maintenant: OffsetDateTime,
    ) -> Result<(), Refus> {
        self.tickets.retain(|_, (_, fin)| *fin > maintenant);
        match self.tickets.get(ticket) {
            Some((proprietaire, _)) if *proprietaire == uid => Ok(()),
            _ => Err(Refus::TicketInvalide),
        }
    }

    fn comparer(&mut self, code: &str, maintenant: OffsetDateTime) -> Result<(), Refus> {
        let Some(empreinte) = &self.empreinte else {
            return Err(Refus::NonDefini);
        };
        if let Some(secondes) = self.verrou_restant(maintenant) {
            return Err(Refus::Verrouille { secondes });
        }
        if empreinte.correspond(code) {
            self.echecs = 0;
            self.verrou = None;
            return Ok(());
        }
        self.echecs += 1;
        if self.echecs >= ECHECS_MAX {
            self.echecs = 0;
            self.verrou = Some(maintenant + VERROU);
            return Err(Refus::Verrouille {
                secondes: VERROU.whole_seconds(),
            });
        }
        Err(Refus::Faux {
            restants: ECHECS_MAX - self.echecs,
        })
    }
}

/// Écrit l'empreinte en `0600`, par renommage : un arrêt au milieu ne laisse pas un fichier
/// tronqué qui fermerait capd au redémarrage.
fn ecrire(fichier: &std::path::Path, empreinte: &Empreinte) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let provisoire = fichier.with_extension("tmp");
    let _ = std::fs::remove_file(&provisoire);
    let mut sortie = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&provisoire)?;
    sortie.write_all(&serde_json::to_vec(empreinte).map_err(std::io::Error::other)?)?;
    sortie.sync_all()?;
    std::fs::rename(provisoire, fichier)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_790_000_000).unwrap()
    }

    #[test]
    fn le_code_n_est_garde_qu_en_empreinte_et_survit_au_redemarrage() {
        let dir = tempfile::tempdir().unwrap();
        let fichier = dir.path().join("code-approbation");
        let mut presence = Presence::ouvrir(fichier.clone()).unwrap();
        assert!(!presence.defini());
        assert_eq!(
            presence.prouver(1000, "123456", t0()).unwrap_err(),
            Refus::NonDefini
        );
        assert_eq!(
            presence.definir("12345", None, false, t0()).unwrap_err(),
            Refus::TropCourt
        );
        presence.definir("pivoine-42", None, false, t0()).unwrap();
        let texte = std::fs::read_to_string(&fichier).unwrap();
        assert!(!texte.contains("pivoine"), "{texte}");
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&fichier).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let mut relu = Presence::ouvrir(fichier).unwrap();
        assert!(relu.defini());
        assert!(relu.prouver(1000, "pivoine-42", t0()).is_ok());
    }

    #[test]
    fn un_ticket_vaut_dix_minutes_pour_son_compte_seulement() {
        let mut presence = Presence::default();
        presence.definir("pivoine-42", None, false, t0()).unwrap();
        let (ticket, fin) = presence.prouver(1000, "pivoine-42", t0()).unwrap();
        assert_eq!(fin, t0() + DUREE_DU_TICKET);
        assert!(
            presence
                .valider(1000, &ticket, t0() + Duration::minutes(9))
                .is_ok()
        );
        assert_eq!(
            presence.valider(1001, &ticket, t0()).unwrap_err(),
            Refus::TicketInvalide,
            "un autre compte"
        );
        assert_eq!(
            presence.valider(1000, "inventé", t0()).unwrap_err(),
            Refus::TicketInvalide
        );
        assert_eq!(
            presence
                .valider(1000, &ticket, t0() + Duration::minutes(11))
                .unwrap_err(),
            Refus::TicketInvalide,
            "périmé"
        );
    }

    #[test]
    fn cinq_codes_faux_verrouillent_cinq_minutes_meme_le_bon() {
        let mut presence = Presence::default();
        presence.definir("pivoine-42", None, false, t0()).unwrap();
        for restants in (1..ECHECS_MAX).rev() {
            assert_eq!(
                presence.prouver(1000, "faux-faux", t0()).unwrap_err(),
                Refus::Faux { restants }
            );
        }
        assert!(matches!(
            presence.prouver(1000, "faux-faux", t0()).unwrap_err(),
            Refus::Verrouille { .. }
        ));
        assert!(matches!(
            presence
                .prouver(1000, "pivoine-42", t0() + Duration::minutes(4))
                .unwrap_err(),
            Refus::Verrouille { .. }
        ));
        assert!(
            presence
                .verrou_restant(t0() + Duration::minutes(4))
                .is_some()
        );
        assert!(
            presence
                .prouver(1000, "pivoine-42", t0() + Duration::minutes(6))
                .is_ok()
        );
        assert_eq!(presence.echecs(), 0);
    }

    #[test]
    fn changer_le_code_demande_l_ancien_sauf_a_l_administrateur_et_annule_les_tickets() {
        let mut presence = Presence::default();
        presence.definir("pivoine-42", None, false, t0()).unwrap();
        let (ticket, _) = presence.prouver(1000, "pivoine-42", t0()).unwrap();
        assert_eq!(
            presence
                .definir("glycine-7", None, false, t0())
                .unwrap_err(),
            Refus::AncienRequis
        );
        assert!(matches!(
            presence
                .definir("glycine-7", Some("faux-faux"), false, t0())
                .unwrap_err(),
            Refus::Faux { .. }
        ));
        presence
            .definir("glycine-7", Some("pivoine-42"), false, t0())
            .unwrap();
        assert_eq!(
            presence.valider(1000, &ticket, t0()).unwrap_err(),
            Refus::TicketInvalide,
            "un nouveau code annule les tickets de l'ancien"
        );
        presence.definir("lilas-2026", None, true, t0()).unwrap();
        assert!(presence.prouver(1000, "lilas-2026", t0()).is_ok());
        assert!(presence.prouver(1000, "glycine-7", t0()).is_err());
    }
}
