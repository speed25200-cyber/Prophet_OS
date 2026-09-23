//! Contrat HTTP du pilote local, avec un serveur de test réellement joint par TCP.
//! Ces tests vérifient le transport et le protocole ; l'inférence réelle est un essai distinct.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpListener;
use std::time::Duration;

use providers::local::{LocalModel, LocalTool};
use providers::native::{ModelClient, ModelTurn};
use serde_json::{Value, json};

fn answer(message: Value, reason: &str) -> Value {
    json!({"choices":[{"message":message, "finish_reason":reason}],
        "usage":{"prompt_tokens":42, "completion_tokens":7}})
}

fn server(responses: Vec<(u16, Value)>) -> (String, std::thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, body) in responses {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(stream);
            let mut start = String::new();
            reader.read_line(&mut start).unwrap();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse::<usize>().unwrap();
                }
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            requests.push(json!({"request":start.trim(), "body":
                if bytes.is_empty() { Value::Null } else { serde_json::from_slice::<Value>(&bytes).unwrap() }}));
            let body = body.to_string();
            write!(reader.get_mut(), "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        requests
    });
    (endpoint, thread)
}

fn tool() -> LocalTool {
    LocalTool {
        name: "fs.read".into(),
        description: "Lire un fichier".into(),
        parameters: json!({"type":"object", "properties":{"path":{"type":"string"}}, "required":["path"]}),
    }
}

#[test]
fn modele_et_usage_viennent_du_serveur() {
    let (url, server) = server(vec![
        (200, json!({"data":[{"id":"qwen-local"}]})),
        (
            200,
            answer(json!({"content":"Bonjour depuis le moteur."}), "stop"),
        ),
    ]);
    let mut model = LocalModel::new(&url, "qwen-local", Duration::from_secs(5)).unwrap();
    assert_eq!(model.models().unwrap(), vec!["qwen-local"]);
    let (turn, usage) = model
        .next_turn(&[json!({"role":"user", "content":"Bonjour"})])
        .unwrap();
    assert_eq!(
        turn,
        ModelTurn::Final {
            text: "Bonjour depuis le moteur.".into()
        }
    );
    assert_eq!((usage.tokens_in, usage.tokens_out), (42, 7));
    let requests = server.join().unwrap();
    assert_eq!(requests[0]["request"], "GET /v1/models HTTP/1.1");
    assert_eq!(requests[1]["request"], "POST /v1/chat/completions HTTP/1.1");
    assert_eq!(requests[1]["body"]["model"], "qwen-local");
    assert_eq!(requests[1]["body"]["messages"][0]["content"], "Bonjour");
    assert!(requests[1]["body"].get("tools").is_none());
}

#[test]
fn appel_et_resultat_sont_retransmis_dans_le_protocole_du_moteur() {
    let (url, server) = server(vec![
        (
            200,
            answer(
                json!({"tool_calls":[{"id":"engine-1", "type":"function", "function":{
            "name":"fs.read", "arguments":"{\"path\":\"~/note.txt\"}"}}]}),
                "tool_calls",
            ),
        ),
        (
            200,
            answer(json!({"content":"Le fichier contient 42."}), "stop"),
        ),
    ]);
    let mut model = LocalModel::new(&url, "local-test", Duration::from_secs(5))
        .unwrap()
        .with_tools(vec![tool()])
        .unwrap();
    let mut history = vec![json!({"role":"user", "content":"Lis note.txt"})];
    let (turn, _) = model.next_turn(&history).unwrap();
    assert_eq!(
        turn,
        ModelTurn::ToolCall {
            tool: "fs.read".into(),
            arguments: json!({"path":"~/note.txt"})
        }
    );
    history.push(json!({"role":"assistant", "tool_call":{"tool":"fs.read", "arguments":{"path":"~/note.txt"}}}));
    history.push(json!({"role":"tool", "ok":true, "result":{"content":"42"}}));
    model.next_turn(&history).unwrap();
    let requests = server.join().unwrap();
    let body = &requests[1]["body"];
    assert_eq!(body["tools"][0]["function"]["name"], "fs.read");
    assert_eq!(body["parallel_tool_calls"], false);
    assert_eq!(
        body["messages"][1]["tool_calls"][0]["id"],
        body["messages"][2]["tool_call_id"]
    );
    let result: Value =
        serde_json::from_str(body["messages"][2]["content"].as_str().unwrap()).unwrap();
    assert_eq!(result["result"]["content"], "42");
}

