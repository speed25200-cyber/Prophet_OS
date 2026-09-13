//! `voice` : l'humain parle à Prophet OS (ADR 0036).
//!
//! Deux gestes, rien de plus : **enregistrer** quelques secondes du micro de la session
//! (PipeWire, sinon ALSA) et **transcrire** un fichier audio en texte par whisper.cpp, en local,
//! sans réseau. Le texte est rendu tel quel ; c'est l'humain, ou la commande qui l'a demandé,
//! qui en fait une intention de mission. Aucun son ne quitte la machine, aucun modèle distant
//! n'est appelé, et rien ici ne donne de droit.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// Erreurs de la parole.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Outil ou modèle absent.
    #[error("{0}")]
    Missing(String),
    /// Un programme n'a pas pu être lancé ou a échoué.
    #[error("{program} : {detail}")]
    Program {
        /// Programme en cause.
        program: String,
        /// Ce qui s'est passé.
        detail: String,
    },
    /// La sortie de whisper.cpp n'a pas la forme attendue.
    #[error("transcription illisible : {0}")]
    Unreadable(String),
    /// Requête invalide.
    #[error("{0}")]
    Invalid(String),
}

/// Ce que la transcription a rendu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transcript {
    /// Le texte, espaces normalisés.
    pub text: String,
    /// Langue détectée ou imposée, si whisper la dit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Segments rendus par whisper.
    pub segments: usize,
    /// Durée de la transcription.
    pub duration_ms: u64,
}

/// L'enregistreur de la session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recorder {
    /// `pw-record` de PipeWire, arrêté proprement par `timeout -s INT`.
    PipeWire(PathBuf),
    /// `arecord` d'ALSA, avec sa durée.
    Alsa(PathBuf),
}

/// Les programmes et le modèle de la parole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    /// `whisper-cli` de whisper.cpp.
    pub whisper: PathBuf,
    /// Modèle ggml de whisper (`ggml-base.bin`…).
    pub model: PathBuf,
    /// Enregistreur, s'il y en a un sur cette machine.
    pub recorder: Option<Recorder>,
    /// `timeout` de coreutils, pour arrêter `pw-record` sur un signal propre.
    pub timeout: Option<PathBuf>,
    /// `piper` de Piper, la synthèse vocale locale, s'il est configuré.
    pub piper: Option<PathBuf>,
    /// Voix de Piper (`*.onnx`, son `.onnx.json` à côté), si configurée.
    pub speaker: Option<PathBuf>,
    /// Lecteur audio de la session (`pw-play`, sinon `aplay`), s'il y en a un.
    pub player: Option<PathBuf>,
}

/// Ce que la synthèse a produit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Speech {
    /// Fichier WAV écrit.
    pub wav: PathBuf,
    /// Octets du fichier.
    pub bytes: u64,
    /// Durée de la synthèse.
    pub duration_ms: u64,
}

impl Tools {
    /// Lit la configuration de la session : `PROPHET_WHISPER` (sinon `whisper-cli` sur le
    /// chemin), `PROPHET_WHISPER_MODEL` (requis), `PROPHET_RECORDER` (sinon `pw-record` puis
    /// `arecord` sur le chemin).
    ///
    /// # Errors
    /// Modèle absent ou whisper introuvable.
    pub fn from_env() -> Result<Self, Error> {
        let model = std::env::var_os("PROPHET_WHISPER_MODEL")
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .ok_or_else(|| {
                Error::Missing(
                    "aucun modèle de parole : PROPHET_WHISPER_MODEL doit nommer un modèle ggml de whisper.cpp".into(),
                )
            })?;
        let whisper = std::env::var_os("PROPHET_WHISPER")
            .map(PathBuf::from)
            .or_else(|| which("whisper-cli"))
            .ok_or_else(|| Error::Missing("whisper-cli introuvable sur cette machine".into()))?;
        let recorder = match std::env::var_os("PROPHET_RECORDER").map(PathBuf::from) {
            Some(p) if p.file_name().is_some_and(|n| n == "arecord") => Some(Recorder::Alsa(p)),
            Some(p) => Some(Recorder::PipeWire(p)),
            None => which("pw-record")
                .map(Recorder::PipeWire)
                .or_else(|| which("arecord").map(Recorder::Alsa)),
        };
        Ok(Self {
            whisper,
            model,
            recorder,
            timeout: which("timeout"),
            piper: std::env::var_os("PROPHET_PIPER")
                .map(PathBuf::from)
                .or_else(|| which("piper")),
            speaker: std::env::var_os("PROPHET_PIPER_VOICE")
                .map(PathBuf::from)
                .filter(|p| p.is_file()),
            player: std::env::var_os("PROPHET_PLAYER")
                .map(PathBuf::from)
                .or_else(|| which("pw-play"))
                .or_else(|| which("aplay")),
        })
    }

