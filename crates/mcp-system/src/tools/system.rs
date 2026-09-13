//! Outils système restants : exécution, approbations, journal, mémoire, secrets, notification.
//!
//! Ils partagent une propriété : chacun est un point d'entrée **contrôlé** vers un daemon qui,
//! lui, applique la règle. L'outil ne décide de rien ; il traduit une intention du modèle en appel
//! vérifiable, et rend une erreur nommée quand le daemon n'est pas là.

use serde_json::{Value, json};

use crate::protocol::{CallResult, ErrorCode, ToolMeta, ToolSpec};
use crate::registry::{Tool, ToolContext};

/// Vrai si le programme, désigné par son nom nu, est un utilitaire confiné ; un chemin ne l'est
/// jamais. La liste vit dans `prophet_types::exec` pour que le manifeste, la politique de capd
/// et cet outil disent la même chose (ADR 0031).
#[must_use]
pub fn is_safe_binary(program: &str) -> bool {
    prophet_types::exec::is_safe_utility(program)
}

/// Niveau de sandbox exigé par une commande : la liste blanche tourne confinée sur place
/// (niveau 0 : espaces de noms, Landlock, seccomp), tout autre programme en microVM. Un niveau
/// demandé ne s'abaisse jamais.
#[must_use]
pub fn required_level_for(program: &str, requested: Option<u8>) -> u8 {
    let base = if is_safe_binary(program) { 0 } else { 2 };
    requested.map_or(base, |asked| asked.max(base))
}

fn string_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)?.as_str().map(ToOwned::to_owned)
}

/// Exécution d'une commande sous sandboxd.
///
/// Le programme tourne dans l'espace de travail de la tâche, confiné sur place (niveau 0) pour
/// la liste blanche, en microVM (niveau 2) pour tout le reste ; le home n'est lisible que selon
/// les droits du jeton, et jamais inscriptible : ce qu'une commande écrit va dans l'espace de
/// travail, à examiner comme toute écriture (ADR 0031).
#[derive(Debug, Default)]
pub struct Exec {
    sandboxd: Option<std::path::PathBuf>,
}

impl Exec {
    /// Un outil qui exécute par ce socket de sandboxd.
    #[must_use]
    pub fn via(sandboxd: std::path::PathBuf) -> Self {
        Self {
            sandboxd: Some(sandboxd),
        }
    }

    /// Ce que la commande deviendrait sous sandbox : programme résolu, niveau, règles, sans
    /// rien lancer, et la cible telle que capd la juge (le nom nu, ou le chemin demandé). Rendu
    /// séparément pour être vérifiable sans sandboxd.
    ///
    /// # Errors
    /// Programme absent ou chemin refusé, arguments invalides.
    pub fn plan(
        args: &Value,
        context: &ToolContext,
    ) -> Result<(sandboxd::SandboxSpec, String), CallResult> {
        let Some(program) = string_arg(args, "program") else {
            return Err(CallResult::error(
                ErrorCode::Invalid,
                "argument `program` manquant",
            ));
        };
        let arguments: Vec<String> = match args.get("args") {
            None => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|v| {
                    v.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                        CallResult::error(ErrorCode::Invalid, "args : chaînes attendues")
                    })
                })
                .collect::<Result<_, _>>()?,
            Some(_) => {
                return Err(CallResult::error(
                    ErrorCode::Invalid,
                    "args : liste attendue",
                ));
            }
        };
        let resolved = resolve_program(&program).ok_or_else(|| {
            CallResult::error(
                ErrorCode::NotFound,
                format!("programme introuvable : {program}"),
            )
        })?;
        // La cible jugée par capd est le programme tel que demandé : un nom nu que le PATH du
        // service a résolu, ou un chemin choisi par l'appelant (jamais confiné sur place).
        let target = program.clone();
        let level = required_level_for(
            &target,
            args.get("level").and_then(Value::as_u64).map(|v| v as u8),
        );
        // Les règles viennent du jeton, en lecture seule : une commande ne modifie jamais le
        // home, elle écrit dans l'espace de travail de la tâche, et seulement là.
        let mut rules = capd::enforce::ruleset_for(&context.token, &context.home);
        for rule in &mut rules.paths {
            rule.write = false;
        }
        rules.paths.push(capd::enforce::PathRule {
            path: context.workdir.clone(),
            read: true,
            write: true,
        });
        rules.paths.sort_by(|a, b| a.path.cmp(&b.path));
        let mut spec = sandboxd::SandboxSpec::new(level, resolved, context.workdir.clone())
            .args(arguments)
            .rules(rules);
        spec.env = vec![
            ("HOME".to_owned(), context.workdir.clone()),
            (
                "PATH".to_owned(),
                std::env::var("PATH")
                    .unwrap_or_else(|_| "/run/current-system/sw/bin:/usr/bin:/bin".to_owned()),
            ),
            ("LANG".to_owned(), "C.UTF-8".to_owned()),
        ];
        Ok((spec, target))
    }
}

