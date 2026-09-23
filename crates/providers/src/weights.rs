//! Le catalogue des poids installés : ce que chaque fichier GGUF dit de lui-même.
//!
//! Le moteur local sert un modèle par son alias ; il ne dit ni son architecture, ni sa
//! quantification, ni la fenêtre de contexte pour laquelle il a été entraîné. Le fichier, lui,
//! le dit dans son en-tête. On le lit sans charger les poids : quelques kilo-octets de
//! métadonnées au début d'un fichier de plusieurs gigaoctets, les tableaux du tokeniseur sautés
//! sans être gardés.
//!
//! Un en-tête est une donnée non fiable : un fichier corrompu ou fabriqué ne doit ni faire
//! allouer des gigaoctets, ni boucler. Chaque compte et chaque longueur est borné avant usage, et
//! un dépassement rend une erreur qui nomme le fichier plutôt qu'un catalogue faux.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Dossier des poids sur l'image installée (ADR 0033), sauf `PROPHET_MODELS_DIR`.
pub const DEFAULT_DIR: &str = "/var/lib/prophet/models";

/// Paires de métadonnées lues au plus dans un en-tête.
const MAX_KV: u64 = 65_536;
/// Longueur maximale d'une clé ou d'une chaîne gardée.
const MAX_STRING: u64 = 1 << 20;
/// Éléments d'un tableau au plus (le vocabulaire d'un tokeniseur en compte quelques centaines de
/// milliers).
const MAX_ARRAY: u64 = 1 << 24;

/// Ce qu'un fichier de poids dit de lui-même.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Weights {
    /// Chemin du fichier.
    pub path: PathBuf,
    /// Taille du fichier en octets.
    pub bytes: u64,
    /// Version du format GGUF.
    pub version: u32,
    /// Architecture (`qwen3`, `llama`, `gemma3`…).
    pub architecture: Option<String>,
    /// Nom que le fichier se donne.
    pub name: Option<String>,
    /// Taille annoncée (`1.7B`, `8B`…).
    pub size_label: Option<String>,
    /// Quantification des poids (`Q8_0`, `Q4_K_M`…).
    pub quantization: Option<String>,
    /// Fenêtre de contexte d'entraînement, en tokens.
    pub context_length: Option<u64>,
    /// Nombre de couches.
    pub layers: Option<u64>,
    /// Nombre de tenseurs annoncé.
    pub tensors: u64,
}

impl Weights {
    /// Taille en gigaoctets, arrondie au dixième, pour l'humain.
    #[must_use]
    pub fn gigabytes(&self) -> f64 {
        (self.bytes as f64 / 1e8).round() / 10.0
    }
}

/// Une valeur de métadonnée gardée : les scalaires et les chaînes, jamais les tableaux.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Unsigned(u64),
    Signed(i64),
    Text(String),
    Other,
}

/// Lit l'en-tête d'un fichier GGUF.
///
/// # Errors
/// Fichier illisible, signature absente, version inconnue, en-tête tronqué ou démesuré.
pub fn read(path: &Path) -> Result<Weights, String> {
    let fail = |e: String| format!("{} : {e}", path.display());
    let file = File::open(path).map_err(|e| fail(e.to_string()))?;
    let bytes = file.metadata().map_err(|e| fail(e.to_string()))?.len();
    let mut reader = BufReader::new(file);
    let mut magic = [0u8; 4];
    reader
        .read_exact(&mut magic)
        .map_err(|_| fail("fichier trop court pour être un GGUF".into()))?;
    if &magic != b"GGUF" {
        return Err(fail("signature GGUF absente".into()));
    }
    let version = u32_le(&mut reader).map_err(fail)?;
    if !(2..=3).contains(&version) {
        return Err(fail(format!("version GGUF {version} non prise en charge")));
    }
    let tensors = u64_le(&mut reader).map_err(fail)?;
    let count = u64_le(&mut reader).map_err(fail)?;
    if count > MAX_KV {
        return Err(fail(format!(
            "{count} métadonnées annoncées, au-delà de {MAX_KV}"
        )));
    }
    let mut keys = Vec::new();
    for _ in 0..count {
        let key = string(&mut reader).map_err(fail)?;
        let kind = u32_le(&mut reader).map_err(fail)?;
        let value = value(&mut reader, kind, bytes).map_err(fail)?;
        keys.push((key, value));
    }
    let text = |k: &str| {
        keys.iter().find_map(|(key, v)| match v {
            Value::Text(t) if key == k => Some(t.clone()),
            _ => None,
        })
    };
    let number = |k: &str| {
        keys.iter().find_map(|(key, v)| match v {
            Value::Unsigned(n) if key == k => Some(*n),
            Value::Signed(n) if key == k => u64::try_from(*n).ok(),
            _ => None,
        })
    };
    let architecture = text("general.architecture");
    let per_arch = |suffix: &str| {
        architecture
            .as_deref()
            .and_then(|a| number(&format!("{a}.{suffix}")))
    };
    Ok(Weights {
        path: path.to_owned(),
        bytes,
        version,
        name: text("general.name"),
        size_label: text("general.size_label"),
        quantization: number("general.file_type").map(file_type),
        context_length: per_arch("context_length"),
        layers: per_arch("block_count"),
        architecture,
        tensors,
    })
}

