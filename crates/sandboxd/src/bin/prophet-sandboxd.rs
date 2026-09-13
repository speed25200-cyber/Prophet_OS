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
    vivantes: Mutex<std::collections::HashMap<String, Arc<Mutex<SandboxHandle>>>>,
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
                vivantes.insert(tache.clone(), Arc::new(Mutex::new(poignee)));
                tracing::info!(%tache, niveau, pid, "sandbox démarrée");
                Ok(json!({ "task": tache, "pid": pid, "level": niveau }))
            }

            // Exécuter et attendre : une commande d'agent (`proc.exec`) vit le temps de la
            // réponse, sa sortie est rendue bornée, et le délai la tue plutôt que de laisser
            // l'appelant attendre. La sandbox est la même que pour `sandbox.start`.
            "sandbox.run" => {
                let tache = commun::texte(&params, "task")?;
                let spec: SandboxSpec = params
                    .get("spec")
                    .ok_or_else(|| Error::new(ErrorCode::InvalidParams, "« spec » attendu"))
                    .and_then(|v| {
                        serde_json::from_value(v.clone()).map_err(|e| {
                            Error::new(ErrorCode::InvalidParams, format!("« spec » invalide : {e}"))
                        })
                    })?;
                let delai = params
                    .get("timeout_s")
                    .and_then(Value::as_u64)
                    .unwrap_or(60)
                    .clamp(1, 600);
                let borne = usize::try_from(
                    params
                        .get("max_bytes")
                        .and_then(Value::as_u64)
                        .unwrap_or(256 * 1024)
                        .clamp(1024, 4 * 1024 * 1024),
                )
                .unwrap_or(256 * 1024);
                let niveau = spec.level;
                // La commande reste contrôlable pendant l'attente. Seul l'accès bref à
                // sa poignée est verrouillé, jamais l'attente ni la lecture des sorties.
                let poignee = {
                    let mut vivantes = self.vivantes.lock().await;
                    if vivantes.contains_key(&tache) {
                        return Err(Error::new(
                            ErrorCode::Conflict,
                            format!("la tâche {tache} a déjà une sandbox vivante"),
                        ));
                    }
                    let handle = Arc::new(Mutex::new(
                        self.manager.run(&tache, &spec).map_err(sandbox)?,
                    ));
                    vivantes.insert(tache.clone(), handle.clone());
                    handle
                };
                let rendu = tokio::task::block_in_place(|| {
                    let pid = poignee.blocking_lock().pid;
                    let sortie = executer_bornee(
                        &self.manager,
                        &poignee,
                        std::time::Duration::from_secs(delai),
                        borne,
                    );
                    Ok::<_, Error>(json!({
                        "task": tache,
                        "pid": pid,
                        "level": niveau,
                        "exit_code": sortie.code,
                        "timed_out": sortie.timed_out,
                        "stdout": String::from_utf8_lossy(&sortie.stdout),
                        "stderr": String::from_utf8_lossy(&sortie.stderr),
                        "truncated": sortie.truncated,
                    }))
                })?;
                let mut vivantes = self.vivantes.lock().await;
                // Un kill peut avoir retiré la commande puis une autre peut avoir repris
                // cet identifiant. Sa fin ne doit pas retirer la nouvelle commande.
                if vivantes
                    .get(&tache)
                    .is_some_and(|h| Arc::ptr_eq(h, &poignee))
                {
                    vivantes.remove(&tache);
                }
                tracing::info!(niveau, "commande exécutée sous sandbox");
                Ok(rendu)
            }

            // Geler plutôt que tuer : une tâche gelée peut être reprise après une décision
            // humaine, une tâche tuée a perdu son état.
            "sandbox.freeze_all" => {
                let mut vivantes = self.vivantes.lock().await;
                let mut frozen = Vec::new();
                let mut errors = Vec::new();
                for (task, handle) in vivantes.iter_mut() {
                    match self.manager.freeze(&mut *handle.lock().await) {
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
                let mut poignee = poignee.lock().await;
                if methode == "sandbox.freeze" {
                    self.manager.freeze(&mut poignee).map_err(sandbox)?;
                } else {
                    self.manager.thaw(&mut poignee).map_err(sandbox)?;
                }
                tracing::info!(%tache, %methode, "état de sandbox changé");
                commun::repondre(&poignee.state())
            }

            "sandbox.kill" => {
                let tache = commun::texte(&params, "task")?;
                let mut vivantes = self.vivantes.lock().await;
                let handle = vivantes.remove(&tache).ok_or_else(|| introuvable(&tache))?;
                let mut poignee = handle.lock().await;
                self.manager.kill(&mut poignee).map_err(sandbox)?;
                tracing::info!(%tache, "sandbox tuée");
                commun::repondre(&poignee.state())
            }

            "sandbox.status" => {
                let tache = commun::texte(&params, "task")?;
                let vivantes = self.vivantes.lock().await;
                let poignee = vivantes.get(&tache).ok_or_else(|| introuvable(&tache))?;
                let poignee = poignee.lock().await;
                Ok(json!({
                    "task": tache,
                    "pid": poignee.pid,
                    "state": serde_json::to_value(poignee.state())
                        .unwrap_or(Value::Null),
                }))
            }

            "sandbox.list" => {
                let vivantes = self.vivantes.lock().await;
                let mut liste = Vec::new();
                for (tache, poignee) in vivantes.iter() {
                    liste.push(json!({ "task": tache, "pid": poignee.lock().await.pid }));
                }
                Ok(json!(liste))
            }

            autre => Err(commun::methode_inconnue(autre)),
        }
    }
}

