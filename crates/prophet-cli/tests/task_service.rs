//! La CLI consulte agentd sans ouvrir les captures privées du service.

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixListener;
use std::process::{Command, Output};

use serde_json::{Value, json};

fn invoke(args: &[&str], method: &str, params: Value, result: Value) -> Output {
    let (output, request) = invoke_with(args, method, result);
    assert_eq!(request["params"], params);
    output
}

/// Comme [`invoke`], mais rend la requête reçue par le service simulé au lieu d'en imposer les
/// paramètres : utile quand une partie de la requête vient d'une transcription.
fn invoke_with(args: &[&str], method: &str, result: Value) -> (Output, Value) {
    invoke_env(args, &[], method, result)
}

/// Comme [`invoke_with`], avec des variables d'environnement pour la commande.
fn invoke_env(args: &[&str], env: &[(&str, &str)], method: &str, result: Value) -> (Output, Value) {
    let (output, mut requests) = invoke_seq(args, env, &[(method, result)]);
    (output, requests.remove(0))
}

/// Comme [`invoke_env`], mais le service simulé répond à une suite de requêtes, une par
/// connexion, chacune devant porter la méthode attendue ; rend les requêtes reçues dans l'ordre.
fn invoke_seq(
    args: &[&str],
    env: &[(&str, &str)],
    responses: &[(&str, Value)],
) -> (Output, Vec<Value>) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    std::fs::create_dir_all(home.join(".prophet")).unwrap();
    // Même sous root, cette sentinelle rend une lecture directe impossible. En production,
    // le répertoire est en 0700 sous l'UID d'agentd ; il ne faut pas élargir ses droits.
    std::fs::write(home.join(".prophet/tasks"), "capture privée du service").unwrap();
    let socket = temp.path().join("agentd.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let responses: Vec<(String, Value)> = responses
        .iter()
        .map(|(m, r)| ((*m).to_owned(), r.clone()))
        .collect();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (method, result) in responses {
            // Chaque tranche d'écoute coûte plusieurs secondes de synthèse et de transcription
            // avant l'appel suivant, bien plus quand d'autres essais vocaux tournent en même
            // temps sur les mêmes cœurs.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "aucun appel {method} à agentd"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], method, "{request}");
            writeln!(
                stream,
                "{}",
                json!({"jsonrpc":"2.0", "id":request["id"], "result":result})
            )
            .unwrap();
            requests.push(request);
        }
        requests
    });
    let output = Command::new(env!("CARGO_BIN_EXE_prophet"))
        .args(args)
        .env("HOME", &home)
        .env("PROPHET_AGENTD_SOCKET", &socket)
        .env("PROPHET_CAPD_SOCKET", &socket)
        .envs(env.iter().copied())
        .output()
        .unwrap();
    let requests = server.join().unwrap();
    assert_eq!(
        std::fs::read_to_string(home.join(".prophet/tasks")).unwrap(),
        "capture privée du service"
    );
    (output, requests)
}

