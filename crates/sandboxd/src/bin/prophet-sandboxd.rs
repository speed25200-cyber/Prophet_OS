//! `prophet-sandboxd` — le gestionnaire d'isolation, en service.
//!
//! « Tout processus non fiable tourne sous `sandboxd` au niveau requis. » Ce programme est ce qui
//! rend cette phrase applicable : il est le seul du système à avoir les capacités nécessaires pour
//! projeter des identifiants et ouvrir `/dev/kvm`, et il ne les prête à personne.
//!
//! Il tient une règle que la bibliothèque connaît déjà et que le service ne doit jamais assouplir :
//! **le niveau demandé est un plancher, pas un souhait**. Si la machine ne sait pas isoler au
//! niveau exigé, la tâche ne démarre pas. Elle ne démarre pas « à un niveau plus bas en
//! attendant » : un agent qui croit tourner en microVM alors qu'il tourne dans un espace de noms
//! agirait avec une confiance qui ne correspond à rien.
//!
//! C'est pour cette raison que `sandbox.capabilities` existe et qu'il rend le rapport complet.
//! Une machine qui ne peut pas doit le dire avant, pas pendant.

use std::sync::Arc;

use prophet_daemon as commun;
use prophet_ipc::{Error, ErrorCode, Handler, PeerIdentity, Server};
use sandboxd::{Manager, SandboxHandle, SandboxSpec};
use serde_json::{Value, json};
use tokio::sync::Mutex;

struct Isolation {
    manager: Manager,
    /// Les sandboxes vivantes, par identifiant de tâche.
    vivantes: Mutex<std::collections::HashMap<String, SandboxHandle>>,
    pairs: commun::Pairs,
}

impl Handler for Isolation {
    async fn call(
        &self,
        pair: PeerIdentity,
        _auth: Option<String>,
        methode: String,
        params: Value,
    ) -> Result<Value, Error> {
        if methode != "ping" && !self.pairs.autorise(pair) {
            tracing::warn!(uid = pair.uid, gid = pair.gid, %methode, "pair refusé");
            return Err(self.pairs.refus());
        }

        match methode.as_str() {
            "ping" => Ok(json!("pong")),

            // Ce que cette machine sait réellement isoler. Le rapport est rendu entier, y compris
            // ce qui manque : c'est ce qui permet à `prophet status` de dire « niveau 2
            // indisponible, faute d'images d'invité » plutôt que « erreur ».
            "sandbox.capabilities" => {
                let caps = self.manager.capabilities();
                Ok(json!({
                    "max_level": caps.max_level(),
                    "landlock_abi": caps.landlock_abi,
                    "seccomp": caps.seccomp,
                    "user_namespaces": caps.user_namespaces,
                    "userns_restreint_par_politique": caps.userns_restreint_par_politique,
                    "cgroups_v2": caps.cgroups_v2,
                    "kvm": caps.kvm,
                    "runsc": caps.runsc,
                    "firecracker": caps.firecracker,
                    "microvm_images": caps.microvm_images.is_some(),
                    "report": caps.report(),
                }))
            }

            // Le niveau qu'une tâche exige, calculé et non négocié : le plus élevé de ce que
            // demandent son manifeste et son jeton, et au moins 2 si elle exécute du code.
            "sandbox.min_level" => {
                let manifeste = entier(&params, "manifest_min")?;
                let jeton = entier(&params, "token_min")?;
                let execute = params
                    .get("executes_code")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                Ok(json!({
                    "level": Manager::required_level(manifeste, jeton, execute)
                }))
            }

            "sandbox.start" => {
                let tache = commun::texte(&params, "task")?;
                let spec: SandboxSpec = params
                    .get("spec")
                    .ok_or_else(|| Error::new(ErrorCode::InvalidParams, "« spec » attendu"))
                    .and_then(|v| {
                        serde_json::from_value(v.clone()).map_err(|e| {
                            Error::new(ErrorCode::InvalidParams, format!("« spec » invalide : {e}"))
                        })
                    })?;

                let mut vivantes = self.vivantes.lock().await;
                if vivantes.contains_key(&tache) {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        format!("la tâche {tache} a déjà une sandbox vivante"),
                    ));
                }
                let niveau = spec.level;
                let poignee = self.manager.run(&tache, &spec).map_err(sandbox)?;
                let pid = poignee.pid;
                vivantes.insert(tache.clone(), poignee);
                tracing::info!(%tache, niveau, pid, "sandbox démarrée");
                Ok(json!({ "task": tache, "pid": pid, "level": niveau }))
            }

