//! Télécharger un poids du catalogue, par le proxy de sortie, et le vérifier (M8-T7, ADR 0046).
//!
//! La requête part sur le socket d'egress, jamais en direct : le jeton de la tâche de
//! téléchargement voyage dans `Proxy-Authorization`, que le proxy retire après que capd a
//! tranché sur l'hôte. Chaque redirection repasse par le proxy, et seulement vers un hôte que
//! l'entrée du catalogue permet.
//!
//! Rien de ce qui arrive n'est cru avant d'être vérifié. Le fichier s'écrit sous un nom caché
//! (`.<fichier>.part`) ; il ne prend son nom qu'une fois sa taille, son empreinte SHA-256 et son
//! en-tête GGUF vérifiés. Une empreinte fausse efface le fichier ; une connexion coupée le garde,
//! et le téléchargement suivant reprend où il s'était arrêté (`Range`).

use std::fs::{File, OpenOptions};
use std::io::{BufRead as _, BufReader, Read, Write as _};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use base64::Engine as _;
use prophet_types::cap::Token;
use serde::Serialize;
use sha2::{Digest as _, Sha256};

use crate::catalogue::{Entry, Url};

/// Redirections suivies au plus.
const MAX_REDIRECTS: usize = 8;
/// Taille maximale d'un en-tête de réponse.
const MAX_HEAD_BYTES: usize = 64 * 1024;
/// Corps d'erreur gardé au plus pour le dire.
const MAX_ERROR_BYTES: u64 = 4096;
/// Un fichier de poids plus grand que ceci est refusé, taille annoncée ou reçue.
const MAX_WEIGHTS_BYTES: u64 = 256 * 1024 * 1024 * 1024;
/// Délai sans un octet reçu avant d'abandonner (le fichier partiel reste, pour reprendre).
const READ_TIMEOUT: Duration = Duration::from_secs(60);
/// Taille des blocs lus et écrits.
const CHUNK: usize = 256 * 1024;

/// Le chemin vers le proxy de sortie, avec le jeton de la tâche de téléchargement.
pub struct Egress {
    socket: PathBuf,
    token_header: String,
}

impl std::fmt::Debug for Egress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Egress")
            .field("socket", &self.socket)
            .finish_non_exhaustive()
    }
}

impl Egress {
    /// Prépare le chemin pour un jeton.
    ///
    /// # Errors
    /// Jeton non sérialisable.
    pub fn new(socket: impl Into<PathBuf>, token: &Token) -> Result<Self, PullError> {
        let json = serde_json::to_string(token)
            .map_err(|e| PullError::Transport(format!("jeton non sérialisable : {e}")))?;
        Ok(Self {
            socket: socket.into(),
            token_header: base64::engine::general_purpose::STANDARD.encode(json),
        })
    }
}

/// Où en est un téléchargement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Progress {
    /// Octets reçus, reprise comprise.
    pub received: u64,
    /// Taille totale, si l'entrée ou le serveur la donne.
    pub total: Option<u64>,
}

/// Pourquoi un téléchargement n'a pas abouti.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PullError {
    /// Le proxy ou le serveur a refusé.
    #[error("refusé ({status}) : {detail}")]
    Refused {
        /// Code HTTP.
        status: u16,
        /// Ce que la réponse en dit.
        detail: String,
    },
    /// Le proxy est injoignable, la connexion coupée, la réponse illisible.
    #[error("{0}")]
    Transport(String),
    /// Le fichier reçu n'est pas celui du catalogue ; il est effacé.
    #[error("fichier refusé : {0}")]
    Integrity(String),
    /// Écriture impossible dans le dossier des poids.
    #[error("écriture impossible : {0}")]
    Io(String),
    /// Arrêté à la demande ; le fichier partiel reste pour reprendre.
    #[error("téléchargement arrêté")]
    Cancelled,
}

/// Le chemin partiel d'une entrée dans un dossier.
#[must_use]
pub fn partial_path(dir: &Path, entry: &Entry) -> PathBuf {
    dir.join(format!(".{}.part", entry.file))
}