    /// L'OS peut parler : Piper et une voix sont configurés.
    #[must_use]
    pub fn can_speak(&self) -> bool {
        self.piper.is_some() && self.speaker.is_some()
    }

    /// Synthétise `text` dans `out` (WAV) par Piper, en local.
    ///
    /// # Errors
    /// Piper ou voix absents, texte vide, programme en échec.
    pub fn speak(&self, text: &str, out: &Path) -> Result<Speech, Error> {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > 4000 {
            return Err(Error::Invalid(
                "texte à dire : entre 1 et 4 000 caractères".into(),
            ));
        }
        let piper = self
            .piper
            .as_ref()
            .ok_or_else(|| Error::Missing("piper introuvable : l'OS n'a pas de voix".into()))?;
        let speaker = self.speaker.as_ref().ok_or_else(|| {
            Error::Missing(
                "aucune voix : PROPHET_PIPER_VOICE doit nommer un modèle .onnx de Piper".into(),
            )
        })?;
        let started = Instant::now();
        let mut child = Command::new(piper)
            .arg("--model")
            .arg(speaker)
            .arg("--output_file")
            .arg(out)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Program {
                program: "piper".into(),
                detail: e.to_string(),
            })?;
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write as _;
            let _ = stdin.write_all(text.as_bytes());
            let _ = stdin.write_all(b"\n");
        }
        let status = child.wait().map_err(|e| Error::Program {
            program: "piper".into(),
            detail: e.to_string(),
        })?;
        let bytes = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
        if !status.success() || bytes == 0 {
            return Err(Error::Program {
                program: "piper".into(),
                detail: format!("terminé avec {status}, aucun son produit"),
            });
        }
        Ok(Speech {
            wav: out.to_path_buf(),
            bytes,
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }

    /// Joue un WAV sur la sortie audio de la session.
    ///
    /// # Errors
    /// Aucun lecteur, fichier absent, lecteur en échec.
    pub fn play(&self, wav: &Path) -> Result<(), Error> {
        if !wav.is_file() {
            return Err(Error::Invalid(format!(
                "fichier audio introuvable : {}",
                wav.display()
            )));
        }
        let player = self.player.as_ref().ok_or_else(|| {
            Error::Missing("aucun lecteur audio : ni pw-play ni aplay sur cette machine".into())
        })?;
        let mut command = Command::new(player);
        if player.file_name().is_some_and(|n| n == "aplay") {
            command.arg("-q");
        }
        let status = command
            .arg(wav)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| Error::Program {
                program: "lecteur audio".into(),
                detail: e.to_string(),
            })?;
        if !status.success() {
            return Err(Error::Program {
                program: "lecteur audio".into(),
                detail: format!("terminé avec {status}"),
            });
        }
        Ok(())
    }

    /// Enregistre `seconds` secondes du micro dans `out` (WAV 16 kHz mono).
    ///
    /// # Errors
    /// Aucun enregistreur, durée nulle, ou programme en échec.
    pub fn record(&self, seconds: u32, out: &Path) -> Result<(), Error> {
        if seconds == 0 || seconds > 120 {
            return Err(Error::Invalid(
                "durée d'enregistrement entre 1 et 120 s".into(),
            ));
        }
        let recorder = self.recorder.as_ref().ok_or_else(|| {
            Error::Missing("aucun enregistreur : ni pw-record ni arecord sur cette machine".into())
        })?;
        let status = match recorder {
            Recorder::PipeWire(program) => {
                let timeout = self.timeout.as_ref().ok_or_else(|| {
                    Error::Missing("timeout (coreutils) introuvable pour arrêter pw-record".into())
                })?;
                Command::new(timeout)
                    .args(["-s", "INT", &format!("{seconds}s")])
                    .arg(program)
                    .args(["--rate", "16000", "--channels", "1", "--format", "s16"])
                    .arg(out)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .status()
            }
            Recorder::Alsa(program) => Command::new(program)
                .args(["-q", "-f", "S16_LE", "-r", "16000", "-c", "1", "-d"])
                .arg(seconds.to_string())
                .arg(out)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .status(),
        }
        .map_err(|e| Error::Program {
            program: "enregistreur".into(),
            detail: e.to_string(),
        })?;
        // `timeout -s INT` rend 124 quand il a dû interrompre : c'est le cas normal ici.
        if !(status.success() || status.code() == Some(124)) || !out.is_file() {
            return Err(Error::Program {
                program: "enregistreur".into(),
                detail: format!("terminé avec {status}, aucun enregistrement"),
            });
        }
        Ok(())
    }

    /// Transcrit `audio` (WAV, MP3, FLAC… ce que whisper.cpp décode) ; `language` est un code
    /// à deux lettres, sinon détection automatique.
    ///
    /// # Errors
    /// Fichier absent, whisper en échec, ou sortie illisible.
    pub fn transcribe(&self, audio: &Path, language: Option<&str>) -> Result<Transcript, Error> {
        if !audio.is_file() {
            return Err(Error::Invalid(format!(
                "fichier audio introuvable : {}",
                audio.display()
            )));
        }
        let language = language.unwrap_or("auto");
        if !language.chars().all(|c| c.is_ascii_lowercase()) || language.len() > 4 {
            return Err(Error::Invalid(
                "langue : code à deux lettres attendu".into(),
            ));
        }
        let scratch = std::env::temp_dir().join(format!(
            "prophet-voix-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        let started = Instant::now();
        let output = Command::new(&self.whisper)
            .arg("-m")
            .arg(&self.model)
            .arg("-f")
            .arg(audio)
            .args(["-l", language, "-np", "-oj", "-of"])
            .arg(&scratch)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| Error::Program {
                program: "whisper-cli".into(),
                detail: e.to_string(),
            })?;
        let json_path = scratch.with_extension("json");
        let json = std::fs::read_to_string(&json_path);
        let _ = std::fs::remove_file(&json_path);
        if !output.status.success() {
            return Err(Error::Program {
                program: "whisper-cli".into(),
                detail: String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .last()
                    .unwrap_or("échec")
                    .chars()
                    .take(200)
                    .collect(),
            });
        }
        let json = json.map_err(|e| Error::Unreadable(e.to_string()))?;
        let mut transcript = parse_whisper_json(&json)?;
        transcript.duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok(transcript)
    }
}

