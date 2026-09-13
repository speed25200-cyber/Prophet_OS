//! Lire les formats : un agent ne « voit » pas un PDF, une image ou une vidéo, il en reçoit
//! le texte et les mesures.
//!
//! `doc.read` lit un fichier du périmètre sous les règles de `fs.read`, reconnaît son format à
//! ses premiers octets (jamais à son seul nom), et en rend ce qu'un modèle peut lire : le texte
//! d'un PDF ou d'un document bureautique, les dimensions et le texte reconnu d'une image, la
//! durée et les flux d'un média, la liste d'une archive. Ce qui exige un programme (poppler,
//! ffprobe, tesseract) l'emploie s'il est là et dit sinon ce qui manque ; la bureautique
//! s'ouvre en Rust pur. Tout est borné : taille lue, texte rendu, temps par programme.

use std::io::{Cursor, Read as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::protocol::{CallResult, ErrorCode, ToolMeta, ToolSpec};
use crate::registry::{ResourceAccess, Tool, ToolContext};
use crate::tools::confined;

/// Octets lus au plus : au-delà, un fichier est rendu tronqué, pas refusé.
pub const MAX_INPUT: usize = 64 * 1024 * 1024;
/// Texte rendu au plus, en octets UTF-8.
pub const MAX_TEXT: usize = 256 * 1024;
/// Temps accordé à chaque programme externe.
pub const TOOL_TIMEOUT: Duration = Duration::from_secs(30);

/// `doc.read` : le contenu lisible d'un document, d'une image, d'un média ou d'une archive.
#[derive(Debug)]
pub struct Read;

impl Tool for Read {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "doc.read".into(),
            description: "Lit un fichier autorisé quel que soit son format et en rend ce qui se lit : texte d'un PDF, d'un document Word, LibreOffice, tableur ou présentation ; dimensions et texte reconnu (OCR) d'une image ; durée, flux et métadonnées d'une vidéo ou d'un son ; texte d'une page HTML ; liste d'une archive ; texte brut sinon. Le format est reconnu aux octets, pas au nom. Rend au plus 256 Kio de texte et signale la troncature.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "max_chars": {"type": "integer", "minimum": 0, "description": "Plafond de texte demandé, borné à 262144 octets."},
                    "ocr": {"type": "boolean", "default": true, "description": "Reconnaître le texte d'une image quand un moteur est disponible."}
                },
                "required": ["path"],
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "fs.read".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, args: &Value, context: &ToolContext) -> Option<String> {
        confined::target(args.get("path")?.as_str()?, context)
    }

    fn call(&self, _args: &Value, _context: &ToolContext) -> CallResult {
        CallResult::error(
            ErrorCode::PolicyDenied,
            "la lecture de documents exige le contrôleur de ressources du registre",
        )
    }

    fn call_checked(
        &self,
        args: &Value,
        context: &ToolContext,
        access: &dyn ResourceAccess,
    ) -> CallResult {
        let Some(path) = args.get("path").and_then(Value::as_str) else {
            return CallResult::error(ErrorCode::Invalid, "chemin texte requis");
        };
        let max_text = match args.get("max_chars") {
            None => MAX_TEXT,
            Some(v) => match v.as_u64() {
                Some(n) => (n as usize).min(MAX_TEXT),
                None => {
                    return CallResult::error(
                        ErrorCode::Invalid,
                        "max_chars doit être un entier positif ou nul",
                    );
                }
            },
        };
        let ocr = args.get("ocr").and_then(Value::as_bool).unwrap_or(true);
        let reading = match confined::read_bytes(path, context, access, MAX_INPUT) {
            Ok(r) => r,
            Err((code, detail)) => return CallResult::error(code, detail),
        };
        let mut rendu = extract(&reading.bytes, &reading.logical, ocr);
        let (text, truncated) = bound(&rendu.text, max_text);
        let mut value = json!({
            "path": reading.logical,
            "kind": rendu.kind,
            "total_bytes": reading.total,
            "text": text,
            "truncated": truncated || reading.truncated,
        });
        if reading.truncated {
            rendu
                .notes
                .push(format!("fichier lu jusqu'à {MAX_INPUT} octets seulement"));
        }
        if let Some(meta) = rendu.meta.take() {
            value["meta"] = meta;
        }
        if !rendu.notes.is_empty() {
            value["notes"] = json!(rendu.notes);
        }
        CallResult::structured(value)
    }
}

