//! Magasin de mémoire.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::embed::{Embedder, cosine, tokenize};

/// Erreur de la mémoire.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    /// Erreur de base de données.
    #[error("base de mémoire : {0}")]
    Database(#[from] rusqlite::Error),
    /// Espace inconnu ou non autorisé.
    #[error("espace non autorisé : {0}")]
    Forbidden(String),
    /// Entrée inconnue.
    #[error("entrée inconnue : {0}")]
    Unknown(String),
    /// Sérialisation.
    #[error("sérialisation : {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Espace de mémoire. Le cloisonnement est le premier mécanisme de confidentialité de la mémoire.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Space(pub String);

impl Space {
    /// Espace nommé.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Espace de travail par défaut.
    #[must_use]
    pub fn work() -> Self {
        Self::new("work")
    }
}

impl std::fmt::Display for Space {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Nature d'une entrée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Un fait sur l'utilisateur ou sa machine.
    Fact,
    /// Le résumé d'une tâche passée.
    Episode,
    /// Une préférence exprimée.
    Preference,
}

/// Une entrée de mémoire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Identifiant.
    pub id: String,
    /// Espace.
    pub space: Space,
    /// Nature.
    pub kind: Kind,
    /// Contenu.
    pub text: String,
    /// Étiquettes.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Tâche à l'origine, s'il y en a une.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_task: Option<String>,
    /// Confiance, de 0 à 1.
    pub confidence: f32,
    /// Date d'enregistrement.
    #[serde(with = "time::serde::rfc3339")]
    pub created: OffsetDateTime,
    /// Pertinence, remplie par une recherche.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
}

/// Ce qu'il faut fournir pour enregistrer une entrée.
///
/// Regrouper ces champs donne un nom à ce qu'une entrée doit porter, et empêche d'intervertir
/// deux valeurs de même type, en particulier le texte et la tâche d'origine.
#[derive(Debug, Clone)]
pub struct NewEntry<'a> {
    /// Espace de destination.
    pub space: &'a Space,
    /// Nature.
    pub kind: Kind,
    /// Contenu.
    pub text: &'a str,
    /// Étiquettes.
    pub tags: &'a [String],
    /// Tâche à l'origine.
    pub source_task: Option<&'a str>,
    /// Confiance, de 0 à 1.
    pub confidence: f32,
}

impl<'a> NewEntry<'a> {
    /// Fait appris, sans étiquette ni provenance.
    #[must_use]
    pub const fn fact(space: &'a Space, text: &'a str) -> Self {
        Self {
            space,
            kind: Kind::Fact,
            text,
            tags: &[],
            source_task: None,
            confidence: 1.0,
        }
    }

    /// Rattache l'entrée à une tâche.
    #[must_use]
    pub const fn from_task(mut self, task: &'a str) -> Self {
        self.source_task = Some(task);
        self
    }

    /// Fixe la confiance.
    #[must_use]
    pub const fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }
}

/// Critères de recherche.
#[derive(Debug, Clone, Default)]
pub struct Query {
    /// Espaces à interroger. Vide signifie « aucun », jamais « tous ».
    pub spaces: Vec<Space>,
    /// Texte recherché.
    pub text: String,
    /// Nombre maximal de résultats.
    pub limit: usize,
    /// Pertinence minimale.
    pub min_score: f32,
}

impl Query {
    /// Recherche dans un espace.
    #[must_use]
    pub fn in_space(space: Space, text: impl Into<String>) -> Self {
        Self {
            spaces: vec![space],
            text: text.into(),
            limit: 10,
            min_score: 0.0,
        }
    }
}

/// Magasin de mémoire, adossé à SQLite.
pub struct Store {
    connection: Connection,
    embedder: Box<dyn Embedder>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("embedder", &self.embedder.name())
            .finish_non_exhaustive()
    }
}

