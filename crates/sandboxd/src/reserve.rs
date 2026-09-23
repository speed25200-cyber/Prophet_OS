//! La réserve de microVM (M5-T4, ADR 0045).
//!
//! Démarrer une microVM coûte le démarrage d'un noyau ; une tâche qui exécute un programme de
//! quelques millisecondes n'a pas à l'attendre. La réserve démarre une fois un invité **sans
//! tâche** : son second disque n'est qu'un disque d'attente, et l'invité le dit
//! (« PROPHET_INVITE_ATTENTE »). Elle le met en pause et en fait un instantané, mémoire
//! comprise, puis garde `taille` machines restaurées depuis cet instantané, en pause. Prendre
//! une machine, c'est remplacer son disque d'attente par celui de la tâche et la reprendre :
//! l'invité voit la taille du disque changer, lit le chemin de travail sur le disque, et
//! exécute comme au démarrage à froid. Une machine ne sert qu'une fois ; la réserve en restaure
//! une autre en arrière-plan.
//!
//! Chaque machine restaurée part de la même mémoire. L'invité ne détient aucun secret à ce
//! moment-là, et le noyau d'invité rafraîchit son aléa au réveil quand l'hyperviseur lui
//! signale le clonage (VMGenID) ; c'est dit dans l'ADR, avec ce qui reste à mesurer.

use std::collections::VecDeque;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::caps::MicrovmImages;
use crate::invite::MARQUE_ATTENTE;

/// Taille du disque d'attente : bien moins que le plus petit disque de travail (64 Mio de
/// marge), pour que son remplacement se voie à la taille.
const ATTENTE_OCTETS: u64 = 1024 * 1024;
/// Le temps qu'un invité a pour démarrer et dire qu'il attend.
const DEMARRAGE: Duration = Duration::from_secs(20);

/// Une machine restaurée, en pause, qui attend le disque de sa tâche.
#[derive(Debug)]
pub struct Membre {
    /// Le moniteur ; sa sortie standard est la console de l'invité.
    pub child: Child,
    /// Dossier propre à cette machine : son socket d'API, bientôt le disque de sa tâche.
    pub dossier: PathBuf,
    /// Socket d'API du moniteur.
    pub api: PathBuf,
}

impl Membre {
    /// Arrête le moniteur et retire le dossier de la machine.
    pub(crate) fn abandonner(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dossier);
    }
}

/// L'état partagé entre la réserve et son fil de remplissage.
#[derive(Debug, Default)]
struct Etat {
    prets: VecDeque<Membre>,
    arret: bool,
    erreur: Option<String>,
    /// Dernière durée de restauration d'une machine, pour le dire.
    restauration: Option<Duration>,
}

#[derive(Debug, Default)]
struct Partage {
    etat: Mutex<Etat>,
    signal: Condvar,
}

/// Ce que la réserve dit d'elle-même.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Statut {
    /// Machines voulues.
    pub cible: usize,
    /// Machines prêtes.
    pub pretes: usize,
    /// Dernière durée de restauration, en millisecondes.
    pub restauration_ms: Option<u64>,
    /// Pourquoi la réserve ne se remplit pas, le cas échéant.
    pub erreur: Option<String>,
}

/// La réserve et son fil de remplissage.
pub struct Reserve {
    partage: Arc<Partage>,
    cible: usize,
    racine: PathBuf,
    fil: Option<std::thread::JoinHandle<()>>,
}

impl std::fmt::Debug for Reserve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reserve")
            .field("statut", &self.statut())
            .finish_non_exhaustive()
    }
}

impl Reserve {
    /// Démarre la réserve : en arrière-plan, un invité sans tâche, son instantané, puis
    /// `cible` machines restaurées.
    #[must_use]
    pub fn demarrer(
        firecracker: &str,
        images: &MicrovmImages,
        cible: usize,
        racine: PathBuf,
    ) -> Self {
        let partage = Arc::new(Partage::default());
        let fil = {
            let partage = partage.clone();
            let firecracker = firecracker.to_owned();
            let images = images.clone();
            let racine = racine.clone();
            std::thread::Builder::new()
                .name("reserve-microvm".into())
                .spawn(move || remplir(&partage, &firecracker, &images, cible, &racine))
                .ok()
        };
        Self {
            partage,
            cible,
            racine,
            fil,
        }
    }

    /// Prend une machine prête, s'il y en a une ; la réserve en restaure une autre.
    #[must_use]
    pub fn prendre(&self) -> Option<Membre> {
        let membre = self.partage.etat.lock().ok()?.prets.pop_front();
        self.partage.signal.notify_all();
        membre
    }