/// Résout un nom de programme par le PATH du service, ou vérifie un chemin absolu ; jamais un
/// chemin relatif ni un `..`.
fn resolve_program(program: &str) -> Option<String> {
    if program.is_empty() || program.contains("..") {
        return None;
    }
    if program.contains('/') {
        let path = std::path::Path::new(program);
        return (path.is_absolute() && path.is_file()).then(|| program.to_owned());
    }
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(program))
            .find(|candidate| candidate.is_file())
            .and_then(|c| c.to_str().map(ToOwned::to_owned))
    })
}

impl Tool for Exec {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "proc.exec".into(),
            description: "Exécute un programme dans une sandbox, avec ses arguments, dans l'espace de travail de la tâche, et rend sa sortie (bornée) et son code de retour. Les utilitaires qui ne modifient rien (cat, ls, wc, head, tail, sort, uniq, grep, cut, tr, diff, file) tournent confinés sur place ; tout autre programme exige une microVM et une décision humaine. Le home n'est lisible que selon vos droits et n'est jamais modifié : écrivez dans l'espace de travail.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "program": {"type": "string", "description": "Nom sur le PATH ou chemin absolu."},
                    "args": {"type": "array", "items": {"type": "string"}},
                    "level": {"type": "integer", "description": "Niveau minimal souhaité ; jamais abaissé."},
                    "timeout_s": {"type": "integer", "minimum": 1, "maximum": 300, "default": 60}
                },
                "required": ["program"],
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "proc.exec".into(),
                irreversible: true,
                external: false,
                sandbox_level_min: Some(0),
            }),
        }
    }

    /// La cible est le programme tel que demandé : le nom nu, ou le chemin. Réduire un chemin à
    /// son nom de base ferait passer `/tmp/x/cat` pour l'utilitaire `cat`.
    fn target(&self, args: &Value, _context: &ToolContext) -> Option<String> {
        string_arg(args, "program")
    }

    /// Les utilitaires de la liste ne modifient rien : ils s'exécutent sans décision humaine.
    /// Tout autre programme est tenu pour irréversible.
    fn effects(&self, args: &Value, _meta: &ToolMeta) -> (bool, bool) {
        let safe = string_arg(args, "program").is_some_and(|p| is_safe_binary(&p));
        (!safe, false)
    }

    fn call(&self, args: &Value, context: &ToolContext) -> CallResult {
        let Some(socket) = &self.sandboxd else {
            return CallResult::error(
                ErrorCode::SandboxError,
                "sandboxd n'est pas configuré pour ce service ; aucune commande n'a été lancée",
            );
        };
        let (spec, target) = match Self::plan(args, context) {
            Ok(p) => p,
            Err(e) => return e,
        };
        let timeout = args
            .get("timeout_s")
            .and_then(Value::as_u64)
            .unwrap_or(60)
            .clamp(1, 300);
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(r) => r,
            Err(e) => return CallResult::error(ErrorCode::SandboxError, e.to_string()),
        };
        let level = spec.level;
        let program = spec.program.clone();
        let params = json!({
            "task": context.task,
            "spec": spec,
            "timeout_s": timeout,
            "max_bytes": 256 * 1024,
        });
        let result = runtime.block_on(async {
            let client = prophet_ipc::Client::connect(socket)
                .await
                .map_err(|e| format!("sandboxd injoignable ({}) : {e}", socket.display()))?;
            client
                .call("sandbox.run", params)
                .await
                .map_err(|e| e.message)
        });
        match result {
            Ok(value) => {
                let mut rendu = json!({
                    "program": program,
                    "name": target,
                    "level": level,
                    "exit_code": value["exit_code"],
                    "stdout": value["stdout"],
                    "stderr": value["stderr"],
                    "timed_out": value["timed_out"],
                    "truncated": value["truncated"],
                });
                if value["timed_out"].as_bool().unwrap_or(false) {
                    rendu["note"] = json!(format!("commande interrompue après {timeout} s"));
                }
                CallResult::structured(rendu)
            }
            Err(e) => CallResult::error(ErrorCode::SandboxError, e),
        }
    }
}

