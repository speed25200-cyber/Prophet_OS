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
        })
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
    fn une_duree_hors_bornes_ou_un_fichier_absent_sont_refuses() {
        let tools = Tools {
            whisper: PathBuf::from("/nonexistent/whisper-cli"),
            model: PathBuf::from("/nonexistent/model.bin"),
            recorder: None,
            timeout: None,
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
    }
}