    /// Attend que la réserve soit pleine, au plus `delai` ; vrai si elle l'est.
    #[must_use]
    pub fn attendre_pleine(&self, delai: Duration) -> bool {
        let limite = Instant::now() + delai;
        let Ok(mut etat) = self.partage.etat.lock() else {
            return false;
        };
        while etat.prets.len() < self.cible && etat.erreur.is_none() {
            let reste = limite.saturating_duration_since(Instant::now());
            if reste.is_zero() {
                return false;
            }
            let Ok((suite, _)) = self.partage.signal.wait_timeout(etat, reste) else {
                return false;
            };
            etat = suite;
        }
        etat.prets.len() >= self.cible
    }

    /// Ce que la réserve dit d'elle-même.
    #[must_use]
    pub fn statut(&self) -> Statut {
        let etat = self.partage.etat.lock().ok();
        Statut {
            cible: self.cible,
            pretes: etat.as_ref().map_or(0, |e| e.prets.len()),
            restauration_ms: etat
                .as_ref()
                .and_then(|e| e.restauration)
                .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
            erreur: etat.and_then(|e| e.erreur.clone()),
        }
    }
}

impl Drop for Reserve {
    fn drop(&mut self) {
        if let Ok(mut etat) = self.partage.etat.lock() {
            etat.arret = true;
        }
        self.partage.signal.notify_all();
        if let Some(fil) = self.fil.take() {
            let _ = fil.join();
        }
        if let Ok(mut etat) = self.partage.etat.lock() {
            for membre in etat.prets.drain(..) {
                membre.abandonner();
            }
        }
        let _ = std::fs::remove_dir_all(&self.racine);
    }
}

/// L'instantané d'un invité en attente : son état et sa mémoire.
struct Instantane {
    etat: PathBuf,
    memoire: PathBuf,
}

fn remplir(
    partage: &Partage,
    firecracker: &str,
    images: &MicrovmImages,
    cible: usize,
    racine: &Path,
) {
    let mut numero = 0u64;
    let depart = std::fs::create_dir_all(racine)
        .map_err(|e| format!("dossier de la réserve {} : {e}", racine.display()))
        .and_then(|()| amorcer(firecracker, images, racine));
    let instantane = match depart {
        Ok((modele, instantane)) => {
            if let Ok(mut etat) = partage.etat.lock() {
                etat.prets.push_back(modele);
            }
            partage.signal.notify_all();
            instantane
        }
        Err(raison) => {
            tracing::warn!(raison = %raison, "réserve de microVM indisponible");
            if let Ok(mut etat) = partage.etat.lock() {
                etat.erreur = Some(raison);
            }
            partage.signal.notify_all();
            return;
        }
    };
    loop {
        {
            let Ok(mut etat) = partage.etat.lock() else {
                return;
            };
            while !etat.arret && etat.prets.len() >= cible {
                let Ok(suite) = partage.signal.wait(etat) else {
                    return;
                };
                etat = suite;
            }
            if etat.arret {
                return;
            }
        }
        numero += 1;
        let debut = Instant::now();
        match restaurer(firecracker, &instantane, &racine.join(format!("m{numero}"))) {
            Ok(membre) => {
                let Ok(mut etat) = partage.etat.lock() else {
                    membre.abandonner();
                    return;
                };
                if etat.arret {
                    membre.abandonner();
                    return;
                }
                etat.restauration = Some(debut.elapsed());
                etat.erreur = None;
                etat.prets.push_back(membre);
            }
            Err(raison) => {
                tracing::warn!(raison = %raison, "restauration d'une microVM de réserve");
                if let Ok(mut etat) = partage.etat.lock() {
                    etat.erreur = Some(raison);
                }
                partage.signal.notify_all();
                // Une restauration qui échoue échouera encore : on ne boucle pas dessus.
                return;
            }
        }
        partage.signal.notify_all();
    }
}

/// La configuration d'un invité de réserve : même machine qu'au démarrage à froid, sans tâche
/// sur la ligne de commande, le disque d'attente en second lecteur, et sans vsock — rien ne
/// l'écoute encore, et un même instantané restauré plusieurs fois ne peut pas lier plusieurs
/// fois le même socket.
#[must_use]
pub fn configuration(images: &MicrovmImages, attente: &Path) -> serde_json::Value {
    serde_json::json!({
        "boot-source": {
            "kernel_image_path": images.kernel,
            "boot_args": "console=ttyS0 reboot=k panic=1 pci=off prophet.pool=1"
        },
        "drives": [
            {
                "drive_id": "rootfs",
                "path_on_host": images.rootfs,
                "is_root_device": true,
                "is_read_only": true
            },
            {
                "drive_id": "travail",
                "path_on_host": attente.display().to_string(),
                "is_root_device": false,
                "is_read_only": false
            }
        ],
        "machine-config": {
            "vcpu_count": 2,
            "mem_size_mib": 1024,
            "smt": false
        }
    })
}