/// Ce qu'une lecture rend, avant bornage.
struct Rendu {
    kind: &'static str,
    text: String,
    meta: Option<Value>,
    notes: Vec<String>,
}

impl Rendu {
    fn new(kind: &'static str) -> Self {
        Self {
            kind,
            text: String::new(),
            meta: None,
            notes: Vec::new(),
        }
    }
}

fn bound(text: &str, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text.to_owned(), false);
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}

/// Reconnaît le format et en extrait ce qui se lit.
fn extract(bytes: &[u8], logical: &Path, ocr: bool) -> Rendu {
    let ext = logical
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if bytes.starts_with(b"%PDF") {
        return pdf(bytes);
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return zip_based(bytes);
    }
    if let Ok(format) = image::guess_format(bytes) {
        return picture(bytes, format, ocr);
    }
    if is_media(bytes) {
        return media(bytes, &ext);
    }
    if bytes.starts_with(b"\x1f\x8b") {
        let mut r = Rendu::new("archive");
        r.notes
            .push("archive gzip : listez-la ou décompressez-la avec un programme".into());
        return r;
    }
    if bytes.len() > 262 && &bytes[257..262] == b"ustar" {
        return tar(bytes);
    }
    if let Ok(text) = std::str::from_utf8(bytes)
        && !text.contains('\0')
    {
        let lower = text
            .trim_start()
            .get(..64)
            .unwrap_or("")
            .to_ascii_lowercase();
        if lower.starts_with("<!doctype html") || lower.starts_with("<html") {
            let mut r = Rendu::new("html");
            r.text = strip_html(text);
            return r;
        }
        let mut r = Rendu::new(match ext.as_str() {
            "json" => "json",
            "csv" | "tsv" => "csv",
            "md" | "markdown" => "markdown",
            "xml" | "svg" => "xml",
            _ => "text",
        });
        r.text = text.to_owned();
        return r;
    }
    let mut r = Rendu::new("binary");
    let head: Vec<String> = bytes.iter().take(16).map(|b| format!("{b:02x}")).collect();
    r.meta = Some(json!({"magic": head.join(" ")}));
    r.notes.push(
        "format non reconnu : ni texte, ni PDF, ni bureautique, ni image, ni média connu".into(),
    );
    r
}

// --- PDF, par poppler ---

fn pdf(bytes: &[u8]) -> Rendu {
    let mut r = Rendu::new("pdf");
    let Some(tmp) = Temp::write(bytes, "pdf") else {
        r.notes.push("fichier temporaire impossible".into());
        return r;
    };
    match run(
        "pdftotext",
        &["-layout", "-enc", "UTF-8", tmp.path_str(), "-"],
        TOOL_TIMEOUT,
    ) {
        Ok(out) => r.text = out.stdout,
        Err(e) => r.notes.push(format!("texte non extrait : {e}")),
    }
    if let Ok(info) = run("pdfinfo", &[tmp.path_str()], TOOL_TIMEOUT) {
        let mut meta = serde_json::Map::new();
        for line in info.stdout.lines() {
            if let Some((k, v)) = line.split_once(':') {
                let k = k.trim();
                if matches!(
                    k,
                    "Pages" | "Title" | "Author" | "Producer" | "CreationDate"
                ) {
                    let v = v.trim();
                    meta.insert(
                        k.to_ascii_lowercase(),
                        v.parse::<u64>().map_or_else(|_| json!(v), Value::from),
                    );
                }
            }
        }
        if !meta.is_empty() {
            r.meta = Some(Value::Object(meta));
        }
    }
    r
}

// --- Bureautique : archive et XML, en Rust pur ---

