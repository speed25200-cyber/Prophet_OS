//! Événements du journal d'audit : structure, chaînage par hachage, catalogue des types.
//!
//! Voir `docs/specs/ledger-event.md`. Un événement ne contient jamais de secret ni de contenu
//! complet : uniquement des empreintes, des tailles, des chemins et des codes de décision.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

use crate::canon::canonical_hash;

/// Version du format d'événement.
pub const EVENT_VERSION: u8 = 0;

/// Valeur de `prev` du premier événement de la chaîne.
pub const GENESIS: &str = "genesis";

/// Catégorie d'événement. La liste est fermée : un type inconnu est refusé à l'écriture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventKind {
    /// Tâche créée.
    #[serde(rename = "task.created")]
    TaskCreated,
    /// Tâche planifiée (pilote, sandbox, grants choisis).
    #[serde(rename = "task.planned")]
    TaskPlanned,
    /// Tâche démarrée.
    #[serde(rename = "task.started")]
    TaskStarted,
    /// Tâche en attente d'approbation.
    #[serde(rename = "task.waiting")]
    TaskWaiting,
    /// Tâche terminée avec succès.
    #[serde(rename = "task.done")]
    TaskDone,
    /// Tâche échouée.
    #[serde(rename = "task.failed")]
    TaskFailed,
    /// Tâche annulée.
    #[serde(rename = "task.cancelled")]
    TaskCancelled,
    /// Tâche annulée après coup (undo).
    #[serde(rename = "task.rolled_back")]
    TaskRolledBack,
    /// Appel d'outil émis.
    #[serde(rename = "tool.call")]
    ToolCall,
    /// Résultat d'outil.
    #[serde(rename = "tool.result")]
    ToolResult,
    /// Décision de politique favorable.
    #[serde(rename = "policy.allow")]
    PolicyAllow,
    /// Décision de politique défavorable.
    #[serde(rename = "policy.deny")]
    PolicyDeny,
    /// Jeton révoqué.
    #[serde(rename = "policy.revoked")]
    PolicyRevoked,
    /// Approbation demandée.
    #[serde(rename = "approval.requested")]
    ApprovalRequested,
    /// Approbation tranchée.
    #[serde(rename = "approval.resolved")]
    ApprovalResolved,
    /// Sous-volume de tâche ouvert.
    #[serde(rename = "fs.begin")]
    FsBegin,
    /// Sous-volume validé.
    #[serde(rename = "fs.commit")]
    FsCommit,
    /// Sous-volume annulé après validation.
    #[serde(rename = "fs.undo")]
    FsUndo,
    /// Sous-volume abandonné.
    #[serde(rename = "fs.abandon")]
    FsAbandon,
    /// Requête réseau sortante.
    #[serde(rename = "net.request")]
    NetRequest,
    /// Requête réseau refusée.
    #[serde(rename = "net.deny")]
    NetDeny,
    /// Exfiltration suspectée.
    #[serde(rename = "net.exfil_suspected")]
    NetExfilSuspected,
    /// Pilote démarré.
    #[serde(rename = "provider.started")]
    ProviderStarted,
    /// Consommation de quota d'abonnement.
    #[serde(rename = "provider.quota")]
    ProviderQuota,
    /// Pilote arrêté.
    #[serde(rename = "provider.stopped")]
    ProviderStopped,
    /// Sandbox démarrée.
    #[serde(rename = "sandbox.started")]
    SandboxStarted,
    /// Sandbox gelée.
    #[serde(rename = "sandbox.frozen")]
    SandboxFrozen,
    /// Sandbox tuée.
    #[serde(rename = "sandbox.killed")]
    SandboxKilled,
    /// Lecture d'arbre sémantique.
    #[serde(rename = "ui.tree")]
    UiTree,
    /// Action d'interface.
    #[serde(rename = "ui.act")]
    UiAct,
    /// Écriture en mémoire.
    #[serde(rename = "memory.write")]
    MemoryWrite,
    /// Scellement périodique de la chaîne.
    #[serde(rename = "ledger.seal")]
    LedgerSeal,
}

impl EventKind {
    /// Vrai si l'événement doit être produit uniquement par un daemon système.
    #[must_use]
    pub const fn is_system_only(self) -> bool {
        matches!(
            self,
            Self::LedgerSeal | Self::PolicyAllow | Self::PolicyDeny | Self::PolicyRevoked
        )
    }
}

/// Acteur à l'origine d'un événement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Actor(pub String);

impl Actor {
    /// Acteur système générique.
    #[must_use]
    pub fn system() -> Self {
        Self("system".to_owned())
    }

    /// Daemon nommé.
    #[must_use]
    pub fn daemon(name: &str) -> Self {
        Self(name.to_owned())
    }

    /// Serveur MCP nommé.
    #[must_use]
    pub fn mcp(name: &str) -> Self {
        Self(format!("mcp:{name}"))
    }

