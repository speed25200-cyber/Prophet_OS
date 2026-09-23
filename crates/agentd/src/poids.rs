//! Les poids gérés (M8-T7, ADR 0046) : télécharger un poids du catalogue du système, suivre le
//! téléchargement, l'arrêter, retirer un poids téléchargé.
//!
//! Le téléchargement lui-même est [`providers::pull`] : par le proxy de sortie, avec un jeton
//! que capd émet pour les seuls hôtes de l'entrée, et vérifié avant d'être posé. Ce module tient
//! ce que le service en sait : un suivi par entrée, un fil par téléchargement en cours. Il ne
//! touche qu'au dossier des téléchargements ; les poids que la configuration du système pose
//! ailleurs ne se retirent pas d'ici.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use prophet_types::cap::{Act, Grant, Res};
use prophet_types::manifest::Manifest;
use providers::catalogue::{Catalogue, Entry};
use providers::pull::{Egress, Progress, PullError, partial_path, pull};
use serde::Serialize;

/// Durée du jeton d'un téléchargement : de quoi tirer plusieurs gigaoctets sur une ligne lente.
pub const TOKEN_TTL_SECONDS: i64 = 6 * 3600;

/// Où en est le téléchargement d'une entrée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PullState {
    /// En cours.
    Running,
    /// Posé et vérifié.
    Done,
    /// Échoué ; le motif est dit.
    Failed,
    /// Arrêté à la demande ; la suite reprendra où il en était.
    Cancelled,
}

/// Le suivi d'un téléchargement, tel que le service le rend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PullStatus {
    /// Identifiant de l'entrée du catalogue.
    pub id: String,
    /// État.
    pub state: PullState,
    /// Octets reçus.
    pub received: u64,
    /// Taille totale, si elle est connue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    /// Motif d'un échec.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Fichier posé.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

/// Une entrée du catalogue, avec ce que la machine en a.
#[derive(Debug, Clone, Serialize)]
pub struct EntryView {
    /// L'entrée.
    #[serde(flatten)]
    pub entry: Entry,
    /// Le fichier est posé dans le dossier des téléchargements.
    pub installed: bool,
    /// La configuration du système fournit déjà ce poids (le modèle par défaut, dans
    /// `/nix/store`, ADR 0033) : rien à télécharger.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provided: Option<PathBuf>,
    /// Son chemin, s'il l'est.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Octets déjà reçus d'un téléchargement interrompu, qui reprendra d'ici.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_bytes: Option<u64>,
    /// Le dernier téléchargement de cette entrée depuis le démarrage du service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pull: Option<PullStatus>,
    /// Ce que le moteur réservera pour la servir à la fenêtre de la machine, et où cela tombe
    /// sur sa mémoire : dit avant de télécharger (ADR 0047).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<providers::memory::Assessment>,
}

/// Ce qu'un téléchargement terminé laisse au journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pulled {
    /// L'entrée.
    pub id: String,
    /// Le fichier posé.
    pub file: String,
    /// Son empreinte, celle du catalogue.
    pub sha256: String,
    /// Sa taille.
    pub bytes: u64,
}

struct Suivi {
    statut: PullStatus,
    arret: Arc<AtomicBool>,
}

/// Les téléchargements du service.
pub struct Pulls {
    dir: PathBuf,
    suivis: Arc<Mutex<BTreeMap<String, Suivi>>>,
}

