//! Dialogue avec le navigateur par le protocole DevTools.

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

/// Erreur du pont.
#[derive(Debug, thiserror::Error)]
pub enum CdpError {
    /// Le navigateur n'a pas démarré.
    #[error("navigateur indisponible : {0}")]
    Launch(String),
    /// Erreur de transport.
    #[error("transport : {0}")]
    Transport(String),
    /// Le navigateur n'a pas répondu dans le délai imparti.
    #[error("le navigateur n'a pas répondu à {method} en {seconds} s")]
    Timeout {
        /// Méthode appelée.
        method: String,
        /// Délai écoulé.
        seconds: u64,
    },
    /// Le navigateur a rendu une erreur.
    #[error("erreur du navigateur : {0}")]
    Protocol(String),
    /// Réponse inattendue.
    #[error("réponse inattendue : {0}")]
    Unexpected(String),
}

/// Délai maximal d'une commande adressée au navigateur.
pub const CALL_TIMEOUT_SECONDS: u64 = 15;

/// Un navigateur piloté.
#[derive(Debug)]
pub struct Browser {
    child: Child,
    port: u16,
}

impl Browser {
    /// Lance un navigateur avec un profil dédié à la tâche.
    ///
    /// Le profil est isolé : une tâche ne récupère ni les sessions, ni l'historique, ni les
    /// cookies d'une autre, et rien ne persiste au-delà de ce que l'appelant conserve.
    ///
    /// # Errors
    /// Si le binaire est absent ou si le point d'écoute ne répond pas.
    pub async fn launch(
        program: &str,
        profile_dir: &std::path::Path,
        port: u16,
    ) -> Result<Self, CdpError> {
        Self::launch_with(program, profile_dir, port, None).await
    }

    /// Comme [`Self::launch`], en dirigeant toute la sortie réseau du navigateur vers un
    /// mandataire HTTP local (`hôte:port`), y compris les adresses de bouclage.
    ///
    /// C'est ainsi que le navigateur d'une tâche ne sort que par le proxy de Prophet OS : le
    /// relais local reçoit chaque requête et chaque tunnel, y ajoute le jeton de la tâche et
    /// les remet au socket d'egress, qui fait trancher capd sur l'hôte. QUIC est désactivé,
    /// parce qu'il contournerait un mandataire HTTP.
    ///
    /// # Errors
    /// Si le binaire est absent ou si le point d'écoute ne répond pas.
    pub async fn launch_with(
        program: &str,
        profile_dir: &std::path::Path,
        port: u16,
        proxy: Option<&str>,
    ) -> Result<Self, CdpError> {
        std::fs::create_dir_all(profile_dir).map_err(|e| CdpError::Launch(e.to_string()))?;
        // Le navigateur reçoit un foyer à lui, sous son profil. Chromium range ses rapports de
        // plantage, son cache de polices et sa base de certificats sous `$HOME`, jamais sous
        // `--user-data-dir` ; lancé par un service dont le foyer est `/var/empty`, il ne peut
        // pas y créer `Crash Reports`, son gestionnaire de plantage (crashpad) s'arrête faute de
        // base, et Chromium s'arrête net (CHECK) avant d'ouvrir son point d'écoute. C'est ce
        // que la sonde d'agentd a constaté sous l'unité durcie le 13 septembre 2026.
        let home = profile_dir.join("home");
        std::fs::create_dir_all(&home).map_err(|e| CdpError::Launch(e.to_string()))?;
        let proxy_args: Vec<String> = match proxy {
            Some(address) => vec![
                format!("--proxy-server=http://{address}"),
                "--proxy-bypass-list=<-loopback>".to_owned(),
                "--disable-quic".to_owned(),
            ],
            // Aucun proxy hérité de l'environnement : la seule sortie réseau d'une tâche est
            // le proxy de Prophet OS, jamais celui qui traînait dans les variables du shell.
            None => vec!["--no-proxy-server".to_owned()],
        };
        // Un port 0 laisse le navigateur choisir lui-même un port libre et l'écrire dans
        // `DevToolsActivePort` ; un fichier laissé par un lancement précédent dirait un port
        // qui n'est plus le sien, on le retire avant de lancer.
        let active_port = profile_dir.join("DevToolsActivePort");
        let _ = std::fs::remove_file(&active_port);
        let child = Command::new(program)
            .args([
                "--headless=new",
                "--no-sandbox",
                "--disable-gpu",
                "--disable-dev-shm-usage",
                // Aucune télémétrie, aucune synchronisation, aucun service en arrière-plan :
                // ce qui n'est pas nécessaire à la tâche n'a rien à faire là.
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-domain-reliability",
                "--disable-client-side-phishing-detection",
                "--metrics-recording-only",
                "--disable-sync",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-extensions",
                &format!("--remote-debugging-port={port}"),
                &format!("--user-data-dir={}", profile_dir.display()),
            ])
            .args(&proxy_args)
            .arg("about:blank")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env_remove("HTTP_PROXY")
            .env_remove("HTTPS_PROXY")
            .env_remove("http_proxy")
            .env_remove("https_proxy")
            .env_remove("ALL_PROXY")
            .env_remove("all_proxy")
            .stdout(Stdio::null())
            // La sortie d'erreur reste celle du service : c'est le seul endroit où un
            // navigateur qui refuse de démarrer sur une machine dit pourquoi.
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| CdpError::Launch(format!("{program} : {e}")))?;

