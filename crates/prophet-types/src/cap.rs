//! Jeton de capacité : la seule preuve de droit dans Prophet OS.
//!
//! Voir `docs/specs/capability-token.md`. Un jeton porte des *grants* (ressource, action, motif,
//! contraintes), une durée de vie, et une signature ed25519 sur sa forme canonique. Un jeton
//! enfant est valide seulement si l'ensemble de ses grants est **inclus** dans ceux du parent
//! ([`Token::is_subset_of`]).

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::canon::{canonical_hash, to_canonical_bytes};
use crate::pattern::{Family, covers, matches, validate};

/// Version du format de jeton produite par cette implémentation.
pub const TOKEN_VERSION: u8 = 0;

/// Ressource visée par un grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Res {
    /// Système de fichiers.
    Fs,
    /// Réseau sortant.
    Net,
    /// Appel d'outil MCP.
    Tool,
    /// Exécution de processus.
    Proc,
    /// Interface utilisateur sémantique.
    Ui,
    /// Journal d'audit.
    Ledger,
    /// Mémoire persistante.
    Memory,
    /// Choix de modèle ou de pilote.
    Model,
    /// Gestion de tâches.
    Task,
    /// Délégation de capacités.
    Cap,
}

/// Action demandée sur une ressource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Act {
    /// Lecture.
    Read,
    /// Écriture.
    Write,
    /// Énumération.
    List,
    /// Sortie réseau.
    Egress,
    /// Appel.
    Call,
    /// Exécution.
    Exec,
    /// Action d'interface.
    Act,
    /// Capture d'écran (interface, dernier recours).
    Vision,
    /// Lecture du journal de toutes les tâches.
    ReadAll,
    /// Usage d'un modèle.
    Use,
    /// Création de sous-tâche.
    Spawn,
    /// Délégation de jeton.
    Delegate,
}

/// Couples (ressource, action) acceptés en v0.
const LEGAL_PAIRS: &[(Res, Act)] = &[
    (Res::Fs, Act::Read),
    (Res::Fs, Act::Write),
    (Res::Fs, Act::List),
    (Res::Net, Act::Egress),
    (Res::Tool, Act::Call),
    (Res::Proc, Act::Exec),
    (Res::Ui, Act::Read),
    (Res::Ui, Act::Act),
    (Res::Ui, Act::Vision),
    (Res::Ledger, Act::Read),
    (Res::Ledger, Act::ReadAll),
    (Res::Memory, Act::Read),
    (Res::Memory, Act::Write),
    (Res::Model, Act::Use),
    (Res::Task, Act::Spawn),
    (Res::Cap, Act::Delegate),
];

/// Exigence d'approbation humaine portée par une contrainte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Approval {
    /// Aucune approbation requise par la contrainte elle-même.
    #[default]
    None,
    /// Approbation humaine obligatoire.
    Required,
}

/// Contraintes attachées à un grant. Toutes optionnelles ; une contrainte absente ne restreint
/// rien. L'enfant peut ajouter une contrainte, jamais en retirer ni en relâcher une.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Constraints {
    /// Nombre maximal d'appels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_calls: Option<u64>,
    /// Volume maximal lu ou écrit, en octets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<u64>,
    /// Volume maximal sortant, en octets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes_out: Option<u64>,
    /// Débit maximal, en requêtes par minute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_per_min: Option<u64>,
    /// Nombre maximal de tokens de modèle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    /// Profondeur maximale de délégation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<u32>,
    /// Nombre maximal de sous-tâches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_children: Option<u32>,
    /// Niveau de sandbox minimal imposé.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    /// Exigence d'approbation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<Approval>,
    /// Listes d'autorisation spécifiques (destinataires, méthodes HTTP, fenêtres…).
    /// L'enfant doit fournir un sous-ensemble de la liste du parent.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub allow: BTreeMap<String, Vec<String>>,
}

