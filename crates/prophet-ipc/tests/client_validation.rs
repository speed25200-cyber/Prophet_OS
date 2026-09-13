//! Une réponse ne peut confirmer que la requête à laquelle elle correspond.
use prophet_ipc::{Client, Error, ErrorCode, MAX_MESSAGE_BYTES};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

async fn response(bytes: Vec<u8>) -> Result<Value, Error> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("peer.sock");
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    let worker = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        let mut request = String::new();
        stream.read_line(&mut request).await.unwrap();
        assert_eq!(serde_json::from_str::<Value>(&request).unwrap()["id"], 1);
        // Un refus de taille peut fermer la connexion avant la fin de l'envoi.
        let _ = stream.get_mut().write_all(&bytes).await;
    });
    let client = Client::connect(path).await.unwrap();
    let result = client.call("task.start", json!({"id":"mission-a"})).await;
    drop(client);
    worker.await.unwrap();
    result
}

#[tokio::test]
async fn un_acquittement_ambigu_ou_destine_a_une_autre_requete_est_refuse() {
    for body in [
        json!({"jsonrpc":"2.0","id":2,"result":{"started":"mission-a"}}),
        json!({"jsonrpc":"1.0","id":1,"result":true}),
        json!({"jsonrpc":"2.0","id":1,"result":true,"error":{"code":-32603,"message":"échec"}}),
        json!({"jsonrpc":"2.0","id":1,"error":null}),
        json!({"jsonrpc":"2.0","id":1}),
    ] {
        let bytes = format!("{body}\n").into_bytes();
        assert_eq!(
            response(bytes).await.unwrap_err().code,
            ErrorCode::ParseError,
            "{body}"
        );
    }
}

#[tokio::test]
async fn une_reponse_tronquee_ou_trop_grande_est_refusee() {
    let body = json!({"jsonrpc":"2.0","id":1,"result":true}).to_string();
    assert_eq!(
        response(body.into_bytes()).await.unwrap_err().code,
        ErrorCode::ParseError
    );
    let bytes = vec![b' '; MAX_MESSAGE_BYTES + 1];
    assert_eq!(
        response(bytes).await.unwrap_err().code,
        ErrorCode::ParseError
    );
}

#[tokio::test]
async fn null_reste_un_resultat_valide_et_les_erreurs_metier_sont_conservees() {
    assert_eq!(
        response(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":null}\n".to_vec())
            .await
            .unwrap(),
        Value::Null
    );
    let body = json!({"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"méthode absente"}});
    let error = response(format!("{body}\n").into_bytes())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::MethodNotFound);
    assert_eq!(error.message, "méthode absente");
}