/// Le catalogue d'un dossier : chaque `*.gguf`, dans l'ordre des noms, lu ou refusé avec sa
/// raison. Un dossier absent est un catalogue vide, pas une erreur : la machine n'a pas de poids.
#[must_use]
pub fn catalog(dir: &Path) -> Vec<Result<Weights, String>> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
                && p.metadata().is_ok_and(|m| m.is_file())
        })
        .collect();
    paths.sort();
    paths.iter().map(|p| read(p)).collect()
}

/// Les fichiers de poids que la configuration du système nomme : `PROPHET_WEIGHTS`, chemins
/// séparés par `:`. Sur l'image, le modèle par défaut vit dans `/nix/store` (ADR 0033), hors du
/// dossier des poids ; le module du moteur local les nomme ici.
#[must_use]
pub fn configured() -> Vec<PathBuf> {
    std::env::var_os("PROPHET_WEIGHTS")
        .map(|v| {
            std::env::split_paths(&v)
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Le catalogue d'une machine : celui du dossier, ceux que le catalogue du système y a
/// téléchargés (sous-dossier [`crate::catalogue::PULLED_DIR`]), puis chaque fichier nommé qui
/// n'y figure pas déjà. Un fichier nommé mais absent est dit refusé : la configuration promet un
/// modèle que la machine n'a pas.
#[must_use]
pub fn installed(dir: &Path, files: &[PathBuf]) -> Vec<Result<Weights, String>> {
    let mut all = catalog(dir);
    all.extend(catalog(&dir.join(crate::catalogue::PULLED_DIR)));
    let seen: Vec<PathBuf> = all
        .iter()
        .filter_map(|e| e.as_ref().ok())
        .filter_map(|w| w.path.canonicalize().ok())
        .collect();
    for file in files {
        if file.canonicalize().is_ok_and(|c| seen.contains(&c)) {
            continue;
        }
        all.push(read(file));
    }
    all
}

/// Le dossier des poids de cette machine : `PROPHET_MODELS_DIR`, sinon [`DEFAULT_DIR`].
#[must_use]
pub fn dir() -> PathBuf {
    std::env::var_os("PROPHET_MODELS_DIR").map_or_else(|| PathBuf::from(DEFAULT_DIR), PathBuf::from)
}

/// Le nom d'une quantification (`general.file_type` de llama.cpp).
fn file_type(n: u64) -> String {
    match n {
        0 => "F32",
        1 => "F16",
        2 => "Q4_0",
        3 => "Q4_1",
        7 => "Q8_0",
        8 => "Q5_0",
        9 => "Q5_1",
        10 => "Q2_K",
        11 => "Q3_K_S",
        12 => "Q3_K_M",
        13 => "Q3_K_L",
        14 => "Q4_K_S",
        15 => "Q4_K_M",
        16 => "Q5_K_S",
        17 => "Q5_K_M",
        18 => "Q6_K",
        19 => "IQ2_XXS",
        20 => "IQ2_XS",
        21 => "Q2_K_S",
        22 => "IQ3_XS",
        23 => "IQ3_XXS",
        24 => "IQ1_S",
        25 => "IQ4_NL",
        26 => "IQ3_S",
        27 => "IQ3_M",
        28 => "IQ2_S",
        29 => "IQ2_M",
        30 => "IQ4_XS",
        31 => "IQ1_M",
        32 => "BF16",
        _ => return format!("type {n}"),
    }
    .to_owned()
}

fn u32_le(r: &mut impl Read) -> Result<u32, String> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)
        .map_err(|_| "en-tête tronqué".to_owned())?;
    Ok(u32::from_le_bytes(b))
}

fn u64_le(r: &mut impl Read) -> Result<u64, String> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)
        .map_err(|_| "en-tête tronqué".to_owned())?;
    Ok(u64::from_le_bytes(b))
}