/// Arrêt d'une commande lancée par la tâche.
#[derive(Debug)]
pub struct Kill;

impl Tool for Kill {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "proc.kill".into(),
            description: "Arrête une commande que cette tâche a lancée.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"sandbox": {"type": "string"}},
                "required": ["sandbox"],
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "tool.call".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, _args: &Value, _context: &ToolContext) -> Option<String> {
        None
    }

    fn call(&self, _args: &Value, _context: &ToolContext) -> CallResult {
        CallResult::error(ErrorCode::SandboxError, "sandboxd n'est pas en service")
    }
}

/// Demande d'approbation humaine.
#[derive(Debug)]
pub struct RequestApproval;

impl Tool for RequestApproval {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "approval.request".into(),
            description: "Demande une décision humaine avant une action engageante. Rend un identifiant à passer à approval.wait. Formulez le résumé du point de vue de l'utilisateur : ce qui va se passer, pas la mécanique.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string"},
                    "target": {"type": "string"},
                    "summary": {"type": "string"}
                },
                "required": ["action", "summary"],
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "tool.call".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, _args: &Value, _context: &ToolContext) -> Option<String> {
        None
    }

    fn call(&self, args: &Value, _context: &ToolContext) -> CallResult {
        if string_arg(args, "summary").is_none_or(|s| s.trim().is_empty()) {
            return CallResult::error(
                ErrorCode::Invalid,
                "un résumé est obligatoire : un humain ne peut pas trancher ce qu'il ne comprend pas",
            );
        }
        CallResult::error(ErrorCode::Internal, "capd n'est pas en service")
    }
}

/// Attente d'une décision humaine.
#[derive(Debug)]
pub struct WaitApproval;

impl Tool for WaitApproval {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "approval.wait".into(),
            description:
                "Attend la décision humaine sur une demande. Rend accordé, refusé ou expiré.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "timeout_s": {"type": "integer"}
                },
                "required": ["id"],
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "tool.call".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, _args: &Value, _context: &ToolContext) -> Option<String> {
        None
    }

    fn call(&self, _args: &Value, _context: &ToolContext) -> CallResult {
        CallResult::error(ErrorCode::Internal, "capd n'est pas en service")
    }
}

/// Lecture du journal de la tâche.
#[derive(Debug)]
pub struct LedgerQuery;