impl std::fmt::Debug for Pulls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pulls")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl Pulls {
    /// Les téléchargements vers `dir`, le dossier des poids téléchargés.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            suivis: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// Le dossier des téléchargements.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Le catalogue, entrée par entrée, avec ce que la machine en a ; `configured` est la
    /// liste des poids que la configuration du système pose ailleurs (`PROPHET_WEIGHTS`).
    #[must_use]
    pub fn view(&self, catalogue: &Catalogue, configured: &[PathBuf]) -> Vec<EntryView> {
        let contexte = providers::memory::context();
        let machine = providers::memory::system();
        catalogue
            .entries
            .iter()
            .map(|entry| {
                let chemin = self.dir.join(&entry.file);
                let installed = chemin.is_file();
                let partial_bytes = std::fs::metadata(partial_path(&self.dir, entry))
                    .ok()
                    .map(|m| m.len());
                EntryView {
                    entry: entry.clone(),
                    installed,
                    provided: provided_by_system(entry, configured),
                    path: installed.then_some(chemin),
                    partial_bytes,
                    pull: self.status(&entry.id),
                    memory: entry
                        .memory(contexte)
                        .map(|need| providers::memory::assess_need(need, machine.as_ref())),
                }
            })
            .collect()
    }

    /// Le suivi d'une entrée.
    #[must_use]
    pub fn status(&self, id: &str) -> Option<PullStatus> {
        self.suivis.lock().ok()?.get(id).map(|s| s.statut.clone())
    }

    /// Tous les suivis.
    #[must_use]
    pub fn all(&self) -> Vec<PullStatus> {
        self.suivis
            .lock()
            .map(|s| s.values().map(|s| s.statut.clone()).collect())
            .unwrap_or_default()
    }

    /// Vrai si un téléchargement de cette entrée est en cours.
    #[must_use]
    pub fn running(&self, id: &str) -> bool {
        self.status(id)
            .is_some_and(|s| s.state == PullState::Running)
    }

    /// Démarre le téléchargement d'une entrée, dans un fil ; `fini` reçoit ce qu'il faut
    /// journaliser quand le poids est posé. Un téléchargement déjà en cours est rendu tel quel.
    ///
    /// # Errors
    /// Le fil n'a pas pu être lancé.
    pub fn start(
        &self,
        entry: Entry,
        egress: Egress,
        fini: impl FnOnce(Pulled) + Send + 'static,
    ) -> Result<PullStatus, String> {
        let arret = Arc::new(AtomicBool::new(false));
        let depart = PullStatus {
            id: entry.id.clone(),
            state: PullState::Running,
            received: 0,
            total: entry.bytes,
            error: None,
            path: None,
        };
        {
            let mut suivis = self.suivis.lock().map_err(|e| e.to_string())?;
            if let Some(suivi) = suivis.get(&entry.id)
                && suivi.statut.state == PullState::Running
            {
                return Ok(suivi.statut.clone());
            }
            suivis.insert(
                entry.id.clone(),
                Suivi {
                    statut: depart.clone(),
                    arret: arret.clone(),
                },
            );
        }
        let suivis = self.suivis.clone();
        let dir = self.dir.clone();
        let id = entry.id.clone();
        let lance = std::thread::Builder::new()
            .name(format!("pull-{}", entry.id))
            .spawn(move || {
                let mettre = |f: &dyn Fn(&mut PullStatus)| {
                    if let Ok(mut suivis) = suivis.lock()
                        && let Some(suivi) = suivis.get_mut(&entry.id)
                    {
                        f(&mut suivi.statut);
                    }
                };
                let resultat = pull(&entry, &dir, &egress, &arret, &mut |p: Progress| {
                    mettre(&|s| {
                        s.received = p.received;
                        s.total = p.total;
                    });
                });
                match resultat {
                    Ok(chemin) => {
                        let bytes = std::fs::metadata(&chemin).map_or(0, |m| m.len());
                        mettre(&|s| {
                            s.state = PullState::Done;
                            s.received = bytes;
                            s.total = Some(bytes);
                            s.path = Some(chemin.clone());
                        });
                        tracing::info!(id = %entry.id, fichier = %chemin.display(), "poids téléchargé et vérifié");
                        fini(Pulled {
                            id: entry.id.clone(),
                            file: entry.file.clone(),
                            sha256: entry.sha256.clone(),
                            bytes,
                        });
                    }
                    Err(PullError::Cancelled) => {
                        mettre(&|s| s.state = PullState::Cancelled);
                        tracing::info!(id = %entry.id, "téléchargement arrêté");
                    }
                    Err(erreur) => {
                        let motif = erreur.to_string();
                        tracing::warn!(id = %entry.id, motif = %motif, "téléchargement échoué");
                        mettre(&|s| {
                            s.state = PullState::Failed;
                            s.error = Some(motif.clone());
                        });
                    }
                }
            });
        if let Err(erreur) = lance {
            if let Ok(mut suivis) = self.suivis.lock() {
                suivis.remove(&id);
            }
            return Err(format!("téléchargement non lancé : {erreur}"));
        }
        Ok(depart)
    }

    /// Demande l'arrêt d'un téléchargement en cours ; faux s'il n'y en a pas.
    #[must_use]
    pub fn cancel(&self, id: &str) -> bool {
        self.suivis.lock().is_ok_and(|suivis| {
            suivis.get(id).is_some_and(|s| {
                let en_cours = s.statut.state == PullState::Running;
                if en_cours {
                    s.arret.store(true, Ordering::Relaxed);
                }
                en_cours
            })
        })
    }

    /// Retire le poids téléchargé d'une entrée, et le début d'un téléchargement interrompu.
    /// Rend ce qu'il faut journaliser, `None` s'il n'y avait rien.
    ///
    /// # Errors
    /// Téléchargement en cours, ou fichier impossible à retirer.
    pub fn remove(&self, entry: &Entry) -> Result<Option<Pulled>, String> {
        if self.running(&entry.id) {
            return Err(format!(
                "{} est en cours de téléchargement : arrêtez-le d'abord",
                entry.id
            ));
        }
        let chemin = self.dir.join(&entry.file);
        let bytes = std::fs::metadata(&chemin).ok().map(|m| m.len());
        let partiel = partial_path(&self.dir, entry);
        let _ = std::fs::remove_file(&partiel);
        if let Ok(mut suivis) = self.suivis.lock() {
            suivis.remove(&entry.id);
        }
        let Some(bytes) = bytes else {
            return Ok(None);
        };
        std::fs::remove_file(&chemin).map_err(|e| format!("{} : {e}", chemin.display()))?;
        Ok(Some(Pulled {
            id: entry.id.clone(),
            file: entry.file.clone(),
            sha256: entry.sha256.clone(),
            bytes,
        }))
    }
}