#[test]
fn aucune_route_distante_ni_identifiant_dans_le_transport_local() {
    for endpoint in [
        "http://example.com/v1",
        "http://192.168.1.1/v1",
        "http://127.0.0.1.example.com/v1",
        "http://user:password@127.0.0.1/v1",
        "http://127.0.0.1/v1?key=secret",
        "https://localhost/v1",
        "http://[::ffff:192.168.1.1]/v1",
    ] {
        assert!(
            LocalModel::new(endpoint, "model", Duration::from_secs(1)).is_err(),
            "{endpoint}"
        );
    }
    for endpoint in [
        "http://localhost:8080/v1",
        "http://127.0.0.1/v1",
        "http://[::1]:8080/v1",
    ] {
        assert!(
            LocalModel::new(endpoint, "model", Duration::from_secs(1)).is_ok(),
            "{endpoint}"
        );
    }
}

#[test]
fn reponse_tronquee_outil_inconnu_ou_usage_absent_ne_passe_pas_pour_un_succes() {
    let bad = vec![
        answer(json!({"content":"Partiel"}), "length"),
        answer(
            json!({"tool_calls":[{"type":"function", "function":{"name":"proc.exec", "arguments":"{}"}}]}),
            "tool_calls",
        ),
        answer(
            json!({"tool_calls":[{"type":"function", "function":{"name":"fs.read", "arguments":"pas JSON"}}]}),
            "tool_calls",
        ),
        json!({"choices":[{"message":{"content":"Incomplet"},"finish_reason":"stop"}]}),
        answer(json!({"tool_calls":[]}), "tool_calls"),
    ];
    let (url, server) = server(bad.into_iter().map(|value| (200, value)).collect());
    let mut model = LocalModel::new(&url, "model", Duration::from_secs(5))
        .unwrap()
        .with_tools(vec![tool()])
        .unwrap();
    for _ in 0..5 {
        assert!(
            model
                .next_turn(&[json!({"role":"user","content":"test"})])
                .is_err()
        );
    }
    server.join().unwrap();
}

#[test]
fn erreur_http_ne_reproduit_pas_le_contenu_prive() {
    let (url, server) = server(vec![(503, json!({"prompt":"secret-prive"}))]);
    let mut model = LocalModel::new(&url, "model", Duration::from_secs(5)).unwrap();
    let error = model.next_turn(&[]).unwrap_err().to_string();
    assert!(error.contains("503"));
    assert!(!error.contains("secret-prive"));
    server.join().unwrap();
}

#[test]
fn un_moteur_silencieux_est_interrompu_par_le_delai() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let waiting = std::thread::spawn(move || {
        let (_stream, _) = listener.accept().unwrap();
        std::thread::sleep(Duration::from_millis(300));
    });
    let mut model = LocalModel::new(&endpoint, "model", Duration::from_millis(80)).unwrap();
    let started = std::time::Instant::now();
    let error = model.next_turn(&[]).unwrap_err().to_string();
    assert!(error.contains("délai"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(2));
    waiting.join().unwrap();
}

#[test]
fn un_moteur_eteint_est_dit_injoignable_en_francais() {
    // Ce que lit l'humain quand le moteur n'écoute pas : où on l'a cherché et quoi faire, pas
    // la phrase anglaise de la bibliothèque HTTP.
    let mut model =
        LocalModel::new("http://127.0.0.1:1/v1", "model", Duration::from_secs(2)).unwrap();
    let error = model.next_turn(&[]).unwrap_err().to_string();
    assert!(
        error.contains("injoignable") && error.contains("127.0.0.1:1"),
        "{error}"
    );
    assert!(!error.contains("error sending request"), "{error}");
}

