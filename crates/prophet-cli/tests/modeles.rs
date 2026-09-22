//! `prophet model ls` : le catalogue des poids, lu par le vrai binaire dans un dossier de
//! fichiers GGUF construits pour l'essai.

use serde_json::Value;

const CLI: &str = env!("CARGO_BIN_EXE_prophet");

/// Un en-tête GGUF v3 minimal : architecture, taille, quantification, contexte.
fn gguf(architecture: &str, taille: &str, file_type: u32, contexte: u32) -> Vec<u8> {
    let mut kv = Vec::new();
    let mut n = 0u64;
    let mut texte = |kv: &mut Vec<u8>, k: &str, v: &str| {
        kv.extend((k.len() as u64).to_le_bytes());
        kv.extend(k.as_bytes());
        kv.extend(8u32.to_le_bytes());
        kv.extend((v.len() as u64).to_le_bytes());
        kv.extend(v.as_bytes());
        n += 1;
    };
    texte(&mut kv, "general.architecture", architecture);
    texte(&mut kv, "general.size_label", taille);
    for (k, v) in [
        ("general.file_type".to_owned(), file_type),
        (format!("{architecture}.context_length"), contexte),
    ] {
        kv.extend((k.len() as u64).to_le_bytes());
        kv.extend(k.as_bytes());
        kv.extend(4u32.to_le_bytes());
        kv.extend(v.to_le_bytes());
        n += 1;
    }
    let mut out = b"GGUF".to_vec();
    out.extend(3u32.to_le_bytes());
    out.extend(0u64.to_le_bytes());
    out.extend(n.to_le_bytes());
    out.extend(kv);
    out
}

fn prophet(args: &[&str]) -> String {
    let sortie = std::process::Command::new(CLI).args(args).output().unwrap();
    assert!(sortie.status.success(), "{sortie:?}");
    String::from_utf8(sortie.stdout).unwrap()
}

#[test]
fn le_catalogue_des_poids_se_lit_en_clair_et_en_json() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("qwen3-1.7b.gguf"),
        gguf("qwen3", "1.7B", 7, 40_960),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("gemma.gguf"),
        gguf("gemma3", "4B", 15, 131_072),
    )
    .unwrap();
    std::fs::write(dir.path().join("casse.gguf"), b"rien").unwrap();
    let chemin = dir.path().to_str().unwrap();

    let clair = prophet(&["model", "ls", "--dir", chemin]);
    assert!(
        clair.contains("qwen3") && clair.contains("Q8_0") && clair.contains("40960"),
        "{clair}"
    );
    assert!(
        clair.contains("gemma3") && clair.contains("Q4_K_M"),
        "{clair}"
    );
    assert!(
        clair.contains("refusé") && clair.contains("casse.gguf"),
        "{clair}"
    );

    let json: Value =
        serde_json::from_str(&prophet(&["--json", "model", "ls", "--dir", chemin])).unwrap();
    let poids = json["weights"].as_array().unwrap();
    assert_eq!(poids.len(), 2, "{json}");
    assert_eq!(poids[1]["architecture"], "qwen3");
    assert_eq!(poids[1]["context_length"], 40_960);
    assert_eq!(json["refused"].as_array().unwrap().len(), 1, "{json}");

    let vide = prophet(&["model", "ls", "--dir", &format!("{chemin}/absent")]);
    assert!(vide.contains("Aucun poids"), "{vide}");
}