/// Parler à l'OS, de bout en bout : une phrase française de synthèse, transcrite en local,
/// devient une mission préparée auprès du service (simulé ici), et l'OS répond à voix haute ;
/// sa réponse est réécoutée par Whisper. Exige les programmes des essais du crate `voice`.
#[test]
#[ignore = "needs_voice_stack: PROPHET_WHISPER_MODEL, PROPHET_WHISPER, PROPHET_PIPER, PROPHET_PIPER_VOICE, PROPHET_TEST_ESPEAK"]
fn une_phrase_dite_devient_une_mission_et_l_os_repond() {
    let espeak = std::env::var("PROPHET_TEST_ESPEAK").unwrap();
    let temp = tempfile::tempdir().unwrap();
    let phrase = temp.path().join("phrase.wav");
    assert!(
        Command::new(&espeak)
            .args(["-v", "fr", "-s", "150", "-w"])
            .arg(&phrase)
            .arg("Écris une note de réunion dans mes documents.")
            .status()
            .unwrap()
            .success()
    );
    let reponse = temp.path().join("reponse.wav");
    let plan = json!({
        "task": "mission-x", "intent": "…",
        "choice": {"reference": "local:m", "reason": "essai"},
        "sandbox_level": 0, "grants": [], "limits": agentd::Limits::default(),
        "scopes": ["~/docs"]
    });
    let (output, request) = invoke_with(
        &[
            "voice",
            "--file",
            phrase.to_str().unwrap(),
            "--language",
            "fr",
            "--prepare",
            "docs",
            "--model",
            "m",
            "--reply",
            "--out",
            reponse.to_str().unwrap(),
        ],
        "task.prepare",
        plan,
    );
    let out = success(output);
    let intent = request["params"]["intent"].as_str().unwrap().to_lowercase();
    assert!(
        intent.contains("note") && intent.contains("documents"),
        "{intent}"
    );
    assert_eq!(request["params"]["profile"], "docs");
    assert_eq!(request["params"]["model"], "m");
    assert!(
        request["params"]["id"]
            .as_str()
            .unwrap()
            .starts_with("mission-")
    );
    assert!(out.contains("Réponse dite"), "{out}");
    let tools = voice::Tools::from_env().unwrap();
    let heard = tools.transcribe(&reponse, Some("fr")).unwrap();
    eprintln!("réponse de l'OS réécoutée : « {} »", heard.text);
    let heard = heard.text.to_lowercase();
    assert!(heard.contains("prépar"), "{heard}");
    assert!(heard.contains("compris"), "{heard}");
}

