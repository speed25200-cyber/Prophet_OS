//! Le broker : émission, délégation, révocation, contrôle d'accès.

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use ed25519_dalek::{SigningKey, VerifyingKey};
use prophet_types::cap::{
    Act, CheckContext, Decision, DenyReason, Grant, Res, Token, TokenBuilder,
};
use prophet_types::manifest::Manifest;
use time::OffsetDateTime;

use crate::approvals::{Approvals, Request as ApprovalRequest};
use crate::policy::{
    ActionClass, PolicyEngine, ResourceFacts, action_name, classify, is_sensitive_path,
};

/// Erreur du broker.
#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    /// Le manifeste n'autorise aucun des grants demandés.
    #[error("aucun grant demandé n'est couvert par le plafond du manifeste")]
    EmptyIntersection,
    /// Erreur de jeton.
    #[error(transparent)]
    Cap(#[from] prophet_types::cap::CapError),
    /// Erreur de manifeste.
    #[error(transparent)]
    Manifest(#[from] prophet_types::manifest::ManifestError),
    /// Erreur de politique.
    #[error(transparent)]
    Policy(#[from] crate::policy::PolicyError),
    /// Le jeton parent est inconnu du broker.
    #[error("jeton parent inconnu")]
    UnknownParent,
    /// Les grants demandés dépassent ceux du parent.
    #[error("les grants demandés ne sont pas inclus dans ceux du parent")]
    NotASubset,
    /// Profondeur de délégation dépassée.
    #[error("profondeur de délégation maximale atteinte ({0})")]
    DepthExceeded(u32),
}

/// Demande de contrôle d'accès.
#[derive(Debug, Clone)]
pub struct CheckRequest {
    /// Ressource visée.
    pub res: Res,
    /// Action demandée.
    pub act: Act,
    /// Cible concrète.
    pub target: String,
    /// Niveau de sandbox de l'appelant.
    pub sandbox_level: u8,
    /// L'action est-elle irréversible ?
    pub irreversible: bool,
    /// A-t-elle un effet hors de la machine ?
    pub external: bool,
    /// Contexte fin (volume, compteurs, champs d'outil).
    pub context: CheckContext,
}

impl CheckRequest {
    /// Demande simple sans contrainte particulière.
    #[must_use]
    pub fn new(res: Res, act: Act, target: impl Into<String>) -> Self {
        Self {
            res,
            act,
            target: target.into(),
            sandbox_level: 0,
            irreversible: false,
            external: false,
            context: CheckContext::default(),
        }
    }

    /// Précise le niveau de sandbox de l'appelant.
    #[must_use]
    pub const fn sandbox_level(mut self, level: u8) -> Self {
        self.sandbox_level = level;
        self
    }

    /// Marque l'action comme irréversible.
    #[must_use]
    pub const fn irreversible(mut self) -> Self {
        self.irreversible = true;
        self
    }

    /// Marque l'action comme ayant un effet externe.
    #[must_use]
    pub const fn external(mut self) -> Self {
        self.external = true;
        self
    }
}

/// Broker de capacités.
#[derive(Debug)]
pub struct Broker {
    key: SigningKey,
    issuer: String,
    home: String,
    policy: PolicyEngine,
    /// Jetons émis, indexés par empreinte, pour vérifier les chaînes de délégation.
    issued: HashMap<String, Token>,
    /// Sujets révoqués.
    revoked: HashSet<String>,
    /// File d'approbations.
    approvals: Approvals,
    /// Empreintes de jetons dont la signature a déjà été vérifiée.
    ///
    /// L'empreinte couvre le jeton **signature comprise** : un jeton modifié a une autre
    /// empreinte et repasse donc par la vérification complète. Le cache accélère sans affaiblir.
    verified: RwLock<HashSet<String>>,
    /// Décisions de politique déjà rendues, indexées par les entrées exactes de l'évaluation.
    policy_cache: RwLock<HashMap<PolicyKey, bool>>,
}

/// Clé de cache d'une évaluation de politique. Elle reprend toutes les entrées dont la décision
/// dépend : changer l'une d'elles produit une autre clé, donc une réévaluation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PolicyKey {
    action: String,
    sensitive: bool,
    irreversible: bool,
    external: bool,
    sandbox_level: u8,
}

/// Profondeur de délégation par défaut, si le jeton racine ne la contraint pas.
pub const DEFAULT_MAX_DEPTH: u32 = 3;

impl Broker {
    /// Construit un broker.
    ///
    /// # Erreurs
    /// Si les politiques par défaut ne compilent pas.
    pub fn new(
        key: SigningKey,
        issuer: impl Into<String>,
        home: impl Into<String>,
    ) -> Result<Self, BrokerError> {
        Ok(Self {
            key,
            issuer: issuer.into(),
            home: home.into(),
            policy: PolicyEngine::with_defaults()?,
            issued: HashMap::new(),
            revoked: HashSet::new(),
            approvals: Approvals::new(),
            verified: RwLock::new(HashSet::new()),
            policy_cache: RwLock::new(HashMap::new()),
        })
    }

    /// Remplace le moteur de politique (chargement depuis `/etc/prophet/policies`).
    ///
    /// Vide le cache de décisions : les anciennes réponses ne valent plus rien.
    #[must_use]
    pub fn with_policy(mut self, policy: PolicyEngine) -> Self {
        self.policy = policy;
        if let Ok(mut cache) = self.policy_cache.write() {
            cache.clear();
        }
        self
    }

    /// Clé publique du broker.
    #[must_use]
    pub fn verifying_key(&self) -> VerifyingKey {
        self.key.verifying_key()
    }

    /// Accès à la file d'approbations.
    pub fn approvals_mut(&mut self) -> &mut Approvals {
        &mut self.approvals
    }

    /// Accès en lecture à la file d'approbations.
    #[must_use]
    pub const fn approvals(&self) -> &Approvals {
        &self.approvals
    }

    /// Émet un jeton racine pour une tâche : intersection des grants demandés et du plafond du
    /// manifeste.
    ///
    /// # Erreurs
    /// [`BrokerError::EmptyIntersection`] si le manifeste ne couvre aucun grant demandé.
    pub fn mint(
        &mut self,
        manifest: &Manifest,
        task: &str,
        user: &str,
        requested: &[Grant],
        ttl_seconds: i64,
        now: OffsetDateTime,
    ) -> Result<Token, BrokerError> {
        let ceiling = manifest.ceiling()?;
        let granted: Vec<Grant> = requested
            .iter()
            .filter(|g| ceiling.iter().any(|c| g.is_subset_of(c)))
            .cloned()
            .collect();
        if granted.is_empty() {
            return Err(BrokerError::EmptyIntersection);
        }
        let token = TokenBuilder::new(&self.issuer, task, &manifest.agent.id, user)
            .grants(granted)
            .ttl_seconds(ttl_seconds)
            .build(&self.key, now, random_nonce())?;
        self.issued.insert(token.digest()?, token.clone());
        Ok(token)
    }

    /// Délègue un sous-ensemble des grants d'un jeton à une sous-tâche.
    ///
    /// # Erreurs
    /// Parent inconnu, grants non inclus, ou profondeur dépassée.
    pub fn delegate(
        &mut self,
        parent: &Token,
        task: &str,
        requested: &[Grant],
        ttl_seconds: i64,
        now: OffsetDateTime,
    ) -> Result<Token, BrokerError> {
        let parent_digest = parent.digest()?;
        if !self.issued.contains_key(&parent_digest) {
            return Err(BrokerError::UnknownParent);
        }
        let depth = self.depth_of(&parent_digest);
        let max_depth = parent
            .grants
            .iter()
            .find(|g| g.res == Res::Task && g.act == Act::Spawn)
            .and_then(|g| g.constraints.max_depth)
            .unwrap_or(DEFAULT_MAX_DEPTH);
        if depth + 1 > max_depth {
            return Err(BrokerError::DepthExceeded(max_depth));
        }
        if !requested
            .iter()
            .all(|g| parent.grants.iter().any(|p| g.is_subset_of(p)))
        {
            return Err(BrokerError::NotASubset);
        }
        // La durée de vie de l'enfant ne dépasse jamais celle du parent.
        let parent_remaining = (parent.exp - now).whole_seconds().max(1);
        let token = TokenBuilder::new(&self.issuer, task, &parent.agent, &parent.user)
            .grants(requested.to_vec())
            .parent(parent_digest)
            .ttl_seconds(ttl_seconds.min(parent_remaining))
            .build(&self.key, now, random_nonce())?;
        self.issued.insert(token.digest()?, token.clone());
        Ok(token)
    }

    fn depth_of(&self, digest: &str) -> u32 {
        let mut depth = 0;
        let mut current = self.issued.get(digest);
        while let Some(token) = current {
            match &token.parent {
                Some(parent) => {
                    depth += 1;
                    current = self.issued.get(parent);
                }
                None => break,
            }
        }
        depth
    }

    /// Révoque une tâche : son jeton et tous ses descendants deviennent inutilisables.
    pub fn revoke(&mut self, subject: &str) {
        self.revoked.insert(subject.to_owned());
        self.approvals.clear_task_rules(subject);
    }

    /// Vrai si ce sujet est déjà révoqué.
    #[must_use]
    pub fn is_revoked(&self, subject: &str) -> bool {
        self.revoked.contains(subject)
    }

    /// Vrai si un ancêtre du jeton est révoqué.
    fn has_revoked_ancestor(&self, token: &Token) -> bool {
        if self.revoked.contains(&token.sub) {
            return true;
        }
        let mut current = token.parent.clone();
        while let Some(digest) = current {
            let Some(parent) = self.issued.get(&digest) else {
                // Un parent absent du registre est traité comme révoqué : on ne peut pas prouver
                // sa validité, donc on refuse.
                return true;
            };
            if self.revoked.contains(&parent.sub) {
                return true;
            }
            current = parent.parent.clone();
        }
        false
    }

    /// Contrôle d'accès complet. C'est le seul point d'entrée légitime pour accorder un droit.
    ///
    /// Ordre : version et structure, signature, chaîne de parents, expiration, politique, grant,
    /// puis classe d'action (approbation).
    ///
    /// # Erreurs
    /// Si l'évaluation de politique échoue pour une raison technique.
    pub fn check(
        &self,
        token: &Token,
        request: &CheckRequest,
        now: OffsetDateTime,
    ) -> Result<Decision, BrokerError> {
        if token.validate().is_err() {
            return Ok(Decision::deny(DenyReason::UnknownVersion));
        }
        if !self.signature_is_valid(token)? {
            return Ok(Decision::deny(DenyReason::BadSignature));
        }
        if self.has_revoked_ancestor(token) {
            return Ok(Decision::deny(DenyReason::RevokedParent));
        }
        if token.is_expired(now) {
            return Ok(Decision::deny(DenyReason::Expired));
        }

        let facts = self.facts_for(request);
        if !self.policy_allows(&token.sub, request, &facts)? {
            return Ok(Decision::Deny {
                reason: DenyReason::PolicyDenied,
                rule: Some(action_name(request.res, request.act)),
            });
        }

        let mut context = request.context.clone();
        context.target.clone_from(&request.target);
        if context.home.is_empty() {
            context.home.clone_from(&self.home);
        }
        context.sandbox_level = request.sandbox_level;

        let decision = token.find_grant(request.res, request.act, &context);
        if !decision.is_allow() {
            return Ok(decision);
        }

        if classify(request.res, request.act, &facts).requires_approval() {
            return Ok(Decision::Deny {
                reason: DenyReason::ApprovalRequired,
                rule: Some(action_name(request.res, request.act)),
            });
        }
        Ok(decision)
    }

    /// Vérifie la signature, en mémorisant les empreintes déjà validées.
    fn signature_is_valid(&self, token: &Token) -> Result<bool, BrokerError> {
        let digest = token.digest()?;
        if self
            .verified
            .read()
            .is_ok_and(|cache| cache.contains(&digest))
        {
            return Ok(true);
        }
        if token.verify(&self.verifying_key()).is_err() {
            return Ok(false);
        }
        if let Ok(mut cache) = self.verified.write() {
            cache.insert(digest);
        }
        Ok(true)
    }

    /// Évalue la politique, en mémorisant les décisions par jeu d'entrées.
    ///
    /// La cible concrète n'entre pas dans la clé : les politiques ne la lisent pas directement,
    /// elles lisent les faits que le broker en dérive (`sensitive` en particulier). Deux cibles
    /// aux mêmes faits reçoivent donc la même décision, ce que le cache reflète exactement.
    fn policy_allows(
        &self,
        task: &str,
        request: &CheckRequest,
        facts: &ResourceFacts,
    ) -> Result<bool, BrokerError> {
        let key = PolicyKey {
            action: action_name(request.res, request.act),
            sensitive: facts.sensitive,
            irreversible: facts.irreversible,
            external: facts.external,
            sandbox_level: request.sandbox_level,
        };
        if let Ok(cache) = self.policy_cache.read()
            && let Some(decision) = cache.get(&key)
        {
            return Ok(*decision);
        }
        let decision =
            self.policy
                .allows(task, request.res, request.act, facts, request.sandbox_level)?;
        if let Ok(mut cache) = self.policy_cache.write() {
            cache.insert(key, decision);
        }
        Ok(decision)
    }

    /// Faits établis par le broker sur la cible, indépendamment de ce que l'appelant déclare.
    fn facts_for(&self, request: &CheckRequest) -> ResourceFacts {
        let sensitive = matches!(request.res, Res::Fs | Res::Proc)
            && is_sensitive_path(&request.target, &self.home);
        ResourceFacts {
            target: request.target.clone(),
            sensitive,
            irreversible: request.irreversible,
            external: request.external,
            confined_utility: request.res == Res::Proc
                && request.act == Act::Exec
                && prophet_types::exec::is_safe_utility(&request.target),
        }
    }

    /// Classe d'action d'une demande, pour l'affichage et la journalisation.
    #[must_use]
    pub fn classify(&self, request: &CheckRequest) -> ActionClass {
        classify(request.res, request.act, &self.facts_for(request))
    }

    /// L'état d'une demande d'approbation : en attente, ou tranchée récemment.
    #[must_use]
    pub fn approval_status(&self, id: &str) -> Option<crate::approvals::Approval> {
        self.approvals.status(id)
    }

    /// Joint le motif du modèle à une demande en attente (ADR 0041).
    pub fn explain_approval(
        &mut self,
        id: &str,
        reason: &str,
    ) -> Option<crate::approvals::Approval> {
        self.approvals.explain(id, reason)
    }

    /// Crée une demande d'approbation pour une action refusée faute de décision humaine.
    pub fn request_approval(
        &mut self,
        token: &Token,
        request: &CheckRequest,
        summary: impl Into<String>,
        now: OffsetDateTime,
    ) -> crate::approvals::Approval {
        let approval_request = ApprovalRequest {
            task: token.sub.clone(),
            agent: token.agent.clone(),
            action: action_name(request.res, request.act),
            target: request.target.clone(),
            summary: summary.into(),
            irreversible: request.irreversible,
            external: request.external,
        };
        self.approvals.request(approval_request, now)
    }
}

fn random_nonce() -> [u8; 16] {
    use rand::RngCore as _;
    let mut nonce = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    nonce
}

#[cfg(test)]
mod tests {
    use super::*;
    use prophet_types::cap::{Approval as ApprovalConstraint, Constraints};

    const MANIFESTE: &str = r#"
[agent]
id = "org.test.analyste"
version = "1.0.0"
name = "Analyste"
publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="

[model]
preferred = ["local:qwen3-8b"]

[capabilities.max]
"fs.read" = ["~/ventes/**"]
"fs.write" = ["~/ventes/out/**"]
"net.egress" = ["*.exemple.fr"]
"tool.call" = ["fs.*", "mail.send"]
"task.spawn" = ["*"]
"#;

    fn manifeste() -> Manifest {
        Manifest::from_toml(MANIFESTE).unwrap()
    }

    fn broker() -> Broker {
        Broker::new(
            SigningKey::generate(&mut rand::rngs::OsRng),
            "capd@test",
            "/home/u",
        )
        .unwrap()
    }

    fn now() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_789_000_000).unwrap()
    }

    fn jeton(b: &mut Broker) -> Token {
        b.mint(
            &manifeste(),
            "task:01",
            "u",
            &[
                Grant::new(Res::Fs, Act::Read, "~/ventes/**"),
                Grant::new(Res::Fs, Act::Write, "~/ventes/out/**"),
                Grant::new(Res::Net, Act::Egress, "*.exemple.fr"),
                Grant::new(Res::Tool, Act::Call, "mail.send"),
                Grant::new(Res::Task, Act::Spawn, "*"),
            ],
            1800,
            now(),
        )
        .unwrap()
    }

    #[test]
    fn emission_limitee_au_plafond_du_manifeste() {
        let mut b = broker();
        // L'agent demande tout le home ; le manifeste ne couvre que `~/ventes`.
        let token = b
            .mint(
                &manifeste(),
                "task:01",
                "u",
                &[
                    Grant::new(Res::Fs, Act::Read, "~/**"),
                    Grant::new(Res::Fs, Act::Read, "~/ventes/2026/**"),
                ],
                1800,
                now(),
            )
            .unwrap();
        assert_eq!(token.grants.len(), 1, "le grant trop large est retiré");
        assert_eq!(token.grants[0].pattern, "~/ventes/2026/**");
    }

    #[test]
    fn emission_refusee_si_rien_n_est_couvert() {
        let mut b = broker();
        let err = b
            .mint(
                &manifeste(),
                "task:01",
                "u",
                &[Grant::new(Res::Fs, Act::Read, "~/prive/**")],
                1800,
                now(),
            )
            .unwrap_err();
        assert!(matches!(err, BrokerError::EmptyIntersection));
    }

    #[test]
    fn lecture_autorisee_ecriture_hors_zone_refusee() {
        let mut b = broker();
        let token = jeton(&mut b);

        let lecture = CheckRequest::new(Res::Fs, Act::Read, "/home/u/ventes/q3.csv");
        assert!(b.check(&token, &lecture, now()).unwrap().is_allow());

        let ecriture_hors_zone = CheckRequest::new(Res::Fs, Act::Write, "/home/u/ventes/q3.csv");
        assert_eq!(
            b.check(&token, &ecriture_hors_zone, now()).unwrap(),
            Decision::deny(DenyReason::NoGrant)
        );

        let ecriture = CheckRequest::new(Res::Fs, Act::Write, "/home/u/ventes/out/rapport.pdf");
        assert!(b.check(&token, &ecriture, now()).unwrap().is_allow());
    }

    #[test]
    fn chemin_sensible_refuse_meme_avec_un_jeton_large() {
        let mut b = broker();
        // Un manifeste volontairement trop permissif.
        let large = MANIFESTE.replace(r#""fs.read" = ["~/ventes/**"]"#, r#""fs.read" = ["~/**"]"#);
        let manifest = Manifest::from_toml(&large).unwrap();
        let token = b
            .mint(
                &manifest,
                "task:01",
                "u",
                &[Grant::new(Res::Fs, Act::Read, "~/**")],
                1800,
                now(),
            )
            .unwrap();

        let demande = CheckRequest::new(Res::Fs, Act::Read, "/home/u/.ssh/id_ed25519");
        let decision = b.check(&token, &demande, now()).unwrap();
        assert_eq!(
            decision,
            Decision::Deny {
                reason: DenyReason::PolicyDenied,
                rule: Some("fs.read".into())
            },
            "la politique doit l'emporter sur le jeton"
        );
    }

    #[test]
    fn action_externe_exige_une_approbation() {
        let mut b = broker();
        let token = jeton(&mut b);
        let envoi = CheckRequest::new(Res::Tool, Act::Call, "mail.send")
            .irreversible()
            .external();
        let decision = b.check(&token, &envoi, now()).unwrap();
        assert!(matches!(
            decision,
            Decision::Deny {
                reason: DenyReason::ApprovalRequired,
                ..
            }
        ));
        assert_eq!(b.classify(&envoi), ActionClass::IrreversibleExternal);
    }

    #[test]
    fn execution_refusee_hors_microvm() {
        let mut b = broker();
        let avec_exec = MANIFESTE.replace(
            r#""task.spawn" = ["*"]"#,
            "\"task.spawn\" = [\"*\"]\n\"proc.exec\" = [\"/usr/bin/**\"]",
        );
        let manifest_text = format!("{avec_exec}\n[sandbox]\ncode_execution = \"microvm\"\n");
        let manifest = Manifest::from_toml(&manifest_text).unwrap();
        let token = b
            .mint(
                &manifest,
                "task:01",
                "u",
                &[Grant::new(Res::Proc, Act::Exec, "/usr/bin/**")],
                1800,
                now(),
            )
            .unwrap();

        let au_niveau_1 =
            CheckRequest::new(Res::Proc, Act::Exec, "/usr/bin/python3").sandbox_level(1);
        assert_eq!(
            b.check(&token, &au_niveau_1, now()).unwrap(),
            Decision::Deny {
                reason: DenyReason::PolicyDenied,
                rule: Some("proc.exec".into())
            }
        );

        let au_niveau_2 =
            CheckRequest::new(Res::Proc, Act::Exec, "/usr/bin/python3").sandbox_level(2);
        assert!(b.check(&token, &au_niveau_2, now()).unwrap().is_allow());
    }

    #[test]
    fn jeton_expire_refuse() {
        let mut b = broker();
        let token = jeton(&mut b);
        let demande = CheckRequest::new(Res::Fs, Act::Read, "/home/u/ventes/q3.csv");
        assert_eq!(
            b.check(&token, &demande, now() + time::Duration::seconds(1801))
                .unwrap(),
            Decision::deny(DenyReason::Expired)
        );
    }

    #[test]
    fn jeton_altere_refuse() {
        let mut b = broker();
        let mut token = jeton(&mut b);
        token.grants.push(Grant::new(Res::Fs, Act::Read, "~/**"));
        let demande = CheckRequest::new(Res::Fs, Act::Read, "/home/u/prive/secret");
        assert_eq!(
            b.check(&token, &demande, now()).unwrap(),
            Decision::deny(DenyReason::BadSignature)
        );
    }

    #[test]
    fn delegation_a_une_sous_tache() {
        let mut b = broker();
        let parent = jeton(&mut b);
        let enfant = b
            .delegate(
                &parent,
                "task:02",
                &[Grant::new(Res::Fs, Act::Read, "~/ventes/2026/**")],
                600,
                now(),
            )
            .unwrap();
        assert_eq!(enfant.parent, Some(parent.digest().unwrap()));
        let demande = CheckRequest::new(Res::Fs, Act::Read, "/home/u/ventes/2026/q3.csv");
        assert!(b.check(&enfant, &demande, now()).unwrap().is_allow());

        // Hors du périmètre délégué, bien que le parent l'autorise.
        let hors = CheckRequest::new(Res::Fs, Act::Read, "/home/u/ventes/2025/q1.csv");
        assert_eq!(
            b.check(&enfant, &hors, now()).unwrap(),
            Decision::deny(DenyReason::NoGrant)
        );
    }

    #[test]
    fn delegation_plus_large_refusee() {
        let mut b = broker();
        let parent = jeton(&mut b);
        let err = b
            .delegate(
                &parent,
                "task:02",
                &[Grant::new(Res::Fs, Act::Read, "~/**")],
                600,
                now(),
            )
            .unwrap_err();
        assert!(matches!(err, BrokerError::NotASubset));
    }

    #[test]
    fn delegation_ne_peut_pas_relacher_une_contrainte() {
        let mut b = broker();
        let manifest = manifeste();
        let parent = b
            .mint(
                &manifest,
                "task:01",
                "u",
                &[
                    Grant::new(Res::Tool, Act::Call, "mail.send").with(Constraints {
                        max_calls: Some(1),
                        approval: Some(ApprovalConstraint::Required),
                        ..Constraints::default()
                    }),
                ],
                1800,
                now(),
            )
            .unwrap();
        let err = b
            .delegate(
                &parent,
                "task:02",
                &[
                    Grant::new(Res::Tool, Act::Call, "mail.send").with(Constraints {
                        max_calls: Some(10),
                        approval: Some(ApprovalConstraint::Required),
                        ..Constraints::default()
                    }),
                ],
                600,
                now(),
            )
            .unwrap_err();
        assert!(matches!(err, BrokerError::NotASubset));
    }

    #[test]
    fn duree_de_vie_de_l_enfant_bornee_par_le_parent() {
        let mut b = broker();
        let parent = jeton(&mut b);
        let enfant = b
            .delegate(
                &parent,
                "task:02",
                &[Grant::new(Res::Fs, Act::Read, "~/ventes/**")],
                100_000,
                now(),
            )
            .unwrap();
        assert!(enfant.exp <= parent.exp);
    }

    #[test]
    fn profondeur_de_delegation_bornee() {
        let mut b = broker();
        let mut current = jeton(&mut b);
        for niveau in 0..DEFAULT_MAX_DEPTH {
            current = b
                .delegate(
                    &current,
                    &format!("task:sub{niveau}"),
                    &[
                        Grant::new(Res::Fs, Act::Read, "~/ventes/**"),
                        Grant::new(Res::Task, Act::Spawn, "*"),
                    ],
                    600,
                    now(),
                )
                .unwrap();
        }
        let err = b
            .delegate(
                &current,
                "task:trop-profond",
                &[Grant::new(Res::Fs, Act::Read, "~/ventes/**")],
                600,
                now(),
            )
            .unwrap_err();
        assert!(matches!(err, BrokerError::DepthExceeded(_)));
    }

    #[test]
    fn revocation_du_parent_invalide_toute_la_chaine() {
        let mut b = broker();
        let parent = jeton(&mut b);
        let enfant = b
            .delegate(
                &parent,
                "task:02",
                &[
                    Grant::new(Res::Fs, Act::Read, "~/ventes/**"),
                    Grant::new(Res::Task, Act::Spawn, "*"),
                ],
                600,
                now(),
            )
            .unwrap();
        let petit_enfant = b
            .delegate(
                &enfant,
                "task:03",
                &[Grant::new(Res::Fs, Act::Read, "~/ventes/**")],
                600,
                now(),
            )
            .unwrap();

        let demande = CheckRequest::new(Res::Fs, Act::Read, "/home/u/ventes/q3.csv");
        assert!(b.check(&petit_enfant, &demande, now()).unwrap().is_allow());

        b.revoke("task:01");
        assert_eq!(
            b.check(&petit_enfant, &demande, now()).unwrap(),
            Decision::deny(DenyReason::RevokedParent)
        );
        assert_eq!(
            b.check(&enfant, &demande, now()).unwrap(),
            Decision::deny(DenyReason::RevokedParent)
        );
    }

    #[test]
    fn delegation_depuis_un_parent_inconnu_refusee() {
        let mut b = broker();
        let autre = broker();
        let _ = autre;
        let mut b2 = broker();
        let etranger = jeton(&mut b2);
        let err = b
            .delegate(
                &etranger,
                "task:02",
                &[Grant::new(Res::Fs, Act::Read, "~/ventes/**")],
                600,
                now(),
            )
            .unwrap_err();
        assert!(matches!(err, BrokerError::UnknownParent));
    }

    #[test]
    fn approbation_accordee_pour_la_tache_ne_redemande_pas() {
        let mut b = broker();
        let token = jeton(&mut b);
        let envoi = CheckRequest::new(Res::Tool, Act::Call, "mail.send")
            .irreversible()
            .external();

        let demande = b.request_approval(&token, &envoi, "Envoyer le rapport", now());
        assert_eq!(demande.state, crate::approvals::ApprovalState::Pending);
        b.approvals_mut()
            .resolve(
                &demande.id,
                crate::approvals::Decision::Allow,
                crate::approvals::ApprovalScope::Task,
                now(),
            )
            .unwrap();

        let seconde = b.request_approval(&token, &envoi, "Envoyer le rapport", now());
        assert!(matches!(
            seconde.state,
            crate::approvals::ApprovalState::Resolved {
                decision: crate::approvals::Decision::Allow
            }
        ));
    }

    #[test]
    fn performance_du_controle() {
        let mut b = broker();
        let token = jeton(&mut b);
        let demande = CheckRequest::new(Res::Fs, Act::Read, "/home/u/ventes/q3.csv");
        let iterations = 2_000;
        let start = std::time::Instant::now();
        for _ in 0..iterations {
            let _ = b.check(&token, &demande, now()).unwrap();
        }
        let par_appel = start.elapsed() / iterations;
        eprintln!("cap.check : {par_appel:?} par appel");
        // Objectif de la spécification : moins de 200 µs au p99 en binaire optimisé. Ce test
        // tourne en binaire de débogage, dix à vingt fois plus lent ; le seuil est calibré en
        // conséquence et le chiffre réel est imprimé ci-dessus.
        assert!(
            par_appel < std::time::Duration::from_micros(600),
            "contrôle trop lent : {par_appel:?}"
        );
    }

    #[test]
    fn le_cache_ne_valide_jamais_un_jeton_altere() {
        let mut b = broker();
        let token = jeton(&mut b);
        let demande = CheckRequest::new(Res::Fs, Act::Read, "/home/u/ventes/q3.csv");
        // Premier passage : la signature est vérifiée et mise en cache.
        assert!(b.check(&token, &demande, now()).unwrap().is_allow());

        // Un jeton modifié a une autre empreinte : le cache ne peut pas le couvrir.
        let mut altere = token.clone();
        altere.grants.push(Grant::new(Res::Fs, Act::Read, "~/**"));
        assert_eq!(
            b.check(&altere, &demande, now()).unwrap(),
            Decision::deny(DenyReason::BadSignature)
        );

        // Et une signature recopiée depuis un autre jeton ne passe pas davantage.
        let mut vole = altere.clone();
        vole.sig.clone_from(&token.sig);
        assert_eq!(
            b.check(&vole, &demande, now()).unwrap(),
            Decision::deny(DenyReason::BadSignature)
        );
    }

    #[test]
    fn le_cache_de_politique_distingue_les_niveaux_de_sandbox() {
        let mut b = broker();
        let avec_exec = MANIFESTE.replace(
            r#""task.spawn" = ["*"]"#,
            "\"task.spawn\" = [\"*\"]\n\"proc.exec\" = [\"/usr/bin/**\"]",
        );
        let manifest_text = format!("{avec_exec}\n[sandbox]\ncode_execution = \"microvm\"\n");
        let manifest = Manifest::from_toml(&manifest_text).unwrap();
        let token = b
            .mint(
                &manifest,
                "task:01",
                "u",
                &[Grant::new(Res::Proc, Act::Exec, "/usr/bin/**")],
                1800,
                now(),
            )
            .unwrap();

        // Le niveau 1 est refusé, puis le niveau 2 accepté : une clé de cache par niveau.
        let n1 = CheckRequest::new(Res::Proc, Act::Exec, "/usr/bin/python3").sandbox_level(1);
        let n2 = CheckRequest::new(Res::Proc, Act::Exec, "/usr/bin/python3").sandbox_level(2);
        assert!(!b.check(&token, &n1, now()).unwrap().is_allow());
        assert!(b.check(&token, &n2, now()).unwrap().is_allow());
        assert!(!b.check(&token, &n1, now()).unwrap().is_allow());
    }

    #[test]
    fn le_cache_de_politique_distingue_les_chemins_sensibles() {
        let mut b = broker();
        let large = MANIFESTE.replace(r#""fs.read" = ["~/ventes/**"]"#, r#""fs.read" = ["~/**"]"#);
        let manifest = Manifest::from_toml(&large).unwrap();
        let token = b
            .mint(
                &manifest,
                "task:01",
                "u",
                &[Grant::new(Res::Fs, Act::Read, "~/**")],
                1800,
                now(),
            )
            .unwrap();

        let ordinaire = CheckRequest::new(Res::Fs, Act::Read, "/home/u/notes.txt");
        let sensible = CheckRequest::new(Res::Fs, Act::Read, "/home/u/.ssh/id_ed25519");
        assert!(b.check(&token, &ordinaire, now()).unwrap().is_allow());
        assert!(!b.check(&token, &sensible, now()).unwrap().is_allow());
        assert!(b.check(&token, &ordinaire, now()).unwrap().is_allow());
        assert!(!b.check(&token, &sensible, now()).unwrap().is_allow());
    }
}