impl Constraints {
    /// Vrai si `self` (enfant) est au moins aussi strict que `parent`.
    #[must_use]
    pub fn is_at_least_as_strict_as(&self, parent: &Self) -> bool {
        fn num_ok(child: Option<u64>, parent: Option<u64>) -> bool {
            match (child, parent) {
                (_, None) => true,
                (Some(c), Some(p)) => c <= p,
                (None, Some(_)) => false,
            }
        }
        fn num32_ok(child: Option<u32>, parent: Option<u32>) -> bool {
            num_ok(child.map(u64::from), parent.map(u64::from))
        }
        // Un niveau de sandbox plus élevé est plus strict.
        let level_ok = match (self.level, parent.level) {
            (_, None) => true,
            (Some(c), Some(p)) => c >= p,
            (None, Some(_)) => false,
        };
        let approval_ok = match (self.approval, parent.approval) {
            (_, None | Some(Approval::None)) => true,
            (Some(Approval::Required), Some(Approval::Required)) => true,
            (None | Some(Approval::None), Some(Approval::Required)) => false,
        };
        let allow_ok = parent.allow.iter().all(|(key, parent_values)| {
            self.allow
                .get(key)
                .is_some_and(|child_values| child_values.iter().all(|v| parent_values.contains(v)))
        });
        num_ok(self.max_calls, parent.max_calls)
            && num_ok(self.max_bytes, parent.max_bytes)
            && num_ok(self.max_bytes_out, parent.max_bytes_out)
            && num_ok(self.rate_per_min, parent.rate_per_min)
            && num_ok(self.max_tokens, parent.max_tokens)
            && num32_ok(self.max_depth, parent.max_depth)
            && num32_ok(self.max_children, parent.max_children)
            && level_ok
            && approval_ok
            && allow_ok
    }

    /// Vrai si un appel décrit par `ctx` satisfait les contraintes.
    #[must_use]
    pub fn satisfied_by(&self, ctx: &CheckContext) -> bool {
        if let Some(level) = self.level
            && ctx.sandbox_level < level
        {
            return false;
        }
        if let Some(max) = self.max_bytes
            && ctx.bytes.is_some_and(|b| b > max)
        {
            return false;
        }
        if let Some(max) = self.max_calls
            && ctx.calls_so_far >= max
        {
            return false;
        }
        self.allow.iter().all(|(key, allowed)| {
            ctx.fields
                .get(key)
                .is_none_or(|values| values.iter().all(|v| allowed.contains(v)))
        })
    }
}

/// Une autorisation élémentaire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    /// Ressource visée.
    pub res: Res,
    /// Action autorisée.
    pub act: Act,
    /// Motif de cible (chemin, domaine ou nom selon la ressource).
    #[serde(rename = "match")]
    pub pattern: String,
    /// Contraintes supplémentaires.
    #[serde(default, skip_serializing_if = "is_default_constraints")]
    pub constraints: Constraints,
}

fn is_default_constraints(c: &Constraints) -> bool {
    *c == Constraints::default()
}

impl Grant {
    /// Construit un grant sans contrainte.
    #[must_use]
    pub fn new(res: Res, act: Act, pattern: impl Into<String>) -> Self {
        Self {
            res,
            act,
            pattern: pattern.into(),
            constraints: Constraints::default(),
        }
    }

    /// Ajoute des contraintes.
    #[must_use]
    pub fn with(mut self, constraints: Constraints) -> Self {
        self.constraints = constraints;
        self
    }

    /// Valide la légalité du couple ressource/action et du motif.
    ///
    /// # Erreurs
    /// [`CapError::IllegalPair`] ou [`CapError::Pattern`].
    pub fn validate(&self) -> Result<(), CapError> {
        if !LEGAL_PAIRS.contains(&(self.res, self.act)) {
            return Err(CapError::IllegalPair(self.res, self.act));
        }
        validate(Family::of(self.res), &self.pattern).map_err(CapError::Pattern)
    }

    /// Vrai si ce grant (enfant) est inclus dans `parent`.
    #[must_use]
    pub fn is_subset_of(&self, parent: &Self) -> bool {
        self.res == parent.res
            && self.act == parent.act
            && covers(Family::of(self.res), &parent.pattern, &self.pattern)
            && self
                .constraints
                .is_at_least_as_strict_as(&parent.constraints)
    }
}

