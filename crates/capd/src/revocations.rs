//! Les révocations survivent au redémarrage (FRONTIER, « révocation persistante »).
//!
//! `cap.revoke` inscrit le sujet dans l'état du service avant de répondre : une ligne JSON par
//! sujet, ajoutée puis synchronisée, dans un fichier `0600` que la session de l'humain ne lit
//! pas. Au démarrage, capd relit ces lignes et révoque de nouveau chaque sujet ; sans cela, le
//! jeton racine d'une mission révoquée redevenait valide jusqu'à son expiration.
//!
//! Une dernière ligne sans fin de ligne est une écriture interrompue : sa révocation n'avait pas
//! été confirmée, elle est retirée du fichier, pour que la suivante ne s'y colle pas. Toute autre
//! ligne illisible arrête le démarrage — capd ne devine pas ce qu'il a révoqué.

use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Une ligne du registre.
#[derive(Debug, Serialize, Deserialize)]
struct Ligne {
    /// Le sujet révoqué : une tâche.
    sub: String,
    /// Quand, pour le diagnostic.
    #[serde(with = "time::serde::rfc3339")]
    at: OffsetDateTime,
}

/// Le registre des révocations d'un capd.
#[derive(Debug)]
pub struct Revocations {
    fichier: PathBuf,
}

impl Revocations {
    /// Ouvre le registre et rend les sujets déjà révoqués, dans l'ordre de leur révocation.
    ///
    /// # Errors
    /// Registre illisible, ou une ligne complète qui n'est pas une révocation.
    pub fn ouvrir(fichier: PathBuf) -> Result<(Self, Vec<String>), String> {
        let texte = match std::fs::read_to_string(&fichier) {
            Ok(texte) => texte,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("{} illisible : {e}", fichier.display())),
        };
        let mut sujets = Vec::new();
        let complet = texte.ends_with('\n');
        let lignes: Vec<&str> = texte.split_terminator('\n').collect();
        for (rang, ligne) in lignes.iter().enumerate() {
            if !complet && rang + 1 == lignes.len() {
                tracing::warn!(
                    fichier = %fichier.display(),
                    "dernière révocation interrompue avant sa fin de ligne : retirée"
                );
                let garde = texte.rfind('\n').map_or(0, |fin| fin + 1);
                let ouvert = std::fs::OpenOptions::new()
                    .write(true)
                    .open(&fichier)
                    .and_then(|f| {
                        f.set_len(garde as u64)?;
                        f.sync_all()
                    });
                ouvert.map_err(|e| {
                    format!("{} : fin interrompue non retirée ({e})", fichier.display())
                })?;
                break;
            }
            let lue: Ligne = serde_json::from_str(ligne).map_err(|e| {
                format!(
                    "{}, ligne {} : révocation illisible ({e})",
                    fichier.display(),
                    rang + 1
                )
            })?;
            sujets.push(lue.sub);
        }
        Ok((Self { fichier }, sujets))
    }

    /// Inscrit une révocation et la synchronise sur le disque avant de rendre la main.
    ///
    /// # Errors
    /// Écriture ou synchronisation impossible.
    pub fn inscrire(&self, sujet: &str, maintenant: OffsetDateTime) -> std::io::Result<()> {
        let nouveau = !self.fichier.exists();
        let mut fichier = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&self.fichier)?;
        let mut ligne = serde_json::to_vec(&Ligne {
            sub: sujet.to_owned(),
            at: maintenant,
        })
        .map_err(std::io::Error::other)?;
        ligne.push(b'\n');
        // Une seule écriture en mode ajout : une ligne n'est jamais entrelacée avec une autre.
        fichier.write_all(&ligne)?;
        fichier.sync_data()?;
        if nouveau && let Some(dossier) = self.fichier.parent() {
            std::fs::File::open(dossier)?.sync_all()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_790_000_000).unwrap()
    }

    #[test]
    fn un_registre_absent_est_vide_puis_se_relit_dans_l_ordre() {
        let dir = tempfile::tempdir().unwrap();
        let chemin = dir.path().join("revocations.jsonl");
        let (registre, sujets) = Revocations::ouvrir(chemin.clone()).unwrap();
        assert!(sujets.is_empty());
        registre.inscrire("task:a", t0()).unwrap();
        registre.inscrire("task:b", t0()).unwrap();
        let (_, sujets) = Revocations::ouvrir(chemin).unwrap();
        assert_eq!(sujets, ["task:a", "task:b"]);
    }

    #[test]
    fn une_derniere_ligne_interrompue_est_ignoree_une_ligne_abimee_arrete() {
        let dir = tempfile::tempdir().unwrap();
        let chemin = dir.path().join("revocations.jsonl");
        let (registre, _) = Revocations::ouvrir(chemin.clone()).unwrap();
        registre.inscrire("task:a", t0()).unwrap();
        let mut texte = std::fs::read_to_string(&chemin).unwrap();
        texte.push_str("{\"sub\":\"task:b\",\"at\":\"2026-");
        std::fs::write(&chemin, &texte).unwrap();
        let (registre, sujets) = Revocations::ouvrir(chemin.clone()).unwrap();
        assert_eq!(
            sujets,
            ["task:a"],
            "l'écriture interrompue n'avait rien confirmé"
        );
        // La suivante ne se colle pas au fragment : le registre reste lisible.
        registre.inscrire("task:c", t0()).unwrap();
        let (_, sujets) = Revocations::ouvrir(chemin.clone()).unwrap();
        assert_eq!(sujets, ["task:a", "task:c"]);

        std::fs::write(&chemin, "pas une révocation\n{\"sub\":\"task:a\"}\n").unwrap();
        let erreur = Revocations::ouvrir(chemin).unwrap_err();
        assert!(erreur.contains("ligne 1"), "{erreur}");
    }
}