    /// Pilote nommé.
    #[must_use]
    pub fn driver(name: &str) -> Self {
        Self(format!("driver:{name}"))
    }

    /// L'humain.
    #[must_use]
    pub fn user() -> Self {
        Self("user".to_owned())
    }
}

/// Événement du journal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    /// Version du format.
    pub v: u8,
    /// Numéro de séquence, attribué par le daemon `ledger`.
    pub seq: u64,
    /// Empreinte de l'événement précédent, ou [`GENESIS`].
    pub prev: String,
    /// Horodatage UTC.
    #[serde(with = "time::serde::rfc3339")]
    pub ts: OffsetDateTime,
    /// Tâche concernée, s'il y en a une.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// Numéro d'étape dans la tâche.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<u32>,
    /// Acteur à l'origine.
    pub actor: Actor,
    /// Type d'événement.
    pub kind: EventKind,
    /// Charge utile, spécifique au type.
    pub payload: Value,
    /// Empreinte de l'événement (calculée sur tout sauf ce champ).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

/// Erreur de manipulation d'événement.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EventError {
    /// Version inconnue.
    #[error("version d'événement inconnue : {0}")]
    UnknownVersion(u8),
    /// La charge utile contient un champ interdit (secret probable).
    #[error("charge utile contenant un champ interdit : {0}")]
    ForbiddenField(String),
    /// Chaîne rompue.
    #[error("chaîne rompue à seq {seq} : {reason}")]
    BrokenChain {
        /// Séquence fautive.
        seq: u64,
        /// Raison.
        reason: String,
    },
    /// Sérialisation impossible.
    #[error("sérialisation impossible : {0}")]
    Serialize(String),
}

/// Champs dont la présence dans une charge utile trahit une fuite de secret.
const FORBIDDEN_PAYLOAD_FIELDS: &[&str] = &[
    "password",
    "secret",
    "token",
    "api_key",
    "apikey",
    "authorization",
    "cookie",
    "private_key",
    "content",
    "body",
];

/// Brouillon d'événement : tout sauf `seq`, `prev` et `hash`, attribués par le journal.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    /// Horodatage.
    pub ts: OffsetDateTime,
    /// Tâche concernée.
    pub task: Option<String>,
    /// Étape.
    pub step: Option<u32>,
    /// Acteur.
    pub actor: Actor,
    /// Type.
    pub kind: EventKind,
    /// Charge utile.
    pub payload: Value,
}

impl Draft {
    /// Nouveau brouillon horodaté à l'instant donné.
    #[must_use]
    pub fn new(ts: OffsetDateTime, actor: Actor, kind: EventKind, payload: Value) -> Self {
        Self {
            ts,
            task: None,
            step: None,
            actor,
            kind,
            payload,
        }
    }

    /// Rattache le brouillon à une tâche.
    #[must_use]
    pub fn task(mut self, task: impl Into<String>) -> Self {
        self.task = Some(task.into());
        self
    }

    /// Précise l'étape.
    #[must_use]
    pub const fn step(mut self, step: u32) -> Self {
        self.step = Some(step);
        self
    }
}

impl Event {
    /// Matérialise un brouillon en événement chaîné.
    ///
    /// # Erreurs
    /// Si la charge utile contient un champ interdit ou si le hachage échoue.
    pub fn seal(draft: Draft, seq: u64, prev: &str) -> Result<Self, EventError> {
        check_payload(&draft.payload)?;
        let mut event = Self {
            v: EVENT_VERSION,
            seq,
            prev: prev.to_owned(),
            ts: draft.ts,
            task: draft.task,
            step: draft.step,
            actor: draft.actor,
            kind: draft.kind,
            payload: draft.payload,
            hash: None,
        };
        event.hash = Some(event.compute_hash()?);
        Ok(event)
    }

    /// Recalcule l'empreinte de l'événement.
    ///
    /// # Erreurs
    /// Si la sérialisation canonique échoue.
    pub fn compute_hash(&self) -> Result<String, EventError> {
        canonical_hash(self, &["hash"]).map_err(|e| EventError::Serialize(e.to_string()))
    }

    /// Vérifie que l'empreinte enregistrée correspond au contenu.
    ///
    /// # Erreurs
    /// [`EventError::BrokenChain`] si l'empreinte est absente ou fausse.
    pub fn verify_hash(&self) -> Result<(), EventError> {
        let recorded = self.hash.as_deref().ok_or(EventError::BrokenChain {
            seq: self.seq,
            reason: "empreinte absente".to_owned(),
        })?;
        let computed = self.compute_hash()?;
        if computed == recorded {
            Ok(())
        } else {
            Err(EventError::BrokenChain {
                seq: self.seq,
                reason: "empreinte ne correspond pas au contenu".to_owned(),
            })
        }
    }
}