/// Télécharge une entrée dans `dir` et la vérifie ; rend le chemin du fichier posé.
///
/// Un fichier déjà là et conforme est rendu sans rien télécharger ; un fichier là mais non
/// conforme n'est pas touché, et c'est une erreur : il n'a pas été posé par ce téléchargement.
///
/// # Errors
/// Voir [`PullError`].
pub fn pull(
    entry: &Entry,
    dir: &Path,
    egress: &Egress,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
) -> Result<PathBuf, PullError> {
    let dest = dir.join(&entry.file);
    if dest.exists() {
        let (taille, empreinte) = hash_file(&dest)?;
        if empreinte == entry.sha256 {
            progress(Progress {
                received: taille,
                total: Some(taille),
            });
            return Ok(dest);
        }
        return Err(PullError::Integrity(format!(
            "{} existe déjà et n'a pas l'empreinte du catalogue ; retirez-le d'abord",
            dest.display()
        )));
    }
    std::fs::create_dir_all(dir).map_err(|e| PullError::Io(format!("{} : {e}", dir.display())))?;
    let part = partial_path(dir, entry);
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    if part.exists() {
        let mut fichier = File::open(&part).map_err(|e| PullError::Io(e.to_string()))?;
        received = hash_into(&mut fichier, &mut hasher)?;
    }
    let mut url = Url::parse(&entry.url).map_err(PullError::Transport)?;
    let mut redirections = 0;
    let (lecteur, total) = loop {
        if !entry.permits(&url.authority) {
            return Err(PullError::Refused {
                status: 0,
                detail: format!(
                    "redirection vers {}, hôte que le catalogue ne permet pas",
                    url.authority
                ),
            });
        }
        let mut lecteur = request(egress, &url, received)?;
        let (status, entetes) = read_head(&mut lecteur)?;
        match status {
            301 | 302 | 303 | 307 | 308 => {
                redirections += 1;
                if redirections > MAX_REDIRECTS {
                    return Err(PullError::Transport(format!(
                        "plus de {MAX_REDIRECTS} redirections"
                    )));
                }
                let location = header(&entetes, "location").ok_or_else(|| {
                    PullError::Transport(format!("redirection {status} sans Location"))
                })?;
                url = url.join(location).map_err(PullError::Transport)?;
            }
            200 => {
                if received > 0 {
                    // Le serveur ignore la reprise : on repart de zéro.
                    received = 0;
                    hasher = Sha256::new();
                    File::create(&part).map_err(|e| PullError::Io(e.to_string()))?;
                }
                let total = header(&entetes, "content-length").and_then(|v| v.parse().ok());
                break (body(lecteur, &entetes)?, total);
            }
            206 => {
                let plage = header(&entetes, "content-range").unwrap_or_default();
                let (debut, total) = content_range(plage).ok_or_else(|| {
                    PullError::Transport(format!("reprise illisible : {plage:?}"))
                })?;
                if debut != received {
                    return Err(PullError::Transport(format!(
                        "reprise à l'octet {debut} au lieu de {received}"
                    )));
                }
                break (body(lecteur, &entetes)?, total);
            }
            416 if received > 0 => {
                // Le fichier partiel est déjà entier, ou plus long que l'original : on le
                // vérifie tel quel, et une empreinte fausse l'effacera.
                break (Box::new(std::io::empty()) as Box<dyn Read>, Some(received));
            }
            _ => {
                let detail = error_detail(lecteur, &entetes);
                return Err(PullError::Refused { status, detail });
            }
        }
    };
    let total = entry.bytes.or(total);
    if let Some(total) = total
        && total > MAX_WEIGHTS_BYTES
    {
        return Err(PullError::Integrity(format!(
            "taille annoncée démesurée : {total} octets"
        )));
    }
    progress(Progress { received, total });
    let mut fichier = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&part)
        .map_err(|e| PullError::Io(format!("{} : {e}", part.display())))?;
    let mut lecteur = lecteur;
    let mut bloc = vec![0u8; CHUNK];
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = fichier.sync_data();
            return Err(PullError::Cancelled);
        }
        let n = match lecteur.read(&mut bloc) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                let _ = fichier.sync_data();
                return Err(PullError::Transport(format!(
                    "connexion coupée après {received} octets ({e}) ; relancez pour reprendre"
                )));
            }
        };
        received += n as u64;
        if received > total.unwrap_or(MAX_WEIGHTS_BYTES) {
            drop(fichier);
            let _ = std::fs::remove_file(&part);
            return Err(PullError::Integrity(format!(
                "plus de {} octets reçus",
                total.unwrap_or(MAX_WEIGHTS_BYTES)
            )));
        }
        fichier
            .write_all(&bloc[..n])
            .map_err(|e| PullError::Io(e.to_string()))?;
        hasher.update(&bloc[..n]);
        progress(Progress { received, total });
    }
    if let Some(total) = total
        && received < total
    {
        let _ = fichier.sync_data();
        return Err(PullError::Transport(format!(
            "connexion coupée à {received} octets sur {total} ; relancez pour reprendre"
        )));
    }
    fichier
        .sync_all()
        .map_err(|e| PullError::Io(e.to_string()))?;
    drop(fichier);
    let rejeter = |raison: String| {
        let _ = std::fs::remove_file(&part);
        Err(PullError::Integrity(raison))
    };
    if let Some(attendu) = entry.bytes
        && attendu != received
    {
        return rejeter(format!("{received} octets au lieu de {attendu}"));
    }
    let empreinte = hex(&hasher.finalize());
    if empreinte != entry.sha256 {
        return rejeter(format!(
            "empreinte {empreinte}, le catalogue attend {}",
            entry.sha256
        ));
    }
    if let Err(raison) = crate::weights::read(&part) {
        return rejeter(format!("en-tête GGUF : {raison}"));
    }
    // Le moteur local tourne sous un autre compte : il doit pouvoir lire le fichier.
    std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o644))
        .map_err(|e| PullError::Io(e.to_string()))?;
    std::fs::rename(&part, &dest).map_err(|e| PullError::Io(e.to_string()))?;
    if let Ok(dossier) = File::open(dir) {
        let _ = dossier.sync_all();
    }
    Ok(dest)
}