fn zip_based(bytes: &[u8]) -> Rendu {
    let mut archive = match zip::ZipArchive::new(Cursor::new(bytes)) {
        Ok(a) => a,
        Err(e) => {
            let mut r = Rendu::new("archive");
            r.notes.push(format!("archive illisible : {e}"));
            return r;
        }
    };
    let names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_owned()))
        .collect();
    let read = |archive: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str| -> Option<String> {
        let file = archive.by_name(name).ok()?;
        let mut s = String::new();
        file.take(8 * 1024 * 1024).read_to_string(&mut s).ok()?;
        Some(s)
    };
    if names.iter().any(|n| n == "word/document.xml") {
        let mut r = Rendu::new("docx");
        if let Some(xml) = read(&mut archive, "word/document.xml") {
            r.text = xml_text(
                &xml,
                &["</w:p>", "</w:tr>"],
                &[("<w:tab/>", "\t"), ("<w:br/>", "\n")],
            );
        }
        return r;
    }
    if names.iter().any(|n| n == "xl/workbook.xml") {
        let mut r = Rendu::new("xlsx");
        let mut parts = Vec::new();
        if let Some(xml) = read(&mut archive, "xl/workbook.xml") {
            let feuilles: Vec<String> = xml
                .split("<sheet ")
                .skip(1)
                .filter_map(|s| {
                    s.split("name=\"")
                        .nth(1)?
                        .split('"')
                        .next()
                        .map(str::to_owned)
                })
                .collect();
            if !feuilles.is_empty() {
                r.meta = Some(json!({"sheets": feuilles}));
            }
        }
        if let Some(xml) = read(&mut archive, "xl/sharedStrings.xml") {
            let strings = xml_text(&xml, &["</si>"], &[]);
            if !strings.trim().is_empty() {
                parts.push(strings);
            }
        }
        let mut sheets: Vec<String> = names
            .iter()
            .filter(|n| n.starts_with("xl/worksheets/sheet") && n.ends_with(".xml"))
            .cloned()
            .collect();
        sheets.sort();
        for sheet in sheets {
            if let Some(xml) = read(&mut archive, &sheet) {
                // Les nombres et formules ; les textes partagés sont déjà rendus ci-dessus.
                let cells = xml_text(&xml, &["</row>"], &[("</c>", "\t")]);
                if !cells.trim().is_empty() {
                    parts.push(format!("[{sheet}]\n{cells}"));
                }
            }
        }
        r.text = parts.join("\n\n");
        return r;
    }
    if names.iter().any(|n| n.starts_with("ppt/slides/slide")) {
        let mut r = Rendu::new("pptx");
        let mut slides: Vec<(u32, String)> = names
            .iter()
            .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
            .filter_map(|n| {
                let num: u32 = n
                    .trim_start_matches("ppt/slides/slide")
                    .trim_end_matches(".xml")
                    .parse()
                    .ok()?;
                Some((num, n.clone()))
            })
            .collect();
        slides.sort();
        let mut parts = Vec::new();
        for (num, name) in &slides {
            if let Some(xml) = read(&mut archive, name) {
                parts.push(format!(
                    "[diapositive {num}]\n{}",
                    xml_text(&xml, &["</a:p>"], &[])
                ));
            }
        }
        r.meta = Some(json!({"slides": slides.len()}));
        r.text = parts.join("\n\n");
        return r;
    }
    if names.iter().any(|n| n == "content.xml") {
        let mimetype = read(&mut archive, "mimetype").unwrap_or_default();
        let mut r = Rendu::new(match mimetype.trim() {
            "application/vnd.oasis.opendocument.text" => "odt",
            "application/vnd.oasis.opendocument.spreadsheet" => "ods",
            "application/vnd.oasis.opendocument.presentation" => "odp",
            _ => "odf",
        });
        if let Some(xml) = read(&mut archive, "content.xml") {
            r.text = xml_text(
                &xml,
                &["</text:p>", "</text:h>", "</table:table-row>"],
                &[
                    ("<text:tab/>", "\t"),
                    ("<text:s/>", " "),
                    ("<text:line-break/>", "\n"),
                    ("</table:table-cell>", "\t"),
                ],
            );
        }
        return r;
    }
    let mut r = Rendu::new("archive");
    let listed: Vec<&String> = names.iter().take(500).collect();
    r.text = listed
        .iter()
        .map(|n| n.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    r.meta = Some(json!({"entries": names.len()}));
    if names.len() > 500 {
        r.notes.push("liste limitée à 500 entrées".into());
    }
    r
}