fn string(r: &mut impl Read) -> Result<String, String> {
    let len = u64_le(r)?;
    if len > MAX_STRING {
        return Err(format!("chaîne de {len} octets, au-delà de {MAX_STRING}"));
    }
    let mut b = vec![0u8; usize::try_from(len).map_err(|e| e.to_string())?];
    r.read_exact(&mut b)
        .map_err(|_| "en-tête tronqué".to_owned())?;
    String::from_utf8(b).map_err(|_| "chaîne qui n'est pas de l'UTF-8".to_owned())
}

/// Taille d'un scalaire d'un type donné, `None` pour les chaînes et tableaux.
fn scalar_size(kind: u32) -> Option<u64> {
    match kind {
        0 | 1 | 7 => Some(1),
        2 | 3 => Some(2),
        4..=6 => Some(4),
        10..=12 => Some(8),
        _ => None,
    }
}

fn skip<R: Read + Seek>(r: &mut R, n: u64, file_len: u64) -> Result<(), String> {
    let at = r.stream_position().map_err(|e| e.to_string())?;
    if at.checked_add(n).is_none_or(|end| end > file_len) {
        return Err("en-tête qui dépasse la fin du fichier".into());
    }
    r.seek(SeekFrom::Current(
        i64::try_from(n).map_err(|e| e.to_string())?,
    ))
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn value<R: Read + Seek>(r: &mut R, kind: u32, file_len: u64) -> Result<Value, String> {
    Ok(match kind {
        0 | 7 => {
            let mut b = [0u8; 1];
            r.read_exact(&mut b)
                .map_err(|_| "en-tête tronqué".to_owned())?;
            Value::Unsigned(u64::from(b[0]))
        }
        1 => {
            let mut b = [0u8; 1];
            r.read_exact(&mut b)
                .map_err(|_| "en-tête tronqué".to_owned())?;
            Value::Signed(i64::from(i8::from_le_bytes(b)))
        }
        2 => {
            let mut b = [0u8; 2];
            r.read_exact(&mut b)
                .map_err(|_| "en-tête tronqué".to_owned())?;
            Value::Unsigned(u64::from(u16::from_le_bytes(b)))
        }
        3 => {
            let mut b = [0u8; 2];
            r.read_exact(&mut b)
                .map_err(|_| "en-tête tronqué".to_owned())?;
            Value::Signed(i64::from(i16::from_le_bytes(b)))
        }
        4 => Value::Unsigned(u64::from(u32_le(r)?)),
        5 => Value::Signed(i64::from(u32_le(r)?.cast_signed())),
        10 => Value::Unsigned(u64_le(r)?),
        11 => Value::Signed(u64_le(r)?.cast_signed()),
        6 | 12 => {
            skip(r, scalar_size(kind).unwrap_or(0), file_len)?;
            Value::Other
        }
        8 => Value::Text(string(r)?),
        9 => {
            let inner = u32_le(r)?;
            let len = u64_le(r)?;
            if len > MAX_ARRAY {
                return Err(format!("tableau de {len} éléments, au-delà de {MAX_ARRAY}"));
            }
            match (inner, scalar_size(inner)) {
                (_, Some(size)) => skip(r, len.saturating_mul(size), file_len)?,
                (8, None) => {
                    for _ in 0..len {
                        let n = u64_le(r)?;
                        if n > MAX_STRING {
                            return Err(format!("chaîne de {n} octets, au-delà de {MAX_STRING}"));
                        }
                        skip(r, n, file_len)?;
                    }
                }
                _ => return Err(format!("tableau d'un type {inner} non pris en charge")),
            }
            Value::Other
        }
        other => return Err(format!("type de métadonnée {other} inconnu")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un en-tête GGUF v3 construit à la main, sans tenseurs.
    struct Entete(Vec<u8>, u64);

    impl Entete {
        fn new() -> Self {
            Self(Vec::new(), 0)
        }
        fn key(&mut self, k: &str, kind: u32) -> &mut Self {
            self.0.extend((k.len() as u64).to_le_bytes());
            self.0.extend(k.as_bytes());
            self.0.extend(kind.to_le_bytes());
            self.1 += 1;
            self
        }
        fn text(&mut self, k: &str, v: &str) -> &mut Self {
            self.key(k, 8);
            self.0.extend((v.len() as u64).to_le_bytes());
            self.0.extend(v.as_bytes());
            self
        }
        fn u32(&mut self, k: &str, v: u32) -> &mut Self {
            self.key(k, 4);
            self.0.extend(v.to_le_bytes());
            self
        }
        fn bytes(&self) -> Vec<u8> {
            let mut out = b"GGUF".to_vec();
            out.extend(3u32.to_le_bytes());
            out.extend(0u64.to_le_bytes());
            out.extend(self.1.to_le_bytes());
            out.extend(&self.0);
            out
        }
    }

    fn ecrire(dir: &Path, nom: &str, octets: &[u8]) -> PathBuf {
        let path = dir.join(nom);
        std::fs::write(&path, octets).unwrap();
        path
    }

    fn qwen() -> Entete {
        let mut e = Entete::new();
        e.text("general.architecture", "qwen3")
            .text("general.name", "Qwen3 1.7B")
            .text("general.size_label", "1.7B")
            .u32("general.file_type", 7)
            .u32("qwen3.context_length", 40_960)
            .u32("qwen3.block_count", 28);
        // Un vocabulaire, qu'il faut sauter sans le garder.
        e.key("tokenizer.ggml.tokens", 9);
        e.0.extend(8u32.to_le_bytes());
        e.0.extend(3u64.to_le_bytes());
        for token in ["<|im_start|>", "bonjour", "é"] {
            e.0.extend((token.len() as u64).to_le_bytes());
            e.0.extend(token.as_bytes());
        }
        // Des scalaires d'autres types, et un tableau de nombres.
        e.key("qwen3.rope.freq_base", 6);
        e.0.extend(1_000_000f32.to_le_bytes());
        e.key("tokenizer.ggml.add_bos_token", 7);
        e.0.push(0);
        e.key("tokenizer.ggml.token_type", 9);
        e.0.extend(5u32.to_le_bytes());
        e.0.extend(3u64.to_le_bytes());
        e.0.extend([1u8, 0, 0, 0, 1, 0, 0, 0, 3, 0, 0, 0]);
        e
    }

    #[test]
    fn un_en_tete_dit_l_architecture_la_quantification_et_le_contexte() {
        let dir = tempfile::tempdir().unwrap();
        let path = ecrire(dir.path(), "qwen3-1.7b.gguf", &qwen().bytes());
        let w = read(&path).unwrap();
        assert_eq!(w.architecture.as_deref(), Some("qwen3"));
        assert_eq!(w.name.as_deref(), Some("Qwen3 1.7B"));
        assert_eq!(w.size_label.as_deref(), Some("1.7B"));
        assert_eq!(w.quantization.as_deref(), Some("Q8_0"));
        assert_eq!(w.context_length, Some(40_960));
        assert_eq!(w.layers, Some(28));
        assert_eq!(w.version, 3);
        assert_eq!(w.bytes, std::fs::metadata(&path).unwrap().len());
    }

    #[test]
    fn un_en_tete_hostile_est_refuse_sans_allouer_ni_boucler() {
        let dir = tempfile::tempdir().unwrap();
        let mut faux = qwen().bytes();
        faux[..4].copy_from_slice(b"GGML");
        assert!(
            read(&ecrire(dir.path(), "a.gguf", &faux))
                .unwrap_err()
                .contains("signature")
        );

        let mut version = qwen().bytes();
        version[4..8].copy_from_slice(&9u32.to_le_bytes());
        assert!(
            read(&ecrire(dir.path(), "b.gguf", &version))
                .unwrap_err()
                .contains("version")
        );

        let mut trop = qwen().bytes();
        trop[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(
            read(&ecrire(dir.path(), "c.gguf", &trop))
                .unwrap_err()
                .contains("métadonnées")
        );

        // Une chaîne qui annonce un téraoctet.
        let mut e = Entete::new();
        e.key("general.name", 8);
        e.0.extend((1u64 << 40).to_le_bytes());
        assert!(
            read(&ecrire(dir.path(), "d.gguf", &e.bytes()))
                .unwrap_err()
                .contains("chaîne")
        );

        // Un tableau de nombres qui dépasse la fin du fichier.
        let mut e = Entete::new();
        e.key("x", 9);
        e.0.extend(4u32.to_le_bytes());
        e.0.extend(1_000_000u64.to_le_bytes());
        assert!(
            read(&ecrire(dir.path(), "e.gguf", &e.bytes()))
                .unwrap_err()
                .contains("fin du fichier")
        );

        let tronque = qwen().bytes();
        let tronque = &tronque[..tronque.len() - 5];
        assert!(read(&ecrire(dir.path(), "f.gguf", tronque)).is_err());
    }

    #[test]
    fn le_catalogue_lit_chaque_gguf_dans_l_ordre_et_dit_ce_qu_il_refuse() {
        let dir = tempfile::tempdir().unwrap();
        ecrire(dir.path(), "b-qwen.gguf", &qwen().bytes());
        ecrire(dir.path(), "a-casse.GGUF", b"pas un modele");
        ecrire(dir.path(), "notes.txt", b"ignore");
        std::fs::create_dir(dir.path().join("dossier.gguf")).unwrap();
        let catalogue = catalog(dir.path());
        assert_eq!(catalogue.len(), 2);
        assert!(catalogue[0].as_ref().unwrap_err().contains("a-casse.GGUF"));
        assert_eq!(
            catalogue[1].as_ref().unwrap().architecture.as_deref(),
            Some("qwen3")
        );
        assert!(catalog(&dir.path().join("absent")).is_empty());
    }

    #[test]
    fn les_fichiers_nommes_par_la_configuration_rejoignent_le_dossier_sans_doublon() {
        let dir = tempfile::tempdir().unwrap();
        let magasin = tempfile::tempdir().unwrap();
        let dans_le_dossier = ecrire(dir.path(), "a.gguf", &qwen().bytes());
        let ailleurs = ecrire(magasin.path(), "defaut.gguf", &qwen().bytes());
        let absent = magasin.path().join("promis.gguf");
        let tout = installed(dir.path(), &[dans_le_dossier, ailleurs.clone(), absent]);
        assert_eq!(tout.len(), 3, "{tout:?}");
        assert_eq!(tout[1].as_ref().unwrap().path, ailleurs);
        assert!(tout[2].as_ref().unwrap_err().contains("promis.gguf"));
        // Les poids téléchargés par le catalogue du système en font partie, pas leurs morceaux.
        let telecharges = dir.path().join(crate::catalogue::PULLED_DIR);
        std::fs::create_dir_all(&telecharges).unwrap();
        let tire = ecrire(&telecharges, "tire.gguf", &qwen().bytes());
        ecrire(&telecharges, ".suivant.gguf.part", b"GGUF");
        let tout = installed(dir.path(), &[]);
        assert_eq!(tout.len(), 2, "{tout:?}");
        assert_eq!(tout[1].as_ref().unwrap().path, tire);
    }

    #[test]
    fn une_quantification_inconnue_garde_son_numero() {
        assert_eq!(file_type(15), "Q4_K_M");
        assert_eq!(file_type(999), "type 999");
    }
}