        let mut browser = Self { child, port };
        if port == 0 {
            browser.port = browser.published_port(&active_port).await?;
        }
        browser.wait_ready().await?;
        Ok(browser)
    }

    /// Lit le port que le navigateur annonce dans son profil, une fois qu'il l'a écrit.
    ///
    /// C'est le navigateur qui a choisi ce port, en le liant : aucune autre tâche ne peut se
    /// glisser entre le choix et la prise, ce qu'une réservation faite d'ici ne garantit pas.
    async fn published_port(&mut self, active_port: &std::path::Path) -> Result<u16, CdpError> {
        for _ in 0..200 {
            if let Ok(text) = std::fs::read_to_string(active_port)
                && let Some(port) = text
                    .lines()
                    .next()
                    .and_then(|l| l.trim().parse::<u16>().ok())
                && port != 0
            {
                return Ok(port);
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                return Err(CdpError::Launch(format!(
                    "le navigateur s'est arrêté avant d'annoncer son port ({status})"
                )));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Err(CdpError::Launch(
            "le navigateur n'a pas annoncé son port de débogage".to_owned(),
        ))
    }

    async fn wait_ready(&self) -> Result<(), CdpError> {
        for _ in 0..100 {
            if tokio::net::TcpStream::connect(("127.0.0.1", self.port))
                .await
                .is_ok()
            {
                // Le port répond ; laisser au navigateur le temps de publier ses cibles.
                tokio::time::sleep(Duration::from_millis(150)).await;
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(CdpError::Launch(
            "le navigateur n'a pas ouvert son point d'écoute".to_owned(),
        ))
    }

    /// Adresse WebSocket de la page à piloter.
    ///
    /// On prend la cible déjà ouverte plutôt que d'en créer une : le point de création
    /// (`/json/new`) n'est pas servi par toutes les versions du navigateur, alors que la liste
    /// des cibles l'est partout. Le navigateur est lancé avec une page vierge, il y en a donc
    /// toujours exactement une.
    ///
    /// # Errors
    /// Si aucune cible de type page n'est disponible.
    pub async fn page_endpoint(&self) -> Result<String, CdpError> {
        for _ in 0..40 {
            let body = http_get(self.port, "/json/list").await?;
            if let Ok(Value::Array(targets)) = serde_json::from_str::<Value>(&body)
                && let Some(endpoint) = targets
                    .iter()
                    .find(|t| t["type"] == "page")
                    .and_then(|t| t["webSocketDebuggerUrl"].as_str())
            {
                return Ok(endpoint.to_owned());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(CdpError::Unexpected(
            "aucune cible de type page dans le navigateur".to_owned(),
        ))
    }

    /// Port de débogage.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }
}

/// Nombre de tentatives de lancement avant d'abandonner.
const TENTATIVES_DE_LANCEMENT: u8 = 5;

/// Demande au noyau un port libre, puis le relâche.
///
/// Le numéro rendu n'est garanti libre qu'à l'instant où il est lu : le navigateur le prendra
/// quelques millisecondes plus tard, et un autre processus peut s'y glisser. C'est pourquoi
/// [`Browser::launch_auto`] réessaie au lieu de traiter ce numéro comme acquis.
fn port_probablement_libre() -> Option<u16> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .ok()
}

impl Browser {
    /// Lance le navigateur sur un port choisi automatiquement.
    ///
    /// Réserver un port puis le relâcher pour que le navigateur s'en saisisse laisse un intervalle
    /// pendant lequel un autre processus peut le prendre. L'intervalle est court, mais il s'ouvre
    /// précisément quand plusieurs tâches démarrent ensemble — le cas d'un agent qui ouvre
    /// plusieurs pages, ou d'une suite de tests parallèle. Un échec de lancement y ressemblait à
    /// de la malchance ; on réessaie donc avec un autre port, ce qui transforme une course en
    /// simple retard.
    ///
    /// # Errors
    /// Si aucune tentative n'aboutit, l'erreur de la dernière est rendue.
    pub async fn launch_auto(
        program: &str,
        profile_dir: &std::path::Path,
    ) -> Result<Self, CdpError> {
        Self::launch_auto_with(program, profile_dir, None).await
    }

    /// Comme [`Self::launch_auto`], avec un mandataire HTTP local (voir [`Self::launch_with`]).
    ///
    /// # Errors
    /// Si aucune tentative n'aboutit, l'erreur de la dernière est rendue.
    pub async fn launch_auto_with(
        program: &str,
        profile_dir: &std::path::Path,
        proxy: Option<&str>,
    ) -> Result<Self, CdpError> {
        let mut derniere = None;
        for tentative in 0..TENTATIVES_DE_LANCEMENT {
            // Le navigateur choisit et lie lui-même son port (port 0) : plus de course. Les
            // tentatives suivantes gardent l'ancienne réservation, au cas où un navigateur
            // n'écrirait pas le fichier d'annonce.
            let port = if tentative == 0 {
                0
            } else {
                let Some(port) = port_probablement_libre() else {
                    continue;
                };
                port
            };
            match Self::launch_with(program, profile_dir, port, proxy).await {
                Ok(navigateur) => return Ok(navigateur),
                // Le navigateur lancé en vain est tué par `Drop` avant la tentative suivante.
                Err(erreur) => derniere = Some(erreur),
            }
        }
        Err(derniere
            .unwrap_or_else(|| CdpError::Launch("aucun port libre n'a pu être obtenu".to_owned())))
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Requête HTTP minimale vers le point d'écoute du navigateur.
///
/// Une bibliothèque HTTP complète serait ici une dépendance de plus pour trois lignes de
/// protocole, sur une connexion locale que nous contrôlons de bout en bout.
async fn http_get(port: u16, path: &str) -> Result<String, CdpError> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let travail = async {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .map_err(|e| CdpError::Transport(e.to_string()))?;
        // Le port doit figurer dans `Host` : le navigateur construit l'adresse WebSocket qu'il
        // annonce à partir de cet en-tête, et l'omettre produit une adresse sans port, donc
        // injoignable.
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|e| CdpError::Transport(e.to_string()))?;

        // On lit jusqu'à disposer du corps annoncé par `Content-Length`, sans attendre que le
        // pair ferme la connexion : le navigateur la garde parfois ouverte, et attendre sa
        // fermeture bloquerait indéfiniment.
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let read = stream
                .read(&mut chunk)
                .await
                .map_err(|e| CdpError::Transport(e.to_string()))?;
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
            let text = String::from_utf8_lossy(&buffer);
            if let Some((headers, body)) = text.split_once("\r\n\r\n") {
                match content_length(headers) {
                    Some(expected) if body.len() >= expected => break,
                    None => break,
                    Some(_) => {}
                }
            }
        }
        let text = String::from_utf8_lossy(&buffer).to_string();
        text.split_once("\r\n\r\n")
            .map(|(_, body)| body.trim().to_owned())
            .ok_or(CdpError::Unexpected(text))
    };

    tokio::time::timeout(Duration::from_secs(CALL_TIMEOUT_SECONDS), travail)
        .await
        .map_err(|_| CdpError::Timeout {
            method: path.to_owned(),
            seconds: CALL_TIMEOUT_SECONDS,
        })?
}

/// Longueur du corps annoncée par les en-têtes.
fn content_length(headers: &str) -> Option<usize> {
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse().ok())
            .flatten()
    })
}

