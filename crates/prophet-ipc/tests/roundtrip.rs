//! Tests d'intégration du transport IPC : aller-retour, authentification, débit.

use std::sync::Arc;

use prophet_ipc::{Client, Error, ErrorCode, Handler, PeerIdentity, Server};
use serde_json::{Value, json};

struct Echo;

impl Handler for Echo {
    async fn call(
        &self,
        peer: PeerIdentity,
        auth: Option<String>,
        method: String,
        params: Value,
    ) -> Result<Value, Error> {
        match method.as_str() {
            "ping" => Ok(json!("pong")),
            "whoami" => Ok(json!({"uid": peer.uid, "authentifie": auth.is_some()})),
            "secret" => auth.map_or_else(
                || Err(Error::new(ErrorCode::Unauthorized, "jeton requis")),
                |token| Ok(json!({"jeton": token})),
            ),
            "echo" => Ok(params),
            _ => Err(Error::new(ErrorCode::MethodNotFound, method)),
        }
    }
}

async fn serveur() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.sock");
    let server = Server::bind(&path).unwrap();
    tokio::spawn(async move {
        let _ = server.serve(Arc::new(Echo)).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    (dir, path)
}

#[tokio::test]
async fn aller_retour_simple() {
    let (_dir, path) = serveur().await;
    let client = Client::connect(&path).await.unwrap();
    assert_eq!(client.call("ping", json!({})).await.unwrap(), json!("pong"));
}

#[tokio::test]
async fn methode_inconnue() {
    let (_dir, path) = serveur().await;
    let client = Client::connect(&path).await.unwrap();
    let err = client.call("inexistante", json!({})).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::MethodNotFound);
}

#[tokio::test]
async fn identite_du_pair_attestee_par_le_noyau() {
    let (_dir, path) = serveur().await;
    let client = Client::connect(&path).await.unwrap();
    let result = client.call("whoami", json!({})).await.unwrap();
    assert_eq!(result["uid"], json!(uid_courant()));
    assert_eq!(result["authentifie"], json!(false));
}

fn uid_courant() -> u32 {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::metadata("/proc/self")
        .map(|m| m.uid())
        .unwrap_or(0)
}

#[tokio::test]
async fn appel_sans_jeton_refuse() {
    let (_dir, path) = serveur().await;
    let client = Client::connect(&path).await.unwrap();
    let err = client.call("secret", json!({})).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Unauthorized);
}

#[tokio::test]
async fn appel_avec_jeton_accepte_et_jeton_retire_des_parametres() {
    let (_dir, path) = serveur().await;
    let client = Client::connect(&path).await.unwrap().with_auth("jeton-abc");
    let result = client.call("secret", json!({})).await.unwrap();
    assert_eq!(result["jeton"], json!("jeton-abc"));

    let echo = client.call("echo", json!({"x": 1})).await.unwrap();
    assert_eq!(echo, json!({"x": 1}));
}

#[tokio::test]
async fn json_illisible_donne_une_erreur_de_parsing() {
    use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
    let (_dir, path) = serveur().await;
    let mut stream = tokio::net::UnixStream::connect(&path).await.unwrap();
    stream
        .write_all(b"{ceci n'est pas du json}\n")
        .await
        .unwrap();
    let (read_half, _write_half) = stream.split();
    let mut line = String::new();
    BufReader::new(read_half)
        .read_line(&mut line)
        .await
        .unwrap();
    assert!(line.contains("-32700"), "{line}");
}

#[tokio::test]
async fn debit_dix_mille_allers_retours() {
    let (_dir, path) = serveur().await;
    let client = Client::connect(&path).await.unwrap();
    let start = std::time::Instant::now();
    for _ in 0..10_000 {
        client.call("ping", json!({})).await.unwrap();
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "10 000 allers-retours en {elapsed:?}"
    );
    eprintln!("10 000 allers-retours en {elapsed:?}");
}

#[tokio::test]
async fn le_nom_du_socket_n_est_jamais_libre_entre_deux_demarrages() {
    // Un membre du groupe qui guette le nom d'un daemon ne doit jamais le trouver libre : ni
    // pendant qu'un nouveau démarrage remplace l'ancien socket, ni après l'arrêt du service.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("capd.sock");
    let premier = Server::bind(&path).unwrap();
    let guet = {
        let path = path.clone();
        std::thread::spawn(move || {
            let fin = std::time::Instant::now() + std::time::Duration::from_millis(300);
            let mut absences = 0u32;
            while std::time::Instant::now() < fin {
                if std::fs::symlink_metadata(&path).is_err() {
                    absences += 1;
                }
            }
            absences
        })
    };
    let mut serveurs = vec![premier];
    for _ in 0..200 {
        serveurs.push(Server::bind(&path).unwrap());
    }
    assert_eq!(guet.join().unwrap(), 0, "le nom a été vu libre");
    // Aucun nom temporaire ne reste à côté.
    let noms: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(noms, vec![std::ffi::OsString::from("capd.sock")]);

    // Arrêté, le service laisse son socket en place : le nom reste à lui, les clients sont
    // refusés au lieu de joindre un autre, et le démarrage suivant le remplace.
    drop(serveurs);
    assert!(std::fs::symlink_metadata(&path).is_ok());
    assert!(Client::connect(&path).await.is_err());
    let suivant = Server::bind(&path).unwrap();
    tokio::spawn(async move {
        let _ = suivant.serve(Arc::new(Echo)).await;
    });
    let client = Client::connect(&path).await.unwrap();
    assert_eq!(client.call("ping", json!({})).await.unwrap(), json!("pong"));
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &std::fs::symlink_metadata(&path).unwrap().permissions(),
    );
    assert_eq!(mode & 0o777, 0o660);
}