impl Tool for LedgerQuery {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "ledger.query".into(),
            description: "Lit les événements du journal de cette tâche : ce qui a été fait, refusé, approuvé. Utile pour comprendre un échec sans le reproduire.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "kinds": {"type": "array", "items": {"type": "string"}},
                    "limit": {"type": "integer"}
                },
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "ledger.read".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, _args: &Value, context: &ToolContext) -> Option<String> {
        // La cible est la tâche elle-même : lire le journal d'une autre exige `ledger.read_all`,
        // qui n'est accordé à aucun agent par défaut.
        Some(context.task.clone())
    }

    fn call(&self, _args: &Value, context: &ToolContext) -> CallResult {
        let racine = std::path::Path::new(&context.home).join(".prophet/ledger");
        let Ok(store) = ledger::Store::open(&racine) else {
            return CallResult::error(ErrorCode::NotFound, "aucun journal sur cette machine");
        };
        match store.query(&ledger::Filter {
            task: Some(context.task.clone()),
            ..ledger::Filter::default()
        }) {
            Ok(events) => {
                let resume: Vec<Value> = events
                    .iter()
                    .map(|e| {
                        json!({
                            "seq": e.seq,
                            "kind": e.kind,
                            "step": e.step,
                            "payload": e.payload
                        })
                    })
                    .collect();
                CallResult::structured(json!({"events": resume, "count": events.len()}))
            }
            Err(erreur) => CallResult::error(ErrorCode::Internal, erreur.to_string()),
        }
    }
}

/// Enregistrement en mémoire.
#[derive(Debug)]
pub struct Remember;

impl Tool for Remember {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "memory.remember".into(),
            description: "Retient un fait durable sur l'utilisateur ou sa machine. À réserver à ce qui servira à d'autres tâches ; le contexte de la tâche courante n'a pas à passer par là.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "space": {"type": "string", "default": "work"},
                    "text": {"type": "string"},
                    "tags": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["text"],
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "memory.write".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, args: &Value, _context: &ToolContext) -> Option<String> {
        Some(string_arg(args, "space").unwrap_or_else(|| "work".to_owned()))
    }

    fn call(&self, args: &Value, context: &ToolContext) -> CallResult {
        let Some(texte) = string_arg(args, "text") else {
            return CallResult::error(ErrorCode::Invalid, "argument `text` manquant");
        };
        let espace =
            memoryd::Space::new(string_arg(args, "space").unwrap_or_else(|| "work".to_owned()));
        let chemin = std::path::Path::new(&context.home).join(".prophet/memoire.db");
        let Ok(store) = memoryd::Store::open(&chemin, Box::new(memoryd::HashEmbedder::default()))
        else {
            return CallResult::error(ErrorCode::Internal, "mémoire inaccessible");
        };
        match store.remember(
            &memoryd::NewEntry::fact(&espace, &texte).from_task(&context.task),
            time::OffsetDateTime::now_utc(),
        ) {
            Ok(id) => CallResult::structured(json!({"id": id, "space": espace.0})),
            Err(erreur) => CallResult::error(ErrorCode::Internal, erreur.to_string()),
        }
    }
}

/// Recherche en mémoire.
#[derive(Debug)]
pub struct Recall;

impl Tool for Recall {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "memory.search".into(),
            description: "Cherche dans la mémoire d'un espace autorisé. Rend les entrées les plus proches, avec leur provenance.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "space": {"type": "string", "default": "work"},
                    "query": {"type": "string"},
                    "limit": {"type": "integer"}
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "memory.read".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, args: &Value, _context: &ToolContext) -> Option<String> {
        Some(string_arg(args, "space").unwrap_or_else(|| "work".to_owned()))
    }

    fn call(&self, args: &Value, context: &ToolContext) -> CallResult {
        let Some(question) = string_arg(args, "query") else {
            return CallResult::error(ErrorCode::Invalid, "argument `query` manquant");
        };
        let espace =
            memoryd::Space::new(string_arg(args, "space").unwrap_or_else(|| "work".to_owned()));
        let chemin = std::path::Path::new(&context.home).join(".prophet/memoire.db");
        let Ok(store) = memoryd::Store::open(&chemin, Box::new(memoryd::HashEmbedder::default()))
        else {
            return CallResult::error(ErrorCode::Internal, "mémoire inaccessible");
        };
        let mut requete = memoryd::Query::in_space(espace, &question);
        if let Some(limite) = args.get("limit").and_then(Value::as_u64) {
            requete.limit = limite as usize;
        }
        match store.search(&requete) {
            Ok(entries) => {
                let resultats: Vec<Value> = entries
                    .iter()
                    .map(|e| {
                        json!({
                            "id": e.id,
                            "text": e.text,
                            "score": e.score,
                            "source_task": e.source_task,
                            "confidence": e.confidence
                        })
                    })
                    .collect();
                CallResult::structured(json!({"results": resultats}))
            }
            Err(erreur) => CallResult::error(ErrorCode::Internal, erreur.to_string()),
        }
    }
}

