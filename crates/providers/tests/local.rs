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
    assert!(model.next_turn(&[]).is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
    waiting.join().unwrap();
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