/// Contexte d'un contrôle d'accès concret.
#[derive(Debug, Clone, Default)]
pub struct CheckContext {
    /// Cible concrète (chemin absolu, hôte, nom d'outil).
    pub target: String,
    /// Répertoire personnel de l'utilisateur, pour résoudre `~/`.
    pub home: String,
    /// Niveau de sandbox dans lequel s'exécute l'appelant.
    pub sandbox_level: u8,
    /// Volume concerné par l'appel, si connu.
    pub bytes: Option<u64>,
    /// Nombre d'appels déjà effectués sur ce grant.
    pub calls_so_far: u64,
    /// Champs spécifiques à l'outil, confrontés aux listes `allow`.
    pub fields: BTreeMap<String, Vec<String>>,
}

/// Erreurs de manipulation de jeton.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CapError {
    /// Couple ressource/action interdit.
    #[error("couple ressource/action interdit : {0:?}/{1:?}")]
    IllegalPair(Res, Act),
    /// Motif invalide.
    #[error("motif invalide : {0}")]
    Pattern(#[from] crate::pattern::PatternError),
    /// Version de jeton inconnue.
    #[error("version de jeton inconnue : {0}")]
    UnknownVersion(u8),
    /// Jeton sans grant.
    #[error("jeton sans grant")]
    NoGrants,
    /// Signature absente ou illisible.
    #[error("signature absente ou mal formée")]
    BadSignatureFormat,
    /// Signature invalide.
    #[error("signature invalide")]
    BadSignature,
    /// Expiration antérieure à l'émission.
    #[error("expiration antérieure à l'émission")]
    BadValidity,
    /// Sérialisation impossible.
    #[error("sérialisation impossible : {0}")]
    Serialize(String),
}

/// Décision rendue par un contrôle d'accès.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    /// Appel autorisé, par le grant d'index `grant`.
    Allow {
        /// Index du grant ayant autorisé l'appel.
        grant: usize,
    },
    /// Appel refusé.
    Deny {
        /// Code de refus stable.
        reason: DenyReason,
        /// Règle ou grant en cause, si identifiable.
        #[serde(skip_serializing_if = "Option::is_none")]
        rule: Option<String>,
    },
}

impl Decision {
    /// Refus avec code seul.
    #[must_use]
    pub fn deny(reason: DenyReason) -> Self {
        Self::Deny { reason, rule: None }
    }

    /// Vrai si la décision autorise l'appel.
    #[must_use]
    pub fn is_allow(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }
}

/// Codes de refus stables (voir la spécification).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DenyReason {
    /// Jeton expiré.
    Expired,
    /// Signature invalide.
    BadSignature,
    /// Un parent de la chaîne a été révoqué.
    RevokedParent,
    /// La politique refuse.
    PolicyDenied,
    /// Aucun grant ne couvre la demande.
    NoGrant,
    /// Un grant couvre la demande mais une contrainte est violée.
    ConstraintViolated,
    /// Une approbation humaine est requise.
    ApprovalRequired,
    /// Version de jeton inconnue.
    UnknownVersion,
}

/// Jeton de capacité signé.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Token {
    /// Version du format.
    pub v: u8,
    /// Émetteur (`capd@<machine-id>`).
    pub iss: String,
    /// Sujet : identifiant de tâche (`task:<ulid>`).
    pub sub: String,
    /// Identifiant de l'agent.
    pub agent: String,
    /// Utilisateur pour le compte duquel la tâche s'exécute.
    pub user: String,
    /// Hash du jeton parent, ou `None` pour un jeton racine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Autorisations portées.
    pub grants: Vec<Grant>,
    /// Date d'émission.
    #[serde(with = "time::serde::rfc3339")]
    pub iat: OffsetDateTime,
    /// Date d'expiration.
    #[serde(with = "time::serde::rfc3339")]
    pub exp: OffsetDateTime,
    /// Aléa anti-rejeu, base64 de 16 octets.
    pub nonce: String,
    /// Signature ed25519, `ed25519:<base64>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
}