#[test]
fn le_serveur_ne_peut_pas_rediriger_la_conversation() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let redirect = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request);
        stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://example.invalid/collect\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    });
    let mut model = LocalModel::new(&endpoint, "model", Duration::from_secs(2)).unwrap();
    assert!(
        model
            .next_turn(&[])
            .unwrap_err()
            .to_string()
            .contains("302")
    );
    redirect.join().unwrap();
}

#[test]
fn une_reponse_demensuree_est_refusee() {
    let (url, server) = server(vec![(
        200,
        answer(json!({"content":"a".repeat(8 * 1024 * 1024)}), "stop"),
    )]);
    let mut model = LocalModel::new(&url, "model", Duration::from_secs(5)).unwrap();
    assert!(
        model
            .next_turn(&[])
            .unwrap_err()
            .to_string()
            .contains("volumineuse")
    );
    server.join().unwrap();
}

/// Un historique de mission : intention, deux appels d'outil et leurs résultats, dont un
/// long, puis un appel récent.
fn historique(long: &str) -> Vec<Value> {
    vec![
        json!({"role":"user","content":"Objectif"}),
        json!({"role":"assistant","tool_call":{"tool":"fs.read","arguments":{"path":"a"}}}),
        json!({"role":"tool","ok":true,"result":{"content":long}}),
        json!({"role":"assistant","tool_call":{"tool":"fs.read","arguments":{"path":"b"}}}),
        json!({"role":"tool","ok":true,"result":{"content":"court"}}),
        json!({"role":"assistant","tool_call":{"tool":"fs.list","arguments":{"path":"c"}}}),
        json!({"role":"tool","ok":true,"result":{"entries":["c/1"]}}),
    ]
}

#[test]
fn la_condensation_allege_les_anciens_resultats_sans_toucher_aux_recents() {
    use providers::local::{AsyncLocalModel, Condensation, condense};
    let long = "x".repeat(4000);
    let model = AsyncLocalModel::new(
        "http://127.0.0.1:9/v1",
        "m",
        Vec::new(),
        Duration::from_secs(1),
        16,
    )
    .unwrap();
    // Sans politique, tout part intact.
    let intact = model.outgoing(&historique(&long)).unwrap();
    assert_eq!(intact.len(), 7);
    assert!(intact[2]["content"].as_str().unwrap().len() > 4000);

    let model = model.with_condensation(Condensation {
        keep_last: 2,
        max_bytes: 512,
    });
    let envoye = model.outgoing(&historique(&long)).unwrap();
    assert_eq!(envoye.len(), 7, "aucun message n'est retiré");
    let condense_ = envoye[2]["content"].as_str().unwrap();
    assert!(condense_.len() < 600, "{}", condense_.len());
    let resume: Value = serde_json::from_str(condense_).unwrap();
    assert_eq!(resume["condensed"], true);
    assert_eq!(resume["ok"], true);
    assert!(resume["bytes"].as_u64().unwrap() > 4000);
    assert!(resume["digest"].as_str().unwrap().starts_with("blake3:"));
    assert_eq!(resume["head"].as_str().unwrap().len(), 160);
    // Les deux derniers résultats restent tels quels, et les appels ne changent pas.
    assert!(envoye[4]["content"].as_str().unwrap().contains("court"));
    assert!(envoye[6]["content"].as_str().unwrap().contains("c/1"));
    assert_eq!(envoye[1]["tool_calls"][0]["function"]["name"], "fs.read");
    // Un résultat ancien mais court n'est pas condensé ; la politique est déterministe.
    let mut messages = model.outgoing(&historique("bref")).unwrap();
    assert!(messages[2]["content"].as_str().unwrap().contains("bref"));
    assert_eq!(
        condense(
            &mut messages,
            Condensation {
                keep_last: 0,
                max_bytes: 0
            }
        ),
        0,
        "rien de plus court que zéro octet… mais rien à épargner non plus"
    );
    let mut deux = model.outgoing(&historique(&long)).unwrap();
    let mut encore = deux.clone();
    assert_eq!(
        condense(&mut deux, Condensation::default()),
        condense(&mut encore, Condensation::default())
    );
    assert_eq!(deux, encore);
}