            // Geler plutôt que tuer : une tâche gelée peut être reprise après une décision
            // humaine, une tâche tuée a perdu son état.
            "sandbox.freeze_all" => {
                let mut vivantes = self.vivantes.lock().await;
                let mut frozen = Vec::new();
                let mut errors = Vec::new();
                for (task, handle) in vivantes.iter_mut() {
                    match self.manager.freeze(handle) {
                        Ok(()) => frozen.push(task.clone()),
                        Err(error) => errors.push(json!({"task":task, "error":error.to_string()})),
                    }
                }
                frozen.sort();
                Ok(json!({"frozen":frozen, "errors":errors}))
            }

            "sandbox.freeze" | "sandbox.thaw" => {
                let tache = commun::texte(&params, "task")?;
                let mut vivantes = self.vivantes.lock().await;
                let poignee = vivantes
                    .get_mut(&tache)
                    .ok_or_else(|| introuvable(&tache))?;
                if methode == "sandbox.freeze" {
                    self.manager.freeze(poignee).map_err(sandbox)?;
                } else {
                    self.manager.thaw(poignee).map_err(sandbox)?;
                }
                tracing::info!(%tache, %methode, "état de sandbox changé");
                commun::repondre(&poignee.state())
            }

            "sandbox.kill" => {
                let tache = commun::texte(&params, "task")?;
                let mut vivantes = self.vivantes.lock().await;
                let mut poignee = vivantes.remove(&tache).ok_or_else(|| introuvable(&tache))?;
                self.manager.kill(&mut poignee).map_err(sandbox)?;
                tracing::info!(%tache, "sandbox tuée");
                commun::repondre(&poignee.state())
            }

            "sandbox.status" => {
                let tache = commun::texte(&params, "task")?;
                let vivantes = self.vivantes.lock().await;
                let poignee = vivantes.get(&tache).ok_or_else(|| introuvable(&tache))?;
                Ok(json!({
                    "task": tache,
                    "pid": poignee.pid,
                    "state": serde_json::to_value(poignee.state())
                        .unwrap_or(Value::Null),
                }))
            }

            "sandbox.list" => {
                let vivantes = self.vivantes.lock().await;
                let liste: Vec<_> = vivantes
                    .iter()
                    .map(|(tache, poignee)| json!({ "task": tache, "pid": poignee.pid }))
                    .collect();
                Ok(json!(liste))
            }

            autre => Err(commun::methode_inconnue(autre)),
        }
    }
}

fn entier(params: &Value, nom: &str) -> Result<u8, Error> {
    params
        .get(nom)
        .and_then(Value::as_u64)
        .and_then(|n| u8::try_from(n).ok())
        .ok_or_else(|| {
            Error::new(
                ErrorCode::InvalidParams,
                format!("paramètre « {nom} » attendu, entier de 0 à 255"),
            )
        })
}

fn introuvable(tache: &str) -> Error {
    Error::new(
        ErrorCode::NotFound,
        format!("aucune sandbox vivante pour {tache}"),
    )
}

/// Une erreur d'isolation en dit plus qu'un code : `LevelUnavailable` porte le rapport complet de
/// ce que la machine sait faire, et c'est cela qui permet de comprendre le refus sans se connecter
/// à la machine.
fn sandbox(erreur: sandboxd::SandboxError) -> Error {
    Error::with_data(
        ErrorCode::SandboxError,
        erreur.to_string(),
        json!({ "detail": erreur.to_string() }),
    )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    commun::journaliser();

    let socket = commun::socket("sandboxd");
    // L'amorçage qui s'exécute *dans* la sandbox. Il est cherché à côté de ce programme, parce que
    // les deux sont installés ensemble et qu'un chemin absolu codé en dur serait faux dès qu'on
    // change de préfixe.
    let aide = std::env::var("PROPHET_SANDBOX_HELPER").unwrap_or_else(|_| {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.join("prophet-sandbox-helper")))
            .map_or_else(
                || "prophet-sandbox-helper".to_owned(),
                |chemin| chemin.display().to_string(),
            )
    });

    let manager = Manager::new(&aide);
    let caps = manager.capabilities();
    tracing::info!(
        aide = %aide,
        niveau_max = caps.max_level(),
        "sandboxd écoute — {}",
        caps.report()
    );

    let serveur = Server::bind(&socket)?;
    serveur
        .serve(Arc::new(Isolation {
            manager,
            vivantes: Mutex::new(std::collections::HashMap::new()),
            pairs: commun::Pairs::detecter()?,
        }))
        .await?;
    Ok(())
}
