//! Le contrat entre sandboxd et l'invité des microVM (ADR 0038).
//!
//! L'hôte écrit le disque de travail : une image ext4 faite du répertoire de travail de la
//! tâche, avec `.prophet/exec.sh` qui porte le programme, ses arguments et son environnement.
//! L'invité la monte à l'endroit que la ligne de commande du noyau nomme, exécute le script, dit
//! tout sur la console série — « PROPHET_INVITE_PRET », la sortie du programme,
//! « PROPHET_INVITE_FIN code=N » — puis redémarre, et le moniteur sort. L'hôte lit la console et
//! rapatrie le disque dans le répertoire de travail. Aucun réseau, aucun partage de fichiers :
//! un disque qui part, un disque qui revient.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::caps::which;
use crate::spec::SandboxSpec;

/// L'invité a monté sa racine et lit la ligne de commande.
pub const MARQUE_PRET: &str = "PROPHET_INVITE_PRET";
/// Le programme a rendu la main ; suit son code de sortie.
pub const MARQUE_FIN: &str = "PROPHET_INVITE_FIN code=";
/// L'invité n'a pas pu remplir sa part du contrat ; suit la raison.
pub const MARQUE_ERREUR: &str = "PROPHET_INVITE_ERREUR";
/// Sous-répertoire du disque de travail réservé au contrat.
pub const DOSSIER: &str = ".prophet";
/// Le script que l'invité exécute, sous ce dossier.
pub const SCRIPT: &str = "exec.sh";
/// Ce que le disque de travail reçoit en plus du contenu du répertoire.
const MARGE_OCTETS: u64 = 64 * 1024 * 1024;
/// Au-delà, le répertoire de travail n'est pas un espace de travail : on refuse de l'imager.
const TAILLE_MAX: u64 = 2 * 1024 * 1024 * 1024;

fn citer(texte: &str) -> String {
    format!("'{}'", texte.replace('\'', "'\\''"))
}

/// Le script que l'invité exécute : le répertoire de travail, l'environnement, le programme et
/// ses arguments, cités pour le shell. Le programme est cherché dans l'invité par son nom
/// (`python3` de l'hôte est `/bin/python3` là-bas) ; un chemin qui n'y répond à rien est pris
/// tel quel, relatif au répertoire de travail. `PATH` reste celui de l'invité.
#[must_use]
pub fn script_exec(spec: &SandboxSpec) -> String {
    let mut script = String::from("#!/bin/sh\n");
    script.push_str(&format!("cd {} || exit 126\n", citer(&spec.workdir)));
    script.push_str("export PATH=/bin\n");
    for (cle, valeur) in &spec.env {
        let nom_valide = !cle.is_empty()
            && cle.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && !cle.as_bytes()[0].is_ascii_digit();
        if nom_valide && cle != "PATH" {
            script.push_str(&format!("export {cle}={}\n", citer(valeur)));
        }
    }
    let nom = Path::new(&spec.program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&spec.program);
    script.push_str(&format!(
        "programme=$(command -v {} 2>/dev/null || echo {})\n",
        citer(nom),
        citer(&spec.program)
    ));
    script.push_str("exec \"$programme\"");
    for argument in &spec.args {
        script.push(' ');
        script.push_str(&citer(argument));
    }
    script.push('\n');
    script
}

/// Ce que la console de l'invité a dit.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Console {
    /// La sortie du programme, entre « prêt » et « fin », sans les lignes du noyau.
    pub sortie: String,
    /// Le code de sortie du programme, si la fin a été vue.
    pub code: Option<i32>,
    /// L'invité a dit « fin » : le programme a rendu la main.
    pub fin_vue: bool,
    /// L'invité n'a pas pu remplir le contrat (espace de travail non monté…).
    pub erreur: Option<String>,
}

/// Lit la console de l'invité : la sortie du programme entre les deux marques, le code, et ce
/// qui a manqué. Les lignes du noyau, horodatées entre crochets, n'appartiennent pas au
/// programme et sont écartées.
#[must_use]
pub fn lire_console(texte: &str) -> Console {
    let mut console = Console::default();
    let mut dedans = false;
    for ligne in texte.lines() {
        let ligne = ligne.trim_end_matches('\r');
        if ligne == MARQUE_PRET {
            dedans = true;
            continue;
        }
        if let Some(reste) = ligne.strip_prefix(MARQUE_FIN) {
            console.code = reste.trim().parse().ok();
            console.fin_vue = true;
            break;
        }
        if let Some(reste) = ligne.strip_prefix(MARQUE_ERREUR) {
            console.erreur = Some(reste.trim().to_owned());
            continue;
        }
        let du_noyau = ligne
            .strip_prefix('[')
            .is_some_and(|r| r.trim_start().starts_with(|c: char| c.is_ascii_digit()));
        if dedans && !du_noyau {
            console.sortie.push_str(ligne);
            console.sortie.push('\n');
        }
    }
    console
}

