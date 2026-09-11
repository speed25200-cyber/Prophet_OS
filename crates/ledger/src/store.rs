//! Stockage : fichiers JSONL en ajout seul, un par jour, plus un index en mémoire persisté.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead as _, BufReader, BufWriter, Write as _};
use std::path::{Path, PathBuf};

use prophet_types::ledger::{Draft, Event, EventError, EventKind, GENESIS, verify_chain};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::seal::Sealer;

/// Erreur du journal.
#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    /// Erreur d'entrée-sortie.
    #[error("erreur d'entrée-sortie : {0}")]
    Io(#[from] std::io::Error),
    /// Erreur de format d'événement.
    #[error(transparent)]
    Event(#[from] EventError),
    /// Ligne illisible dans un fichier de journal.
    #[error("ligne illisible dans {file} à la ligne {line} : {source}")]
    BadLine {
        /// Fichier concerné.
        file: String,
        /// Numéro de ligne (à partir de 1).
        line: usize,
        /// Cause.
        source: serde_json::Error,
    },
    /// Sérialisation impossible.
    #[error("sérialisation impossible : {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Critères de sélection d'événements.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Restreint à une tâche.
    pub task: Option<String>,
    /// Restreint à des types d'événements.
    pub kinds: Vec<EventKind>,
    /// Borne inférieure de séquence, incluse.
    pub since_seq: Option<u64>,
    /// Borne supérieure de séquence, incluse.
    pub until_seq: Option<u64>,
    /// Nombre maximal de résultats.
    pub limit: Option<usize>,
}

impl Filter {
    /// Vrai si l'événement satisfait le filtre (hors limite de nombre).
    #[must_use]
    pub fn accepts(&self, event: &Event) -> bool {
        if let Some(task) = &self.task
            && event.task.as_deref() != Some(task.as_str())
        {
            return false;
        }
        if !self.kinds.is_empty() && !self.kinds.contains(&event.kind) {
            return false;
        }
        if self.since_seq.is_some_and(|s| event.seq < s) {
            return false;
        }
        if self.until_seq.is_some_and(|s| event.seq > s) {
            return false;
        }
        true
    }
}

/// Résultat d'une vérification d'intégrité.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    /// Vrai si la chaîne et les sceaux sont intacts.
    pub ok: bool,
    /// Nombre d'événements examinés.
    pub checked: u64,
    /// Nombre de sceaux vérifiés.
    pub seals: u64,
    /// Première séquence fautive.
    pub first_bad_seq: Option<u64>,
    /// Explication de l'écart.
    pub reason: Option<String>,
}

/// Journal sur disque.
#[derive(Debug)]
pub struct Store {
    root: PathBuf,
    next_seq: u64,
    last_hash: String,
    /// Index `seq -> (fichier, offset de ligne)`, reconstruit au démarrage.
    index: BTreeMap<u64, (PathBuf, usize)>,
    since_last_seal: u64,
    sealer: Option<Sealer>,
}

