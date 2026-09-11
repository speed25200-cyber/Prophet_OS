//! Outils de fichiers.
//!
//! Tous travaillent dans l'espace de travail de la tâche, jamais directement dans celui de
//! l'utilisateur : une écriture n'atteint le disque réel qu'à la validation, et reste annulable.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::protocol::{CallResult, ErrorCode, ToolMeta, ToolSpec};
use crate::registry::{Tool, ToolContext};

/// Taille maximale rendue par défaut à un modèle, en octets.
pub const DEFAULT_MAX_BYTES: usize = 256 * 1024;

/// Résout un chemin d'argument vers son emplacement réel et son emplacement de travail.
///
/// Refuse tout chemin qui, une fois résolu, sortirait du répertoire personnel : c'est ici que
/// `..` et les chemins relatifs sont neutralisés, avant même le contrôle de capacité.
fn resolve(raw: &str, context: &ToolContext) -> Option<(PathBuf, PathBuf)> {
    let home = Path::new(&context.home);
    let real = if let Some(rest) = raw.strip_prefix("~/") {
        home.join(rest)
    } else if raw.starts_with('/') {
        PathBuf::from(raw)
    } else {
        home.join(raw)
    };
    if real
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return None;
    }
    let relative = real.strip_prefix(home).ok()?;
    let work = Path::new(&context.workdir).join(relative);
    Some((real, work))
}

/// Chemin à lire : la version de travail si la tâche l'a déjà touchée, la version réelle sinon.
fn read_path(real: &Path, work: &Path) -> PathBuf {
    if work.exists() {
        work.to_path_buf()
    } else {
        real.to_path_buf()
    }
}

fn object_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)?.as_str().map(ToOwned::to_owned)
}

/// Lecture d'un fichier.
#[derive(Debug)]
pub struct Read;

impl Tool for Read {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.read".into(),
            description: "Lit un fichier texte. Le chemin doit être dans le périmètre accordé à la tâche. Rend au plus 256 Kio ; au-delà, le résultat est tronqué et le signale.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Chemin absolu ou relatif au répertoire personnel."},
                    "max_bytes": {"type": "integer", "description": "Plafond de lecture, par défaut 262144."}
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
        let raw = object_string(args, "path")?;
        resolve(&raw, context).map(|(real, _)| real.display().to_string())
    }

    fn call(&self, args: &Value, context: &ToolContext) -> CallResult {
        let Some(raw) = object_string(args, "path") else {
            return CallResult::error(ErrorCode::Invalid, "argument `path` manquant");
        };
        let Some((real, work)) = resolve(&raw, context) else {
            return CallResult::error(
                ErrorCode::PolicyDenied,
                format!("chemin hors du répertoire personnel : {raw}"),
            );
        };
        let path = read_path(&real, &work);
        let max = args
            .get("max_bytes")
            .and_then(Value::as_u64)
            .map_or(DEFAULT_MAX_BYTES, |v| v as usize);

        match std::fs::read(&path) {
            Ok(bytes) => {
                let total = bytes.len();
                let truncated = total > max;
                let slice = &bytes[..total.min(max)];
                CallResult::structured(json!({
                    "path": real.display().to_string(),
                    "content": String::from_utf8_lossy(slice),
                    "total_bytes": total,
                    "truncated": truncated
                }))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                CallResult::error(ErrorCode::NotFound, format!("fichier absent : {raw}"))
            }
            Err(error) => CallResult::error(ErrorCode::Internal, error.to_string()),
        }
    }
}

/// Écriture d'un fichier, dans l'espace de travail de la tâche.
#[derive(Debug)]
pub struct Write;

impl Tool for Write {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.write".into(),
            description: "Écrit un fichier dans l'espace de travail de la tâche. Le contenu n'atteint le disque de l'utilisateur qu'à la validation de la tâche, et reste annulable ensuite.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "fs.write".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, args: &Value, context: &ToolContext) -> Option<String> {
        let raw = object_string(args, "path")?;
        resolve(&raw, context).map(|(real, _)| real.display().to_string())
    }

    fn call(&self, args: &Value, context: &ToolContext) -> CallResult {
        let (Some(raw), Some(content)) =
            (object_string(args, "path"), object_string(args, "content"))
        else {
            return CallResult::error(ErrorCode::Invalid, "arguments `path` et `content` requis");
        };
        let Some((real, work)) = resolve(&raw, context) else {
            return CallResult::error(
                ErrorCode::PolicyDenied,
                format!("chemin hors du répertoire personnel : {raw}"),
            );
        };
        if let Some(parent) = work.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            return CallResult::error(ErrorCode::Internal, error.to_string());
        }
        match std::fs::write(&work, content.as_bytes()) {
            Ok(()) => CallResult::structured(json!({
                "path": real.display().to_string(),
                "bytes": content.len(),
                "staged": true,
                "note": "écrit dans l'espace de travail ; validez la tâche pour l'appliquer"
            })),
            Err(error) => CallResult::error(ErrorCode::Internal, error.to_string()),
        }
    }
}