fn commande(programme: &str, arguments: &[&str]) -> Result<(), String> {
    let sortie = Command::new(programme)
        .args(arguments)
        .output()
        .map_err(|e| format!("{programme} : {e}"))?;
    if sortie.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{programme} {} : {}",
            arguments.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&sortie.stderr).trim()
        ))
    }
}

/// Les octets des fichiers d'un répertoire, et le nombre de ses entrées (fichiers, dossiers,
/// liens) : le disque doit avoir la place des uns et un inode pour chacune des autres.
fn taille_du(dossier: &Path) -> (u64, u64) {
    let mut total = 0;
    let mut entrees_vues = 0;
    let mut pile = vec![dossier.to_path_buf()];
    while let Some(courant) = pile.pop() {
        let Ok(entrees) = std::fs::read_dir(&courant) else {
            continue;
        };
        for entree in entrees.flatten() {
            let Ok(meta) = entree.metadata() else {
                continue;
            };
            entrees_vues += 1;
            if meta.is_dir() {
                pile.push(entree.path());
            } else {
                total += meta.len();
            }
        }
    }
    (total, entrees_vues)
}

/// Construit le disque de travail : une image ext4 du répertoire de travail, puis le script sous
/// `.prophet/exec.sh`. Tout se fait sans privilège, par `mkfs.ext4 -d` et `debugfs`.
///
/// # Errors
/// e2fsprogs absent, ou l'image impossible à écrire.
pub fn disque_de_travail(workdir: &Path, script: &str, destination: &Path) -> Result<(), String> {
    // Le répertoire de travail est celui d'une tâche, jamais la racine ni un dossier de la
    // machine : imager `/` serait aussi lent que faux.
    let canonique = workdir
        .canonicalize()
        .map_err(|e| format!("répertoire de travail {} : {e}", workdir.display()))?;
    if canonique == Path::new("/") || !canonique.is_dir() {
        return Err(format!(
            "répertoire de travail {} : la racine ou un non-répertoire ne s'image pas",
            workdir.display()
        ));
    }
    let mkfs = which("mkfs.ext4").ok_or("il manque mkfs.ext4 (e2fsprogs)")?;
    let debugfs = which("debugfs").ok_or("il manque debugfs (e2fsprogs)")?;
    let (contenu, entrees) = taille_du(workdir);
    if contenu > TAILLE_MAX {
        return Err(format!(
            "répertoire de travail de {} Mio : plus de {} Mio, ce n'est pas un espace de travail",
            contenu / (1024 * 1024),
            TAILLE_MAX / (1024 * 1024)
        ));
    }
    // Deux fois les octets, un bloc par entrée, et une marge ; et autant d'inodes qu'il faut :
    // le ratio par défaut de mkfs (un inode par 16 Kio) en manque pour un répertoire de
    // milliers de petits fichiers — « Could not allocate » à l'image, vu en CI sur /tmp.
    let octets = contenu.saturating_mul(2) + entrees.saturating_mul(4096) + MARGE_OCTETS;
    let mio = octets.div_ceil(1024 * 1024);
    let inodes = (entrees.saturating_mul(2) + 256).to_string();
    let fichier = std::fs::File::create(destination).map_err(|e| e.to_string())?;
    fichier
        .set_len(mio * 1024 * 1024)
        .map_err(|e| e.to_string())?;
    drop(fichier);
    let dest = destination.display().to_string();
    let src = workdir.display().to_string();
    commande(
        &mkfs,
        &[
            "-q", "-F", "-N", &inodes, "-d", &src, "-L", "travail", &dest,
        ],
    )?;
    let script_hote = destination.with_extension("exec.sh");
    std::fs::write(&script_hote, script).map_err(|e| e.to_string())?;
    let _ = commande(&debugfs, &["-w", &dest, "-R", &format!("mkdir {DOSSIER}")]);
    commande(
        &debugfs,
        &[
            "-w",
            &dest,
            "-R",
            &format!("write {} {DOSSIER}/{SCRIPT}", script_hote.display()),
        ],
    )?;
    let _ = std::fs::remove_file(&script_hote);
    Ok(())
}