fn moniteur(firecracker: &str, api: &Path, configuration: Option<&Path>) -> Result<Child, String> {
    let mut commande = Command::new(firecracker);
    commande.arg("--api-sock").arg(api);
    if let Some(configuration) = configuration {
        commande.arg("--config-file").arg(configuration);
    }
    commande
        .env_clear()
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{firecracker} : {e}"))
}

/// Démarre l'invité modèle, attend qu'il dise son attente, le met en pause et en fait
/// l'instantané. Le modèle reste en pause : il est la première machine de la réserve.
fn amorcer(
    firecracker: &str,
    images: &MicrovmImages,
    racine: &Path,
) -> Result<(Membre, Instantane), String> {
    let attente = racine.join("attente.img");
    std::fs::File::create(&attente)
        .and_then(|f| f.set_len(ATTENTE_OCTETS))
        .map_err(|e| format!("disque d'attente : {e}"))?;
    let dossier = racine.join("m0");
    std::fs::create_dir_all(&dossier).map_err(|e| e.to_string())?;
    let api = dossier.join("api.sock");
    let chemin_configuration = dossier.join("config.json");
    std::fs::write(
        &chemin_configuration,
        configuration(images, &attente).to_string(),
    )
    .map_err(|e| e.to_string())?;
    let mut child = moniteur(firecracker, &api, Some(&chemin_configuration))?;
    let sortie = child.stdout.take().ok_or("console du moniteur absente")?;
    match attendre_la_marque(sortie, MARQUE_ATTENTE, DEMARRAGE) {
        Ok(sortie) => child.stdout = Some(sortie),
        Err(raison) => {
            let _ = child.kill();
            let mut erreur = String::new();
            if let Some(flux) = child.stderr.as_mut() {
                let _ = flux.read_to_string(&mut erreur);
            }
            let _ = child.wait();
            return Err(format!("{raison} {}", erreur.trim()));
        }
    }
    let instantane = Instantane {
        etat: racine.join("etat.snap"),
        memoire: racine.join("memoire.snap"),
    };
    let fait = api_appel(
        &api,
        "PATCH",
        "/vm",
        &serde_json::json!({"state": "Paused"}),
    )
    .and_then(|()| {
        api_appel(
            &api,
            "PUT",
            "/snapshot/create",
            &serde_json::json!({
                "snapshot_type": "Full",
                "snapshot_path": instantane.etat,
                "mem_file_path": instantane.memoire,
            }),
        )
    });
    let membre = Membre {
        child,
        dossier,
        api,
    };
    match fait {
        Ok(()) => Ok((membre, instantane)),
        Err(raison) => {
            membre.abandonner();
            Err(format!("instantané de l'invité en attente : {raison}"))
        }
    }
}