impl Store {
    /// Ouvre ou crée un journal dans `root`, en reconstruisant l'index et en vérifiant la chaîne.
    ///
    /// # Erreurs
    /// Si le répertoire est illisible ou si la chaîne existante est rompue.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, LedgerError> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;
        let mut store = Self {
            root,
            next_seq: 0,
            last_hash: GENESIS.to_owned(),
            index: BTreeMap::new(),
            since_last_seal: 0,
            sealer: None,
        };
        store.rebuild_index()?;
        Ok(store)
    }

    /// Attache un signataire pour le scellement périodique.
    #[must_use]
    pub fn with_sealer(mut self, sealer: Sealer) -> Self {
        self.sealer = Some(sealer);
        self
    }

    /// Prochaine séquence qui sera attribuée.
    #[must_use]
    pub const fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// Empreinte du dernier événement écrit.
    #[must_use]
    pub fn last_hash(&self) -> &str {
        &self.last_hash
    }

    fn day_files(&self) -> Result<Vec<PathBuf>, LedgerError> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&self.root)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
            .collect();
        files.sort();
        Ok(files)
    }

    fn rebuild_index(&mut self) -> Result<(), LedgerError> {
        for path in self.day_files()? {
            let file = File::open(&path)?;
            for (line_number, line) in BufReader::new(file).lines().enumerate() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                let event: Event =
                    serde_json::from_str(&line).map_err(|source| LedgerError::BadLine {
                        file: path.display().to_string(),
                        line: line_number + 1,
                        source,
                    })?;
                self.index.insert(event.seq, (path.clone(), line_number));
                self.next_seq = event.seq + 1;
                self.last_hash = event.hash.clone().unwrap_or_else(|| GENESIS.to_owned());
                if event.kind == EventKind::LedgerSeal {
                    self.since_last_seal = 0;
                } else {
                    self.since_last_seal += 1;
                }
            }
        }
        Ok(())
    }

    fn path_for(&self, ts: OffsetDateTime) -> PathBuf {
        let date = ts.date();
        self.root.join(format!(
            "{:04}-{:02}-{:02}.jsonl",
            date.year(),
            u8::from(date.month()),
            date.day()
        ))
    }

    /// Écrit un événement et renvoie sa forme scellée.
    ///
    /// # Erreurs
    /// Si la charge utile est refusée ou si l'écriture échoue.
    pub fn append(&mut self, draft: Draft) -> Result<Event, LedgerError> {
        let ts = draft.ts;
        let event = Event::seal(draft, self.next_seq, &self.last_hash)?;
        self.write_line(&event, ts)?;
        self.next_seq += 1;
        self.last_hash = event.hash.clone().unwrap_or_else(|| GENESIS.to_owned());
        self.since_last_seal += 1;
        Ok(event)
    }

    fn write_line(&mut self, event: &Event, ts: OffsetDateTime) -> Result<(), LedgerError> {
        let path = self.path_for(ts);
        let existing_lines = if path.exists() {
            BufReader::new(File::open(&path)?).lines().count()
        } else {
            0
        };
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(&mut writer, event)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        self.index.insert(event.seq, (path, existing_lines));
        Ok(())
    }

    /// Écrit un lot d'événements.
    ///
    /// # Erreurs
    /// Comme [`Store::append`]. Les événements déjà écrits le restent.
    pub fn append_batch(&mut self, drafts: Vec<Draft>) -> Result<Vec<Event>, LedgerError> {
        drafts.into_iter().map(|d| self.append(d)).collect()
    }

    /// Scelle la chaîne si le seuil d'événements est atteint, ou si `force`.
    ///
    /// Retourne l'événement de scellement écrit, le cas échéant.
    ///
    /// # Erreurs
    /// Si l'écriture échoue.
    pub fn maybe_seal(
        &mut self,
        now: OffsetDateTime,
        threshold: u64,
        force: bool,
    ) -> Result<Option<Event>, LedgerError> {
        if self.sealer.is_none() || (!force && self.since_last_seal < threshold) {
            return Ok(None);
        }
        let Some(sealer) = &self.sealer else {
            return Ok(None);
        };
        let seal = sealer.seal(self.next_seq.saturating_sub(1), &self.last_hash);
        let draft = Draft::new(
            now,
            prophet_types::ledger::Actor::daemon("ledger"),
            EventKind::LedgerSeal,
            serde_json::to_value(&seal)?,
        );
        let event = self.append(draft)?;
        self.since_last_seal = 0;
        Ok(Some(event))
    }

    /// Lit tous les événements du journal, dans l'ordre.
    ///
    /// # Erreurs
    /// Si un fichier est illisible ou une ligne malformée.
    pub fn read_all(&self) -> Result<Vec<Event>, LedgerError> {
        let mut events = Vec::with_capacity(self.index.len());
        for path in self.day_files()? {
            let file = File::open(&path)?;
            for (line_number, line) in BufReader::new(file).lines().enumerate() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                events.push(serde_json::from_str(&line).map_err(|source| {
                    LedgerError::BadLine {
                        file: path.display().to_string(),
                        line: line_number + 1,
                        source,
                    }
                })?);
            }
        }
        events.sort_by_key(|e: &Event| e.seq);
        Ok(events)
    }

    /// Interroge le journal.
    ///
    /// # Erreurs
    /// Comme [`Store::read_all`].
    pub fn query(&self, filter: &Filter) -> Result<Vec<Event>, LedgerError> {
        let mut out: Vec<Event> = self
            .read_all()?
            .into_iter()
            .filter(|e| filter.accepts(e))
            .collect();
        if let Some(limit) = filter.limit {
            out.truncate(limit);
        }
        Ok(out)
    }

    /// Vérifie la chaîne et les sceaux.
    ///
    /// # Erreurs
    /// Si le journal est illisible.
    pub fn verify(&self) -> Result<VerifyReport, LedgerError> {
        let events = self.read_all()?;
        let checked = events.len() as u64;
        if let Err(error) = verify_chain(&events) {
            let (seq, reason) = match &error {
                EventError::BrokenChain { seq, reason } => (Some(*seq), reason.clone()),
                other => (None, other.to_string()),
            };
            return Ok(VerifyReport {
                ok: false,
                checked,
                seals: 0,
                first_bad_seq: seq,
                reason: Some(reason),
            });
        }
        let mut seals = 0;
        if let Some(sealer) = &self.sealer {
            for event in &events {
                if event.kind != EventKind::LedgerSeal {
                    continue;
                }
                let seal: crate::seal::Seal = serde_json::from_value(event.payload.clone())?;
                if !sealer.verify(&seal) {
                    return Ok(VerifyReport {
                        ok: false,
                        checked,
                        seals,
                        first_bad_seq: Some(event.seq),
                        reason: Some("sceau invalide".to_owned()),
                    });
                }
                seals += 1;
            }
        }
        Ok(VerifyReport {
            ok: true,
            checked,
            seals,
            first_bad_seq: None,
            reason: None,
        })
    }

    /// Résumé lisible d'une tâche, pour `prophet log replay`.
    ///
    /// # Erreurs
    /// Comme [`Store::query`].
    pub fn replay(&self, task: &str) -> Result<String, LedgerError> {
        let events = self.query(&Filter {
            task: Some(task.to_owned()),
            ..Filter::default()
        })?;
        let mut out = String::new();
        for event in &events {
            let ts = event.ts.format(&Rfc3339).unwrap_or_default();
            let step = event
                .step
                .map_or_else(|| "   ".to_owned(), |s| format!("{s:>3}"));
            let kind = serde_json::to_string(&event.kind)
                .unwrap_or_default()
                .trim_matches('"')
                .to_owned();
            let detail = summarize(event);
            out.push_str(&format!(
                "{ts}  étape {step}  {:<22} {} {detail}\n",
                kind, event.actor.0
            ));
        }
        if events.is_empty() {
            out.push_str("aucun événement pour cette tâche\n");
        }
        Ok(out)
    }
}