/// Le texte d'un XML : les balises de fin de paragraphe deviennent des retours à la ligne,
/// quelques balises deviennent des espaces, les autres disparaissent, les entités sont rendues.
fn xml_text(xml: &str, breaks: &[&str], replacements: &[(&str, &str)]) -> String {
    let mut s = xml.to_owned();
    for b in breaks {
        s = s.replace(b, "\n");
    }
    for (from, to) in replacements {
        s = s.replace(from, to);
    }
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    collapse(&decode_entities(&out))
}

/// Des lignes vides en rafale n'apprennent rien : au plus une, et pas de blanc en bordure.
fn collapse(text: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    let mut blank = 0;
    for line in text.lines() {
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        lines.push(line.trim_end());
    }
    lines.join("\n").trim().to_owned()
}

fn decode_entities(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

fn strip_html(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    while i < html.len() {
        let rest = &lower[i..];
        if let Some(tag) = ["<script", "<style"].iter().find(|t| rest.starts_with(*t)) {
            let close = if *tag == "<script" {
                "</script>"
            } else {
                "</style>"
            };
            match rest.find(close) {
                Some(end) => {
                    i += end + close.len();
                    continue;
                }
                None => break,
            }
        }
        if rest.starts_with('<') {
            let end = rest.find('>').map_or(html.len() - i, |e| e + 1);
            let tag = &rest[..end];
            if tag.starts_with("</p")
                || tag.starts_with("<br")
                || tag.starts_with("</div")
                || tag.starts_with("</h")
                || tag.starts_with("</li")
                || tag.starts_with("</tr")
            {
                out.push('\n');
            } else if tag.starts_with("</td") || tag.starts_with("</th") {
                out.push('\t');
            }
            i += end;
            continue;
        }
        let ch = html[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    collapse(&decode_entities(&out))
}

// --- Images ---

fn picture(bytes: &[u8], format: image::ImageFormat, ocr: bool) -> Rendu {
    let mut r = Rendu::new("image");
    let format_name = format!("{format:?}").to_ascii_lowercase();
    match image::load_from_memory(bytes) {
        Ok(img) => {
            r.meta = Some(json!({
                "format": format_name,
                "width": img.width(),
                "height": img.height(),
            }));
        }
        Err(e) => {
            r.meta = Some(json!({"format": format_name}));
            r.notes.push(format!("image non décodée : {e}"));
        }
    }
    if ocr {
        let ext = format.extensions_str().first().copied().unwrap_or("img");
        match Temp::write(bytes, ext) {
            Some(tmp) => {
                let langs = ["fra+eng", "eng"];
                let mut done = false;
                for lang in langs {
                    match run(
                        "tesseract",
                        &[tmp.path_str(), "-", "-l", lang],
                        TOOL_TIMEOUT,
                    ) {
                        Ok(out) => {
                            r.text = out.stdout.trim().to_owned();
                            done = true;
                            break;
                        }
                        Err(e) if e.contains("absent") => {
                            r.notes.push("texte non reconnu : tesseract absent".into());
                            done = true;
                            break;
                        }
                        Err(_) => continue,
                    }
                }
                if !done {
                    r.notes
                        .push("texte non reconnu : aucune langue de tesseract n'a répondu".into());
                }
            }
            None => r.notes.push("fichier temporaire impossible".into()),
        }
    }
    r
}

// --- Médias, par ffprobe ---

fn is_media(bytes: &[u8]) -> bool {
    (bytes.len() > 12 && &bytes[4..8] == b"ftyp")
        || (bytes.starts_with(b"RIFF")
            && bytes.len() > 12
            && matches!(&bytes[8..12], b"AVI " | b"WAVE"))
        || bytes.starts_with(b"\x1aE\xdf\xa3")
        || bytes.starts_with(b"ID3")
        || bytes.starts_with(b"OggS")
        || bytes.starts_with(b"fLaC")
        || (bytes.len() > 2 && bytes[0] == 0xff && (bytes[1] & 0xe0) == 0xe0 && bytes.len() > 1000)
}

fn media(bytes: &[u8], ext: &str) -> Rendu {
    let mut r = Rendu::new("media");
    let ext = if ext.is_empty() { "bin" } else { ext };
    let Some(tmp) = Temp::write(bytes, ext) else {
        r.notes.push("fichier temporaire impossible".into());
        return r;
    };
    match run(
        "ffprobe",
        &[
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            tmp.path_str(),
        ],
        TOOL_TIMEOUT,
    ) {
        Ok(out) => match serde_json::from_str::<Value>(&out.stdout) {
            Ok(probe) => {
                let format = &probe["format"];
                let streams: Vec<Value> = probe["streams"]
                    .as_array()
                    .map(|s| {
                        s.iter()
                            .map(|st| {
                                let mut v = json!({
                                    "type": st["codec_type"],
                                    "codec": st["codec_name"],
                                });
                                for k in [
                                    "width",
                                    "height",
                                    "r_frame_rate",
                                    "channels",
                                    "sample_rate",
                                    "duration",
                                ] {
                                    if !st[k].is_null() {
                                        v[k] = st[k].clone();
                                    }
                                }
                                v
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let mut lines = Vec::new();
                if let Some(d) = format["duration"]
                    .as_str()
                    .and_then(|d| d.parse::<f64>().ok())
                {
                    lines.push(format!("durée : {:.1} s", d));
                }
                if let Some(f) = format["format_long_name"].as_str() {
                    lines.push(format!("conteneur : {f}"));
                }
                for st in &streams {
                    let mut l = format!(
                        "flux {} {}",
                        st["type"].as_str().unwrap_or("?"),
                        st["codec"].as_str().unwrap_or("?")
                    );
                    if let (Some(w), Some(h)) = (st["width"].as_u64(), st["height"].as_u64()) {
                        l.push_str(&format!(" {w}×{h}"));
                    }
                    if let Some(c) = st["channels"].as_u64() {
                        l.push_str(&format!(" {c} canal(aux)"));
                    }
                    lines.push(l);
                }
                if let Some(tags) = format["tags"].as_object() {
                    for (k, v) in tags.iter().take(12) {
                        if let Some(v) = v.as_str() {
                            lines.push(format!("{k} : {v}"));
                        }
                    }
                }
                r.text = lines.join("\n");
                r.meta = Some(json!({
                    "format": format["format_name"],
                    "duration": format["duration"],
                    "streams": streams,
                    "tags": format["tags"],
                }));
            }
            Err(e) => r.notes.push(format!("réponse de ffprobe illisible : {e}")),
        },
        Err(e) => r.notes.push(format!("média non analysé : {e}")),
    }
    r
}

// --- Archives tar ---

fn tar(bytes: &[u8]) -> Rendu {
    let mut r = Rendu::new("archive");
    let mut names = Vec::new();
    let mut offset = 0;
    while offset + 512 <= bytes.len() && names.len() < 500 {
        let header = &bytes[offset..offset + 512];
        if header.iter().all(|b| *b == 0) {
            break;
        }
        let name = String::from_utf8_lossy(&header[..100])
            .trim_end_matches('\0')
            .to_owned();
        let size = std::str::from_utf8(&header[124..136])
            .ok()
            .and_then(|s| usize::from_str_radix(s.trim_end_matches('\0').trim(), 8).ok())
            .unwrap_or(0);
        if !name.is_empty() {
            names.push(name);
        }
        offset += 512 + size.div_ceil(512) * 512;
    }
    r.meta = Some(json!({"entries": names.len()}));
    r.text = names.join("\n");
    r
}

// --- Programmes externes, bornés ---

#[derive(Debug)]
struct Output {
    stdout: String,
}

/// Lance un programme du PATH avec un délai ; absent, il le dit ; trop long, il le tue.
fn run(program: &str, args: &[&str], timeout: Duration) -> Result<Output, String> {
    let mut child = match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("{program} absent"));
        }
        Err(e) => return Err(format!("{program} : {e}")),
    };
    let stdout = child.stdout.take().expect("stdout demandé");
    let stderr = child.stderr.take().expect("stderr demandé");
    let lecteur_out = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.take(MAX_TEXT as u64 * 4).read_to_end(&mut buf);
        buf
    });
    let lecteur_err = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.take(8192).read_to_end(&mut buf);
        buf
    });
    let debut = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if debut.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Err(_) => break None,
        }
    };
    let out = lecteur_out.join().unwrap_or_default();
    let err = lecteur_err.join().unwrap_or_default();
    match status {
        Some(s) if s.success() => Ok(Output {
            stdout: String::from_utf8_lossy(&out).into_owned(),
        }),
        Some(s) => Err(format!(
            "{program} a échoué ({}) : {}",
            s.code().map_or("signal".to_owned(), |c| c.to_string()),
            String::from_utf8_lossy(&err).trim()
        )),
        None => Err(format!(
            "{program} interrompu après {} s",
            timeout.as_secs()
        )),
    }
}