/// Une nouvelle machine depuis l'instantané, laissée en pause.
fn restaurer(firecracker: &str, instantane: &Instantane, dossier: &Path) -> Result<Membre, String> {
    std::fs::create_dir_all(dossier).map_err(|e| e.to_string())?;
    let api = dossier.join("api.sock");
    let _ = std::fs::remove_file(&api);
    let child = moniteur(firecracker, &api, None)?;
    let membre = Membre {
        child,
        dossier: dossier.to_owned(),
        api,
    };
    let limite = Instant::now() + Duration::from_secs(2);
    while !membre.api.exists() {
        if Instant::now() >= limite {
            membre.abandonner();
            return Err("le moniteur n'a pas créé son socket d'API en deux secondes".into());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let charge = api_appel(
        &membre.api,
        "PUT",
        "/snapshot/load",
        &serde_json::json!({
            "snapshot_path": instantane.etat,
            "mem_backend": {"backend_type": "File", "backend_path": instantane.memoire},
            "resume_vm": false,
        }),
    );
    match charge {
        Ok(()) => Ok(membre),
        Err(raison) => {
            membre.abandonner();
            Err(format!("restauration depuis l'instantané : {raison}"))
        }
    }
}

/// Confie à une machine de la réserve le disque de sa tâche, et la reprend.
///
/// # Errors
/// Le moniteur refuse le remplacement du disque ou la reprise.
pub fn confier(membre: &Membre, disque: &Path) -> Result<(), String> {
    let remplacer = || {
        api_appel(
            &membre.api,
            "PATCH",
            "/drives/travail",
            &serde_json::json!({"drive_id": "travail", "path_on_host": disque}),
        )
    };
    let reprendre = || {
        api_appel(
            &membre.api,
            "PATCH",
            "/vm",
            &serde_json::json!({"state": "Resumed"}),
        )
    };
    // Remplacer puis reprendre : l'invité se réveille sur le bon disque. Un moniteur qui ne
    // remplace un disque que machine en marche reçoit l'ordre inverse.
    match remplacer() {
        Ok(()) => reprendre(),
        Err(en_pause) => reprendre()
            .and_then(|()| remplacer())
            .map_err(|apres| format!("{en_pause} ; puis, machine reprise : {apres}")),
    }
}

/// Lit la console d'un moniteur jusqu'à une ligne donnée, au plus `delai`, et rend le flux pour
/// la suite. Lu octet par octet : rien de ce qui suit la marque n'est avalé.
fn attendre_la_marque(
    sortie: ChildStdout,
    marque: &str,
    delai: Duration,
) -> Result<ChildStdout, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let attendue = marque.to_owned();
    std::thread::spawn(move || {
        let marque = attendue;
        let mut sortie = sortie;
        let mut ligne = Vec::new();
        let mut fin = Vec::new();
        let mut octet = [0u8; 1];
        loop {
            match sortie.read(&mut octet) {
                Ok(0) | Err(_) => {
                    let _ = tx.send(Err(String::from_utf8_lossy(&fin).into_owned()));
                    return;
                }
                Ok(_) if octet[0] == b'\n' => {
                    if String::from_utf8_lossy(&ligne).trim_end_matches('\r') == marque {
                        let _ = tx.send(Ok(sortie));
                        return;
                    }
                    fin.extend_from_slice(&ligne);
                    fin.push(b'\n');
                    if fin.len() > 4096 {
                        fin.drain(..fin.len() - 4096);
                    }
                    ligne.clear();
                }
                Ok(_) => ligne.push(octet[0]),
            }
        }
    });
    match rx.recv_timeout(delai) {
        Ok(Ok(sortie)) => Ok(sortie),
        Ok(Err(console)) => Err(format!(
            "l'invité s'est arrêté avant de dire « {marque} » ; fin de sa console : {}",
            console.trim()
        )),
        Err(_) => Err(format!(
            "l'invité n'a pas dit « {marque} » en {} s",
            delai.as_secs()
        )),
    }
}

/// Un appel à l'API du moniteur : HTTP/1.1 sur son socket Unix, corps JSON, réponse 2xx
/// attendue ; sinon, l'erreur qu'il donne.
///
/// # Errors
/// Socket injoignable, réponse illisible, ou refus du moniteur.
pub fn api_appel(
    socket: &Path,
    methode: &str,
    chemin: &str,
    corps: &serde_json::Value,
) -> Result<(), String> {
    let corps = corps.to_string();
    let mut flux = UnixStream::connect(socket).map_err(|e| format!("{methode} {chemin} : {e}"))?;
    flux.set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    write!(
        flux,
        "{methode} {chemin} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\n\r\n{corps}",
        corps.len()
    )
    .map_err(|e| format!("{methode} {chemin} : {e}"))?;
    let mut lecteur = BufReader::new(flux);
    let mut statut = String::new();
    lecteur
        .read_line(&mut statut)
        .map_err(|e| format!("{methode} {chemin} : {e}"))?;
    let code: u16 = statut
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| {
            format!(
                "{methode} {chemin} : réponse illisible « {} »",
                statut.trim()
            )
        })?;
    let mut longueur = 0usize;
    loop {
        let mut entete = String::new();
        if lecteur.read_line(&mut entete).map_err(|e| e.to_string())? == 0 {
            break;
        }
        let entete = entete.trim_end();
        if entete.is_empty() {
            break;
        }
        if let Some((nom, valeur)) = entete.split_once(':')
            && nom.eq_ignore_ascii_case("content-length")
        {
            longueur = valeur.trim().parse().unwrap_or(0);
        }
    }
    let mut reponse = vec![0; longueur.min(64 * 1024)];
    lecteur
        .read_exact(&mut reponse)
        .map_err(|e| format!("{methode} {chemin} : {e}"))?;
    if (200..300).contains(&code) {
        Ok(())
    } else {
        let message = serde_json::from_slice::<serde_json::Value>(&reponse)
            .ok()
            .and_then(|v| v["fault_message"].as_str().map(str::to_owned))
            .unwrap_or_else(|| String::from_utf8_lossy(&reponse).into_owned());
        Err(format!("{methode} {chemin} : HTTP {code} {message}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// Un moniteur simulé : une réponse par connexion, et les requêtes reçues.
    fn moniteur_simule(
        reponses: Vec<&'static str>,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        std::thread::JoinHandle<Vec<String>>,
    ) {
        let dossier = tempfile::tempdir().unwrap();
        let socket = dossier.path().join("api.sock");
        let ecoute = UnixListener::bind(&socket).unwrap();
        let fil = std::thread::spawn(move || {
            let mut recues = Vec::new();
            for reponse in reponses {
                let (flux, _) = ecoute.accept().unwrap();
                let mut lecteur = BufReader::new(flux);
                let mut requete = String::new();
                let mut longueur = 0;
                loop {
                    let mut ligne = String::new();
                    lecteur.read_line(&mut ligne).unwrap();
                    if let Some(v) = ligne.to_ascii_lowercase().strip_prefix("content-length:") {
                        longueur = v.trim().parse().unwrap();
                    }
                    requete.push_str(&ligne);
                    if ligne == "\r\n" {
                        break;
                    }
                }
                let mut corps = vec![0; longueur];
                lecteur.read_exact(&mut corps).unwrap();
                requete.push_str(&String::from_utf8_lossy(&corps));
                lecteur.get_mut().write_all(reponse.as_bytes()).unwrap();
                recues.push(requete);
            }
            recues
        });
        (dossier, socket, fil)
    }

    #[test]
    fn l_api_du_moniteur_se_parle_en_http_sur_son_socket() {
        let (_dossier, socket, fil) = moniteur_simule(vec![
            "HTTP/1.1 204 No Content\r\nServer: Firecracker API\r\n\r\n",
            "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: 38\r\n\r\n{\"fault_message\":\"invalid operation\"}\n",
        ]);
        api_appel(
            &socket,
            "PATCH",
            "/vm",
            &serde_json::json!({"state": "Paused"}),
        )
        .unwrap();
        let refus =
            api_appel(&socket, "PUT", "/snapshot/load", &serde_json::json!({})).unwrap_err();
        assert!(
            refus.contains("HTTP 400") && refus.contains("invalid operation"),
            "{refus}"
        );
        let recues = fil.join().unwrap();
        assert!(
            recues[0].starts_with("PATCH /vm HTTP/1.1\r\n"),
            "{}",
            recues[0]
        );
        assert!(
            recues[0].ends_with("{\"state\":\"Paused\"}"),
            "{}",
            recues[0]
        );
        assert!(recues[1].starts_with("PUT /snapshot/load HTTP/1.1\r\n"));
    }

    #[test]
    fn la_configuration_de_reserve_n_a_ni_tache_ni_vsock() {
        let images = MicrovmImages {
            kernel: "/noyau".into(),
            rootfs: "/racine.squashfs".into(),
        };
        let configuration = configuration(&images, Path::new("/reserve/attente.img"));
        let ligne = configuration["boot-source"]["boot_args"].as_str().unwrap();
        assert!(ligne.contains("prophet.pool=1"), "{ligne}");
        assert!(!ligne.contains("prophet.workdir"), "{ligne}");
        assert!(configuration.get("vsock").is_none());
        assert_eq!(configuration["drives"][0]["is_read_only"], true);
        assert_eq!(configuration["drives"][1]["drive_id"], "travail");
        assert_eq!(
            configuration["drives"][1]["path_on_host"],
            "/reserve/attente.img"
        );
        assert_eq!(configuration["machine-config"]["mem_size_mib"], 1024);
    }

    #[test]
    fn la_marque_d_attente_est_lue_sans_avaler_la_suite() {
        let mut enfant = Command::new("sh")
            .args([
                "-c",
                "echo '[ 0.1] noyau'; echo PROPHET_INVITE_ATTENTE; echo suite",
            ])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let sortie = enfant.stdout.take().unwrap();
        let mut sortie =
            attendre_la_marque(sortie, MARQUE_ATTENTE, Duration::from_secs(5)).unwrap();
        let mut reste = String::new();
        sortie.read_to_string(&mut reste).unwrap();
        assert_eq!(reste, "suite\n");
        let _ = enfant.wait();
        // Un invité qui s'arrête avant de dire son attente est dit tel.
        let mut muet = Command::new("sh")
            .args(["-c", "echo panique"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let erreur = attendre_la_marque(
            muet.stdout.take().unwrap(),
            MARQUE_ATTENTE,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(erreur.contains("panique"), "{erreur}");
        let _ = muet.wait();
    }
}