impl Token {
    /// Empreinte canonique du jeton, signature comprise.
    ///
    /// # Erreurs
    /// Si la sérialisation échoue.
    pub fn digest(&self) -> Result<String, CapError> {
        canonical_hash(self, &[]).map_err(|e| CapError::Serialize(e.to_string()))
    }

    /// Octets signés (jeton canonique sans le champ `sig`).
    ///
    /// # Erreurs
    /// Si la sérialisation échoue.
    pub fn signing_bytes(&self) -> Result<Vec<u8>, CapError> {
        to_canonical_bytes(self, &["sig"]).map_err(|e| CapError::Serialize(e.to_string()))
    }

    /// Valide la structure du jeton, indépendamment de la signature.
    ///
    /// # Erreurs
    /// Version inconnue, absence de grant, validité incohérente, grant illégal.
    pub fn validate(&self) -> Result<(), CapError> {
        if self.v != TOKEN_VERSION {
            return Err(CapError::UnknownVersion(self.v));
        }
        if self.grants.is_empty() {
            return Err(CapError::NoGrants);
        }
        if self.exp <= self.iat {
            return Err(CapError::BadValidity);
        }
        for grant in &self.grants {
            grant.validate()?;
        }
        Ok(())
    }

    /// Signe le jeton en place.
    ///
    /// # Erreurs
    /// Si la sérialisation échoue.
    pub fn sign(&mut self, key: &SigningKey) -> Result<(), CapError> {
        self.sig = None;
        let bytes = self.signing_bytes()?;
        let signature: Signature = key.sign(&bytes);
        self.sig = Some(format!("ed25519:{}", B64.encode(signature.to_bytes())));
        Ok(())
    }

    /// Vérifie la signature du jeton.
    ///
    /// # Erreurs
    /// Signature absente, mal formée ou invalide.
    pub fn verify(&self, key: &VerifyingKey) -> Result<(), CapError> {
        let sig = self.sig.as_deref().ok_or(CapError::BadSignatureFormat)?;
        let raw = sig
            .strip_prefix("ed25519:")
            .ok_or(CapError::BadSignatureFormat)?;
        let bytes = B64.decode(raw).map_err(|_| CapError::BadSignatureFormat)?;
        let array: [u8; 64] = bytes.try_into().map_err(|_| CapError::BadSignatureFormat)?;
        let signature = Signature::from_bytes(&array);
        key.verify(&self.signing_bytes()?, &signature)
            .map_err(|_| CapError::BadSignature)
    }

    /// Vrai si tous les grants de `self` sont inclus dans ceux de `parent` et si les métadonnées
    /// de délégation sont cohérentes.
    #[must_use]
    pub fn is_subset_of(&self, parent: &Self) -> bool {
        if self.user != parent.user || self.exp > parent.exp {
            return false;
        }
        self.grants
            .iter()
            .all(|child| parent.grants.iter().any(|p| child.is_subset_of(p)))
    }

    /// Vrai si le jeton est expiré à l'instant `now`.
    #[must_use]
    pub fn is_expired(&self, now: OffsetDateTime) -> bool {
        now >= self.exp
    }

    /// Cherche un grant autorisant `res`/`act` sur la cible de `ctx`.
    ///
    /// Ne vérifie ni la signature, ni l'expiration, ni la politique : c'est le rôle de `capd`.
    #[must_use]
    pub fn find_grant(&self, res: Res, act: Act, ctx: &CheckContext) -> Decision {
        let family = Family::of(res);
        let mut constraint_hit = false;
        for (index, grant) in self.grants.iter().enumerate() {
            if grant.res != res || grant.act != act {
                continue;
            }
            if !matches(family, &grant.pattern, &ctx.target, &ctx.home) {
                continue;
            }
            if grant.constraints.approval == Some(Approval::Required) {
                return Decision::Deny {
                    reason: DenyReason::ApprovalRequired,
                    rule: Some(format!("grant#{index}")),
                };
            }
            if grant.constraints.satisfied_by(ctx) {
                return Decision::Allow { grant: index };
            }
            constraint_hit = true;
        }
        if constraint_hit {
            Decision::deny(DenyReason::ConstraintViolated)
        } else {
            Decision::deny(DenyReason::NoGrant)
        }
    }
}