struct Sortie {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    code: Option<i32>,
    timed_out: bool,
    truncated: bool,
}

/// Lit la sortie d'une commande jusqu'à une borne, l'attend jusqu'à un délai, et la tue au-delà.
fn executer_bornee(
    manager: &Manager,
    poignee: &Mutex<SandboxHandle>,
    delai: std::time::Duration,
    borne: usize,
) -> Sortie {
    use std::io::Read as _;
    let (stdout, stderr) = match poignee.blocking_lock().child_mut() {
        Some(child) => (child.stdout.take(), child.stderr.take()),
        None => (None, None),
    };
    let lire = |flux: Option<std::process::ChildStdout>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut f) = flux {
                let _ = f.by_ref().take(borne as u64 + 1).read_to_end(&mut buf);
                // Vider le reste pour ne pas bloquer la commande sur un tube plein.
                let _ = std::io::copy(&mut f, &mut std::io::sink());
            }
            buf
        })
    };
    let lecteur_out = lire(stdout);
    let lecteur_err = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut f) = stderr {
            let _ = f.by_ref().take(64 * 1024).read_to_end(&mut buf);
            let _ = std::io::copy(&mut f, &mut std::io::sink());
        }
        buf
    });
    let debut = std::time::Instant::now();
    let mut timed_out = false;
    let code = loop {
        let statut = poignee
            .blocking_lock()
            .child_mut()
            .and_then(|c| c.try_wait().ok());
        match statut {
            Some(Some(status)) => break status.code(),
            Some(None) if debut.elapsed() < delai => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Some(None) => {
                timed_out = true;
                let mut poignee = poignee.blocking_lock();
                let _ = manager.kill(&mut poignee);
                let _ = poignee.wait();
                break None;
            }
            None => break None,
        }
    };
    let _ = poignee.blocking_lock().wait();
    let mut stdout = lecteur_out.join().unwrap_or_default();
    let stderr = lecteur_err.join().unwrap_or_default();
    let truncated = stdout.len() > borne;
    stdout.truncate(borne);
    Sortie {
        stdout,
        stderr,
        code,
        timed_out,
        truncated,
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
