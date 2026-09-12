//! Le daemon `prophet-sandboxd` refuse-t-il ce qu'il ne sait pas faire ?
//!
//! C'est la seule question qui compte pour ce service. Lancer un processus est facile ; ce qui est
//! difficile — et ce qui fait la différence entre une isolation et un décor — c'est de **refuser
//! de lancer** quand la machine ne sait pas isoler au niveau exigé.
//!
//! Un agent qui croit tourner en microVM alors qu'il tourne dans un espace de noms agirait avec
//! une confiance qui ne correspond à rien. Le refus est donc la fonctionnalité, pas l'échec.
//!
//! Ces tests tournent partout, y compris dans un conteneur sans KVM ni gVisor — parce que c'est
//! précisément la machine sur laquelle un mauvais repli serait invisible.

use prophet_daemon::essai::Daemon;
use serde_json::json;

const PROGRAMME: &str = env!("CARGO_BIN_EXE_prophet-sandboxd");

async fn daemon(temp: &tempfile::TempDir) -> (Daemon, prophet_ipc::Client) {
    let daemon = Daemon::lancer(
        PROGRAMME,
        &temp.path().join("sandboxd.sock"),
        &temp.path().join("etat"),
    );
    let client = daemon.joindre().await;
    (daemon, client)
}

#[tokio::test]
async fn la_machine_dit_ce_qu_elle_sait_isoler_et_ce_qui_lui_manque() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let (_daemon, client) = daemon(&temp).await;

    let caps = client
        .call("sandbox.capabilities", json!({}))
        .await
        .expect("une machine doit pouvoir dire ce qu'elle sait faire");

    assert!(
        caps["max_level"].as_u64().is_some(),
        "le niveau maximal doit être un nombre : {caps}"
    );
    assert!(
        caps["report"].as_str().is_some_and(|r| !r.is_empty()),
        "et le rapport doit expliquer pourquoi, pas seulement chiffrer : {caps}"
    );
    // Les trois champs qui distinguent « absent » de « présent mais refusé ». Les confondre a déjà
    // coûté une demi-journée (ADR-0006) ; le daemon doit les rendre séparément.
    assert!(caps["user_namespaces"].is_boolean());
    assert!(caps["userns_restreint_par_politique"].is_boolean());
    assert!(caps["kvm"].is_boolean());
}

#[tokio::test]
async fn le_niveau_exige_est_calcule_et_non_negocie() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let (_daemon, client) = daemon(&temp).await;

    let cas = [
        // (manifeste, jeton, exécute du code, attendu)
        (0, 0, false, 0),
        (1, 0, false, 1),
        (0, 2, false, 2),
        // L'exécution de code force le niveau 2, quoi qu'en disent le manifeste et le jeton.
        (0, 0, true, 2),
        (1, 1, true, 2),
    ];
    for (manifeste, jeton, execute, attendu) in cas {
        let reponse = client
            .call(
                "sandbox.min_level",
                json!({
                    "manifest_min": manifeste,
                    "token_min": jeton,
                    "executes_code": execute
                }),
            )
            .await
            .expect("le niveau se calcule");
        assert_eq!(
            reponse["level"], attendu,
            "manifeste {manifeste}, jeton {jeton}, code {execute} → attendu {attendu}"
        );
    }
}

#[tokio::test]
async fn un_niveau_que_la_machine_ne_tient_pas_est_refuse_et_non_abaisse() {
    // Le test central. On demande un niveau au-dessus de ce que la machine sait faire, et on
    // vérifie qu'elle refuse — et non qu'elle lance à un niveau plus bas « en attendant ».
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let (_daemon, client) = daemon(&temp).await;

    let caps = client
        .call("sandbox.capabilities", json!({}))
        .await
        .expect("capacités");
    let maximum = caps["max_level"].as_u64().expect("un nombre");
    if maximum >= 2 {
        // Sur une machine complète il n'y a rien à refuser : le test n'a alors pas d'objet, et le
        // dire vaut mieux que de le faire passer pour concluant.
        eprintln!("machine capable du niveau {maximum} : aucun refus à observer ici");
        return;
    }

    let trop_haut = maximum + 1;
    let erreur = client
        .call(
            "sandbox.start",
            json!({
                "task": "task:trop-exigeante",
                "spec": {
                    "level": trop_haut,
                    "program": "/bin/true",
                    "args": [],
                    "workdir": "/",
                    "env": [],
                    "rules": {
                        "paths": [],
                        "egress": [],
                        "exec": [],
                        "min_sandbox_level": 0
                    },
                    "read_only_mounts": []
                }
            }),
        )
        .await
        .expect_err("une machine qui ne sait pas isoler à ce niveau doit refuser de lancer");

    assert_eq!(erreur.code, prophet_ipc::ErrorCode::SandboxError);
    assert!(
        erreur.message.contains(&trop_haut.to_string()),
        "le refus doit nommer le niveau demandé, obtenu : {}",
        erreur.message
    );

    // Et rien ne doit tourner : un refus qui laisse un processus derrière lui n'est pas un refus.
    let liste = client
        .call("sandbox.list", json!({}))
        .await
        .expect("la liste se lit");
    assert_eq!(
        liste.as_array().map(Vec::len),
        Some(0),
        "aucune sandbox ne doit survivre à un refus : {liste}"
    );
}

#[tokio::test]
async fn une_sandbox_inconnue_ne_se_gele_pas() {
    let temp = tempfile::tempdir().expect("répertoire temporaire");
    let (_daemon, client) = daemon(&temp).await;

    for methode in ["sandbox.freeze", "sandbox.thaw", "sandbox.kill"] {
        let erreur = client
            .call(methode, json!({ "task": "task:fantome" }))
            .await
            .unwrap_err();
        assert_eq!(
            erreur.code,
            prophet_ipc::ErrorCode::NotFound,
            "{methode} sur une tâche inexistante"
        );
    }
}