/// Constructeur de jeton.
#[derive(Debug, Clone)]
pub struct TokenBuilder {
    issuer: String,
    subject: String,
    agent: String,
    user: String,
    parent: Option<String>,
    grants: Vec<Grant>,
    ttl_seconds: i64,
}

impl TokenBuilder {
    /// Nouveau constructeur pour une tâche.
    #[must_use]
    pub fn new(
        issuer: impl Into<String>,
        subject: impl Into<String>,
        agent: impl Into<String>,
        user: impl Into<String>,
    ) -> Self {
        Self {
            issuer: issuer.into(),
            subject: subject.into(),
            agent: agent.into(),
            user: user.into(),
            parent: None,
            grants: Vec::new(),
            ttl_seconds: 1800,
        }
    }

    /// Ajoute un grant.
    #[must_use]
    pub fn grant(mut self, grant: Grant) -> Self {
        self.grants.push(grant);
        self
    }

    /// Ajoute plusieurs grants.
    #[must_use]
    pub fn grants(mut self, grants: impl IntoIterator<Item = Grant>) -> Self {
        self.grants.extend(grants);
        self
    }

    /// Fixe la durée de vie en secondes.
    #[must_use]
    pub const fn ttl_seconds(mut self, seconds: i64) -> Self {
        self.ttl_seconds = seconds;
        self
    }

    /// Déclare le jeton parent (son empreinte).
    #[must_use]
    pub fn parent(mut self, digest: impl Into<String>) -> Self {
        self.parent = Some(digest.into());
        self
    }