/// Ce qui suit le mot d'activation dans une phrase transcrite, si elle commence par lui.
///
/// La comparaison ignore la casse, les accents et la ponctuation : Whisper rend « Prophète, »
/// ou « prophet » selon l'humeur de la voix. `wake` peut être un mot ou plusieurs.
#[must_use]
pub fn after_wake_word(text: &str, wake: &str) -> Option<String> {
    let normalize = |s: &str| -> Vec<String> {
        s.chars()
            .map(|c| match c {
                'à' | 'â' | 'ä' => 'a',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'î' | 'ï' => 'i',
                'ô' | 'ö' => 'o',
                'ù' | 'û' | 'ü' => 'u',
                'ç' => 'c',
                c if c.is_alphanumeric() => c.to_ascii_lowercase(),
                _ => ' ',
            })
            .collect::<String>()
            .split_whitespace()
            .map(str::to_owned)
            .collect()
    };
    let wake_words = normalize(wake);
    if wake_words.is_empty() {
        return None;
    }
    let words = normalize(text);
    if words.len() < wake_words.len()
        || !words
            .iter()
            .zip(&wake_words)
            .all(|(heard, wanted)| close_enough(heard, wanted))
    {
        return None;
    }
    // Le reste de la phrase, tel que dit : on retrouve la coupure dans le texte d'origine en
    // comptant les mots, pour garder accents et ponctuation de l'intention.
    let mut seen = 0;
    let mut cut = 0;
    let mut in_word = false;
    for (i, c) in text.char_indices() {
        let is_word = c.is_alphanumeric();
        if is_word && !in_word {
            if seen == wake_words.len() {
                cut = i;
                break;
            }
            seen += 1;
        }
        in_word = is_word;
        cut = i + c.len_utf8();
    }
    let rest = text[cut..]
        .trim()
        .trim_start_matches([',', ':', ';', '.', '!', '?'])
        .trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_owned())
    }
}

/// Deux mots normalisés se valent si, une fois « ph » ramené à « f » et les lettres doublées
/// réduites, ils sont égaux, ou ne diffèrent que par une lettre substituée, ou par une lettre
/// de plus à la fin. Whisper entend « Profète », « Profette » ou « prophet » pour « Prophète » ;
/// il n'entend pas « prophétie », qui reste distinct.
fn close_enough(heard: &str, wanted: &str) -> bool {
    let sound = |w: &str| -> Vec<char> {
        let mut out: Vec<char> = Vec::new();
        let mut chars = w.chars().peekable();
        while let Some(c) = chars.next() {
            let c = if c == 'p' && chars.peek() == Some(&'h') {
                chars.next();
                'f'
            } else {
                c
            };
            if out.last() != Some(&c) {
                out.push(c);
            }
        }
        out
    };
    let a = sound(heard);
    let b = sound(wanted);
    if a == b {
        return true;
    }
    if b.len() < 4 {
        return false;
    }
    if a.len() == b.len() {
        return a.iter().zip(&b).filter(|(x, y)| x != y).count() <= 1;
    }
    let (short, long) = if a.len() < b.len() { (&a, &b) } else { (&b, &a) };
    long.len() == short.len() + 1 && long[..short.len()] == short[..]
}