fn check_payload(payload: &Value) -> Result<(), EventError> {
    match payload {
        Value::Object(map) => {
            for (key, value) in map {
                let lower = key.to_ascii_lowercase();
                if FORBIDDEN_PAYLOAD_FIELDS.contains(&lower.as_str()) {
                    return Err(EventError::ForbiddenField(key.clone()));
                }
                check_payload(value)?;
            }
            Ok(())
        }
        Value::Array(items) => items.iter().try_for_each(check_payload),
        _ => Ok(()),
    }
}

/// Vérifie une chaîne complète d'événements : empreintes, chaînage, séquence.
///
/// # Erreurs
/// [`EventError::BrokenChain`] au premier écart, en nommant la séquence fautive.
pub fn verify_chain(events: &[Event]) -> Result<(), EventError> {
    let mut expected_prev = GENESIS.to_owned();
    let mut expected_seq: Option<u64> = None;
    for event in events {
        if event.v != EVENT_VERSION {
            return Err(EventError::UnknownVersion(event.v));
        }
        if let Some(expected) = expected_seq
            && event.seq != expected
        {
            return Err(EventError::BrokenChain {
                seq: event.seq,
                reason: format!("séquence attendue {expected}"),
            });
        }
        if event.prev != expected_prev {
            return Err(EventError::BrokenChain {
                seq: event.seq,
                reason: "chaînage `prev` incorrect".to_owned(),
            });
        }
        event.verify_hash()?;
        expected_prev = event.hash.clone().unwrap_or_default();
        expected_seq = Some(event.seq + 1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chaine(n: u64) -> Vec<Event> {
        let mut events = Vec::new();
        let mut prev = GENESIS.to_owned();
        for seq in 0..n {
            let draft = Draft::new(
                OffsetDateTime::from_unix_timestamp(1_789_000_000 + i64::try_from(seq).unwrap())
                    .unwrap(),
                Actor::daemon("agentd"),
                EventKind::ToolCall,
                json!({"tool": "fs.read", "args_digest": "blake3:aa", "args_size": 12}),
            )
            .task("task:01")
            .step(u32::try_from(seq).unwrap());
            let event = Event::seal(draft, seq, &prev).unwrap();
            prev = event.hash.clone().unwrap();
            events.push(event);
        }
        events
    }

    #[test]
    fn chaine_valide() {
        verify_chain(&chaine(200)).unwrap();
    }

    #[test]
    fn modification_detectee() {
        let mut events = chaine(50);
        events[20].payload = json!({"tool": "fs.write"});
        let err = verify_chain(&events).unwrap_err();
        assert!(
            matches!(err, EventError::BrokenChain { seq: 20, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn suppression_detectee() {
        let mut events = chaine(50);
        events.remove(20);
        let err = verify_chain(&events).unwrap_err();
        assert!(
            matches!(err, EventError::BrokenChain { seq: 21, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn insertion_detectee() {
        let mut events = chaine(50);
        let faux = events[10].clone();
        events.insert(30, faux);
        assert!(verify_chain(&events).is_err());
    }

    #[test]
    fn reecriture_complete_detectee_par_le_scellement() {
        // Un attaquant qui réécrit toute la chaîne produit une chaîne cohérente ; c'est la
        // signature du scellement (ledger::seal, M3-T3) qui l'attrape. Ici on vérifie seulement
        // qu'une réécriture partielle ne passe pas.
        let mut events = chaine(10);
        events[5].ts = OffsetDateTime::from_unix_timestamp(0).unwrap();
        events[5].hash = Some(events[5].compute_hash().unwrap());
        assert!(verify_chain(&events).is_err());
    }

    #[test]
    fn champ_interdit_dans_la_charge_utile() {
        let draft = Draft::new(
            OffsetDateTime::from_unix_timestamp(0).unwrap(),
            Actor::system(),
            EventKind::NetRequest,
            json!({"host": "x.fr", "authorization": "Bearer sk-123"}),
        );
        let err = Event::seal(draft, 0, GENESIS).unwrap_err();
        assert_eq!(err, EventError::ForbiddenField("authorization".into()));
    }

    #[test]
    fn champ_interdit_imbrique() {
        let draft = Draft::new(
            OffsetDateTime::from_unix_timestamp(0).unwrap(),
            Actor::system(),
            EventKind::ToolCall,
            json!({"tool": "http.fetch", "meta": {"headers": [{"cookie": "abc"}]}}),
        );
        assert!(Event::seal(draft, 0, GENESIS).is_err());
    }

    #[test]
    fn types_serialises_avec_un_point() {
        assert_eq!(
            serde_json::to_string(&EventKind::ToolCall).unwrap(),
            "\"tool.call\""
        );
        assert_eq!(
            serde_json::from_str::<EventKind>("\"net.exfil_suspected\"").unwrap(),
            EventKind::NetExfilSuspected
        );
        assert!(serde_json::from_str::<EventKind>("\"inconnu.x\"").is_err());
    }

    #[test]
    fn aller_retour_json() {
        let events = chaine(3);
        let json = serde_json::to_string(&events[1]).unwrap();
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(events[1], back);
        back.verify_hash().unwrap();
    }
}
