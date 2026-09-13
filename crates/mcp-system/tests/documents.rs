//! `doc.read` sur des fichiers fabriqués pour l'occasion : chaque format est reconnu à ses
//! octets et rendu lisible, sous le même périmètre que `fs.read`.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use capd::Broker;
use mcp_system::protocol::CallResult;
use mcp_system::registry::{MemoryJournal, Registry, ToolContext};
use prophet_types::cap::{Act, Grant, Res};
use prophet_types::manifest::Manifest;
use serde_json::{Value, json};
use time::OffsetDateTime;

struct Monde {
    _dir: tempfile::TempDir,
    docs: PathBuf,
    context: ToolContext,
    registry: Registry,
}

fn monde() -> Monde {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let work = home.join(".prophet/tasks/docs/work");
    let docs = home.join("docs");
    for p in [&work, &docs, &home.join("prive")] {
        std::fs::create_dir_all(p).unwrap();
    }
    let manifest = Manifest::from_toml(
        r#"
[agent]
id = "org.prophet.doc-test"
version = "1.0.0"
name = "Test documents"
publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
[model]
preferred = ["local:test"]
[capabilities.max]
"fs.read" = ["~/docs/**"]
"tool.call" = ["fs.*", "doc.read"]
"#,
    )
    .unwrap();
    let mut broker = Broker::new(
        ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng),
        "capd@test",
        home.display().to_string(),
    )
    .unwrap();
    let grants = vec![
        Grant::new(Res::Tool, Act::Call, "fs.*"),
        Grant::new(Res::Tool, Act::Call, "doc.read"),
        Grant::new(Res::Fs, Act::Read, "~/docs/**"),
    ];
    let token = broker
        .mint(
            &manifest,
            "docs",
            "u",
            &grants,
            3600,
            OffsetDateTime::now_utc(),
        )
        .unwrap();
    let broker = Arc::new(Mutex::new(broker));
    let journal = Arc::new(MemoryJournal::new());
    let mut registry = Registry::new(broker, journal);
    mcp_system::tools::register_all(&mut registry);
    let context = ToolContext {
        token,
        task: "docs".into(),
        home: home.display().to_string(),
        workdir: work.display().to_string(),
        sandbox_level: 1,
        step: 1,
    };
    Monde {
        _dir: dir,
        docs,
        context,
        registry,
    }
}

impl Monde {
    fn lire(&self, path: &str, extra: Value) -> CallResult {
        let mut args = json!({"path": path});
        if let Some(o) = extra.as_object() {
            for (k, v) in o {
                args[k] = v.clone();
            }
        }
        self.registry
            .call("doc.read", &args, &self.context, OffsetDateTime::now_utc())
    }

    fn ecrire(&self, name: &str, bytes: &[u8]) {
        std::fs::write(self.docs.join(name), bytes).unwrap();
    }
}

fn structured(result: CallResult) -> Value {
    assert!(
        !result.is_error,
        "{}",
        serde_json::to_string(&result).unwrap_or_default()
    );
    result.structured.expect("résultat structuré")
}

fn present(program: &str) -> bool {
    std::process::Command::new(program)
        .arg("-v")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// Un PDF minimal, écrit à la main, dont le texte est « bonjour ».
fn pdf_minimal() -> Vec<u8> {
    let objets = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
        "<< /Length 44 >>\nstream\nBT /F1 24 Tf 20 40 Td (bonjour) Tj ET\nendstream",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ];
    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, o) in objets.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{o}\nendobj\n", i + 1));
    }
    let xref = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objets.len() + 1
    ));
    for off in offsets {
        out.push_str(&format!("{off:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objets.len() + 1
    ));
    out.into_bytes()
}

fn docx(texte: &str) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("[Content_Types].xml", opts).unwrap();
        w.write_all(b"<Types/>").unwrap();
        w.start_file("word/document.xml", opts).unwrap();
        w.write_all(format!("<w:document><w:body><w:p><w:r><w:t>{texte}</w:t></w:r></w:p><w:p><w:r><w:t>fin</w:t></w:r></w:p></w:body></w:document>").as_bytes()).unwrap();
        w.finish().unwrap();
    }
    buf.into_inner()
}

fn png(largeur: u32, hauteur: u32) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(largeur, hauteur, image::Rgb([200, 30, 30]));
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

/// Un WAV de silence : 8 000 Hz, mono, 16 bits, une demi-seconde.
fn wav() -> Vec<u8> {
    let echantillons: u32 = 4000;
    let data = echantillons * 2;
    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&8000u32.to_le_bytes());
    v.extend_from_slice(&16000u32.to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data.to_le_bytes());
    v.resize(v.len() + data as usize, 0);
    v
}