#[test]
fn la_consigne_de_systeme_precede_l_intention_a_chaque_tour() {
    use providers::local::AsyncLocalModel;
    let model = AsyncLocalModel::new(
        "http://127.0.0.1:9/v1",
        "m",
        Vec::new(),
        Duration::from_secs(1),
        16,
    )
    .unwrap()
    .with_system(Some("Votre rôle est l'exécution.".into()));
    let envoye = model.outgoing(&historique("bref")).unwrap();
    assert_eq!(envoye.len(), 8);
    assert_eq!(envoye[0]["role"], "system");
    assert_eq!(envoye[0]["content"], "Votre rôle est l'exécution.");
    assert_eq!(envoye[1]["role"], "user");
    // Une consigne vide n'en pose aucune.
    let muet = AsyncLocalModel::new(
        "http://127.0.0.1:9/v1",
        "m",
        Vec::new(),
        Duration::from_secs(1),
        16,
    )
    .unwrap()
    .with_system(Some("   ".into()));
    assert_eq!(
        muet.outgoing(&historique("bref")).unwrap()[0]["role"],
        "user"
    );
}

/// Ce que llama-server rend quand l'historique ne tient pas dans sa fenêtre.
fn debordement(n_prompt: u64, n_ctx: u64) -> Value {
    json!({"error":{"code":400,
        "message":"the request exceeds the available context size, try increasing it",
        "type":"exceed_context_size_error","n_prompt_tokens":n_prompt,"n_ctx":n_ctx}})
}

/// Octets des résultats d'outils d'une requête envoyée au moteur.
fn octets_des_resultats(requete: &Value) -> usize {
    requete["body"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["content"].as_str().unwrap().len())
        .sum()
}

#[test]
fn un_historique_trop_long_pour_la_fenetre_du_moteur_est_resserre_puis_renvoye() {
    use providers::local::{AsyncLocalModel, Condensation};
    // Le dernier résultat lu est un fichier de 40 ko : aucune condensation des anciens ne
    // suffit, c'est lui qu'il faut tronquer. Le moteur annonce une fenêtre de 4096 tokens.
    let fichier = "ligne de journal assez ordinaire\n".repeat(1200);
    let mut history = historique("bref");
    history.push(
        json!({"role":"assistant","tool_call":{"tool":"fs.read","arguments":{"path":"gros"}}}),
    );
    history.push(json!({"role":"tool","ok":true,"result":{"content":fichier}}));
    let (url, server) = server(vec![
        (400, debordement(12_400, 4096)),
        (
            200,
            answer(
                json!({"content":"Le journal commence par des lignes ordinaires."}),
                "stop",
            ),
        ),
        (
            200,
            answer(json!({"content":"Toujours ordinaire."}), "stop"),
        ),
    ]);
    let model = AsyncLocalModel::new(&url, "m", Vec::new(), Duration::from_secs(5), 512)
        .unwrap()
        .with_condensation(Condensation::default());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let reply = runtime.block_on(model.next_turn(&history)).unwrap();
    assert!(
        matches!(reply.turn, Ok(ModelTurn::Final { .. })),
        "{:?}",
        reply.turn
    );
    // Au tour suivant, la fenêtre apprise sert d'emblée : pas de nouveau refus.
    history.push(
        json!({"role":"assistant","content":"Le journal commence par des lignes ordinaires."}),
    );
    history.push(json!({"role":"user","content":"Et ensuite ?"}));
    let reply = runtime.block_on(model.next_turn(&history)).unwrap();
    assert!(
        matches!(reply.turn, Ok(ModelTurn::Final { .. })),
        "{:?}",
        reply.turn
    );

    let requetes = server.join().unwrap();
    assert_eq!(requetes.len(), 3);
    let (premiere, deuxieme, troisieme) = (&requetes[0], &requetes[1], &requetes[2]);
    assert!(octets_des_resultats(premiere) > 40_000);
    // 12 400 tokens pour 4096 : il faut retirer plus des deux tiers ; ce qui repart tient dans
    // la fenêtre avec la place de la réponse, au rapport octets par token observé.
    let par_token = premiere["body"]["messages"].to_string().len() as f64 / 12_400.0;
    for requete in [deuxieme, troisieme] {
        let envoye = requete["body"]["messages"].to_string().len() as f64;
        assert!(
            envoye / par_token < f64::from(4096 - 512),
            "{envoye} octets"
        );
    }
    // Le dernier résultat garde son début et dit au modèle ce qui s'est passé.
    let messages = deuxieme["body"]["messages"].as_array().unwrap();
    let dernier = messages.iter().rev().find(|m| m["role"] == "tool").unwrap();
    let dernier = dernier["content"].as_str().unwrap();
    assert!(dernier.contains("tronqué"), "{dernier}");
    assert!(dernier.contains("fenêtre de contexte"), "{dernier}");
    assert!(
        dernier.contains("ligne de journal assez ordinaire"),
        "{dernier}"
    );
    // L'intention, elle, n'est jamais touchée.
    assert_eq!(messages[0]["content"], "Objectif");
}