/// Le poids que la configuration du système fournit déjà pour cette entrée, s'il y en a un : un
/// fichier du même nom, ou, dans `/nix/store`, un chemin qui finit par `-<nom>` (`fetchurl` le
/// nomme ainsi, sous l'empreinte même que porte le catalogue).
#[must_use]
pub fn provided_by_system(entry: &Entry, configured: &[PathBuf]) -> Option<PathBuf> {
    let suffixe = format!("-{}", entry.file);
    configured
        .iter()
        .find(|chemin| {
            chemin.is_file()
                && chemin
                    .file_name()
                    .map(|n| n.to_string_lossy())
                    .is_some_and(|n| n == entry.file || n.ends_with(&suffixe))
        })
        .cloned()
}

/// Ce qu'un modèle du moteur demanderait en mémoire, quand il ne tient pas sur cette machine
/// (ADR 0047). Le routeur nomme un poids de son dossier par son fichier sans `.gguf` : c'est
/// par ce nom qu'on le retrouve parmi les poids installés. Un modèle que rien ne désigne, ou
/// dont l'en-tête ne s'estime pas, n'est pas refusé : on ne devine pas.
#[must_use]
pub fn too_large(
    model: &str,
    installed: &[Result<providers::weights::Weights, String>],
    context: u64,
    system: Option<&providers::memory::System>,
) -> Option<providers::memory::Assessment> {
    let weights = installed.iter().flatten().find(|w| {
        w.path
            .file_stem()
            .is_some_and(|stem| stem.to_string_lossy() == model)
    })?;
    providers::memory::assess(weights, context, system)
        .filter(|a| a.fit == Some(providers::memory::Fit::TooLarge))
}

/// Pourquoi une mission sur `model` ne démarre pas : ce qu'il demande, et ce que la machine a.
#[must_use]
pub fn too_large_reason(
    model: &str,
    assessment: &providers::memory::Assessment,
    system: Option<&providers::memory::System>,
) -> String {
    use providers::memory::gigabytes;
    format!(
        "« {model} » demande environ {} pour une fenêtre de {} tokens (poids {}, cache KV {}){} : le charger ferait paginer toute la machine. Choisissez un modèle plus petit (prophet model ls dit ce que chacun demande).",
        gigabytes(assessment.need.total),
        assessment.need.context,
        gigabytes(assessment.need.weights),
        gigabytes(assessment.need.kv_cache),
        system.map_or_else(String::new, |s| format!(
            " ; cette machine a {}, et le système en garde {}",
            gigabytes(s.total),
            gigabytes(providers::memory::SYSTEM_RESERVE)
        ))
    )
}

