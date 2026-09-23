//! `prophet model ls` : le catalogue des poids, lu par le vrai binaire dans un dossier de
//! fichiers GGUF construits pour l'essai.

use serde_json::Value;

const CLI: &str = env!("CARGO_BIN_EXE_prophet");

/// Un en-tête GGUF v3 minimal : architecture, taille, quantification, contexte.
fn gguf(architecture: &str, taille: &str, file_type: u32, contexte: u32) -> Vec<u8> {
    gguf_avec(architecture, taille, file_type, contexte, &[])
}

/// Le même, avec des nombres de plus sous l'architecture (`block_count`, têtes…).
fn gguf_avec(
    architecture: &str,
    taille: &str,
    file_type: u32,
    contexte: u32,
    en_plus: &[(&str, u32)],
) -> Vec<u8> {
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
    let mut nombres = vec![
        ("general.file_type".to_owned(), file_type),
        (format!("{architecture}.context_length"), contexte),
    ];
    nombres.extend(
        en_plus
            .iter()
            .map(|(k, v)| (format!("{architecture}.{k}"), *v)),
    );
    for (k, v) in nombres {
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

/// Un faux agentd qui rend ce catalogue à chaque `model.catalog`.
fn agentd_au_catalogue(catalogue: Value) -> (tempfile::TempDir, std::path::PathBuf) {
    use std::io::{BufRead as _, Write as _};
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("agentd.sock");
    let ecoute = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    std::thread::spawn(move || {
        for flux in ecoute.incoming() {
            let Ok(flux) = flux else { return };
            let mut lecteur = std::io::BufReader::new(flux);
            let mut ligne = String::new();
            if lecteur.read_line(&mut ligne).unwrap_or(0) == 0 {
                continue;
            }
            let requete: Value = serde_json::from_str(&ligne).unwrap();
            assert_eq!(requete["method"], "model.catalog", "{requete}");
            let reponse =
                serde_json::json!({"jsonrpc": "2.0", "id": requete["id"], "result": catalogue});
            let _ = writeln!(lecteur.get_mut(), "{reponse}");
        }
    });
    (dir, socket)
}

/// Un faux routeur de llama-server : le modèle est déchargé jusqu'à ce qu'on demande de le
/// charger. Rend l'adresse et les requêtes reçues.
fn routeur(chemin: std::path::PathBuf) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use std::io::{BufRead as _, Read as _, Write as _};
    let ecoute = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let adresse = format!("http://{}/v1", ecoute.local_addr().unwrap());
    let recues = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let notees = recues.clone();
    std::thread::spawn(move || {
        let mut charge = false;
        for flux in ecoute.incoming() {
            let Ok(flux) = flux else { return };
            let mut lecteur = std::io::BufReader::new(flux);
            let mut premiere = String::new();
            let _ = lecteur.read_line(&mut premiere);
            let mut longueur = 0;
            loop {
                let mut ligne = String::new();
                if lecteur.read_line(&mut ligne).unwrap_or(0) == 0 || ligne == "\r\n" {
                    break;
                }
                if let Some(v) = ligne.to_ascii_lowercase().strip_prefix("content-length:") {
                    longueur = v.trim().parse().unwrap();
                }
            }
            let mut corps = vec![0; longueur];
            let _ = lecteur.read_exact(&mut corps);
            notees.lock().unwrap().push(format!(
                "{} {}",
                premiere.trim(),
                String::from_utf8_lossy(&corps)
            ));
            let reponse = if premiere.starts_with("POST /models/load ") {
                charge = true;
                serde_json::json!({"success": true})
            } else {
                // La forme du routeur épinglé : le fichier dans les arguments de l'instance.
                serde_json::json!({"data": [
                    {"id": "qwen3-1.7b", "status": {"value": "loaded"}},
                    {"id": "Qwen3-4B-Q4_K_M",
                     "status": {"value": if charge { "loaded" } else { "unloaded" },
                                "args": ["llama-server", "--model", chemin]}}
                ]})
            }
            .to_string();
            let _ = write!(
                lecteur.get_mut(),
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reponse}",
                reponse.len()
            );
        }
    });
    (adresse, recues)
}

#[test]
fn servir_un_poids_telecharge_le_fait_charger_par_le_routeur() {
    let poids = tempfile::tempdir().unwrap();
    let chemin = poids.path().join("Qwen3-4B-Q4_K_M.gguf");
    std::fs::write(&chemin, gguf("qwen3", "4B", 15, 40_960)).unwrap();
    let (_agentd, socket) = agentd_au_catalogue(serde_json::json!({"entries": [
        {"id": "qwen3-4b-q4", "name": "Qwen3 4B", "installed": true, "path": chemin},
        {"id": "absent", "name": "Absent", "installed": false}
    ]}));
    let (moteur, recues) = routeur(chemin.clone());
    let sortie = std::process::Command::new(CLI)
        .args(["model", "serve", "qwen3-4b-q4", "--endpoint", &moteur])
        .env("PROPHET_AGENTD_SOCKET", &socket)
        .output()
        .unwrap();
    assert!(sortie.status.success(), "{sortie:?}");
    let dit = String::from_utf8(sortie.stdout).unwrap();
    assert!(dit.contains("« Qwen3-4B-Q4_K_M »"), "{dit}");
    // Le poids était déchargé : on dit ce que son chargement a pris.
    assert!(dit.contains(", chargé en "), "{dit}");
    let recues = recues.lock().unwrap().clone();
    assert!(
        recues
            .iter()
            .any(|r| r.starts_with("POST /models/load ") && r.contains("\"Qwen3-4B-Q4_K_M\"")),
        "{recues:?}"
    );
    // Un poids qui n'est pas sur la machine : on dit comment l'avoir.
    let sortie = std::process::Command::new(CLI)
        .args(["model", "serve", "absent", "--endpoint", &moteur])
        .env("PROPHET_AGENTD_SOCKET", &socket)
        .output()
        .unwrap();
    assert!(!sortie.status.success());
    let erreur = String::from_utf8_lossy(&sortie.stderr);
    assert!(erreur.contains("prophet model pull absent"), "{erreur}");
}