/// Connexion à une cible DevTools.
#[derive(Debug)]
pub struct Session {
    socket: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    next_id: u64,
}

impl Session {
    /// Ouvre une session.
    ///
    /// # Errors
    /// Si la connexion échoue ou n'aboutit pas dans le délai.
    pub async fn connect(endpoint: &str) -> Result<Self, CdpError> {
        // Le navigateur publie l'adresse de sa page avant d'accepter les connexions dessus. Un
        // « connexion refusée » juste après le lancement ne dit donc pas « non », il dit « pas
        // encore » — et les deux se distinguent par le fait de réessayer. Sur une machine chargée,
        // où plusieurs navigateurs démarrent ensemble, l'intervalle s'allonge assez pour que la
        // première tentative tombe dedans.
        let debut = std::time::Instant::now();
        let limite = Duration::from_secs(CALL_TIMEOUT_SECONDS);
        let mut attente = Duration::from_millis(25);
        loop {
            match tokio::time::timeout(limite, tokio_tungstenite::connect_async(endpoint)).await {
                Err(_) => {
                    return Err(CdpError::Timeout {
                        method: "connect".to_owned(),
                        seconds: CALL_TIMEOUT_SECONDS,
                    });
                }
                Ok(Ok((socket, _))) => return Ok(Self { socket, next_id: 1 }),
                Ok(Err(erreur)) => {
                    // Tout le reste — adresse invalide, poignée de main refusée — est un vrai
                    // refus : réessayer ne ferait que retarder le diagnostic.
                    let refus_temporaire = matches!(
                        &erreur,
                        tokio_tungstenite::tungstenite::Error::Io(e)
                            if e.kind() == std::io::ErrorKind::ConnectionRefused
                    );
                    if !refus_temporaire || debut.elapsed() >= limite {
                        return Err(CdpError::Transport(erreur.to_string()));
                    }
                    tokio::time::sleep(attente).await;
                    attente = (attente * 2).min(Duration::from_millis(400));
                }
            }
        }
    }