/// Taille et empreinte SHA-256 d'un fichier.
///
/// # Errors
/// Fichier illisible.
pub fn hash_file(path: &Path) -> Result<(u64, String), PullError> {
    let mut fichier =
        File::open(path).map_err(|e| PullError::Io(format!("{} : {e}", path.display())))?;
    let mut hasher = Sha256::new();
    let taille = hash_into(&mut fichier, &mut hasher)?;
    Ok((taille, hex(&hasher.finalize())))
}

fn hash_into(fichier: &mut File, hasher: &mut Sha256) -> Result<u64, PullError> {
    let mut bloc = vec![0u8; CHUNK];
    let mut taille = 0u64;
    loop {
        let n = fichier
            .read(&mut bloc)
            .map_err(|e| PullError::Io(e.to_string()))?;
        if n == 0 {
            return Ok(taille);
        }
        hasher.update(&bloc[..n]);
        taille += n as u64;
    }
}

fn hex(octets: &[u8]) -> String {
    use std::fmt::Write as _;
    octets.iter().fold(String::new(), |mut s, o| {
        let _ = write!(s, "{o:02x}");
        s
    })
}

/// Envoie la requête sur le socket du proxy, en forme absolue.
fn request(egress: &Egress, url: &Url, from: u64) -> Result<BufReader<UnixStream>, PullError> {
    let mut flux = UnixStream::connect(&egress.socket).map_err(|e| {
        PullError::Transport(format!(
            "proxy de sortie injoignable ({}) : {e}",
            egress.socket.display()
        ))
    })?;
    flux.set_read_timeout(Some(READ_TIMEOUT))
        .and_then(|()| flux.set_write_timeout(Some(READ_TIMEOUT)))
        .map_err(|e| PullError::Transport(e.to_string()))?;
    let mut requete = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nProxy-Authorization: Prophet {}\r\nUser-Agent: prophet-os\r\nAccept: */*\r\nConnection: close\r\n",
        url.full(),
        url.authority,
        egress.token_header
    );
    if from > 0 {
        requete.push_str(&format!("Range: bytes={from}-\r\n"));
    }
    requete.push_str("\r\n");
    flux.write_all(requete.as_bytes())
        .map_err(|e| PullError::Transport(format!("envoi au proxy : {e}")))?;
    Ok(BufReader::with_capacity(CHUNK, flux))
}

type Headers = Vec<(String, String)>;

/// Lit la ligne d'état et les en-têtes.
fn read_head(lecteur: &mut BufReader<UnixStream>) -> Result<(u16, Headers), PullError> {
    let mut lu = 0usize;
    let mut ligne = String::new();
    let mut lire = |ligne: &mut String| -> Result<(), PullError> {
        ligne.clear();
        let n = lecteur
            .read_line(ligne)
            .map_err(|e| PullError::Transport(format!("réponse du proxy : {e}")))?;
        lu += n;
        if n == 0 {
            return Err(PullError::Transport(
                "le proxy a fermé sans répondre".into(),
            ));
        }
        if lu > MAX_HEAD_BYTES {
            return Err(PullError::Transport("en-tête de réponse démesuré".into()));
        }
        Ok(())
    };
    lire(&mut ligne)?;
    let status = ligne
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|_| ligne.starts_with("HTTP/1."))
        .ok_or_else(|| PullError::Transport(format!("réponse illisible : {:?}", ligne.trim())))?;
    let mut entetes = Vec::new();
    loop {
        lire(&mut ligne)?;
        let brut = ligne.trim_end_matches(['\r', '\n']);
        if brut.is_empty() {
            break;
        }
        if let Some((nom, valeur)) = brut.split_once(':') {
            entetes.push((nom.trim().to_ascii_lowercase(), valeur.trim().to_owned()));
        }
    }
    Ok((status, entetes))
}

fn header<'a>(entetes: &'a Headers, nom: &str) -> Option<&'a str> {
    entetes
        .iter()
        .find(|(n, _)| n == nom)
        .map(|(_, v)| v.as_str())
}

/// `bytes <début>-<fin>/<total>` : le début, et le total s'il est donné.
fn content_range(valeur: &str) -> Option<(u64, Option<u64>)> {
    let reste = valeur.strip_prefix("bytes ")?;
    let (plage, total) = reste.split_once('/')?;
    let (debut, _) = plage.split_once('-')?;
    Some((debut.trim().parse().ok()?, total.trim().parse().ok()))
}

/// Le corps, selon son cadrage : longueur, découpage ou jusqu'à la fermeture.
fn body(lecteur: BufReader<UnixStream>, entetes: &Headers) -> Result<Box<dyn Read>, PullError> {
    let decoupe = header(entetes, "transfer-encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    if decoupe {
        return Ok(Box::new(Chunked {
            inner: lecteur,
            reste: 0,
            fini: false,
        }));
    }
    match header(entetes, "content-length") {
        Some(v) => {
            let n: u64 = v
                .parse()
                .map_err(|_| PullError::Transport(format!("Content-Length illisible : {v:?}")))?;
            Ok(Box::new(lecteur.take(n)))
        }
        None => Ok(Box::new(lecteur)),
    }
}

fn error_detail(lecteur: BufReader<UnixStream>, entetes: &Headers) -> String {
    let mut texte = String::new();
    if let Ok(corps) = body(lecteur, entetes) {
        let _ = corps.take(MAX_ERROR_BYTES).read_to_string(&mut texte);
    }
    // Le proxy dit son refus en JSON (`{"code", "detail"}`) ; un serveur, en texte.
    serde_json::from_str::<serde_json::Value>(&texte)
        .ok()
        .and_then(|v| {
            let code = v.get("code")?.as_str()?.to_owned();
            let detail = v.get("detail").and_then(|d| d.as_str()).unwrap_or_default();
            Some(format!("{code} : {detail}"))
        })
        .unwrap_or_else(|| {
            let court: String = texte.trim().chars().take(300).collect();
            if court.is_empty() {
                header(entetes, "x-prophet")
                    .unwrap_or("sans détail")
                    .to_owned()
            } else {
                court
            }
        })
}

/// Un corps découpé (`Transfer-Encoding: chunked`).
struct Chunked<R> {
    inner: R,
    reste: u64,
    fini: bool,
}

impl<R: std::io::BufRead> Read for Chunked<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.fini || buf.is_empty() {
            return Ok(0);
        }
        if self.reste == 0 {
            let mut ligne = String::new();
            if self.inner.read_line(&mut ligne)? == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "morceau attendu",
                ));
            }
            let taille = ligne.trim().split(';').next().unwrap_or_default();
            self.reste = u64::from_str_radix(taille, 16).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("taille de morceau illisible : {taille:?}"),
                )
            })?;
            if self.reste == 0 {
                self.fini = true;
                return Ok(0);
            }
        }
        let max = usize::try_from(self.reste)
            .unwrap_or(usize::MAX)
            .min(buf.len());
        let n = self.inner.read(&mut buf[..max])?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "morceau tronqué",
            ));
        }
        self.reste -= n as u64;
        if self.reste == 0 {
            let mut fin = String::new();
            self.inner.read_line(&mut fin)?;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::{Arc, Mutex};

    /// Un en-tête GGUF minimal valide : signature, version 3, aucun tenseur, une métadonnée.
    fn gguf(remplissage: usize) -> Vec<u8> {
        let mut v = b"GGUF".to_vec();
        v.extend(3u32.to_le_bytes());
        v.extend(0u64.to_le_bytes());
        v.extend(1u64.to_le_bytes());
        let cle = b"general.architecture";
        v.extend((cle.len() as u64).to_le_bytes());
        v.extend(cle);
        v.extend(8u32.to_le_bytes());
        v.extend(5u64.to_le_bytes());
        v.extend(b"qwen3");
        v.extend(std::iter::repeat_n(7u8, remplissage));
        v
    }

    fn sha(octets: &[u8]) -> String {
        hex(&Sha256::digest(octets))
    }

    fn entree(url: &str, sha256: String) -> Entry {
        Entry {
            id: "essai".into(),
            name: "Essai".into(),
            file: "essai.gguf".into(),
            url: url.into(),
            sha256,
            bytes: None,
            hosts: vec!["depot.example".into(), "*.cdn.example".into()],
            quantization: None,
            licence: None,
            note: None,
        }
    }

    /// Un faux proxy : une réponse par connexion, décidée d'après la requête reçue.
    struct FauxProxy {
        _dossier: tempfile::TempDir,
        socket: PathBuf,
        requetes: Arc<Mutex<Vec<String>>>,
    }

    impl FauxProxy {
        fn poser(repondre: impl Fn(&str) -> Vec<u8> + Send + 'static) -> Self {
            let dossier = tempfile::tempdir().unwrap();
            let socket = dossier.path().join("egress.sock");
            let ecoute = UnixListener::bind(&socket).unwrap();
            let requetes = Arc::new(Mutex::new(Vec::new()));
            let vues = requetes.clone();
            std::thread::spawn(move || {
                for flux in ecoute.incoming() {
                    let Ok(flux) = flux else { return };
                    let mut lecteur = BufReader::new(flux);
                    let mut requete = String::new();
                    loop {
                        let mut ligne = String::new();
                        if lecteur.read_line(&mut ligne).unwrap_or(0) == 0 || ligne == "\r\n" {
                            break;
                        }
                        requete.push_str(&ligne);
                    }
                    let reponse = repondre(&requete);
                    vues.lock().unwrap().push(requete);
                    let _ = lecteur.get_mut().write_all(&reponse);
                }
            });
            Self {
                _dossier: dossier,
                socket,
                requetes,
            }
        }

        fn egress(&self) -> Egress {
            Egress {
                socket: self.socket.clone(),
                token_header: "jeton".into(),
            }
        }
    }

    fn ok(corps: &[u8]) -> Vec<u8> {
        let mut r =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", corps.len()).into_bytes();
        r.extend(corps);
        r
    }

    fn tirer(
        entry: &Entry,
        dir: &Path,
        egress: &Egress,
    ) -> (Result<PathBuf, PullError>, Vec<Progress>) {
        let mut vus = Vec::new();
        let r = pull(entry, dir, egress, &AtomicBool::new(false), &mut |p| {
            vus.push(p)
        });
        (r, vus)
    }

    #[test]
    fn un_poids_suit_ses_redirections_par_le_proxy_et_n_est_pose_que_verifie() {
        let contenu = gguf(300_000);
        let servi = contenu.clone();
        let proxy = FauxProxy::poser(move |requete| {
            if requete.starts_with("GET https://depot.example/r/abc/essai.gguf ") {
                b"HTTP/1.1 302 Found\r\nLocation: https://x.cdn.example/blob?sig=1\r\nContent-Length: 0\r\n\r\n".to_vec()
            } else if requete.starts_with("GET https://x.cdn.example/blob?sig=1 ") {
                ok(&servi)
            } else {
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec()
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let entry = entree("https://depot.example/r/abc/essai.gguf", sha(&contenu));
        let (r, vus) = tirer(&entry, dir.path(), &proxy.egress());
        let pose = r.unwrap();
        assert_eq!(pose, dir.path().join("essai.gguf"));
        assert_eq!(std::fs::read(&pose).unwrap(), contenu);
        assert!(!partial_path(dir.path(), &entry).exists());
        assert_eq!(
            std::fs::metadata(&pose).unwrap().permissions().mode() & 0o777,
            0o644
        );
        let dernier = vus.last().unwrap();
        assert_eq!(dernier.received, contenu.len() as u64);
        assert_eq!(dernier.total, Some(contenu.len() as u64));
        let requetes = proxy.requetes.lock().unwrap();
        assert_eq!(requetes.len(), 2);
        // Le jeton accompagne chaque requête, redirection comprise, et l'hôte visé est dit.
        assert!(
            requetes
                .iter()
                .all(|r| r.contains("Proxy-Authorization: Prophet jeton\r\n"))
        );
        assert!(requetes[1].contains("Host: x.cdn.example\r\n"));
        drop(requetes);
        // Déjà là et conforme : rien n'est retéléchargé.
        let (r, _) = tirer(&entry, dir.path(), &proxy.egress());
        assert_eq!(r.unwrap(), pose);
        assert_eq!(proxy.requetes.lock().unwrap().len(), 2);
    }

    #[test]
    fn une_redirection_hors_des_hotes_permis_est_refusee() {
        let proxy = FauxProxy::poser(|_| {
            b"HTTP/1.1 302 Found\r\nLocation: https://ailleurs.example/vol\r\nContent-Length: 0\r\n\r\n".to_vec()
        });
        let dir = tempfile::tempdir().unwrap();
        let entry = entree("https://depot.example/essai.gguf", sha(b"x"));
        let (r, _) = tirer(&entry, dir.path(), &proxy.egress());
        let erreur = r.unwrap_err();
        assert!(erreur.to_string().contains("ailleurs.example"), "{erreur}");
        assert_eq!(
            proxy.requetes.lock().unwrap().len(),
            1,
            "rien n'est parti vers l'autre hôte"
        );
    }

    #[test]
    fn une_empreinte_fausse_efface_le_fichier_et_ne_pose_rien() {
        let contenu = gguf(1000);
        let servi = contenu.clone();
        let proxy = FauxProxy::poser(move |_| ok(&servi));
        let dir = tempfile::tempdir().unwrap();
        let entry = entree("https://depot.example/essai.gguf", sha(b"autre chose"));
        let (r, _) = tirer(&entry, dir.path(), &proxy.egress());
        assert!(
            matches!(r, Err(PullError::Integrity(ref m)) if m.contains("empreinte")),
            "{r:?}"
        );
        assert!(!dir.path().join("essai.gguf").exists());
        assert!(!partial_path(dir.path(), &entry).exists());
        // Un fichier de la bonne empreinte qui n'est pas un GGUF est refusé de même.
        let texte = b"ceci n'est pas un poids".to_vec();
        let servi = texte.clone();
        let proxy = FauxProxy::poser(move |_| ok(&servi));
        let entry = entree("https://depot.example/essai.gguf", sha(&texte));
        let (r, _) = tirer(&entry, dir.path(), &proxy.egress());
        assert!(
            matches!(r, Err(PullError::Integrity(ref m)) if m.contains("GGUF")),
            "{r:?}"
        );
        assert!(!dir.path().join("essai.gguf").exists());
    }

    #[test]
    fn une_connexion_coupee_garde_le_debut_et_la_suite_reprend_par_range() {
        let contenu = gguf(500_000);
        let moitie = contenu.len() / 2;
        let servi = contenu.clone();
        let appels = Arc::new(Mutex::new(0));
        let compte = appels.clone();
        let proxy = FauxProxy::poser(move |requete| {
            let mut n = compte.lock().unwrap();
            *n += 1;
            if *n == 1 {
                // Annonce tout, n'envoie que la moitié, et ferme.
                let mut r = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", servi.len())
                    .into_bytes();
                r.extend(&servi[..moitie]);
                r
            } else {
                assert!(
                    requete.contains(&format!("Range: bytes={moitie}-\r\n")),
                    "{requete}"
                );
                let reste = &servi[moitie..];
                let mut r = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {moitie}-{}/{}\r\nTransfer-Encoding: chunked\r\n\r\n",
                    servi.len() - 1,
                    servi.len()
                )
                .into_bytes();
                for morceau in reste.chunks(70_000) {
                    r.extend(format!("{:x}\r\n", morceau.len()).as_bytes());
                    r.extend(morceau);
                    r.extend(b"\r\n");
                }
                r.extend(b"0\r\n\r\n");
                r
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let entry = entree("https://depot.example/essai.gguf", sha(&contenu));
        let (r, _) = tirer(&entry, dir.path(), &proxy.egress());
        assert!(
            matches!(r, Err(PullError::Transport(ref m)) if m.contains("reprendre")),
            "{r:?}"
        );
        assert_eq!(
            std::fs::metadata(partial_path(dir.path(), &entry))
                .unwrap()
                .len(),
            moitie as u64
        );
        let (r, vus) = tirer(&entry, dir.path(), &proxy.egress());
        assert_eq!(std::fs::read(r.unwrap()).unwrap(), contenu);
        assert_eq!(
            vus[0].received, moitie as u64,
            "la reprise part de la moitié"
        );
    }

    #[test]
    fn un_refus_du_proxy_est_dit_avec_son_motif() {
        let proxy = FauxProxy::poser(|_| {
            let corps =
                r#"{"code":"PolicyDenied","detail":"net.egress refusé pour depot.example"}"#;
            format!("HTTP/1.1 403 Prophet\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{corps}", corps.len()).into_bytes()
        });
        let dir = tempfile::tempdir().unwrap();
        let entry = entree("https://depot.example/essai.gguf", sha(b"x"));
        let (r, _) = tirer(&entry, dir.path(), &proxy.egress());
        assert_eq!(
            r.unwrap_err(),
            PullError::Refused {
                status: 403,
                detail: "PolicyDenied : net.egress refusé pour depot.example".into()
            }
        );
        let absent = Egress {
            socket: dir.path().join("nulle-part.sock"),
            token_header: String::new(),
        };
        let (r, _) = tirer(&entry, dir.path(), &absent);
        assert!(r.unwrap_err().to_string().contains("injoignable"));
    }

    #[test]
    fn un_fichier_plus_long_que_le_catalogue_est_arrete_et_efface() {
        let contenu = gguf(10_000);
        let servi = contenu.clone();
        let proxy = FauxProxy::poser(move |_| ok(&servi));
        let dir = tempfile::tempdir().unwrap();
        let mut entry = entree("https://depot.example/essai.gguf", sha(&contenu));
        entry.bytes = Some(5_000);
        let (r, _) = tirer(&entry, dir.path(), &proxy.egress());
        assert!(matches!(r, Err(PullError::Integrity(_))), "{r:?}");
        assert!(!partial_path(dir.path(), &entry).exists());
    }

    #[test]
    fn un_telechargement_arrete_garde_son_debut() {
        let contenu = gguf(2_000_000);
        let servi = contenu.clone();
        let proxy = FauxProxy::poser(move |_| ok(&servi));
        let dir = tempfile::tempdir().unwrap();
        let entry = entree("https://depot.example/essai.gguf", sha(&contenu));
        let arret = AtomicBool::new(false);
        let r = pull(&entry, dir.path(), &proxy.egress(), &arret, &mut |p| {
            if p.received > 0 {
                arret.store(true, Ordering::Relaxed);
            }
        });
        assert_eq!(r.unwrap_err(), PullError::Cancelled);
        assert!(partial_path(dir.path(), &entry).exists());
        assert!(!dir.path().join("essai.gguf").exists());
    }
}