fn summarize(event: &Event) -> String {
    let p = &event.payload;
    match event.kind {
        EventKind::ToolCall => p["tool"].as_str().unwrap_or("?").to_owned(),
        EventKind::ToolResult => format!(
            "{} {}",
            p["tool"].as_str().unwrap_or("?"),
            if p["ok"].as_bool().unwrap_or(false) {
                "ok"
            } else {
                "échec"
            }
        ),
        EventKind::PolicyDeny => format!(
            "{}/{} {}",
            p["res"].as_str().unwrap_or("?"),
            p["act"].as_str().unwrap_or("?"),
            p["reason"].as_str().unwrap_or("")
        ),
        EventKind::NetRequest | EventKind::NetDeny => p["host"].as_str().unwrap_or("?").to_owned(),
        EventKind::FsCommit | EventKind::FsUndo => {
            format!("{} fichiers", p["files"]["modified"].as_u64().unwrap_or(0))
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prophet_types::ledger::Actor;
    use serde_json::json;

    fn now(offset: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_789_000_000 + offset).unwrap()
    }

    fn draft(offset: i64, task: &str) -> Draft {
        Draft::new(
            now(offset),
            Actor::daemon("agentd"),
            EventKind::ToolCall,
            json!({"tool": "fs.read", "args_digest": "blake3:aa"}),
        )
        .task(task)
    }

    #[test]
    fn ecriture_relecture_et_verification() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        for i in 0..100 {
            store.append(draft(i, "task:01")).unwrap();
        }
        assert_eq!(store.next_seq(), 100);
        let report = store.verify().unwrap();
        assert!(report.ok, "{report:?}");
        assert_eq!(report.checked, 100);
    }

    #[test]
    fn reouverture_reprend_la_chaine() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = Store::open(dir.path()).unwrap();
            for i in 0..10 {
                store.append(draft(i, "task:01")).unwrap();
            }
        }
        let mut store = Store::open(dir.path()).unwrap();
        assert_eq!(store.next_seq(), 10);
        store.append(draft(11, "task:01")).unwrap();
        assert!(store.verify().unwrap().ok);
    }

    #[test]
    fn alteration_d_une_ligne_detectee() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        for i in 0..20 {
            store.append(draft(i, "task:01")).unwrap();
        }
        // Modifie une ligne sur le disque, comme le ferait un attaquant avec les droits root.
        let path = store.day_files().unwrap()[0].clone();
        let content = std::fs::read_to_string(&path).unwrap();
        let mut lines: Vec<String> = content.lines().map(ToOwned::to_owned).collect();
        lines[7] = lines[7].replace("fs.read", "fs.write");
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();

        let report = Store::open(dir.path()).unwrap().verify().unwrap();
        assert!(!report.ok);
        assert_eq!(report.first_bad_seq, Some(7));
    }

    #[test]
    fn suppression_d_une_ligne_detectee() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        for i in 0..20 {
            store.append(draft(i, "task:01")).unwrap();
        }
        let path = store.day_files().unwrap()[0].clone();
        let content = std::fs::read_to_string(&path).unwrap();
        let mut lines: Vec<String> = content.lines().map(ToOwned::to_owned).collect();
        lines.remove(10);
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();

        let report = Store::open(dir.path()).unwrap().verify().unwrap();
        assert!(!report.ok);
        assert_eq!(report.first_bad_seq, Some(11));
    }

    #[test]
    fn filtrage_par_tache_et_par_type() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.append(draft(0, "task:01")).unwrap();
        store.append(draft(1, "task:02")).unwrap();
        store
            .append(
                Draft::new(
                    now(2),
                    Actor::daemon("capd"),
                    EventKind::PolicyDeny,
                    json!({"res": "fs", "act": "read", "reason": "no_grant"}),
                )
                .task("task:01"),
            )
            .unwrap();

        let par_tache = store
            .query(&Filter {
                task: Some("task:01".into()),
                ..Filter::default()
            })
            .unwrap();
        assert_eq!(par_tache.len(), 2);

        let par_type = store
            .query(&Filter {
                kinds: vec![EventKind::PolicyDeny],
                ..Filter::default()
            })
            .unwrap();
        assert_eq!(par_type.len(), 1);

        let limite = store
            .query(&Filter {
                limit: Some(1),
                ..Filter::default()
            })
            .unwrap();
        assert_eq!(limite.len(), 1);
    }

    #[test]
    fn rejeu_lisible() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.append(draft(0, "task:01")).unwrap();
        let texte = store.replay("task:01").unwrap();
        assert!(texte.contains("tool.call"), "{texte}");
        assert!(texte.contains("fs.read"), "{texte}");
        assert!(store.replay("task:99").unwrap().contains("aucun événement"));
    }

    #[test]
    fn changement_de_jour_cree_un_nouveau_fichier_sans_rompre_la_chaine() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.append(draft(0, "task:01")).unwrap();
        store.append(draft(86_400 * 2, "task:01")).unwrap();
        assert_eq!(store.day_files().unwrap().len(), 2);
        assert!(store.verify().unwrap().ok);
    }
}