#[test]
fn le_texte_le_html_et_le_json_se_lisent_tels_quels() {
    let m = monde();
    m.ecrire("note.md", "# Titre\n\nbonjour".as_bytes());
    let r = m.lire("~/docs/note.md", json!({}));
    let v = structured(r);
    assert_eq!(v["kind"], "markdown");
    assert_eq!(v["text"], "# Titre\n\nbonjour");
    m.ecrire(
        "page.html",
        b"<html><head><script>x()</script></head><body><p>Un</p><p>Deux &amp; trois</p></body></html>",
    );
    let v = structured(m.lire("~/docs/page.html", json!({})));
    assert_eq!(v["kind"], "html");
    assert_eq!(v["text"], "Un\nDeux & trois");
}

#[test]
fn le_perimetre_est_celui_de_fs_read() {
    let m = monde();
    std::fs::write(m.docs.parent().unwrap().join("prive/secret.txt"), "NON").unwrap();
    let r = m.lire("~/prive/secret.txt", json!({}));
    assert!(r.is_error, "{r:?}");
    let r = m.lire("~/docs/absent.pdf", json!({}));
    assert!(r.is_error);
}

#[test]
fn un_document_word_se_lit_en_rust_pur() {
    let m = monde();
    m.ecrire("lettre.docx", &docx("bonjour"));
    let v = structured(m.lire("~/docs/lettre.docx", json!({})));
    assert_eq!(v["kind"], "docx", "{v}");
    assert_eq!(v["text"], "bonjour\nfin");
    // Le même fichier sous un autre nom : le format vient des octets.
    m.ecrire("lettre.bin", &docx("bonjour"));
    assert_eq!(
        structured(m.lire("~/docs/lettre.bin", json!({})))["kind"],
        "docx"
    );
}

#[test]
fn une_image_rend_ses_dimensions() {
    let m = monde();
    m.ecrire("carre.png", &png(3, 2));
    let v = structured(m.lire("~/docs/carre.png", json!({"ocr": false})));
    assert_eq!(v["kind"], "image", "{v}");
    assert_eq!(v["meta"]["width"], 3);
    assert_eq!(v["meta"]["height"], 2);
    assert_eq!(v["meta"]["format"], "png");
}

#[test]
fn une_archive_se_liste() {
    let m = monde();
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        for n in ["a.txt", "dossier/b.txt"] {
            w.start_file(n, opts).unwrap();
            w.write_all(b"x").unwrap();
        }
        w.finish().unwrap();
    }
    m.ecrire("archive.zip", &buf.into_inner());
    let v = structured(m.lire("~/docs/archive.zip", json!({})));
    assert_eq!(v["kind"], "archive");
    assert_eq!(v["meta"]["entries"], 2);
    assert!(v["text"].as_str().unwrap().contains("dossier/b.txt"));
}

#[test]
fn un_pdf_rend_son_texte_quand_poppler_est_la() {
    let m = monde();
    m.ecrire("doc.pdf", &pdf_minimal());
    let v = structured(m.lire("~/docs/doc.pdf", json!({})));
    assert_eq!(v["kind"], "pdf", "{v}");
    if present("pdftotext") {
        assert!(v["text"].as_str().unwrap().contains("bonjour"), "{v}");
        assert_eq!(v["meta"]["pages"], 1, "{v}");
    } else {
        assert!(v["notes"][0].as_str().unwrap().contains("absent"), "{v}");
    }
}

#[test]
fn un_son_rend_sa_duree_quand_ffprobe_est_la() {
    let m = monde();
    m.ecrire("silence.wav", &wav());
    let v = structured(m.lire("~/docs/silence.wav", json!({})));
    assert_eq!(v["kind"], "media", "{v}");
    if present("ffprobe") {
        let duree: f64 = v["meta"]["duration"].as_str().unwrap().parse().unwrap();
        assert!((duree - 0.5).abs() < 0.05, "{v}");
        assert!(v["text"].as_str().unwrap().contains("flux audio"), "{v}");
    } else {
        assert!(v["notes"][0].as_str().unwrap().contains("absent"), "{v}");
    }
}

#[test]
fn la_borne_de_texte_est_dite() {
    let m = monde();
    m.ecrire("long.txt", "a".repeat(10_000).as_bytes());
    let v = structured(m.lire("~/docs/long.txt", json!({"max_chars": 100})));
    assert_eq!(v["text"].as_str().unwrap().len(), 100);
    assert_eq!(v["truncated"], true);
    assert_eq!(v["total_bytes"], 10_000);
}