/// Un fichier temporaire privé, effacé à la fin : les programmes externes veulent un chemin.
struct Temp(PathBuf);

impl Temp {
    fn write(bytes: &[u8], ext: &str) -> Option<Self> {
        use rand::RngCore;
        let mut nonce = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut nonce);
        let name = format!(
            "prophet-doc-{}-{}.{ext}",
            std::process::id(),
            nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );
        let path = std::env::temp_dir().join(name);
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .ok()?;
            std::io::Write::write_all(&mut f, bytes).ok()?;
        }
        Some(Self(path))
    }

    fn path_str(&self) -> &str {
        self.0.to_str().unwrap_or("")
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_xml_devient_du_texte_avec_ses_paragraphes() {
        let xml = r#"<w:document><w:body><w:p><w:r><w:t>Bonjour</w:t></w:r><w:tab/><w:r><w:t>&amp; bienvenue</w:t></w:r></w:p><w:p><w:r><w:t>Seconde ligne</w:t></w:r></w:p></w:body></w:document>"#;
        let texte = xml_text(xml, &["</w:p>"], &[("<w:tab/>", "\t")]);
        assert_eq!(texte, "Bonjour\t& bienvenue\nSeconde ligne");
    }

    #[test]
    fn le_html_perd_ses_scripts_et_garde_ses_lignes() {
        let html = "<!DOCTYPE html><html><head><style>p{}</style><script>alert(1)</script></head><body><h1>Titre</h1><p>Un &lt;mot&gt;</p><p>Deux</p></body></html>";
        let r = extract(html.as_bytes(), Path::new("page.html"), false);
        assert_eq!(r.kind, "html");
        assert_eq!(r.text, "Titre\nUn <mot>\nDeux");
    }

    #[test]
    fn le_format_vient_des_octets_pas_du_nom() {
        let r = extract(b"%PDF-1.4 rien", Path::new("photo.png"), false);
        assert_eq!(r.kind, "pdf");
        let r = extract(b"{\"a\":1}", Path::new("données.json"), false);
        assert_eq!(r.kind, "json");
        let r = extract(&[0u8, 1, 2, 3, 0xff], Path::new("x.txt"), false);
        assert_eq!(r.kind, "binary");
        assert!(r.notes[0].contains("non reconnu"));
    }

    #[test]
    fn la_borne_respecte_les_caracteres() {
        let (t, tronque) = bound("éééé", 3);
        assert_eq!(t, "é");
        assert!(tronque);
        let (t, tronque) = bound("abc", 3);
        assert_eq!(t, "abc");
        assert!(!tronque);
    }

    #[test]
    fn un_programme_absent_le_dit_et_un_programme_lent_est_tue() {
        assert!(
            run("prophet-programme-inexistant", &[], TOOL_TIMEOUT)
                .unwrap_err()
                .contains("absent")
        );
        let e = run("sleep", &["5"], Duration::from_millis(200)).unwrap_err();
        assert!(e.contains("interrompu"), "{e}");
    }
}