/// Lit la sortie JSON de whisper.cpp (`-oj`) : le texte des segments, la langue.
///
/// # Errors
/// JSON illisible ou sans segment.
pub fn parse_whisper_json(json: &str) -> Result<Transcript, Error> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| Error::Unreadable(e.to_string()))?;
    let segments = value["transcription"]
        .as_array()
        .ok_or_else(|| Error::Unreadable("aucun segment".into()))?;
    let text = segments
        .iter()
        .filter_map(|s| s["text"].as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    Ok(Transcript {
        text,
        language: value["result"]["language"]
            .as_str()
            .map(str::to_owned)
            .filter(|l| !l.is_empty()),
        segments: segments.len(),
        duration_ms: 0,
    })
}

/// Cherche un programme sur le `PATH` de la session.
#[must_use]
pub fn which(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|d| d.join(program))
            .find(|p| p.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_sortie_de_whisper_devient_un_texte_propre() {
        let json = r#"{"result":{"language":"fr"},"transcription":[
            {"timestamps":{"from":"00:00:00,000","to":"00:00:02,000"},"text":" Écris une note "},
            {"timestamps":{"from":"00:00:02,000","to":"00:00:04,000"},"text":"  dans mes documents."}]}"#;
        let t = parse_whisper_json(json).unwrap();
        assert_eq!(t.text, "Écris une note dans mes documents.");
        assert_eq!(t.language.as_deref(), Some("fr"));
        assert_eq!(t.segments, 2);
        assert!(parse_whisper_json("{}").is_err());
        assert!(parse_whisper_json("pas du json").is_err());
    }

    #[test]
    fn le_mot_d_activation_se_reconnait_malgre_casse_accents_et_ponctuation() {
        assert_eq!(
            after_wake_word("Prophète, écris une note dans mes documents.", "prophète").as_deref(),
            Some("écris une note dans mes documents.")
        );
        assert_eq!(
            after_wake_word(" prophet : Résume le rapport", "Prophète").as_deref(),
            Some("Résume le rapport")
        );
        assert_eq!(
            after_wake_word("Dis Prophète, résume", "dis prophète").as_deref(),
            Some("résume")
        );
        // Sans le mot en tête, ou sans rien après, rien n'est déclenché.
        assert_eq!(after_wake_word("Il fait beau, Prophète.", "prophète"), None);
        assert_eq!(after_wake_word("Prophète.", "prophète"), None);
        assert_eq!(after_wake_word("", "prophète"), None);
        assert_eq!(after_wake_word("Prophète, vas-y", ""), None);
        // « prophétie » n'est pas « prophète ».
        assert_eq!(after_wake_word("Prophétie du jour", "prophète"), None);
        // Ce que Whisper entend réellement d'une voix de synthèse : « Profète », « Profette ».
        assert_eq!(
            after_wake_word("Profète, écrite une note de réunion.", "prophète").as_deref(),
            Some("écrite une note de réunion.")
        );
        assert_eq!(
            after_wake_word("Profette, résume le rapport", "prophète").as_deref(),
            Some("résume le rapport")
        );
        assert_eq!(after_wake_word("Profond, résume", "prophète"), None);
    }

    #[test]
    fn une_duree_hors_bornes_ou_un_fichier_absent_sont_refuses() {
        let tools = Tools {
            whisper: PathBuf::from("/nonexistent/whisper-cli"),
            model: PathBuf::from("/nonexistent/model.bin"),
            recorder: None,
            timeout: None,
            piper: None,
            speaker: None,
            player: None,
        };
        assert!(matches!(
            tools.record(0, Path::new("/tmp/x.wav")),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            tools.record(5, Path::new("/tmp/x.wav")),
            Err(Error::Missing(_))
        ));
        assert!(matches!(
            tools.transcribe(Path::new("/nonexistent/a.wav"), None),
            Err(Error::Invalid(_))
        ));
        // Sans Piper ni voix, l'OS ne parle pas et le dit ; un texte vide est refusé avant.
        assert!(!tools.can_speak());
        assert!(matches!(
            tools.speak("   ", Path::new("/tmp/x.wav")),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            tools.speak("Bonjour", Path::new("/tmp/x.wav")),
            Err(Error::Missing(_))
        ));
        assert!(matches!(
            tools.play(Path::new("/nonexistent/a.wav")),
            Err(Error::Invalid(_))
        ));
    }
}