/// Énumération d'un répertoire.
#[derive(Debug)]
pub struct List;

impl Tool for List {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.list".into(),
            description: "Liste le contenu d'un répertoire, avec pour chaque entrée son nom, son type et sa taille.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "fs.list".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, args: &Value, context: &ToolContext) -> Option<String> {
        let raw = object_string(args, "path")?;
        resolve(&raw, context).map(|(real, _)| real.display().to_string())
    }

    fn call(&self, args: &Value, context: &ToolContext) -> CallResult {
        let Some(raw) = object_string(args, "path") else {
            return CallResult::error(ErrorCode::Invalid, "argument `path` manquant");
        };
        let Some((real, work)) = resolve(&raw, context) else {
            return CallResult::error(
                ErrorCode::PolicyDenied,
                "chemin hors du répertoire personnel",
            );
        };
        let path = read_path(&real, &work);
        let Ok(entries) = std::fs::read_dir(&path) else {
            return CallResult::error(ErrorCode::NotFound, format!("répertoire absent : {raw}"));
        };
        let mut items: Vec<Value> = entries
            .filter_map(Result::ok)
            .map(|entry| {
                let metadata = entry.metadata().ok();
                json!({
                    "name": entry.file_name().to_string_lossy(),
                    "kind": metadata.as_ref().map_or("inconnu", |m| if m.is_dir() { "répertoire" } else { "fichier" }),
                    "size": metadata.as_ref().map_or(0, std::fs::Metadata::len)
                })
            })
            .collect();
        items.sort_by_key(|v| v["name"].as_str().unwrap_or_default().to_owned());
        CallResult::structured(json!({"path": real.display().to_string(), "entries": items}))
    }
}

/// Métadonnées d'un fichier.
#[derive(Debug)]
pub struct Stat;

impl Tool for Stat {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.stat".into(),
            description: "Donne la taille, le type et la date de modification d'un fichier, sans lire son contenu.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
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
        let raw = object_string(args, "path")?;
        resolve(&raw, context).map(|(real, _)| real.display().to_string())
    }

    fn call(&self, args: &Value, context: &ToolContext) -> CallResult {
        let Some(raw) = object_string(args, "path") else {
            return CallResult::error(ErrorCode::Invalid, "argument `path` manquant");
        };
        let Some((real, work)) = resolve(&raw, context) else {
            return CallResult::error(
                ErrorCode::PolicyDenied,
                "chemin hors du répertoire personnel",
            );
        };
        let path = read_path(&real, &work);
        match std::fs::metadata(&path) {
            Ok(metadata) => CallResult::structured(json!({
                "path": real.display().to_string(),
                "kind": if metadata.is_dir() { "répertoire" } else { "fichier" },
                "size": metadata.len(),
                "readonly": metadata.permissions().readonly()
            })),
            Err(_) => CallResult::error(ErrorCode::NotFound, format!("chemin absent : {raw}")),
        }
    }
}

/// Recherche par nom et par contenu.
#[derive(Debug)]
pub struct Search;

impl Tool for Search {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.search".into(),
            description: "Cherche des fichiers par fragment de nom, et optionnellement par contenu. Rend au plus 200 résultats.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "root": {"type": "string"},
                    "name_contains": {"type": "string"},
                    "content_contains": {"type": "string"}
                },
                "required": ["root"],
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
        let raw = object_string(args, "root")?;
        resolve(&raw, context).map(|(real, _)| real.display().to_string())
    }

    fn call(&self, args: &Value, context: &ToolContext) -> CallResult {
        let Some(raw) = object_string(args, "root") else {
            return CallResult::error(ErrorCode::Invalid, "argument `root` manquant");
        };
        let Some((real, work)) = resolve(&raw, context) else {
            return CallResult::error(
                ErrorCode::PolicyDenied,
                "chemin hors du répertoire personnel",
            );
        };
        let root = read_path(&real, &work);
        let name_needle = object_string(args, "name_contains");
        let content_needle = object_string(args, "content_contains");

        let mut results = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                    continue;
                };
                if metadata.is_symlink() {
                    continue;
                }
                if metadata.is_dir() {
                    stack.push(path);
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if name_needle.as_ref().is_some_and(|n| !name.contains(n)) {
                    continue;
                }
                if let Some(needle) = &content_needle {
                    let Ok(content) = std::fs::read_to_string(&path) else {
                        continue;
                    };
                    if !content.contains(needle) {
                        continue;
                    }
                }
                results.push(json!({
                    "path": path.display().to_string(),
                    "size": metadata.len()
                }));
                if results.len() >= 200 {
                    return CallResult::structured(json!({"results": results, "truncated": true}));
                }
            }
        }
        results.sort_by_key(|v| v["path"].as_str().unwrap_or_default().to_owned());
        CallResult::structured(json!({"results": results, "truncated": false}))
    }
}