impl Store {
    /// Ouvre ou crée un magasin.
    ///
    /// # Errors
    /// Si la base est inaccessible.
    pub fn open(path: &std::path::Path, embedder: Box<dyn Embedder>) -> Result<Self, MemoryError> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let connection = Connection::open(path)?;
        Self::from_connection(connection, embedder)
    }

    /// Magasin en mémoire, pour les tests.
    ///
    /// # Errors
    /// Si la base ne peut pas être créée.
    pub fn in_memory(embedder: Box<dyn Embedder>) -> Result<Self, MemoryError> {
        Self::from_connection(Connection::open_in_memory()?, embedder)
    }

    fn from_connection(
        connection: Connection,
        embedder: Box<dyn Embedder>,
    ) -> Result<Self, MemoryError> {
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS entries (
                id TEXT PRIMARY KEY,
                space TEXT NOT NULL,
                kind TEXT NOT NULL,
                text TEXT NOT NULL,
                tags TEXT NOT NULL,
                source_task TEXT,
                confidence REAL NOT NULL,
                created TEXT NOT NULL,
                vector BLOB NOT NULL,
                embedder TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS entries_space ON entries(space);",
        )?;
        Ok(Self {
            connection,
            embedder,
        })
    }

    /// Enregistre une entrée et rend son identifiant.
    ///
    /// # Errors
    /// Si l'écriture échoue.
    pub fn remember(
        &self,
        entry: &NewEntry<'_>,
        now: OffsetDateTime,
    ) -> Result<String, MemoryError> {
        let NewEntry {
            space,
            kind,
            text,
            tags,
            source_task,
            confidence,
        } = *entry;
        // L'identifiant ne dérive pas du contenu : deux faits identiques enregistrés au même
        // instant sont deux entrées distinctes, et une horloge grossière ne doit pas les faire
        // entrer en collision.
        let id = prophet_types::ids::Id::new(prophet_types::ids::Kind::Memory).to_string();
        let vector = self.embedder.embed(text);
        let bytes: Vec<u8> = vector.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.connection.execute(
            "INSERT INTO entries (id, space, kind, text, tags, source_task, confidence, created, vector, embedder)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id,
                space.0,
                serde_json::to_string(&kind)?,
                text,
                serde_json::to_string(tags)?,
                source_task,
                f64::from(confidence),
                now.format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default(),
                bytes,
                self.embedder.name(),
            ],
        )?;
        Ok(id)
    }

    /// Cherche dans les espaces autorisés.
    ///
    /// La recherche combine similarité vectorielle et présence des mots : un terme rare et exact
    /// doit ressortir même quand le vecteur le dilue.
    ///
    /// # Errors
    /// Si la lecture échoue.
    pub fn search(&self, query: &Query) -> Result<Vec<Entry>, MemoryError> {
        if query.spaces.is_empty() {
            return Ok(Vec::new());
        }
        let cible = self.embedder.embed(&query.text);
        let mots = tokenize(&query.text);
        let mut resultats = Vec::new();

        for space in &query.spaces {
            let mut statement = self.connection.prepare(
                "SELECT id, space, kind, text, tags, source_task, confidence, created, vector, embedder
                 FROM entries WHERE space = ?1",
            )?;
            let rows = statement.query_map(params![space.0], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, f64>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Vec<u8>>(8)?,
                    row.get::<_, String>(9)?,
                ))
            })?;

            for row in rows {
                let (
                    id,
                    space_name,
                    kind,
                    text,
                    tags,
                    source_task,
                    confidence,
                    created,
                    vector,
                    embedder,
                ) = row?;
                // Un vecteur produit par un autre modèle n'est pas comparable : on le dit plutôt
                // que de rendre un score qui n'a pas de sens.
                let vectoriel = if embedder == self.embedder.name() {
                    let floats: Vec<f32> = vector
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|c| f32::from_le_bytes(*c))
                        .collect();
                    cosine(&cible, &floats)
                } else {
                    0.0
                };
                let lexical = if mots.is_empty() {
                    0.0
                } else {
                    let bas = text.to_lowercase();
                    let trouves = mots.iter().filter(|m| bas.contains(m.as_str())).count();
                    trouves as f32 / mots.len() as f32
                };
                let score = 0.5 * vectoriel.max(0.0) + 0.5 * lexical;
                if score < query.min_score {
                    continue;
                }
                resultats.push(Entry {
                    id,
                    space: Space::new(space_name),
                    kind: serde_json::from_str(&kind)?,
                    text,
                    tags: serde_json::from_str(&tags)?,
                    source_task,
                    confidence: confidence as f32,
                    created: OffsetDateTime::parse(
                        &created,
                        &time::format_description::well_known::Rfc3339,
                    )
                    .unwrap_or(OffsetDateTime::UNIX_EPOCH),
                    score: Some(score),
                });
            }
        }
        resultats.sort_by(|a, b| {
            b.score
                .unwrap_or(0.0)
                .partial_cmp(&a.score.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        resultats.truncate(query.limit.max(1));
        Ok(resultats)
    }

    /// Liste toutes les entrées d'un espace, les plus récentes d'abord.
    ///
    /// # Errors
    /// Si la lecture échoue.
    pub fn list(&self, space: &Space) -> Result<Vec<Entry>, MemoryError> {
        let query = Query {
            spaces: vec![space.clone()],
            text: String::new(),
            limit: usize::MAX,
            min_score: -1.0,
        };
        let mut entries = self.search(&query)?;
        entries.sort_by_key(|a| std::cmp::Reverse(a.created));
        Ok(entries)
    }

    /// Oublie une entrée.
    ///
    /// # Errors
    /// Si la suppression échoue.
    pub fn forget(&self, id: &str) -> Result<bool, MemoryError> {
        let changed = self
            .connection
            .execute("DELETE FROM entries WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }

    /// Oublie tout un espace.
    ///
    /// # Errors
    /// Si la suppression échoue.
    pub fn forget_space(&self, space: &Space) -> Result<usize, MemoryError> {
        Ok(self
            .connection
            .execute("DELETE FROM entries WHERE space = ?1", params![space.0])?)
    }

    /// Nombre d'entrées d'un espace.
    ///
    /// # Errors
    /// Si la lecture échoue.
    pub fn count(&self, space: &Space) -> Result<usize, MemoryError> {
        Ok(self.connection.query_row(
            "SELECT COUNT(*) FROM entries WHERE space = ?1",
            params![space.0],
            |row| row.get::<_, i64>(0),
        )? as usize)
    }

    /// Espaces existants.
    ///
    /// # Errors
    /// Si la lecture échoue.
    pub fn spaces(&self) -> Result<Vec<Space>, MemoryError> {
        let mut statement = self
            .connection
            .prepare("SELECT DISTINCT space FROM entries ORDER BY space")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        Ok(rows.filter_map(Result::ok).map(Space::new).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::HashEmbedder;

    fn now(offset: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_789_000_000 + offset).unwrap()
    }

    fn magasin() -> Store {
        Store::in_memory(Box::new(HashEmbedder::default())).unwrap()
    }

    fn retenir(store: &Store, space: &str, text: &str, offset: i64) -> String {
        let space = Space::new(space);
        store
            .remember(
                &NewEntry::fact(&space, text)
                    .from_task("task:01")
                    .with_confidence(0.9),
                now(offset),
            )
            .unwrap()
    }

    #[test]
    fn retrouver_ce_qui_a_ete_appris() {
        let s = magasin();
        retenir(&s, "work", "les factures sont dans le dossier compta", 0);
        retenir(
            &s,
            "work",
            "le rapport de ventes se fait au format PDF A4",
            1,
        );
        retenir(&s, "work", "la réunion hebdomadaire a lieu le mardi", 2);

        let resultats = s
            .search(&Query::in_space(Space::work(), "où sont les factures"))
            .unwrap();
        assert_eq!(
            resultats[0].text,
            "les factures sont dans le dossier compta"
        );
        assert!(resultats[0].score.unwrap() > 0.0);
    }

    #[test]
    fn un_espace_ne_voit_pas_les_autres() {
        let s = magasin();
        retenir(&s, "work", "le client principal est Exemple SA", 0);
        retenir(&s, "personal", "le code de l'alarme est dans le carnet", 1);

        let travail = s
            .search(&Query::in_space(Space::work(), "alarme carnet code"))
            .unwrap();
        assert!(
            travail.is_empty() || !travail[0].text.contains("alarme"),
            "la mémoire personnelle ne doit pas fuir vers l'espace de travail : {travail:?}"
        );
        assert_eq!(s.count(&Space::new("personal")).unwrap(), 1);
    }

    #[test]
    fn une_recherche_sans_espace_ne_rend_rien() {
        let s = magasin();
        retenir(&s, "work", "quelque chose", 0);
        let resultats = s
            .search(&Query {
                spaces: Vec::new(),
                text: "quelque chose".into(),
                limit: 10,
                min_score: 0.0,
            })
            .unwrap();
        assert!(
            resultats.is_empty(),
            "aucun espace demandé signifie aucun accès, jamais un accès à tout"
        );
    }

    #[test]
    fn recherche_sur_plusieurs_espaces_autorises() {
        let s = magasin();
        retenir(&s, "work", "le rapport trimestriel est attendu le 5", 0);
        retenir(&s, "projet-x", "le rapport du projet X sort le 12", 1);
        let resultats = s
            .search(&Query {
                spaces: vec![Space::work(), Space::new("projet-x")],
                text: "rapport".into(),
                limit: 10,
                min_score: 0.0,
            })
            .unwrap();
        assert_eq!(resultats.len(), 2);
    }

    #[test]
    fn un_terme_exact_ressort_meme_si_le_vecteur_le_dilue() {
        let s = magasin();
        retenir(
            &s,
            "work",
            "référence du contrat : ZX-99417 signé en mars",
            0,
        );
        for i in 0..20 {
            retenir(
                &s,
                "work",
                &format!("note quelconque numéro {i} sans rapport"),
                i + 1,
            );
        }
        let resultats = s
            .search(&Query::in_space(Space::work(), "ZX-99417"))
            .unwrap();
        assert!(resultats[0].text.contains("ZX-99417"), "{:?}", resultats[0]);
    }

    #[test]
    fn rappel_sur_un_jeu_de_faits() {
        let s = magasin();
        let faits = [
            (
                "les sauvegardes tournent chaque nuit à deux heures",
                "quand tournent les sauvegardes",
            ),
            (
                "le serveur de production s'appelle atlas",
                "comment s'appelle le serveur de production",
            ),
            (
                "les notes de frais se rendent avant le dix du mois",
                "date limite des notes de frais",
            ),
            (
                "le mot de passe du wifi est chez le gardien",
                "où trouver le wifi",
            ),
            (
                "les réunions d'équipe durent trente minutes",
                "durée des réunions d'équipe",
            ),
        ];
        for (index, (fait, _)) in faits.iter().enumerate() {
            retenir(&s, "work", fait, index as i64);
        }
        // Du bruit, pour que la recherche ait un vrai travail à faire.
        for i in 0..50 {
            retenir(
                &s,
                "work",
                &format!("information sans importance numéro {i}"),
                100 + i,
            );
        }

        let mut trouves = 0;
        for (fait, question) in &faits {
            let resultats = s
                .search(&Query::in_space(Space::work(), *question))
                .unwrap();
            if resultats.iter().take(3).any(|e| &e.text == fait) {
                trouves += 1;
            }
        }
        assert!(
            trouves >= 4,
            "rappel insuffisant : {trouves} sur {} dans les trois premiers",
            faits.len()
        );
    }

    #[test]
    fn l_humain_peut_oublier() {
        let s = magasin();
        let id = retenir(&s, "work", "un fait devenu faux", 0);
        assert!(s.forget(&id).unwrap());
        assert!(!s.forget(&id).unwrap());
        assert_eq!(s.count(&Space::work()).unwrap(), 0);
    }

    #[test]
    fn l_humain_peut_vider_un_espace_entier() {
        let s = magasin();
        for i in 0..5 {
            retenir(&s, "personal", &format!("note privée {i}"), i);
        }
        retenir(&s, "work", "note de travail", 10);
        assert_eq!(s.forget_space(&Space::new("personal")).unwrap(), 5);
        assert_eq!(s.count(&Space::new("personal")).unwrap(), 0);
        assert_eq!(s.count(&Space::work()).unwrap(), 1);
    }

    #[test]
    fn la_provenance_est_conservee() {
        let s = magasin();
        retenir(&s, "work", "un fait appris pendant une tâche", 0);
        let entries = s.list(&Space::work()).unwrap();
        assert_eq!(entries[0].source_task.as_deref(), Some("task:01"));
        assert!((entries[0].confidence - 0.9).abs() < 1e-5);
        assert_eq!(entries[0].created, now(0));
    }

    #[test]
    fn la_liste_est_du_plus_recent_au_plus_ancien() {
        let s = magasin();
        retenir(&s, "work", "le plus ancien", 0);
        retenir(&s, "work", "le plus récent", 100);
        let entries = s.list(&Space::work()).unwrap();
        assert_eq!(entries[0].text, "le plus récent");
    }

    #[test]
    fn les_espaces_existants_sont_listables() {
        let s = magasin();
        retenir(&s, "work", "x", 0);
        retenir(&s, "personal", "y", 1);
        assert_eq!(
            s.spaces().unwrap(),
            vec![Space::new("personal"), Space::work()]
        );
    }

    #[test]
    fn un_vecteur_d_un_autre_modele_ne_produit_pas_de_score_fantaisiste() {
        let dir = tempfile::tempdir().unwrap();
        let chemin = dir.path().join("memoire.db");
        {
            let s = Store::open(&chemin, Box::new(HashEmbedder::new(64))).unwrap();
            let espace = Space::work();
            s.remember(
                &NewEntry::fact(&espace, "un fait enregistré avec un petit modèle"),
                now(0),
            )
            .unwrap();
        }
        // Réouverture avec un autre modèle : la similarité vectorielle est neutralisée, seule la
        // correspondance lexicale subsiste.
        let s = Store::open(&chemin, Box::new(HashEmbedder::new(256))).unwrap();
        let resultats = s
            .search(&Query::in_space(Space::work(), "un fait enregistré"))
            .unwrap();
        assert_eq!(resultats.len(), 1);
        assert!(resultats[0].score.unwrap() <= 0.5 + 1e-5);
    }

    #[test]
    fn persistance_sur_disque() {
        let dir = tempfile::tempdir().unwrap();
        let chemin = dir.path().join("memoire.db");
        {
            let s = Store::open(&chemin, Box::new(HashEmbedder::default())).unwrap();
            let espace = Space::work();
            s.remember(
                &NewEntry {
                    kind: Kind::Episode,
                    ..NewEntry::fact(&espace, "une tâche passée")
                },
                now(0),
            )
            .unwrap();
        }
        let s = Store::open(&chemin, Box::new(HashEmbedder::default())).unwrap();
        assert_eq!(s.count(&Space::work()).unwrap(), 1);
    }
}