/// Le manifeste du téléchargement d'une entrée : sortie réseau vers ses seuls hôtes, rien
/// d'autre. capd émet le jeton sous ce plafond, et la politique Cedar tranche comme pour toute
/// sortie.
///
/// # Errors
/// Manifeste invalide (hôte du catalogue refusé par le schéma des manifestes).
pub fn manifest(entry: &Entry) -> Result<Manifest, String> {
    let hotes = entry
        .hosts
        .iter()
        .map(|h| serde_json::to_string(h).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(", ");
    let texte = format!(
        r#"
[agent]
id = "org.prophet.model-pull"
version = "1.0.0"
name = "Téléchargement de poids"
publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
[model]
# Le schéma exige un modèle ; ce jeton n'en appelle aucun.
preferred = ["local:catalogue"]
[capabilities.max]
"net.egress" = [{hotes}]
"#
    );
    let manifeste = Manifest::from_toml(&texte).map_err(|e| e.to_string())?;
    manifeste.validate().map_err(|e| e.to_string())?;
    Ok(manifeste)
}

/// Les grants demandés pour une entrée : un par hôte permis.
#[must_use]
pub fn grants(entry: &Entry) -> Vec<Grant> {
    entry
        .hosts
        .iter()
        .map(|h| Grant::new(Res::Net, Act::Egress, h))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poids(chemin: &str, octets: u64, kv: Option<u64>) -> providers::weights::Weights {
        providers::weights::Weights {
            path: chemin.into(),
            bytes: octets,
            version: 3,
            architecture: Some("qwen3".into()),
            name: None,
            size_label: None,
            quantization: None,
            context_length: None,
            layers: None,
            tensors: 0,
            kv_bytes_per_token: kv,
            vocabulary: Some(151_936),
            template: None,
        }
    }

    #[test]
    fn un_modele_trop_grand_se_retrouve_par_le_nom_que_le_routeur_lui_donne() {
        let machine = providers::memory::System {
            total: 8 << 30,
            available: 6 << 30,
        };
        let installes = vec![
            Ok(poids(
                "/m/catalogue/Qwen3-8B-Q4_K_M.gguf",
                5_027_783_488,
                Some(147_456),
            )),
            Ok(poids(
                "/m/catalogue/Enorme.gguf",
                40_000_000_000,
                Some(147_456),
            )),
            Ok(poids("/m/catalogue/Muet.gguf", 40_000_000_000, None)),
            Err("casse.gguf : illisible".into()),
        ];
        // Qwen3 8B tient dans 8 Gio ; l'énorme non, et le refus dit ce qu'il demande.
        assert!(too_large("Qwen3-8B-Q4_K_M", &installes, 4096, Some(&machine)).is_none());
        let refus = too_large("Enorme", &installes, 4096, Some(&machine)).unwrap();
        assert!(refus.need.total > 40_000_000_000);
        let raison = too_large_reason("Enorme", &refus, Some(&machine));
        assert!(
            raison.contains("paginer") && raison.contains("8,6 Go"),
            "{raison}"
        );
        // Ni un en-tête sans de quoi estimer, ni un nom inconnu (un préréglage), ni une machine
        // dont la mémoire est inconnue ne font refuser : on ne devine pas.
        assert!(too_large("Muet", &installes, 4096, Some(&machine)).is_none());
        assert!(too_large("reflect", &installes, 4096, Some(&machine)).is_none());
        assert!(too_large("Enorme", &installes, 4096, None).is_none());
    }

    #[test]
    fn le_manifeste_d_un_telechargement_ne_permet_que_ses_hotes() {
        let entry = Catalogue::builtin().entries[0].clone();
        let manifeste = manifest(&entry).unwrap();
        let plafond = manifeste.ceiling().unwrap();
        assert_eq!(plafond.len(), entry.hosts.len());
        assert!(
            plafond
                .iter()
                .all(|g| g.res == Res::Net && g.act == Act::Egress)
        );
        let demandes = grants(&entry);
        assert!(
            demandes
                .iter()
                .all(|g| plafond.iter().any(|c| g.is_subset_of(c)))
        );
    }

    #[test]
    fn retirer_ne_touche_que_le_dossier_des_telechargements() {
        let dir = tempfile::tempdir().unwrap();
        let pulls = Pulls::new(dir.path().join("catalogue"));
        let entry = Catalogue::builtin().entries[0].clone();
        assert_eq!(pulls.remove(&entry).unwrap(), None, "rien à retirer");
        std::fs::create_dir_all(pulls.dir()).unwrap();
        std::fs::write(pulls.dir().join(&entry.file), b"GGUF").unwrap();
        std::fs::write(partial_path(pulls.dir(), &entry), b"GG").unwrap();
        let vue = pulls.view(&Catalogue::builtin(), &[]);
        assert!(vue[0].installed);
        assert_eq!(vue[0].partial_bytes, Some(2));
        let retire = pulls.remove(&entry).unwrap().unwrap();
        assert_eq!(retire.bytes, 4);
        assert!(!pulls.dir().join(&entry.file).exists());
        assert!(!partial_path(pulls.dir(), &entry).exists());
        assert!(!pulls.view(&Catalogue::builtin(), &[])[0].installed);
    }

    #[test]
    fn un_poids_fourni_par_la_configuration_n_est_pas_a_telecharger() {
        let magasin = tempfile::tempdir().unwrap();
        let entry = Catalogue::builtin().entries[0].clone();
        let fourni = magasin.path().join(format!("0a1b2c3d4e5f-{}", entry.file));
        std::fs::write(&fourni, b"GGUF").unwrap();
        let autre = magasin.path().join("autre.gguf");
        std::fs::write(&autre, b"GGUF").unwrap();
        assert_eq!(
            provided_by_system(&entry, &[autre.clone(), fourni.clone()]),
            Some(fourni)
        );
        assert_eq!(provided_by_system(&entry, &[autre]), None);
        // Nommé mais absent : la configuration promet un fichier que la machine n'a pas.
        assert_eq!(
            provided_by_system(&entry, &[magasin.path().join(&entry.file)]),
            None
        );
    }
}