/// Trois ordres à la suite, par la voix : « Prophète, écris une note… » prépare une mission,
/// « Prophète, lance la mission » la lance (c'est l'approbation de l'humain, dite), « Prophète,
/// résultat » fait dire son résultat. Le service simulé reçoit les trois appels, sur la même
/// mission ; la dernière réponse de l'OS est réécoutée par Whisper.
#[test]
#[ignore = "needs_voice_stack: PROPHET_WHISPER_MODEL, PROPHET_WHISPER, PROPHET_PIPER, PROPHET_PIPER_VOICE"]
fn la_voix_prepare_lance_puis_fait_dire_le_resultat() {
    let mut tools = voice::Tools::from_env().unwrap();
    tools.deterministic = true;
    assert!(tools.can_speak(), "{tools:?}");
    let temp = tempfile::tempdir().unwrap();
    let synth = |nom: &str, phrase: &str| {
        let wav = temp.path().join(nom);
        tools.speak(phrase, &wav).unwrap();
        wav
    };
    let tranches = [
        synth(
            "un.wav",
            "Prophète, écris une note de réunion dans mes documents.",
        ),
        synth("deux.wav", "Prophète, lance la mission."),
        synth("trois.wav", "Prophète, résultat."),
    ];
    let compteur = temp.path().join("appels");
    let script = temp.path().join("faux-pw-record.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nset -e\nn=0; [ -f {c} ] && n=$(cat {c}); n=$((n+1)); echo $n > {c}\n\
             for last; do :; done\n\
             case \"$n\" in 1) cp {a} \"$last\";; 2) cp {b} \"$last\";; *) cp {d} \"$last\";; esac\n",
            c = compteur.display(),
            a = tranches[0].display(),
            b = tranches[1].display(),
            d = tranches[2].display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let reponse = temp.path().join("reponse.wav");
    let plan = json!({
        "task": "mission-x", "intent": "…",
        "choice": {"reference": "local:m", "reason": "essai"},
        "sandbox_level": 0, "grants": [], "limits": agentd::Limits::default(),
        "scopes": ["~/docs"]
    });
    let (output, requests) = invoke_seq(
        &[
            "voice",
            "--listen",
            "--wake",
            "prophète",
            "--seconds",
            "1",
            "--rounds",
            "3",
            "--language",
            "fr",
            "--prepare",
            "docs",
            "--model",
            "m",
            "--reply",
            "--out",
            reponse.to_str().unwrap(),
        ],
        &[("PROPHET_RECORDER", script.to_str().unwrap())],
        &[
            ("task.prepare", plan),
            ("task.start", json!({"state": "running"})),
            (
                "task.result",
                json!({
                    "state": "done",
                    "text": "La note de réunion est écrite dans vos documents.",
                    "diff": {"changes": [{"path": "docs/note.md", "kind": "added"}]}
                }),
            ),
        ],
    );
    let out = success(output);
    assert_eq!(std::fs::read_to_string(&compteur).unwrap().trim(), "3");
    let id = requests[0]["params"]["id"].as_str().unwrap().to_owned();
    assert!(id.starts_with("mission-"), "{id}");
    assert_eq!(requests[1]["params"]["id"], id, "{}", requests[1]);
    assert_eq!(requests[2]["params"]["id"], id, "{}", requests[2]);
    assert!(out.contains("lancée par la voix"), "{out}");
    assert!(out.contains("Mission terminée."), "{out}");
    let heard = voice::Tools::from_env()
        .unwrap()
        .transcribe(&reponse, Some("fr"))
        .unwrap()
        .text
        .to_lowercase();
    eprintln!("dernière réponse de l'OS : « {heard} »");
    assert!(heard.contains("termin"), "{heard}");
    assert!(
        heard.contains("note") && heard.contains("documents"),
        "{heard}"
    );
}

/// Le résultat d'une mission, dit par l'OS : `task result --say --out` demande le résultat au
/// service (simulé), en fait une phrase courte, la synthétise avec Piper ; Whisper la réécoute.
#[test]
#[ignore = "needs_voice_stack: PROPHET_WHISPER_MODEL, PROPHET_WHISPER, PROPHET_PIPER, PROPHET_PIPER_VOICE"]
fn le_resultat_d_une_mission_est_dit_a_voix_haute() {
    let temp = tempfile::tempdir().unwrap();
    let reponse = temp.path().join("resultat.wav");
    let (output, _) = invoke_with(
        &[
            "task",
            "result",
            "mission-x",
            "--say",
            "--out",
            reponse.to_str().unwrap(),
        ],
        "task.result",
        json!({
            "state": "done",
            "text": "La note de réunion est écrite dans vos documents.",
            "diff": {"changes": [{"path": "docs/note.md", "kind": "added"}]}
        }),
    );
    let out = success(output);
    assert!(out.contains("Mission mission-x · done"), "{out}");
    assert!(
        out.contains("Résultat écrit dans") && out.contains("Mission terminée."),
        "{out}"
    );
    let tools = voice::Tools::from_env().unwrap();
    let heard = tools.transcribe(&reponse, Some("fr")).unwrap();
    eprintln!("résultat réécouté : « {} »", heard.text);
    let heard = heard.text.to_lowercase();
    assert!(heard.contains("termin"), "{heard}");
    assert!(
        heard.contains("note") && heard.contains("documents"),
        "{heard}"
    );
    assert!(heard.contains("changement"), "{heard}");
}

/// Sans chaîne vocale, `--say` ne rend pas un résultat muet pour un succès : le service a bien
/// été consulté, la commande échoue en nommant ce qui manque.
#[test]
fn sans_voix_le_resultat_n_est_pas_dit_et_la_commande_le_dit() {
    let (output, _) = invoke_env(
        &["task", "result", "mission-x", "--say"],
        &[("PROPHET_WHISPER_MODEL", "/nonexistent/ggml.bin")],
        "task.result",
        json!({"state": "done", "text": "Fini."}),
    );
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("modèle de parole"), "{err}");
}