    /// Envoie une commande et attend son résultat.
    ///
    /// Les événements arrivant entre-temps sont ignorés : ce pont n'observe la page que sur
    /// demande, ce qui évite de payer un flux continu dont l'agent ne ferait rien.
    ///
    /// # Errors
    /// Transport rompu, ou erreur rendue par le navigateur.
    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value, CdpError> {
        let id = self.next_id;
        self.next_id += 1;
        let message = json!({"id": id, "method": method, "params": params});
        self.socket
            .send(Message::Text(message.to_string()))
            .await
            .map_err(|e| CdpError::Transport(e.to_string()))?;

        // Un navigateur qui cesse de répondre ne doit pas figer la tâche : la boucle d'attente
        // est bornée, et l'échec est nommé.
        let deadline = std::time::Instant::now() + Duration::from_secs(CALL_TIMEOUT_SECONDS);
        loop {
            let restant = deadline.saturating_duration_since(std::time::Instant::now());
            if restant.is_zero() {
                return Err(CdpError::Timeout {
                    method: method.to_owned(),
                    seconds: CALL_TIMEOUT_SECONDS,
                });
            }
            let Ok(next) = tokio::time::timeout(restant, self.socket.next()).await else {
                return Err(CdpError::Timeout {
                    method: method.to_owned(),
                    seconds: CALL_TIMEOUT_SECONDS,
                });
            };
            let Some(next) = next else {
                return Err(CdpError::Transport("connexion fermée".to_owned()));
            };
            let message = next.map_err(|e| CdpError::Transport(e.to_string()))?;
            let Message::Text(text) = message else {
                continue;
            };
            let value: Value =
                serde_json::from_str(&text).map_err(|e| CdpError::Unexpected(e.to_string()))?;
            if value["id"].as_u64() != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                return Err(CdpError::Protocol(error.to_string()));
            }
            return Ok(value["result"].clone());
        }
    }
}
