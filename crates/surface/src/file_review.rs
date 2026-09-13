//! Lecture à la demande et comparaison des fichiers, hors de la boucle de dessin.
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use agentd::{ChangeKind, ChangeReview, PreviewContent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Equal,
    Removed,
    Added,
}

pub(crate) struct Line {
    pub(crate) before: Option<usize>,
    pub(crate) after: Option<usize>,
    pub(crate) kind: Kind,
    pub(crate) text: String,
}

pub(crate) struct Comparison {
    pub(crate) rows: Vec<Line>,
    pub(crate) grouped: bool,
}

/// Préfixe et suffixe communs conservés ; LCS bornée sur le milieu restant.
fn lines(before: &str, after: &str) -> Comparison {
    let old: Vec<_> = before.split_inclusive('\n').collect();
    let new: Vec<_> = after.split_inclusive('\n').collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old_end = old.len() - suffix;
    let new_end = new.len() - suffix;
    let old_count = old_end - prefix;
    let new_count = new_end - prefix;
    let grouped = old_count.saturating_mul(new_count) > 1_000_000;
    let mut rows = Vec::new();
    let mut add = |i: Option<usize>, j: Option<usize>| {
        rows.push(Line {
            before: i.map(|v| v + 1),
            after: j.map(|v| v + 1),
            kind: match (i, j) {
                (Some(_), Some(_)) => Kind::Equal,
                (Some(_), None) => Kind::Removed,
                _ => Kind::Added,
            },
            text: i.map_or_else(|| new[j.unwrap()], |v| old[v]).into(),
        });
    };
    for i in 0..prefix {
        add(Some(i), Some(i));
    }
    if grouped {
        for i in prefix..old_end {
            add(Some(i), None);
        }
        for j in prefix..new_end {
            add(None, Some(j));
        }
    } else {
        let columns = new_count + 1;
        let mut lengths = vec![0_u32; (old_count + 1) * columns];
        for i in (0..old_count).rev() {
            for j in (0..new_count).rev() {
                lengths[i * columns + j] = if old[prefix + i] == new[prefix + j] {
                    lengths[(i + 1) * columns + j + 1] + 1
                } else {
                    lengths[(i + 1) * columns + j].max(lengths[i * columns + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < old_count || j < new_count {
            if i < old_count && j < new_count && old[prefix + i] == new[prefix + j] {
                add(Some(prefix + i), Some(prefix + j));
                i += 1;
                j += 1;
            } else if i < old_count
                && (j == new_count
                    || lengths[(i + 1) * columns + j] >= lengths[i * columns + j + 1])
            {
                add(Some(prefix + i), None);
                i += 1;
            } else {
                add(None, Some(prefix + j));
                j += 1;
            }
        }
    }
    for i in 0..suffix {
        add(Some(old_end + i), Some(new_end + i));
    }
    Comparison { rows, grouped }
}

pub(crate) struct Document {
    pub(crate) review: ChangeReview,
    pub(crate) lines: Option<Comparison>,
}

fn document(value: serde_json::Value, task: &str, path: &str) -> Result<Document, String> {
    let review: ChangeReview =
        serde_json::from_value(value).map_err(|_| "Aperçu de fichier illisible.")?;
    if review.task != task || review.file.path != std::path::Path::new(path) {
        return Err("La réponse ne correspond pas au fichier demandé.".into());
    }
    if !matches!(
        (review.file.kind, &review.file.before, &review.file.after),
        (ChangeKind::Added, None, Some(_))
            | (ChangeKind::Deleted, Some(_), None)
            | (ChangeKind::Modified, Some(_), Some(_))
    ) {
        return Err("Versions incohérentes avec le changement annoncé.".into());
    }
    for version in [&review.file.before, &review.file.after]
        .into_iter()
        .flatten()
    {
        if let PreviewContent::Text { text } = &version.content
            && (text.len() > 65536 || text.len() as u64 != version.size || text.contains('\0'))
        {
            return Err("Taille ou contenu incohérent dans l'aperçu.".into());
        }
    }
    let text = |version: &Option<agentd::FilePreview>| match version {
        None => Some(String::new()),
        Some(v) => match &v.content {
            PreviewContent::Text { text } => Some(text.clone()),
            _ => None,
        },
    };
    let rows = text(&review.file.before)
        .zip(text(&review.file.after))
        .map(|(before, after)| lines(&before, &after));
    Ok(Document {
        review,
        lines: rows,
    })
}

type Reply = (u64, Result<Document, String>);

pub(crate) struct Review {
    task: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) document: Option<Document>,
    pub(crate) error: Option<String>,
    revision: u64,
    flight: Option<u64>,
    needed: bool,
    tx: Sender<Reply>,
    rx: Receiver<Reply>,
}

impl Default for Review {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            task: None,
            path: None,
            document: None,
            error: None,
            revision: 0,
            flight: None,
            needed: false,
            tx,
            rx,
        }
    }
}

impl Review {
    pub(crate) fn select(&mut self, task: Option<&str>) {
        if self.task.as_deref() != task {
            self.task = task.map(str::to_owned);
            self.revision = self.revision.wrapping_add(1);
            self.path = None;
            self.document = None;
            self.error = None;
            self.needed = false;
        }
    }
    pub(crate) fn open(&mut self, path: String) {
        self.revision = self.revision.wrapping_add(1);
        self.path = Some(path);
        self.document = None;
        self.error = None;
        self.needed = true;
    }
    pub(crate) fn loading(&self) -> bool {
        self.path.is_some() && (self.flight.is_some() || self.needed)
    }
    pub(crate) fn update(&mut self, socket: Option<&PathBuf>) {
        while let Ok((revision, reply)) = self.rx.try_recv() {
            if self.flight == Some(revision) {
                self.flight = None;
            }
            if revision == self.revision {
                match reply {
                    Ok(document) => {
                        self.document = Some(document);
                        self.error = None;
                    }
                    Err(error) => {
                        self.document = None;
                        self.error = Some(error);
                    }
                }
            }
        }
        if self.flight.is_none()
            && self.needed
            && let (Some(socket), Some(task), Some(path)) =
                (socket.cloned(), self.task.clone(), self.path.clone())
        {
            self.needed = false;
            self.flight = Some(self.revision);
            let revision = self.revision;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let result = crate::missions::rpc(
                    socket,
                    "task.change",
                    serde_json::json!({"id":task,"path":path}),
                )
                .and_then(|value| document(value, &task, &path));
                let _ = tx.send((revision, result));
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_reponse_ne_peut_pas_changer_la_mission_le_fichier_ou_les_versions() {
        let value = serde_json::json!({
            "task":"a", "file": {"path":"note.txt", "kind":"added", "before":null,
                "after":{"hash":"empreinte", "size":2, "mode":33152,
                    "content":{"kind":"text", "text":"é"}}}
        });
        assert!(document(value.clone(), "a", "note.txt").is_ok());
        assert!(document(value.clone(), "b", "note.txt").is_err());
        assert!(document(value.clone(), "a", "autre.txt").is_err());
        let mut missing = value.clone();
        missing["file"]["after"] = serde_json::Value::Null;
        assert!(document(missing, "a", "note.txt").is_err());
        let mut wrong_kind = value.clone();
        wrong_kind["file"]["kind"] = "modified".into();
        assert!(document(wrong_kind, "a", "note.txt").is_err());
        let mut truncated = value;
        truncated["file"]["after"]["size"] = 1.into();
        assert!(document(truncated, "a", "note.txt").is_err());
    }

    #[test]
    fn un_long_document_conserve_ses_lignes_communes() {
        let prefix = "Ligne conservée\n".repeat(1200);
        let suffix = "Suite conservée\n".repeat(1200);
        let before = format!("{prefix}Ancienne phrase\n{suffix}");
        let after = format!("{prefix}Nouvelle phrase\n{suffix}");
        let comparison = lines(&before, &after);
        assert!(!comparison.grouped);
        let rows = comparison.rows;
        assert_eq!(rows.iter().filter(|r| r.kind == Kind::Equal).count(), 2400);
        assert_eq!(rows.iter().filter(|r| r.kind == Kind::Added).count(), 1);
        assert_eq!(rows.iter().filter(|r| r.kind == Kind::Removed).count(), 1);
        assert!(
            lines(&before, &before)
                .rows
                .iter()
                .all(|r| r.kind == Kind::Equal)
        );
    }

    #[test]
    fn les_lignes_reconstituent_les_deux_textes_sans_perdre_les_espaces() {
        for (before, after) in [
            ("a\na\nb\n", "a\nb\na\n"),
            ("é\r\n\ttexte\n", "é\n  texte"),
            ("", "ajout"),
            ("suppression", ""),
        ] {
            let rows = lines(before, after).rows;
            assert_eq!(
                rows.iter()
                    .filter(|r| r.before.is_some())
                    .map(|r| r.text.as_str())
                    .collect::<String>(),
                before
            );
            assert_eq!(
                rows.iter()
                    .filter(|r| r.after.is_some())
                    .map(|r| r.text.as_str())
                    .collect::<String>(),
                after
            );
        }
        let before = "a\n".repeat(1200);
        let after = "b\n".repeat(1200);
        let comparison = lines(&before, &after);
        assert!(comparison.grouped);
        let rows = comparison.rows;
        assert_eq!(rows.len(), 2400);
        assert_eq!(
            rows.iter()
                .filter(|r| r.after.is_some())
                .map(|r| r.text.as_str())
                .collect::<String>(),
            after
        );
    }

    #[test]
    fn une_ancienne_lecture_ne_remplace_pas_la_nouvelle_selection() {
        let mut view = Review::default();
        view.select(Some("a"));
        view.open("one.txt".into());
        let previous = view.revision;
        view.flight = Some(previous);
        view.select(Some("b"));
        view.open("two.txt".into());
        view.tx
            .send((previous, Err("Ancienne erreur".into())))
            .unwrap();
        view.update(None);
        assert_eq!(view.path.as_deref(), Some("two.txt"));
        assert!(view.error.is_none());
        assert!(view.document.is_none());
        assert!(view.flight.is_none());
        assert!(view.needed);
    }
}