/// Le mot d'activation, de bout en bout : un faux enregistreur (un script à la place de
/// `pw-record`) livre d'abord une tranche sans le mot, puis « Prophète, écris une note de réunion
/// dans mes documents » ; seule la seconde devient une mission, l'OS répond, et l'écoute s'arrête
/// au nombre de tranches demandé. Exige la chaîne vocale des essais du crate `voice`.
#[test]
#[ignore = "needs_voice_stack: PROPHET_WHISPER_MODEL, PROPHET_WHISPER, PROPHET_PIPER, PROPHET_PIPER_VOICE, PROPHET_TEST_ESPEAK"]
fn le_mot_d_activation_declenche_une_mission_et_le_reste_est_ignore() {
    // Les phrases sont dites par la voix de l'OS elle-même (Piper) : Whisper la comprend bien
    // mieux que la voix d'espeak, dont il n'attrape pas le mot d'activation.
    let mut tools = voice::Tools::from_env().unwrap();
    tools.deterministic = true;
    assert!(tools.can_speak(), "{tools:?}");
    let temp = tempfile::tempdir().unwrap();
    let synth = |nom: &str, phrase: &str| {
        let wav = temp.path().join(nom);
        tools.speak(phrase, &wav).unwrap();
        wav
    };
    let bruit = synth("bruit.wav", "Il fait beau aujourd'hui.");
    let ordre = synth(
        "ordre.wav",
        "Prophète, écris une note de réunion dans mes documents.",
    );
    // L'enregistreur factice : appelé comme pw-record (le fichier de sortie en dernier), il
    // copie le bruit au premier appel, l'ordre ensuite.
    let compteur = temp.path().join("appels");
    let script = temp.path().join("faux-pw-record.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nset -e\nn=0; [ -f {c} ] && n=$(cat {c}); n=$((n+1)); echo $n > {c}\n\
             for last; do :; done\n\
             if [ \"$n\" = 1 ]; then cp {b} \"$last\"; else cp {o} \"$last\"; fi\n",
            c = compteur.display(),
            b = bruit.display(),
            o = ordre.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let reponse = temp.path().join("reponse.wav");
    let plan = json!({
        "task": "mission-x", "intent": "…",
        "choice": {"reference": "local:m", "reason": "essai"},
        "sandbox_level": 0, "grants": [], "limits": agentd::Limits::default(),
        "scopes": ["~/docs"]
    });
    // Le service simulé ne répond qu'à une requête : la tranche de bruit ne doit rien préparer.
    let (output, request) = invoke_env(
        &[
            "voice",
            "--listen",
            "--wake",
            "prophète",
            "--seconds",
            "1",
            "--rounds",
            "2",
            "--language",
            "fr",
            "--prepare",
            "docs",
            "--model",
            "m",
            "--reply",
            "--out",
            reponse.to_str().unwrap(),
        ],
        &[("PROPHET_RECORDER", script.to_str().unwrap())],
        "task.prepare",
        plan,
    );
    let out = success(output);
    assert_eq!(std::fs::read_to_string(&compteur).unwrap().trim(), "2");
    let intent = request["params"]["intent"].as_str().unwrap().to_lowercase();
    assert!(
        intent.contains("note") && intent.contains("documents") && !intent.contains("proph"),
        "{intent}"
    );
    assert!(out.contains("Réponse dite"), "{out}");
    let heard = voice::Tools::from_env()
        .unwrap()
        .transcribe(&reponse, Some("fr"))
        .unwrap()
        .text
        .to_lowercase();
    eprintln!("réponse après le mot d'activation : « {heard} »");
    assert!(heard.contains("prépar"), "{heard}");
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn failure(output: Output) -> String {
    assert!(
        !output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8(output.stderr).unwrap()
}

fn mission() -> agentd::Task {
    agentd::Task::new(
        "task:service",
        "Préparer un document",
        "org.prophet.documents",
        "pilot",
        agentd::Budget::new(agentd::Limits::default()),
        time::OffsetDateTime::UNIX_EPOCH,
    )
}

fn inspection() -> Value {
    json!(agentd::Inspection {
        task: mission(),
        plan: None,
        result: Some(
            json!({"task":"task:service", "state":"done", "text":"Document préparé.",
            "diff":{"changes":[{"path":"Documents/Prophet/rapport.txt", "kind":"added", "size_after":24}]}})
        ),
        can_start: false,
        can_cancel: false,
        start_reason: None,
        publication: None,
        can_apply: false,
        can_undo: false,
        conflict: None,
        conflict_in_undo: false,
        can_resolve: false,
        kept: Vec::new(),
        browsing: None,
        client_hosts: None,
    })
}

#[test]
fn liste_du_service_lisible_sans_acces_aux_captures() {
    for result in [json!([]), json!([mission()])] {
        let output = success(invoke(
            &["task", "ls"],
            "task.list",
            json!({}),
            result.clone(),
        ));
        if result.as_array().unwrap().is_empty() {
            assert!(output.contains("aucune tâche"), "{output}");
        } else {
            assert!(output.contains("task:service"), "{output}");
            assert!(output.contains("Préparer un document"), "{output}");
        }
        assert!(!output.contains("aucun espace de travail"), "{output}");
    }
}

#[test]
fn le_detail_nomme_qui_mene_la_mission_et_qui_l_a_confiee() {
    let mut confiee = inspection();
    confiee["task"]["driver"] = json!("driver:claude-code");
    confiee["task"]["parent"] = json!("duo");
    confiee["task"]["role"] = json!("review");
    let rendu = success(invoke(
        &["task", "show", "task:service"],
        "task.inspect",
        json!({"id":"task:service"}),
        confiee,
    ));
    assert!(
        rendu.contains("Menée par : Claude Code (Anthropic)"),
        "{rendu}"
    );
    assert!(rendu.contains("Confiée par la mission duo"), "{rendu}");
    assert!(rendu.contains("Rôle dans le relais : review"), "{rendu}");
    let mut locale = inspection();
    locale["task"]["driver"] = json!("local:qwen3-1.7b");
    let rendu = success(invoke(
        &["task", "show", "task:service"],
        "task.inspect",
        json!({"id":"task:service"}),
        locale,
    ));
    assert!(rendu.contains("Menée par : qwen3-1.7b"), "{rendu}");
    assert!(!rendu.contains("Confiée par"), "{rendu}");
}

#[test]
fn detail_et_diff_proviennent_de_la_meme_inspection_que_la_surface() {
    for action in ["show", "diff"] {
        let output = success(invoke(
            &["task", action, "task:service"],
            "task.inspect",
            json!({"id":"task:service"}),
            inspection(),
        ));
        assert!(output.contains("Documents/Prophet/rapport.txt"), "{output}");
        assert!(output.contains("non appliqu"), "{output}");
        let output = success(invoke(
            &["--json", "task", action, "task:service"],
            "task.inspect",
            json!({"id":"task:service"}),
            inspection(),
        ));
        let value: Value = serde_json::from_str(&output).unwrap();
        if action == "show" {
            assert_eq!(value, inspection());
        } else {
            assert_eq!(value, inspection()["result"]["diff"]);
        }
    }
}

#[test]
fn les_contextes_du_service_se_lisent_et_une_mission_se_prepare_sans_manifeste_fourni() {
    let options = json!({
        "profiles":[{"id":"web","name":"Recherche sur le web","description":"Consulter le web","models":["qwen3-1.7b"],"scopes":["~/Documents/Prophet"],"grants":["net.egress sur *"],"limits":agentd::Limits::default(),"web":true}],
        "model_error":null,
        "browser":{"program":"/run/current-system/sw/bin/chromium","ready":true,"detail":"HeadlessChrome/131.0"}
    });
    let rendu = success(invoke(
        &["task", "options"],
        "task.options",
        json!({}),
        options,
    ));
    assert!(
        rendu.contains("web — Recherche sur le web (consulte le web)"),
        "{rendu}"
    );
    assert!(rendu.contains("modèles : qwen3-1.7b"), "{rendu}");
    assert!(
        rendu.contains(
            "Navigateur piloté : ✓ /run/current-system/sw/bin/chromium — HeadlessChrome/131.0"
        ),
        "{rendu}"
    );
    assert!(!rendu.contains("Clients officiels"), "{rendu}");

    // Avec un lanceur de pilotes : les clients connectés sont des modèles, nommés en premier
    // et lisibles ; les autres disent comment se connecter (ADR 0035).
    let options = json!({
        "profiles":[{"id":"documents","name":"Documents","description":"Préparer","models":["codex","qwen3-1.7b"],"scopes":["~/Documents/Prophet"],"grants":[],"limits":agentd::Limits::default(),"web":false}],
        "model_error":null,
        "browser":null,
        "pilot":{"drivers":[
            {"driver":"claude-code","connection":"logged-out","executable":"/run/current-system/sw/bin/claude","version":null},
            {"driver":"codex","connection":"connected","executable":"/run/current-system/sw/bin/codex","version":"0.40"}
        ]}
    });
    let rendu = success(invoke(
        &["task", "options"],
        "task.options",
        json!({}),
        options,
    ));
    assert!(
        rendu.contains("modèles : codex — Codex (ChatGPT), qwen3-1.7b"),
        "{rendu}"
    );
    assert!(
        rendu.contains("✓ Codex (ChatGPT) (codex) : connected"),
        "{rendu}"
    );
    assert!(
        rendu.contains(
            "✗ Claude Code (Anthropic) (claude-code) : logged-out — prophet provider login claude-code"
        ),
        "{rendu}"
    );

    // La CLI ne fournit ni manifeste, ni droits : seulement l'intention et le contexte choisi.
    let rendu = success(invoke(
        &[
            "--json",
            "task",
            "prepare",
            "--profile",
            "web",
            "--model",
            "qwen3-1.7b",
            "--id",
            "essai-cli",
            "Lire une page et noter son titre",
        ],
        "task.prepare",
        json!({"id":"essai-cli","intent":"Lire une page et noter son titre","profile":"web","model":"qwen3-1.7b"}),
        json!({"task":"essai-cli"}),
    ));
    assert_eq!(
        serde_json::from_str::<Value>(&rendu).unwrap()["task"],
        "essai-cli"
    );

    // Pour un client MCP, le service apprend que le moteur local n'est pas requis.
    let rendu = success(invoke(
        &[
            "--json",
            "task",
            "prepare",
            "--client",
            "--profile",
            "web",
            "--model",
            "qwen3-1.7b",
            "--id",
            "essai-client",
            "Travailler avec Claude Code",
        ],
        "task.prepare",
        json!({"id":"essai-client","intent":"Travailler avec Claude Code","profile":"web","model":"qwen3-1.7b","client":true}),
        json!({"task":"essai-client"}),
    ));
    assert_eq!(
        serde_json::from_str::<Value>(&rendu).unwrap()["task"],
        "essai-client"
    );
}

#[test]
fn la_configuration_mcp_ne_vise_qu_une_mission_preparee() {
    let mut planned = inspection();
    planned["task"]["state"] = json!("planned");
    planned["result"] = Value::Null;
    let rendu = success(invoke(
        &["task", "mcp-config", "task:service"],
        "task.inspect",
        json!({"id":"task:service"}),
        planned,
    ));
    let config: Value = serde_json::from_str(&rendu).unwrap();
    assert_eq!(
        config["mcpServers"]["prophet"]["env"]["PROPHET_TASK"],
        "task:service"
    );
    assert!(
        config["mcpServers"]["prophet"]["command"]
            .as_str()
            .unwrap()
            .ends_with("prophet-mcp"),
        "{config}"
    );

    // Une mission terminée n'accueille plus de client : la CLI le dit au lieu de configurer.
    let output = invoke(
        &["task", "mcp-config", "task:service"],
        "task.inspect",
        json!({"id":"task:service"}),
        inspection(),
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("seule une mission préparée"));
}

#[test]
fn une_seance_se_pilote_a_la_main_depuis_le_terminal() {
    // Ouvrir, appeler un outil, fermer : trois appels que la CLI transmet tels quels au
    // créateur de la mission, sans jeton ni fichier.
    let rendu = success(invoke(
        &["task", "attach", "essai", "--client", "essai"],
        "task.attach",
        json!({"id":"essai","client":"essai"}),
        json!({"task":"essai","tools":[{"name":"ui.tree"},{"name":"ui.act"}]}),
    ));
    assert!(rendu.contains("ui.tree, ui.act"), "{rendu}");
    let rendu = success(invoke(
        &[
            "--json",
            "task",
            "call",
            "essai",
            "ui.tree",
            r#"{"app":"mousepad"}"#,
        ],
        "task.call",
        json!({"id":"essai","name":"ui.tree","arguments":{"app":"mousepad"}}),
        json!({"content":[{"type":"text","text":"{\"nodes\":3}"}],"isError":false,"structured":{"nodes":3}}),
    ));
    assert_eq!(
        serde_json::from_str::<Value>(&rendu).unwrap()["structured"]["nodes"],
        3
    );
    // Un outil qui échoue est une erreur de la commande, avec le texte de l'outil.
    let erreur = failure(invoke(
        &[
            "task",
            "call",
            "essai",
            "ui.act",
            r#"{"app":"mousepad","action":"click","node":"9"}"#,
        ],
        "task.call",
        json!({"id":"essai","name":"ui.act","arguments":{"app":"mousepad","action":"click","node":"9"}}),
        json!({"content":[{"type":"text","text":"élément introuvable : 9"}],"isError":true}),
    ));
    assert!(erreur.contains("élément introuvable"), "{erreur}");
    let rendu = success(invoke(
        &["task", "detach", "essai", "--text", "fini"],
        "task.detach",
        json!({"id":"essai","text":"fini"}),
        json!({"task":"essai","state":"done"}),
    ));
    assert!(rendu.contains("done"), "{rendu}");
}

/// Les décisions humaines depuis le terminal (ADR 0041) : voir ce qui attend, accorder pour la
/// mission, refuser ; chaque commande parle à capd, avec les paramètres exacts de sa méthode.
#[test]
fn les_decisions_humaines_se_prennent_depuis_le_terminal() {
    let demande = json!({
        "id": "apr_1", "task": "duo", "agent": "org.prophet.atelier", "action": "tool.call",
        "target": "http.fetch", "summary": "Appeler http.fetch", "irreversible": true,
        "external": true, "created": "2026-09-14T14:00:00Z", "expires": "2026-09-15T14:00:00Z",
        "state": "pending", "reason": "Le rapport doit partir avant midi."
    });
    let rendu = success(invoke(
        &["cap", "approvals"],
        "approval.pending",
        json!({}),
        json!([demande]),
    ));
    assert!(rendu.contains("apr_1"), "{rendu}");
    assert!(rendu.contains("Appeler http.fetch"), "{rendu}");
    assert!(
        rendu.contains("irréversible, hors de la machine"),
        "{rendu}"
    );
    assert!(
        rendu.contains("le modèle dit : « Le rapport doit partir avant midi. »"),
        "{rendu}"
    );
    assert!(rendu.contains("prophet cap approve apr_1"), "{rendu}");
    let rendu = success(invoke(
        &["cap", "approvals"],
        "approval.pending",
        json!({}),
        json!([]),
    ));
    assert!(rendu.contains("Aucune décision en attente"), "{rendu}");

    let tranchee = json!({"id": "apr_1", "summary": "Appeler http.fetch", "state": {"resolved": {"decision": "allow"}}});
    let rendu = success(invoke(
        &["cap", "approve", "apr_1", "--scope", "task"],
        "approval.resolve",
        json!({"id": "apr_1", "decision": "allow", "scope": "task"}),
        tranchee.clone(),
    ));
    assert!(
        rendu.contains("Accordé : Appeler http.fetch (pour toute la mission)"),
        "{rendu}"
    );
    let rendu = success(invoke(
        &["cap", "deny", "apr_1"],
        "approval.resolve",
        json!({"id": "apr_1", "decision": "deny", "scope": "once"}),
        tranchee,
    ));
    assert!(rendu.contains("Refusé : Appeler http.fetch"), "{rendu}");
    let rendu = success(invoke(
        &["cap", "rules"],
        "approval.rules",
        json!({}),
        json!([{"id": "regle_1", "decision": "allow", "action": "tool.call", "target": "http.fetch", "task": "duo"}]),
    ));
    assert!(
        rendu.contains("allow — tool.call sur http.fetch (mission duo)"),
        "{rendu}"
    );
}

#[test]
fn une_mission_echouee_se_prepare_a_nouveau_par_son_contexte() {
    // `prophet task retry` relit la mission, puis repasse par la préparation du catalogue :
    // même intention, même contexte, même modèle, une nouvelle référence, et rien de lancé.
    let mut echouee = inspection();
    echouee["task"]["state"] = json!("failed");
    echouee["plan"] = json!({"task":"task:service","intent":"Préparer un document",
        "choice":{"reference":"local:qwen3-1.7b","reason":"préféré"},"sandbox_level":0,
        "grants":[],"limits":{"tokens":1000,"steps":10,"wall_time_s":60,"approvals":1,"cost_eur":0.0},
        "scopes":["~/Documents/Prophet"],"profile":"documents"});
    let plan = json!({"task":"relance","intent":"Préparer un document",
        "choice":{"reference":"local:qwen3-1.7b","reason":"préféré"},"sandbox_level":0,
        "grants":[],"limits":{"tokens":1000,"steps":10,"wall_time_s":60,"approvals":1,"cost_eur":0.0},
        "scopes":["~/Documents/Prophet"],"profile":"documents"});
    let (output, requetes) = invoke_seq(
        &["task", "retry", "task:service", "--id", "relance"],
        &[],
        &[("task.inspect", echouee), ("task.prepare", plan)],
    );
    let sortie = success(output);
    assert_eq!(requetes[0]["params"], json!({"id":"task:service"}));
    assert_eq!(
        requetes[1]["params"],
        json!({"id":"relance","intent":"Préparer un document","profile":"documents","model":"qwen3-1.7b"})
    );
    assert!(sortie.contains("prophet task start relance"), "{sortie}");

    // Une mission réussie, ou préparée hors du catalogue, ne se relance pas ainsi.
    let (output, _) = invoke_seq(
        &["task", "retry", "task:service"],
        &[],
        &[("task.inspect", inspection())],
    );
    assert!(failure(output).contains("échouée ou arrêtée"));
    let mut sans_profil = inspection();
    sans_profil["task"]["state"] = json!("cancelled");
    let (output, _) = invoke_seq(
        &["task", "retry", "task:service"],
        &[],
        &[("task.inspect", sans_profil)],
    );
    assert!(failure(output).contains("prophet task prepare"));
}

/// L'arrêt d'urgence depuis le terminal : `prophet task halt` appelle `task.halt` sans
/// paramètre et dit ce qui s'arrête, ce qui est annulé et ce qui a résisté.
#[test]
fn l_arret_d_urgence_se_demande_depuis_le_terminal() {
    // Un arrêt partiel sort en erreur, et dit quand même ce qui s'est arrêté.
    let rendu = failure(invoke(
        &["task", "halt"],
        "task.halt",
        json!({}),
        json!({"cancel_requested":["a","b"],"cancelled":["plan"],"unattended":[],"errors":[{"id":"c","error":"transition impossible"}]}),
    ));
    assert!(
        rendu.contains("Arrêt demandé pour 2 mission(s) : a, b."),
        "{rendu}"
    );
    assert!(rendu.contains("1 plan(s) annulé(s) : plan."), "{rendu}");
    assert!(rendu.contains("Non arrêtée(s) : c"), "{rendu}");
    let rendu = success(invoke(
        &["task", "halt"],
        "task.halt",
        json!({}),
        json!({"cancel_requested":[],"cancelled":[],"unattended":[],"errors":[]}),
    ));
    assert!(rendu.contains("rien à arrêter"), "{rendu}");
}