/// Liste des secrets disponibles, sans leurs valeurs.
#[derive(Debug)]
pub struct ListSecrets;

impl Tool for ListSecrets {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "secrets.list_refs".into(),
            description: "Liste les secrets utilisables, par nom et par domaine autorisé. Les valeurs ne sont jamais rendues : utilisez la référence dans un en-tête, le proxy la remplacera à la sortie.".into(),
            input_schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
            meta: Some(ToolMeta {
                requires: "tool.call".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, _args: &Value, _context: &ToolContext) -> Option<String> {
        None
    }

    fn call(&self, _args: &Value, context: &ToolContext) -> CallResult {
        let racine = std::path::Path::new(&context.home).join(".prophet");
        let Ok(coffre) = vault::Vault::open(racine.join("vault.json"), racine.join("vault.key"))
        else {
            return CallResult::error(ErrorCode::Internal, "coffre inaccessible");
        };
        let refs: Vec<Value> = coffre
            .list()
            .iter()
            .map(|info| {
                json!({
                    "ref": vault::SecretRef::new(&info.name).as_str(),
                    "domains": info.domains,
                    "header": info.header,
                    "description": info.description
                })
            })
            .collect();
        CallResult::structured(json!({"secrets": refs}))
    }
}

/// Usage d'un secret : rend une référence, jamais une valeur.
#[derive(Debug)]
pub struct UseSecret;

impl Tool for UseSecret {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "secrets.use".into(),
            description: "Rend la référence d'un secret, à placer telle quelle dans un en-tête. Le proxy y substitue la valeur au moment de la sortie, après avoir vérifié le domaine. Aucune valeur ne transite par le modèle.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"],
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "tool.call".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, _args: &Value, _context: &ToolContext) -> Option<String> {
        None
    }

    fn call(&self, args: &Value, context: &ToolContext) -> CallResult {
        let Some(nom) = string_arg(args, "name") else {
            return CallResult::error(ErrorCode::Invalid, "argument `name` manquant");
        };
        let racine = std::path::Path::new(&context.home).join(".prophet");
        let Ok(coffre) = vault::Vault::open(racine.join("vault.json"), racine.join("vault.key"))
        else {
            return CallResult::error(ErrorCode::Internal, "coffre inaccessible");
        };
        match coffre.info(&nom) {
            Some(info) => CallResult::structured(json!({
                "ref": vault::SecretRef::new(&nom).as_str(),
                "header": info.header,
                "domains": info.domains,
                "note": "placez cette référence dans l'en-tête ; la valeur est substituée à la sortie"
            })),
            None => CallResult::error(ErrorCode::NotFound, format!("secret inconnu : {nom}")),
        }
    }
}

/// Notification hors bande à l'humain.
#[derive(Debug)]
pub struct Notify;

impl Tool for Notify {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "notify.human".into(),
            description: "Signale quelque chose à l'utilisateur sans interrompre la tâche. À employer avec parcimonie : une notification qui n'appelle pas d'action est du bruit.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": {"type": "string"},
                    "urgency": {"type": "string", "enum": ["basse", "normale", "haute"]}
                },
                "required": ["message"],
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "tool.call".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, _args: &Value, _context: &ToolContext) -> Option<String> {
        None
    }

    fn call(&self, args: &Value, _context: &ToolContext) -> CallResult {
        let Some(message) = string_arg(args, "message") else {
            return CallResult::error(ErrorCode::Invalid, "argument `message` manquant");
        };
        CallResult::structured(json!({
            "delivered": false,
            "message": message,
            "note": "aucun canal de notification en service ; le message figure au journal"
        }))
    }
}