/// Des têtes qui demandent des téraoctets de cache KV : aucune machine ne les tient.
const DEMESURE: &[(&str, u32)] = &[
    ("block_count", 100_000),
    ("attention.head_count", 64),
    ("attention.key_length", 128),
    ("attention.value_length", 128),
];

#[test]
fn le_catalogue_dit_la_memoire_que_chaque_poids_demande() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("petit.gguf"),
        gguf_avec(
            "qwen3",
            "0.6B",
            7,
            40_960,
            &[
                ("block_count", 28),
                ("attention.head_count", 16),
                ("attention.head_count_kv", 8),
                ("attention.key_length", 128),
                ("attention.value_length", 128),
            ],
        ),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("demesure.gguf"),
        gguf_avec("llama", "1T", 15, 4096, DEMESURE),
    )
    .unwrap();
    let chemin = dir.path().to_str().unwrap();
    let sortie = std::process::Command::new(CLI)
        .args(["--json", "model", "ls", "--dir", chemin])
        .env("PROPHET_LOCAL_CONTEXT", "2048")
        .output()
        .unwrap();
    assert!(sortie.status.success(), "{sortie:?}");
    let json: Value = serde_json::from_slice(&sortie.stdout).unwrap();
    assert_eq!(json["context"], 2048);
    let poids = json["weights"].as_array().unwrap();
    let memoire = &poids
        .iter()
        .find(|p| p["path"].as_str().unwrap().ends_with("petit.gguf"))
        .unwrap()["memory"];
    // 28 couches × 8 têtes KV × (128 + 128) × 2 octets, pour 2 048 tokens.
    assert_eq!(memoire["kv_cache"], 28 * 8 * 256 * 2 * 2048, "{json}");
    assert_eq!(memoire["context"], 2048);
    assert!(memoire["total"].as_u64().unwrap() > memoire["kv_cache"].as_u64().unwrap());
    if json["system_memory"].is_object() {
        let demesure = poids
            .iter()
            .find(|p| p["path"].as_str().unwrap().ends_with("demesure.gguf"))
            .unwrap();
        assert_eq!(demesure["memory"]["fit"], "too_large", "{json}");
    }

    let clair = std::process::Command::new(CLI)
        .args(["model", "ls", "--dir", chemin])
        .output()
        .unwrap();
    let clair = String::from_utf8(clair.stdout).unwrap();
    assert!(clair.contains("mémoire"), "{clair}");
    assert!(clair.contains("fenêtre de 4096 tokens"), "{clair}");
    assert!(
        clair.contains("✗ ne tient pas en mémoire ici : demesure.gguf"),
        "{clair}"
    );
}

#[test]
fn un_poids_qui_ne_tient_pas_en_memoire_n_est_pas_charge_sans_force() {
    let poids = tempfile::tempdir().unwrap();
    let chemin = poids.path().join("Qwen3-4B-Q4_K_M.gguf");
    std::fs::write(&chemin, gguf_avec("qwen3", "4B", 15, 40_960, DEMESURE)).unwrap();
    let (_agentd, socket) = agentd_au_catalogue(serde_json::json!({"entries": [
        {"id": "qwen3-4b-q4", "name": "Qwen3 4B", "installed": true, "path": chemin}
    ]}));
    let (moteur, recues) = routeur(chemin.clone());
    let sortie = std::process::Command::new(CLI)
        .args(["model", "serve", "qwen3-4b-q4", "--endpoint", &moteur])
        .env("PROPHET_AGENTD_SOCKET", &socket)
        .output()
        .unwrap();
    assert!(!sortie.status.success(), "{sortie:?}");
    let erreur = String::from_utf8_lossy(&sortie.stderr);
    assert!(
        erreur.contains("ne tiendrait pas") && erreur.contains("--force"),
        "{erreur}"
    );
    assert!(
        !recues
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.starts_with("POST /models/load ")),
        "rien n'est demandé au moteur"
    );
    // En connaissance de cause, on charge quand même.
    let sortie = std::process::Command::new(CLI)
        .args([
            "--json",
            "model",
            "serve",
            "qwen3-4b-q4",
            "--endpoint",
            &moteur,
            "--force",
        ])
        .env("PROPHET_AGENTD_SOCKET", &socket)
        .output()
        .unwrap();
    assert!(sortie.status.success(), "{sortie:?}");
    let servi: Value = serde_json::from_slice(&sortie.stdout).unwrap();
    assert_eq!(servi["memory"]["fit"], "too_large", "{servi}");
    assert!(
        recues
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.starts_with("POST /models/load ")),
        "chargé avec --force"
    );
}