fn copier_arbre(source: &Path, cible: &Path) -> std::io::Result<()> {
    for entree in std::fs::read_dir(source)? {
        let entree = entree?;
        let nom = entree.file_name();
        if nom == DOSSIER || nom == "lost+found" {
            continue;
        }
        let vers = cible.join(&nom);
        let meta = entree.metadata()?;
        if meta.is_dir() {
            std::fs::create_dir_all(&vers)?;
            copier_arbre(&entree.path(), &vers)?;
        } else if meta.file_type().is_symlink() {
            let lien = std::fs::read_link(entree.path())?;
            let _ = std::fs::remove_file(&vers);
            std::os::unix::fs::symlink(lien, &vers)?;
        } else {
            std::fs::copy(entree.path(), &vers)?;
        }
    }
    Ok(())
}

/// Rapatrie le disque de travail dans le répertoire de travail : ce que le programme a écrit ou
/// modifié revient, ce qu'il a effacé reste (le répertoire de travail est un espace annulable
/// par ailleurs). `.prophet` et `lost+found` n'en font pas partie.
///
/// # Errors
/// e2fsprogs absent, ou l'image illisible.
pub fn rapatrier(disque: &Path, workdir: &Path) -> Result<(), String> {
    let debugfs = which("debugfs").ok_or("il manque debugfs (e2fsprogs)")?;
    let extraction: PathBuf = std::env::temp_dir().join(format!(
        "prophet-rapatriement-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    std::fs::create_dir_all(&extraction).map_err(|e| e.to_string())?;
    let resultat = commande(
        &debugfs,
        &[
            &disque.display().to_string(),
            "-R",
            &format!("rdump / {}", extraction.display()),
        ],
    )
    .and_then(|()| copier_arbre(&extraction, workdir).map_err(|e| e.to_string()));
    let _ = std::fs::remove_dir_all(&extraction);
    resultat
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_script_cite_tout_et_cherche_le_programme_par_son_nom() {
        let spec = SandboxSpec::new(2, "/usr/bin/python3", "/tmp/travail d'essai")
            .args(["-c", "print('bonjour')"])
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "fr_FR.UTF-8")
            .env("1invalide", "x");
        let script = script_exec(&spec);
        assert!(script.starts_with("#!/bin/sh\ncd '/tmp/travail d'\\''essai' || exit 126\n"));
        assert!(script.contains("export PATH=/bin\n"));
        assert!(script.contains("export LANG='fr_FR.UTF-8'\n"));
        assert!(!script.contains("1invalide"));
        assert!(
            !script.contains("/usr/bin:/bin"),
            "le PATH de l'hôte ne passe pas"
        );
        assert!(script.contains(
            "programme=$(command -v 'python3' 2>/dev/null || echo '/usr/bin/python3')\n"
        ));
        assert!(script.ends_with("exec \"$programme\" '-c' 'print('\\''bonjour'\\'')'\n"));
    }

    #[test]
    fn la_racine_ne_s_image_pas() {
        let dest = std::env::temp_dir().join(format!("prophet-refus-{}.ext4", std::process::id()));
        let erreur = disque_de_travail(
            Path::new("/"),
            "#!/bin/sh
",
            &dest,
        )
        .unwrap_err();
        assert!(erreur.contains("la racine"), "{erreur}");
        let absent = disque_de_travail(
            Path::new("/nulle/part/ici"),
            "#!/bin/sh
",
            &dest,
        )
        .unwrap_err();
        assert!(absent.contains("répertoire de travail"), "{absent}");
        let _ = std::fs::remove_file(&dest);
    }

    #[test]
    fn la_console_rend_la_sortie_entre_les_marques_et_le_code() {
        let texte = "[    0.8] Run /sbin/init as init process\nPROPHET_INVITE_PRET\n[    0.9] EXT4-fs (vdb): mounted filesystem\nbonjour\ndeuxième ligne\nPROPHET_INVITE_FIN code=3\n[    1.0] reboot: Restarting system\n";
        let console = lire_console(texte);
        assert_eq!(console.sortie, "bonjour\ndeuxième ligne\n");
        assert_eq!(console.code, Some(3));
        assert!(console.fin_vue);
        assert_eq!(console.erreur, None);
        // Sans fin : rien n'est conclu, ce qui a été dit est gardé.
        let coupe = lire_console("PROPHET_INVITE_PRET\nen cours\n");
        assert_eq!(coupe.sortie, "en cours\n");
        assert!(!coupe.fin_vue);
        assert_eq!(coupe.code, None);
        // Une erreur de l'invité est rapportée, et la fin la suit.
        let rate = lire_console(
            "PROPHET_INVITE_PRET\nPROPHET_INVITE_ERREUR espace de travail non monté\nPROPHET_INVITE_FIN code=125\n",
        );
        assert_eq!(rate.erreur.as_deref(), Some("espace de travail non monté"));
        assert_eq!(rate.code, Some(125));
        assert_eq!(rate.sortie, "");
        // Avant « prêt », rien n'appartient au programme.
        assert_eq!(lire_console("bruit\n").sortie, "");
    }
}
