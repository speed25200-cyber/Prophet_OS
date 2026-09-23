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

/// Un moteur simulé qui répond à `/props` comme llama-server, pour une seule requête.
fn moteur(props: Value) -> (String, std::thread::JoinHandle<String>) {
    use std::io::{BufRead as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let fil = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut lecture = std::io::BufReader::new(stream);
        let mut premiere = String::new();
        lecture.read_line(&mut premiere).unwrap();
        loop {
            let mut ligne = String::new();
            lecture.read_line(&mut ligne).unwrap();
            if ligne == "\r\n" || ligne.is_empty() {
                break;
            }
        }
        let corps = props.to_string();
        write!(
            lecture.get_mut(),
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{corps}",
            corps.len()
        )
        .unwrap();
        premiere
    });
    (endpoint, fil)
}

#[test]
fn le_catalogue_dit_quel_poids_le_moteur_sert_et_avec_quelle_fenetre() {
    // Un agent qui dose ses lectures doit connaître la fenêtre servie (4 096), pas seulement
    // celle que le fichier annonce (40 960).
    let dir = tempfile::tempdir().unwrap();
    let servi = dir.path().join("qwen3-1.7b.gguf");
    std::fs::write(&servi, gguf("qwen3", "1.7B", 7, 40_960)).unwrap();
    std::fs::write(
        dir.path().join("gemma.gguf"),
        gguf("gemma3", "4B", 15, 131_072),
    )
    .unwrap();
    let chemin = dir.path().to_str().unwrap();
    let props = serde_json::json!({"model_path": servi, "total_slots": 1,
        "default_generation_settings": {"n_ctx": 4096}});

    let (endpoint, fil) = moteur(props.clone());
    let clair = prophet(&["model", "ls", "--dir", chemin, "--endpoint", &endpoint]);
    assert!(fil.join().unwrap().starts_with("GET /props "));
    let ligne = clair
        .lines()
        .find(|l| l.starts_with("qwen3-1.7b.gguf"))
        .unwrap();
    assert!(ligne.contains("servi") && ligne.contains("4096"), "{clair}");
    let autre = clair.lines().find(|l| l.starts_with("gemma.gguf")).unwrap();
    assert!(!autre.contains("servi"), "{clair}");

    let (endpoint, fil) = moteur(props);
    let json: Value = serde_json::from_str(&prophet(&[
        "--json",
        "model",
        "ls",
        "--dir",
        chemin,
        "--endpoint",
        &endpoint,
    ]))
    .unwrap();
    fil.join().unwrap();
    assert_eq!(json["served"]["n_ctx"], 4096, "{json}");
    assert_eq!(json["served"]["path"], servi.to_str().unwrap());

    // Sans moteur, le catalogue se lit quand même, et le dit.
    let seul = prophet(&[
        "model",
        "ls",
        "--dir",
        chemin,
        "--endpoint",
        "http://127.0.0.1:1/v1",
    ]);
    assert!(
        seul.contains("qwen3") && seul.contains("injoignable"),
        "{seul}"
    );
}
