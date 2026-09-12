//! Contrat du transport en flux, avec un serveur HTTP de test.

use providers::stream::{ChatClient, StreamError};
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

async fn server(body: Vec<u8>) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await.unwrap();
        // Fragmenter jusqu'au milieu des caractères UTF-8.
        for byte in body {
            if socket.write_all(&[byte]).await.is_err() {
                break;
            }
            tokio::task::yield_now().await;
        }
    });
    (endpoint, task)
}

fn frame(content: &str, finish: serde_json::Value) -> String {
    format!(
        "data: {}\r\n\r\n",
        json!({"choices":[{"index":0,"delta":{"content":content},"finish_reason":finish}]})
    )
}

fn usage() -> String {
    format!(
        "data: {}\n\n",
        json!({"choices":[],"usage":{"prompt_tokens":6,"completion_tokens":3}})
    )
}

#[tokio::test]
async fn le_flux_reconstitue_le_texte_et_exige_une_fin_mesuree() {
    let body = format!(
        ": keep-alive\r\n\r\n{}{}{}data: [DONE]\n\n",
        frame("Un été ", json!(null)),
        frame("☀", json!("stop")),
        usage()
    );
    let (url, server) = server(body.into_bytes()).await;
    let client = ChatClient::new(&url, Duration::from_secs(5)).unwrap();
    let (_cancel, receiver) = watch::channel(false);
    let mut deltas = String::new();
    let result = client
        .generate(
            "model",
            &[json!({"role":"user","content":"Bonjour"})],
            128,
            receiver,
            |delta| deltas.push_str(delta),
        )
        .await
        .unwrap();
    assert_eq!(result.text, "Un été ☀");
    assert_eq!(deltas, result.text);
    assert_eq!(result.usage.tokens_out, 3);
    assert!(result.first_token.is_some());
    server.await.unwrap();
}

#[tokio::test]
async fn une_connexion_tronquee_ne_devient_pas_un_succes() {
    for body in [
        frame("Début", json!(null)),
        format!("{}data: [DONE]\n\n", frame("Partiel", json!("length"))),
        format!("{}data: [DONE]\n\n", frame("Sans mesure", json!("stop"))),
        "data: {malforme}\n\n".to_owned(),
    ] {
        let (url, server) = server(body.into_bytes()).await;
        let client = ChatClient::new(&url, Duration::from_secs(3)).unwrap();
        let (_cancel, receiver) = watch::channel(false);
        assert!(
            client
                .generate("model", &[], 128, receiver, |_| {})
                .await
                .is_err()
        );
        server.await.unwrap();
    }
}

#[tokio::test]
async fn annuler_ferme_la_requete_meme_si_le_moteur_ne_produit_rien() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let (cancel, receiver) = watch::channel(false);
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 8192];
        read_request(&mut socket).await;
        cancel.send(true).unwrap();
        let closed = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut request))
            .await
            .unwrap();
        assert!(
            matches!(closed, Ok(0) | Err(_)),
            "le transport doit se fermer"
        );
    });
    let client = ChatClient::new(&endpoint, Duration::from_secs(60)).unwrap();
    let started = Instant::now();
    assert!(matches!(
        client.generate("model", &[], 128, receiver, |_| {}).await,
        Err(StreamError::Cancelled)
    ));
    assert!(started.elapsed() < Duration::from_secs(2));
    server.await.unwrap();
}

async fn read_request(socket: &mut TcpStream) {
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        let byte = socket.read_u8().await.unwrap();
        header.push(byte);
        assert!(header.len() < 16_384);
    }
    let header = String::from_utf8(header).unwrap();
    assert!(header.starts_with("POST /v1/chat/completions"));
    let length: usize = header
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().unwrap())
        })
        .unwrap();
    assert!(length < 16_384);
    let mut body = vec![0; length];
    socket.read_exact(&mut body).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["stream"], true);
    assert_eq!(body["stream_options"]["include_usage"], true);
}