    /// Construit et signe le jeton.
    ///
    /// # Erreurs
    /// Si la structure est invalide ou si la signature échoue.
    pub fn build(
        self,
        key: &SigningKey,
        now: OffsetDateTime,
        nonce: [u8; 16],
    ) -> Result<Token, CapError> {
        let mut token = Token {
            v: TOKEN_VERSION,
            iss: self.issuer,
            sub: self.subject,
            agent: self.agent,
            user: self.user,
            parent: self.parent,
            grants: self.grants,
            iat: now,
            exp: now + time::Duration::seconds(self.ttl_seconds),
            nonce: B64.encode(nonce),
            sig: None,
        };
        token.validate()?;
        token.sign(key)?;
        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn key() -> SigningKey {
        SigningKey::generate(&mut OsRng)
    }

    fn now() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_789_000_000).unwrap()
    }

    fn base_token(k: &SigningKey) -> Token {
        TokenBuilder::new("capd@test", "task:01", "org.test.agent", "u")
            .grants([
                Grant::new(Res::Fs, Act::Read, "~/ventes/**"),
                Grant::new(Res::Fs, Act::Write, "~/ventes/out/**"),
                Grant::new(Res::Net, Act::Egress, "*.exemple.fr"),
            ])
            .build(k, now(), [7; 16])
            .unwrap()
    }

    #[test]
    fn signature_valide_et_alteration_detectee() {
        let k = key();
        let token = base_token(&k);
        token.verify(&k.verifying_key()).unwrap();

        let mut altered = token.clone();
        altered.grants[0].pattern = "~/**".into();
        assert_eq!(
            altered.verify(&k.verifying_key()),
            Err(CapError::BadSignature)
        );
    }

    #[test]
    fn signature_d_une_autre_cle_refusee() {
        let k = key();
        let other = key();
        let token = base_token(&k);
        assert_eq!(
            token.verify(&other.verifying_key()),
            Err(CapError::BadSignature)
        );
    }

    #[test]
    fn ordre_des_grants_sans_effet_sur_la_signature() {
        let k = key();
        let mut a = base_token(&k);
        // La signature couvre l'ordre du tableau : permuter doit invalider.
        a.grants.swap(0, 1);
        assert!(a.verify(&k.verifying_key()).is_err());
    }

    #[test]
    fn recherche_de_grant_sur_cible_concrete() {
        let k = key();
        let token = base_token(&k);
        let ctx = CheckContext {
            target: "/home/u/ventes/q3.csv".into(),
            home: "/home/u".into(),
            ..CheckContext::default()
        };
        assert!(token.find_grant(Res::Fs, Act::Read, &ctx).is_allow());

        let hors = CheckContext {
            target: "/home/u/.ssh/id_ed25519".into(),
            home: "/home/u".into(),
            ..CheckContext::default()
        };
        assert_eq!(
            token.find_grant(Res::Fs, Act::Read, &hors),
            Decision::deny(DenyReason::NoGrant)
        );
    }

    #[test]
    fn approbation_requise_remonte_avant_autorisation() {
        let k = key();
        let token = TokenBuilder::new("capd@test", "task:01", "a", "u")
            .grant(
                Grant::new(Res::Tool, Act::Call, "mail.send").with(Constraints {
                    approval: Some(Approval::Required),
                    ..Constraints::default()
                }),
            )
            .build(&k, now(), [1; 16])
            .unwrap();
        let ctx = CheckContext {
            target: "mail.send".into(),
            ..CheckContext::default()
        };
        assert!(matches!(
            token.find_grant(Res::Tool, Act::Call, &ctx),
            Decision::Deny {
                reason: DenyReason::ApprovalRequired,
                ..
            }
        ));
    }

    #[test]
    fn inclusion_de_grants() {
        let parent = Grant::new(Res::Fs, Act::Read, "~/ventes/**");
        let child = Grant::new(Res::Fs, Act::Read, "~/ventes/out/**");
        assert!(child.is_subset_of(&parent));
        assert!(!parent.is_subset_of(&child));

        let autre_action = Grant::new(Res::Fs, Act::Write, "~/ventes/out/**");
        assert!(!autre_action.is_subset_of(&parent));
    }

    #[test]
    fn contrainte_enfant_ne_peut_pas_etre_relachee() {
        let parent = Grant::new(Res::Tool, Act::Call, "mail.send").with(Constraints {
            max_calls: Some(1),
            approval: Some(Approval::Required),
            ..Constraints::default()
        });
        let trop_large = Grant::new(Res::Tool, Act::Call, "mail.send").with(Constraints {
            max_calls: Some(5),
            approval: Some(Approval::Required),
            ..Constraints::default()
        });
        assert!(!trop_large.is_subset_of(&parent));

        let sans_approbation = Grant::new(Res::Tool, Act::Call, "mail.send").with(Constraints {
            max_calls: Some(1),
            ..Constraints::default()
        });
        assert!(!sans_approbation.is_subset_of(&parent));

        let plus_strict = Grant::new(Res::Tool, Act::Call, "mail.send").with(Constraints {
            max_calls: Some(1),
            approval: Some(Approval::Required),
            ..Constraints::default()
        });
        assert!(plus_strict.is_subset_of(&parent));
    }

    #[test]
    fn niveau_de_sandbox_plus_eleve_est_plus_strict() {
        let parent = Grant::new(Res::Proc, Act::Exec, "/usr/bin/**").with(Constraints {
            level: Some(1),
            ..Constraints::default()
        });
        let enfant_strict = Grant::new(Res::Proc, Act::Exec, "/usr/bin/**").with(Constraints {
            level: Some(2),
            ..Constraints::default()
        });
        let enfant_laxiste = Grant::new(Res::Proc, Act::Exec, "/usr/bin/**").with(Constraints {
            level: Some(0),
            ..Constraints::default()
        });
        assert!(enfant_strict.is_subset_of(&parent));
        assert!(!enfant_laxiste.is_subset_of(&parent));
    }

    #[test]
    fn listes_allow_doivent_etre_incluses() {
        let mut parent_allow = BTreeMap::new();
        parent_allow.insert(
            "to".to_owned(),
            vec!["marie@x.fr".to_owned(), "paul@x.fr".to_owned()],
        );
        let parent = Grant::new(Res::Tool, Act::Call, "mail.send").with(Constraints {
            allow: parent_allow,
            ..Constraints::default()
        });

        let mut ok_allow = BTreeMap::new();
        ok_allow.insert("to".to_owned(), vec!["marie@x.fr".to_owned()]);
        let ok = Grant::new(Res::Tool, Act::Call, "mail.send").with(Constraints {
            allow: ok_allow,
            ..Constraints::default()
        });
        assert!(ok.is_subset_of(&parent));

        let mut ko_allow = BTreeMap::new();
        ko_allow.insert("to".to_owned(), vec!["eve@evil.com".to_owned()]);
        let ko = Grant::new(Res::Tool, Act::Call, "mail.send").with(Constraints {
            allow: ko_allow,
            ..Constraints::default()
        });
        assert!(!ko.is_subset_of(&parent));
    }

    #[test]
    fn inclusion_de_jetons_et_duree_de_vie() {
        let k = key();
        let parent = base_token(&k);
        let enfant = TokenBuilder::new("capd@test", "task:02", "org.test.agent", "u")
            .grant(Grant::new(Res::Fs, Act::Read, "~/ventes/2026/**"))
            .parent(parent.digest().unwrap())
            .ttl_seconds(600)
            .build(&k, now(), [9; 16])
            .unwrap();
        assert!(enfant.is_subset_of(&parent));

        let trop_long = TokenBuilder::new("capd@test", "task:03", "org.test.agent", "u")
            .grant(Grant::new(Res::Fs, Act::Read, "~/ventes/2026/**"))
            .ttl_seconds(100_000)
            .build(&k, now(), [9; 16])
            .unwrap();
        assert!(!trop_long.is_subset_of(&parent));

        let autre_user = TokenBuilder::new("capd@test", "task:04", "org.test.agent", "autre")
            .grant(Grant::new(Res::Fs, Act::Read, "~/ventes/2026/**"))
            .ttl_seconds(600)
            .build(&k, now(), [9; 16])
            .unwrap();
        assert!(!autre_user.is_subset_of(&parent));
    }

    #[test]
    fn inclusion_reflexive_et_transitive() {
        let patterns = ["~/a/**", "~/a/b/**", "~/a/b/c/**", "~/autre/**", "**"];
        for p in patterns {
            let g = Grant::new(Res::Fs, Act::Read, p);
            assert!(g.is_subset_of(&g), "réflexivité pour {p}");
        }
        for a in patterns {
            for b in patterns {
                for c in patterns {
                    let (ga, gb, gc) = (
                        Grant::new(Res::Fs, Act::Read, a),
                        Grant::new(Res::Fs, Act::Read, b),
                        Grant::new(Res::Fs, Act::Read, c),
                    );
                    if gc.is_subset_of(&gb) && gb.is_subset_of(&ga) {
                        assert!(gc.is_subset_of(&ga), "transitivité {c} ⊆ {b} ⊆ {a}");
                    }
                }
            }
        }
    }

    #[test]
    fn couple_ressource_action_illegal_refuse() {
        let g = Grant::new(Res::Fs, Act::Egress, "~/x/**");
        assert_eq!(
            g.validate(),
            Err(CapError::IllegalPair(Res::Fs, Act::Egress))
        );
    }

    #[test]
    fn jeton_sans_grant_refuse() {
        let k = key();
        let err = TokenBuilder::new("capd@test", "task:01", "a", "u")
            .build(&k, now(), [0; 16])
            .unwrap_err();
        assert_eq!(err, CapError::NoGrants);
    }

    #[test]
    fn expiration() {
        let k = key();
        let token = base_token(&k);
        assert!(!token.is_expired(now()));
        assert!(token.is_expired(now() + time::Duration::seconds(1801)));
    }

    #[test]
    fn serialisation_aller_retour() {
        let k = key();
        let token = base_token(&k);
        let json = serde_json::to_string(&token).unwrap();
        let back: Token = serde_json::from_str(&json).unwrap();
        assert_eq!(token, back);
        back.verify(&k.verifying_key()).unwrap();
    }

    #[test]
    fn champ_inconnu_refuse() {
        let json = r#"{"v":0,"iss":"a","sub":"b","agent":"c","user":"u","grants":[],
            "iat":"2026-09-11T00:00:00Z","exp":"2026-09-11T01:00:00Z","nonce":"AA==","inconnu":1}"#;
        assert!(serde_json::from_str::<Token>(json).is_err());
    }
}