#[test]
fn un_historique_qui_ne_tient_jamais_rend_une_erreur_qui_le_dit() {
    use providers::local::AsyncLocalModel;
    // Une intention démesurée ne se tronque pas : rien ne peut être resserré, le tour n'est
    // pas renvoyé pour rien, et l'erreur donne les nombres, jamais le contenu.
    let intention = format!("secret-prive {}", "mot ".repeat(20_000));
    let (url, server) = server(vec![(400, debordement(20_010, 4096))]);
    let model = AsyncLocalModel::new(&url, "m", Vec::new(), Duration::from_secs(5), 512).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let error = runtime
        .block_on(model.next_turn(&[json!({"role":"user","content":intention})]))
        .unwrap_err()
        .to_string();
    assert!(error.contains("fenêtre de contexte"), "{error}");
    assert!(error.contains("4096"), "{error}");
    assert!(!error.contains("secret-prive"), "{error}");
    assert_eq!(server.join().unwrap().len(), 1);
}

#[test]
fn une_longue_mission_garde_la_place_du_dernier_resultat() {
    use providers::local::{fit, tool_bytes};
    // Quarante lectures d'un kilo-octet puis une dernière de 8 ko : les résumés des anciennes
    // pèsent eux-mêmes trop ; ils se réduisent à leur issue, du plus ancien au plus récent,
    // jusqu'à laisser au dernier résultat la place de son début.
    let mut messages = vec![json!({"role":"user","content":"Objectif"})];
    for i in 0..40 {
        messages.push(json!({"role":"assistant","content":null,"tool_calls":[{"id":format!("c{i}"),"type":"function","function":{"name":"fs.read","arguments":"{}"}}]}));
        messages.push(json!({"role":"tool","tool_call_id":format!("c{i}"),"content":json!({"ok":i != 3,"result":"a".repeat(1100)}).to_string()}));
    }
    messages.push(json!({"role":"tool","tool_call_id":"dernier","content":"é".repeat(4000)}));
    let avant = tool_bytes(&messages);
    let mut resserre = messages.clone();
    assert!(fit(&mut resserre, 4096), "{}", tool_bytes(&resserre));
    assert!(tool_bytes(&resserre) <= 4096);
    assert!(avant > 40_000);
    // Aucun message n'est retiré, l'intention ne change pas, les issues restent lisibles.
    assert_eq!(resserre.len(), messages.len());
    assert_eq!(resserre[0], messages[0]);
    let ancien: Value = serde_json::from_str(resserre[8]["content"].as_str().unwrap()).unwrap();
    assert_eq!(
        ancien["ok"], false,
        "l'échec du quatrième appel se lit encore"
    );
    assert_eq!(ancien["condensed"], true);
    let dernier = resserre.last().unwrap()["content"].as_str().unwrap();
    assert!(
        dernier.contains("tronqué") && dernier.contains("éé"),
        "{dernier}"
    );
    assert!(dernier.len() >= 512);
    // Déterministe, et sans effet sur ce qui tient déjà.
    let mut encore = messages.clone();
    assert!(fit(&mut encore, 4096));
    assert_eq!(encore, resserre);
    let mut deja = resserre.clone();
    assert!(fit(&mut deja, 4096));
    assert_eq!(deja, resserre);
}