/// Modèles disponibles.
#[derive(Debug)]
pub struct ListModels;

impl Tool for ListModels {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "model.list".into(),
            description: "Liste les pilotes et modèles utilisables, avec leur mode d'authentification et l'état de leur session.".into(),
            input_schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
            meta: Some(ToolMeta {
                requires: "tool.call".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, _args: &Value, _context: &ToolContext) -> Option<String> {
        None
    }

    fn call(&self, _args: &Value, context: &ToolContext) -> CallResult {
        let racine = std::path::Path::new(&context.home).join(".local/state/prophet");
        let utilisateur = &context.token.user;
        let pilotes: Vec<Value> = providers::official::ClientProfile::all()
            .into_iter()
            .map(|profile| {
                let driver =
                    providers::official::OfficialDriver::new(profile.clone(), &racine, utilisateur);
                json!({
                    "driver": profile.driver,
                    "preferred_auth": "subscription",
                    "client_present": driver.client_available(),
                    "connection": driver.connection_state(),
                    "agent_execution_ready": false
                })
            })
            .collect();
        CallResult::structured(json!({"drivers": pilotes}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_commande_inconnue_impose_la_microvm() {
        assert_eq!(required_level_for("/usr/bin/python3", None), 2);
        assert_eq!(required_level_for("/tmp/installeur.sh", Some(0)), 2);
    }

    #[test]
    fn un_utilitaire_nomme_reste_sur_place_mais_pas_un_chemin() {
        assert_eq!(required_level_for("cat", None), 0);
        assert_eq!(required_level_for("/bin/cat", None), 2);
        assert!(is_safe_binary("grep"));
        assert!(!is_safe_binary("/usr/bin/grep"));
        assert!(!is_safe_binary("curl"));
    }

    #[test]
    fn un_niveau_demande_plus_eleve_est_respecte() {
        assert_eq!(required_level_for("cat", Some(2)), 2);
        assert_eq!(required_level_for("cat", Some(1)), 1);
    }

    #[test]
    fn une_demande_d_approbation_sans_resume_est_refusee() {
        let contexte = ToolContext {
            token: prophet_types::cap::Token {
                v: 0,
                iss: "x".into(),
                sub: "task:01".into(),
                agent: "a".into(),
                user: "u".into(),
                parent: None,
                grants: Vec::new(),
                iat: time::OffsetDateTime::UNIX_EPOCH,
                exp: time::OffsetDateTime::UNIX_EPOCH,
                nonce: String::new(),
                sig: None,
            },
            task: "task:01".into(),
            home: "/tmp".into(),
            workdir: "/tmp".into(),
            sandbox_level: 1,
            step: 1,
        };
        let result = RequestApproval.call(&json!({"action": "mail.send"}), &contexte);
        assert!(result.is_error);
        assert_eq!(result.structured.unwrap()["code"], json!("Invalid"));
    }

    #[test]
    fn le_journal_de_la_tache_est_la_cible_par_defaut() {
        let contexte = ToolContext {
            token: prophet_types::cap::Token {
                v: 0,
                iss: "x".into(),
                sub: "task:01".into(),
                agent: "a".into(),
                user: "u".into(),
                parent: None,
                grants: Vec::new(),
                iat: time::OffsetDateTime::UNIX_EPOCH,
                exp: time::OffsetDateTime::UNIX_EPOCH,
                nonce: String::new(),
                sig: None,
            },
            task: "task:01".into(),
            home: "/tmp".into(),
            workdir: "/tmp".into(),
            sandbox_level: 1,
            step: 1,
        };
        assert_eq!(
            LedgerQuery.target(&json!({}), &contexte).as_deref(),
            Some("task:01"),
            "lire le journal d'une autre tâche exige ledger.read_all"
        );
    }
}
