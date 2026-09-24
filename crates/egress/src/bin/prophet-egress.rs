//! `prophet-egress` — le proxy de sortie, en service.
//!
//! C'est le seul chemin par lequel quoi que ce soit sort de cette machine. « Toute sortie réseau
//! passe par `egress` » est un invariant de Prophet OS ; ce programme est l'endroit où il devient
//! une propriété du système, parce que les sandboxes n'ont pas de pile réseau et que ce socket est
//! la seule chose qu'on leur monte.
//!
//! Quatre choses se passent pour chaque requête, dans cet ordre, et aucune n'est facultative :
//!
//! 1. **Qui demande ?** Le jeton de capacité voyage dans un en-tête `Proxy-Authorization` interne,
//!    retiré avant la sortie. Sans jeton, rien ne part.
//! 2. **A-t-il le droit ?** `capd` tranche, sur l'hôte réellement visé. Le proxy ne décide de rien
//!    lui-même : il demande.
//! 3. **Est-ce une exfiltration ?** Le détecteur regarde le volume, l'entropie et les motifs de
//!    secrets dans ce qui sort.
//! 4. **Alors seulement**, le relais.
//!
//! L'hôte contrôlé et l'hôte joint sont **la même variable**. C'est la propriété qui fait tenir
//! tout le reste : contrôler `api.exemple.fr` puis se connecter à ce que dit un autre en-tête
//! serait une passoire avec l'apparence d'un contrôle.

use std::sync::Arc;

use egress::{Detector, Outbound, Policy, Proxy, QueryHosts, RequestLog, parse_request};
use prophet_daemon as commun;
use prophet_ipc::Client;
use serde_json::{Value, json};
use tokio::io::{
    AsyncBufReadExt as _, AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, BufReader,
};
use tokio::net::{TcpStream, UnixListener, UnixStream};

/// En-tête portant le jeton de la tâche. Interne : il ne sort jamais.
const ENTETE_JETON: &str = "proxy-authorization";

/// Préfixe du jeton dans cet en-tête.
const PREFIXE: &str = "Prophet ";

/// Taille maximale d'un en-tête de requête.
const TETE_MAX: usize = 64 * 1024;

/// Taille maximale d'un corps lu en mémoire pour l'analyse.
///
/// Au-delà, la requête est refusée plutôt que relayée sans être regardée : un corps qu'on ne peut
/// pas inspecter est exactement celui par lequel on exfiltrerait.
const CORPS_MAX: usize = 8 * 1024 * 1024;

struct Sortie {
    capd: std::path::PathBuf,
    /// Le journal, où chaque décision s'inscrit sous la mission du jeton.
    journal: std::path::PathBuf,
    /// Le coffre. Lui seul peut rendre une valeur, et seulement à ce processus.
    coffre: std::path::PathBuf,
    detecteur: Detector,
    /// Hôtes dont un `POST` est une question et non un effet (`PROPHET_EGRESS_QUERY_HOSTS`).
    hotes_d_interrogation: QueryHosts,
    /// Racines de confiance pour joindre un amont en TLS. Le proxy termine TLS lui-même quand
    /// une requête vise `https://` en forme absolue : c'est la seule façon de substituer un
    /// secret dans une requête chiffrée, puisqu'un tunnel `CONNECT` ne laisse rien voir
    /// (ADR-0007).
    tls: Arc<rustls::ClientConfig>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    commun::journaliser();

    // Posé d'un coup sur l'ancien, comme les sockets des autres daemons (ADR 0044).
    let socket = commun::socket("egress");
    let ecoute = prophet_ipc::publish(&socket, |temporaire| UnixListener::bind(temporaire))?;

    let capd = std::env::var("PROPHET_CAPD_SOCKET").map_or_else(
        |_| prophet_ipc::socket_path("capd"),
        std::path::PathBuf::from,
    );
    let coffre = std::env::var("PROPHET_VAULT_SOCKET").map_or_else(
        |_| prophet_ipc::socket_path("vault"),
        std::path::PathBuf::from,
    );
    let journal = std::env::var("PROPHET_LEDGER_SOCKET").map_or_else(
        |_| prophet_ipc::socket_path("ledger"),
        std::path::PathBuf::from,
    );
    // Une liste illisible arrête le service : une configuration qui ne se lit pas ne doit pas
    // se transformer silencieusement en « aucun hôte », ni en « tous ».
    let hotes_d_interrogation = std::env::var("PROPHET_EGRESS_QUERY_HOSTS")
        .ok()
        .map(|liste| QueryHosts::parse(&liste))
        .transpose()
        .map_err(anyhow::Error::msg)?
        .unwrap_or_default();
    let tls = racines_de_confiance()?;

    tracing::info!(
        socket = %socket.display(),
        capd = %capd.display(),
        coffre = %coffre.display(),
        interrogation = ?hotes_d_interrogation.patterns(),
        "egress écoute ; rien ne sort sans un jeton que capd approuve"
    );

    let sortie = Arc::new(Sortie {
        capd,
        journal,
        coffre,
        detecteur: Detector::new(),
        hotes_d_interrogation,
        tls,
    });

    loop {
        let (flux, _) = ecoute.accept().await?;
        let sortie = Arc::clone(&sortie);
        tokio::spawn(async move {
            if let Err(erreur) = sortie.servir(flux).await {
                tracing::debug!(%erreur, "requête sortante interrompue");
            }
        });
    }
}

impl Sortie {
    async fn servir(&self, flux: UnixStream) -> anyhow::Result<()> {
        let (lecture, mut ecriture) = flux.into_split();
        let mut lecteur = BufReader::new(lecture);

        let Some(tete) = lire_la_tete(&mut lecteur).await? else {
            return Ok(());
        };
        let mut requete = match parse_request(&tete) {
            Ok(r) => r,
            Err(erreur) => {
                ecriture
                    .write_all(&reponse(400, "BadRequest", &erreur.to_string()))
                    .await?;
                return Ok(());
            }
        };

        // Le corps, pour pouvoir le regarder. `CONNECT` n'en a pas.
        if requete.method != "CONNECT" {
            // Un cadrage ambigu est refusé, pas interprété. Deux `Content-Length`, ou un
            // `Transfer-Encoding` à côté, permettent au proxy et au serveur de lire deux corps
            // différents dans les mêmes octets — c'est la faille de contrebande de requêtes, et
            // elle vit précisément dans les cas où chacun « fait de son mieux ».
            if let Some(raison) = cadrage_ambigu(&requete.headers) {
                tracing::warn!(hote = %requete.host, %raison, "cadrage ambigu refusé");
                ecriture
                    .write_all(&reponse(400, "AmbiguousFraming", raison))
                    .await?;
                return Ok(());
            }
            let attendu = longueur_du_corps(&requete.headers);
            if attendu > CORPS_MAX {
                ecriture
                    .write_all(&reponse(
                        413,
                        "BodyTooLarge",
                        "un corps qu'on ne peut pas inspecter ne sort pas",
                    ))
                    .await?;
                return Ok(());
            }
            requete.body = vec![0; attendu];
            if attendu > 0 {
                lecteur.read_exact(&mut requete.body).await?;
            }
        }

        // 1. Qui demande ?
        let Some(jeton) = jeton(&requete.headers) else {
            tracing::warn!(hote = %requete.host, "requête sans jeton");
            ecriture
                .write_all(&reponse(
                    407,
                    "Unauthorized",
                    "aucun jeton de capacité : rien ne sort d'ici sans qu'on sache pour qui",
                ))
                .await?;
            return Ok(());
        };

        // 2. A-t-il le droit ? C'est `capd` qui tranche, et lui seul.
        //
        // L'hôte soumis au contrôle est `requete.host`, celui qu'a extrait l'analyseur — et c'est
        // exactement celui auquel le relais se connectera plus bas.
        let modifiante =
            egress::is_mutating(&requete.method, &requete.host, &self.hotes_d_interrogation);
        let decision = self
            .demander_a_capd(&jeton, &requete.host, modifiante)
            .await;

        let decision = match decision {
            Ok(d) => d,
            Err(erreur) => {
                // Un `capd` injoignable ne veut pas dire « autorisé ». Il veut dire qu'on ne sait
                // pas, et on ne sort pas sur un « je ne sais pas ».
                tracing::error!(%erreur, "capd injoignable : sortie refusée");
                ecriture
                    .write_all(&reponse(
                        503,
                        "BrokerUnavailable",
                        "le broker de capacités est injoignable ; aucune sortie n'est autorisée \
                         tant qu'on ne peut pas vérifier un droit",
                    ))
                    .await?;
                return Ok(());
            }
        };

        // La mission est le sujet du jeton ; capd vient d'en juger la signature. Un jeton forgé
        // n'écrit rien au journal : il ne ferait qu'y imiter une autre mission.
        let mission = jeton["sub"].as_str().map(str::to_owned);
        if decision["decision"] != "allow" {
            let motif = decision["reason"].as_str().unwrap_or("refusé").to_owned();
            tracing::info!(hote = %requete.host, %motif, "sortie refusée par capd");
            // Un jeton dont capd n'a pas pu vérifier la signature ne dit rien de sa mission.
            if !matches!(
                motif.as_str(),
                "bad_signature" | "unknown_version" | "BadSignature" | "UnknownVersion"
            ) {
                self.journaliser(
                    mission.as_deref(),
                    "net.deny",
                    json!({"host": requete.host, "reason": motif}),
                );
            }
            ecriture
                .write_all(&reponse(403, &motif, &decision.to_string()))
                .await?;
            return Ok(());
        }

        // 3. Est-ce une exfiltration ?
        let signaux = self.detecteur.inspect(&Outbound {
            host: &requete.host,
            url: &requete.target,
            headers: &requete.headers,
            body: &requete.body,
        });
        if Detector::should_block(&signaux) {
            let explication = signaux
                .iter()
                .map(egress::Signal::explain)
                .collect::<Vec<_>>()
                .join(" ; ");
            tracing::warn!(hote = %requete.host, %explication, "exfiltration suspectée");
            self.journaliser(
                mission.as_deref(),
                "net.exfil_suspected",
                json!({"host": requete.host, "reason": explication}),
            );
            ecriture
                .write_all(&reponse(403, "ExfiltrationSuspected", &explication))
                .await?;
            return Ok(());
        }

        // 4. Alors seulement, le relais.
        tracing::info!(
            hote = %requete.host,
            methode = %requete.method,
            chemin = %requete.path(),
            "sortie autorisée"
        );
        if requete.method == "CONNECT" {
            // Rien à substituer dans un tunnel : le proxy n'y voit que des octets chiffrés
            // (ADR-0007). Une demande d'injection y serait sans effet, et une tâche qui la croit
            // faite enverrait un handle littéral au serveur. On la refuse donc plutôt que de la
            // laisser passer sans effet.
            if requete
                .headers
                .iter()
                .any(|(_, valeur)| egress::reference_dans(valeur).is_some())
            {
                tracing::warn!(hote = %requete.host, "injection demandée dans un tunnel");
                ecriture
                    .write_all(&reponse(
                        400,
                        "InjectionImpossible",
                        "un tunnel chiffré ne reçoit pas de secret injecté (ADR-0007) ; \
                         passez par l'outil http.fetch",
                    ))
                    .await?;
                return Ok(());
            }
            let bilan = relayer_tunnel(&requete, lecteur, ecriture).await?;
            self.journaliser_la_sortie(mission.as_deref(), &requete, bilan);
            Ok(())
        } else {
            // La substitution a lieu ici, au tout dernier moment, et jamais avant : ce qui a été
            // journalisé et inspecté plus haut ne contenait que des références.
            match self.substituer(&requete.host, &requete.headers).await {
                Ok(entetes) => {
                    requete.headers = entetes;
                    let bilan = relayer_http(&requete, ecriture, &self.tls).await?;
                    self.journaliser_la_sortie(mission.as_deref(), &requete, bilan);
                    Ok(())
                }
                Err(raison) => {
                    tracing::warn!(hote = %requete.host, %raison, "substitution refusée");
                    ecriture
                        .write_all(&reponse(403, "SecretRefused", &raison))
                        .await?;
                    Ok(())
                }
            }
        }
    }

    /// Remplace les références de secrets par leurs valeurs, en demandant au coffre.
    ///
    /// Le coffre ne rend une valeur qu'à ce processus : c'est vérifié de son côté par
    /// `SO_PEERCRED`, et c'est ce qui fait que « le Vault rend des poignées, jamais des valeurs »
    /// tient pour tout le reste du système.
    ///
    /// Une référence inconnue ou interdite pour cet hôte **arrête la requête**. La laisser partir
    /// telle quelle enverrait le handle littéral au serveur distant : inoffensif — un handle ne
    /// vaut rien sans le coffre — mais la tâche croirait son secret transmis et ne comprendrait pas
    /// l'échec d'authentification qui suivrait.
    async fn substituer(
        &self,
        hote: &str,
        entetes: &[(String, String)],
    ) -> Result<Vec<(String, String)>, String> {
        // Le cas courant : aucune référence, aucun aller-retour vers le coffre.
        if !entetes
            .iter()
            .any(|(_, valeur)| egress::reference_dans(valeur).is_some())
        {
            return Ok(entetes.to_vec());
        }

        let client = Client::connect(&self.coffre)
            .await
            .map_err(|e| format!("coffre injoignable : {e}"))?;

        let mut sortants = Vec::with_capacity(entetes.len());
        for (nom, valeur) in entetes {
            let Some(reference) = egress::reference_dans(valeur) else {
                sortants.push((nom.clone(), valeur.clone()));
                continue;
            };
            let secret = reference.name.clone();

            let permis = client
                .call(
                    "secrets.allowed_for",
                    json!({ "name": secret, "host": hote }),
                )
                .await
                .map_err(|e| e.message.clone())?;
            if permis["allowed"] != true {
                return Err(format!("le secret {secret} n'est pas destiné à {hote}"));
            }

            let rendu = client
                .call("secrets.use", json!({ "name": secret }))
                .await
                .map_err(|e| format!("{secret} : {}", e.message))?;
            let valeur_reelle = rendu["value"]
                .as_str()
                .ok_or_else(|| format!("{secret} : le coffre n'a pas rendu de valeur"))?;

            // Le nom est journalisé plus haut ; la valeur ne l'est nulle part, et surtout pas ici.
            sortants.push((nom.clone(), reference.remplacer_par(valeur_reelle)));
        }
        Ok(sortants)
    }

    /// Inscrit une décision au journal, sous la mission du jeton, sans retenir la requête : une
    /// panne du journal se dit dans le journal du service, elle n'arrête pas la sortie déjà
    /// tranchée.
    fn journaliser(&self, mission: Option<&str>, genre: &'static str, charge: Value) {
        let Some(mission) = mission.map(str::to_owned) else {
            return;
        };
        let journal = self.journal.clone();
        tokio::spawn(async move {
            let ecrit = async {
                let client = Client::connect(&journal).await.map_err(|e| e.to_string())?;
                client
                    .call(
                        "ledger.append",
                        json!({"kind": genre, "task": mission, "actor": "egress", "payload": charge}),
                    )
                    .await
                    .map_err(|e| e.message)
            };
            if let Err(erreur) = ecrit.await {
                tracing::warn!(%erreur, genre, "décision de sortie non journalisée");
            }
        });
    }

    /// Une sortie relayée, telle que la mission la relira : hôte, port, méthode, octets et
    /// statut. Ni chemin complet, ni en-tête, ni corps.
    fn journaliser_la_sortie(
        &self,
        mission: Option<&str>,
        requete: &egress::ParsedRequest,
        bilan: Bilan,
    ) {
        let defaut = if requete.method == "CONNECT" || requete.target.starts_with("https://") {
            443
        } else {
            80
        };
        self.journaliser(
            mission,
            "net.request",
            json!({
                "host": requete.host,
                "port": port_de(&requete.target, defaut),
                "method": requete.method,
                "bytes_out": bilan.sortis,
                "bytes_in": bilan.recus,
                "status": bilan.statut,
            }),
        );
    }

    /// Demande à `capd` si ce jeton autorise une sortie vers cet hôte.
    async fn demander_a_capd(
        &self,
        jeton: &Value,
        hote: &str,
        modifiante: bool,
    ) -> anyhow::Result<Value> {
        let client = Client::connect(&self.capd).await?;
        let decision = client
            .call(
                "cap.check",
                json!({
                    "token": jeton,
                    "res": "net",
                    "act": "egress",
                    "target": hote,
                    // Ce qui distingue une lecture d'un effet, c'est la méthode — pas le fait de
                    // sortir. `docs/PLAN.md` le pose en toutes lettres : un GET sur un domaine
                    // autorisé passe automatiquement, tandis qu'envoyer, payer, poster ou
                    // supprimer à distance exige une approbation. Marquer toute sortie comme
                    // externe ferait demander une décision humaine pour chaque lecture, et une
                    // approbation qu'on donne cent fois par jour n'est plus une approbation.
                    "external": modifiante,
                    "irreversible": modifiante,
                }),
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e.message))?;
        Ok(decision)
    }
}

/// Lit l'en-tête de la requête, jusqu'à la ligne vide.
async fn lire_la_tete(
    lecteur: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
) -> std::io::Result<Option<String>> {
    let mut brut = String::new();
    loop {
        let mut ligne = String::new();
        if lecteur.read_line(&mut ligne).await? == 0 {
            return Ok(None);
        }
        let fin = ligne == "\r\n" || ligne == "\n";
        brut.push_str(&ligne);
        if fin {
            return Ok(Some(brut));
        }
        if brut.len() > TETE_MAX {
            // Analyser une tête coupée en deux donnerait une requête que personne n'a écrite.
            return Ok(None);
        }
    }
}

/// Le cadrage du corps est-il ambigu ?
///
/// Rend la raison du refus, ou `None` si la requête se lit d'une seule façon.
fn cadrage_ambigu(entetes: &[(String, String)]) -> Option<&'static str> {
    let compte = |nom: &str| {
        entetes
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(nom))
            .count()
    };
    if compte("content-length") > 1 {
        return Some("deux en-têtes Content-Length : le corps se lirait de deux façons");
    }
    if compte("transfer-encoding") > 0 {
        // Le proxy ne sait pas décoder le découpage en morceaux ; l'accepter reviendrait à
        // transmettre des en-têtes qui décrivent un corps que personne n'a lu.
        return Some(
            "Transfer-Encoding n'est pas accepté : ce proxy lit le corps avant de le laisser sortir",
        );
    }
    None
}

fn longueur_du_corps(entetes: &[(String, String)]) -> usize {
    entetes
        .iter()
        .find(|(nom, _)| nom.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, valeur)| valeur.parse().ok())
        .unwrap_or(0)
}

/// Extrait le jeton de l'en-tête interne.
fn jeton(entetes: &[(String, String)]) -> Option<Value> {
    use base64::Engine as _;
    let brut = entetes
        .iter()
        .find(|(nom, _)| nom.eq_ignore_ascii_case(ENTETE_JETON))
        .map(|(_, valeur)| valeur.as_str())?;
    let encode = brut.strip_prefix(PREFIXE)?;
    let octets = base64::engine::general_purpose::STANDARD
        .decode(encode)
        .ok()?;
    serde_json::from_slice(&octets).ok()
}

/// Les en-têtes à transmettre : tout, sauf ce qui est interne au proxy.
///
/// L'en-tête du jeton ne sort jamais. Le laisser passer donnerait la capacité de la tâche au
/// serveur distant, qui pourrait s'en servir.
fn entetes_sortants(entetes: &[(String, String)]) -> Vec<(String, String)> {
    entetes
        .iter()
        .filter(|(nom, _)| !nom.eq_ignore_ascii_case(ENTETE_JETON))
        .cloned()
        .collect()
}

/// La cible telle qu'un serveur l'attend : chemin **et** chaîne de requête.
///
/// `ParsedRequest::path()` retire la chaîne de requête, et c'est voulu — une chaîne de requête peut
/// porter des données, et le journal n'en veut pas. Mais la transmettre amputée changerait le sens
/// de toute requête qui en a une : `/chercher?q=x` deviendrait `/chercher`. Le journal et le relais
/// n'ont pas besoin de la même chose.
fn cible_relative(cible: &str) -> String {
    let sans_schema = cible.split_once("://").map_or(cible, |(_, reste)| reste);
    match sans_schema.find('/') {
        Some(index) => sans_schema[index..].to_owned(),
        // `GET http://hote HTTP/1.1` sans chemin : la racine.
        None => "/".to_owned(),
    }
}

/// Les racines TLS de la machine, lues une fois au démarrage.
///
/// Elles viennent du magasin système (`/etc/ssl/certs`, ou `SSL_CERT_FILE` s'il est défini),
/// jamais d'une liste embarquée dans le programme : c'est l'administrateur de la machine qui
/// décide à qui elle fait confiance, et c'est lui qui la met à jour.
fn racines_de_confiance() -> anyhow::Result<Arc<rustls::ClientConfig>> {
    let mut racines = rustls::RootCertStore::empty();
    let natives = rustls_native_certs::load_native_certs();
    let (ajoutees, ignorees) = racines.add_parsable_certificates(natives.certs);
    for erreur in &natives.errors {
        tracing::warn!(%erreur, "racine de confiance illisible");
    }
    if ajoutees == 0 {
        tracing::warn!("aucune racine de confiance : aucun amont HTTPS ne pourra être joint");
    } else {
        tracing::info!(ajoutees, ignorees, "racines de confiance chargées");
    }
    Ok(Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(racines)
            .with_no_client_auth(),
    ))
}

/// Délai accordé à l'amont pour accepter la connexion. Sans lui, un hôte qui ne répond pas
/// retiendrait l'appelant jusqu'au délai du noyau, plus de deux minutes : un proxy qui ne
/// répond pas est pire qu'un proxy qui refuse.
const DELAI_DE_CONNEXION: std::time::Duration = std::time::Duration::from_secs(15);

/// Joint l'amont, ou dit pourquoi il ne l'a pas fait, dans un délai borné.
async fn joindre_amont(hote: &str, port: u16) -> Result<TcpStream, String> {
    match tokio::time::timeout(DELAI_DE_CONNEXION, TcpStream::connect((hote, port))).await {
        Ok(Ok(flux)) => Ok(flux),
        Ok(Err(erreur)) => Err(erreur.to_string()),
        Err(_) => Err(format!(
            "{hote}:{port} n'a pas accepté la connexion en {} s",
            DELAI_DE_CONNEXION.as_secs()
        )),
    }
}

/// Ce qu'une sortie relayée a fait passer : octets dans chaque sens et statut rendu par l'amont
/// (`None` si l'amont n'a rien répondu de lisible).
#[derive(Debug, Clone, Copy, Default)]
struct Bilan {
    sortis: u64,
    recus: u64,
    statut: Option<u16>,
}

/// Relaie une requête HTTP, en clair vers `http://`, sous TLS terminé ici vers `https://`.
async fn relayer_http(
    requete: &egress::ParsedRequest,
    mut ecriture: tokio::net::unix::OwnedWriteHalf,
    tls: &Arc<rustls::ClientConfig>,
) -> anyhow::Result<Bilan> {
    let chiffre = requete.target.starts_with("https://");
    let port = port_de(&requete.target, if chiffre { 443 } else { 80 });
    let tcp = match joindre_amont(&requete.host, port).await {
        Ok(flux) => flux,
        Err(erreur) => {
            ecriture
                .write_all(&reponse(502, "Unreachable", &erreur))
                .await?;
            return Ok(Bilan {
                statut: Some(502),
                ..Bilan::default()
            });
        }
    };
    if chiffre {
        let nom = match rustls_pki_types::ServerName::try_from(requete.host.clone()) {
            Ok(nom) => nom,
            Err(erreur) => {
                ecriture
                    .write_all(&reponse(502, "BadServerName", &erreur.to_string()))
                    .await?;
                return Ok(Bilan {
                    statut: Some(502),
                    ..Bilan::default()
                });
            }
        };
        let connecteur = tokio_rustls::TlsConnector::from(Arc::clone(tls));
        match connecteur.connect(nom, tcp).await {
            Ok(amont) => transmettre(requete, amont, ecriture).await,
            Err(erreur) => {
                // Un certificat que la machine ne reconnaît pas ferme la sortie ; il ne la
                // dégrade pas en clair.
                ecriture
                    .write_all(&reponse(502, "TlsFailed", &erreur.to_string()))
                    .await?;
                Ok(Bilan {
                    statut: Some(502),
                    ..Bilan::default()
                })
            }
        }
    } else {
        transmettre(requete, tcp, ecriture).await
    }
}

/// Écrit la requête sur l'amont et recopie sa réponse octet pour octet.
async fn transmettre<A: AsyncRead + AsyncWrite + Unpin>(
    requete: &egress::ParsedRequest,
    mut amont: A,
    mut ecriture: tokio::net::unix::OwnedWriteHalf,
) -> anyhow::Result<Bilan> {
    let mut brut = format!(
        "{} {} HTTP/1.1\r\n",
        requete.method,
        cible_relative(&requete.target)
    );
    for (nom, valeur) in entetes_sortants(&requete.headers) {
        brut.push_str(&format!("{nom}: {valeur}\r\n"));
    }
    brut.push_str("\r\n");
    amont.write_all(brut.as_bytes()).await?;
    amont.write_all(&requete.body).await?;
    amont.flush().await?;

    // La ligne de statut est relue au passage, pour le journal ; le reste est recopié tel quel.
    let mut amont = BufReader::new(amont);
    let mut statut = Vec::new();
    amont.read_until(b'\n', &mut statut).await?;
    ecriture.write_all(&statut).await?;
    let reste = tokio::io::copy_buf(&mut amont, &mut ecriture).await?;
    ecriture.flush().await?;
    Ok(Bilan {
        sortis: (brut.len() + requete.body.len()) as u64,
        recus: statut.len() as u64 + reste,
        statut: String::from_utf8_lossy(&statut)
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok()),
    })
}

/// Établit un tunnel `CONNECT`.
///
/// Une fois le tunnel ouvert, le proxy ne voit plus rien : le contenu est chiffré de bout en bout
/// entre la tâche et le serveur. C'est voulu — c'est ce qui rend TLS utile — mais cela a une
/// conséquence qu'il vaut mieux écrire que découvrir : **la substitution de secrets est impossible
/// dans un tunnel**. Un secret ne peut être injecté que dans une requête que le proxy peut lire.
/// Voir ADR-0007.
async fn relayer_tunnel(
    requete: &egress::ParsedRequest,
    mut lecteur: BufReader<tokio::net::unix::OwnedReadHalf>,
    mut ecriture: tokio::net::unix::OwnedWriteHalf,
) -> anyhow::Result<Bilan> {
    let port = port_de(&requete.target, 443);
    let amont = match joindre_amont(&requete.host, port).await {
        Ok(flux) => flux,
        Err(erreur) => {
            ecriture
                .write_all(&reponse(502, "Unreachable", &erreur))
                .await?;
            return Ok(Bilan {
                statut: Some(502),
                ..Bilan::default()
            });
        }
    };
    ecriture
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    ecriture.flush().await?;

    let (mut amont_lecture, mut amont_ecriture) = amont.into_split();
    let montant = async {
        let octets = tokio::io::copy_buf(&mut lecteur, &mut amont_ecriture).await?;
        amont_ecriture.shutdown().await?;
        Ok::<u64, std::io::Error>(octets)
    };
    let descendant = async {
        let octets = tokio::io::copy(&mut amont_lecture, &mut ecriture).await?;
        ecriture.shutdown().await?;
        Ok::<u64, std::io::Error>(octets)
    };
    let (sortis, recus) = tokio::join!(montant, descendant);
    Ok(Bilan {
        sortis: sortis.unwrap_or(0),
        recus: recus.unwrap_or(0),
        statut: Some(200),
    })
}

fn port_de(cible: &str, defaut: u16) -> u16 {
    let sans_schema = cible
        .split_once("://")
        .map_or(cible, |(_, reste)| reste)
        .split('/')
        .next()
        .unwrap_or(cible);
    sans_schema
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse().ok())
        .unwrap_or(defaut)
}

/// Une réponse d'erreur explicite. Jamais un silence : une tâche qui ne comprend pas pourquoi elle
/// est bloquée réessaiera, ou pire, contournera.
fn reponse(statut: u16, code: &str, detail: &str) -> Vec<u8> {
    let corps = json!({ "code": code, "detail": detail }).to_string();
    format!(
        "HTTP/1.1 {statut} Prophet\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{corps}",
        corps.len()
    )
    .into_bytes()
}

/// Le cœur de décision reste celui de la bibliothèque, et il reste testable sans réseau.
///
/// Cette fonction n'est pas appelée par le chemin ci-dessus — `capd` y tranche — mais elle existe
/// pour que le daemon puisse être exercé hors ligne, avec une politique fixe et sans broker.
#[cfg_attr(not(test), expect(dead_code, reason = "chemin d'essai hors ligne"))]
fn hors_ligne(politique: Policy, requete: &egress::ParsedRequest) -> RequestLog {
    Proxy::new("task:hors-ligne", politique).evaluate(requete)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egress::Verdict;

    #[test]
    fn le_jeton_ne_sort_jamais() {
        // Le laisser passer donnerait la capacité de la tâche au serveur distant.
        let entetes = vec![
            ("Host".to_owned(), "api.exemple.fr".to_owned()),
            ("Proxy-Authorization".to_owned(), "Prophet eyJ9".to_owned()),
            ("Accept".to_owned(), "application/json".to_owned()),
        ];
        let sortants = entetes_sortants(&entetes);
        assert_eq!(sortants.len(), 2);
        assert!(
            !sortants
                .iter()
                .any(|(nom, _)| nom.eq_ignore_ascii_case("proxy-authorization"))
        );
    }

    #[test]
    fn le_port_se_lit_sans_se_deviner() {
        assert_eq!(port_de("api.exemple.fr:8443", 443), 8443);
        assert_eq!(port_de("api.exemple.fr", 443), 443);
        assert_eq!(port_de("http://api.exemple.fr/v1/x", 80), 80);
        assert_eq!(port_de("http://api.exemple.fr:8080/v1/x", 80), 8080);
    }

    #[test]
    fn un_jeton_absent_ou_illisible_ne_devient_pas_un_jeton_vide() {
        // Un `Some(null)` passerait ensuite pour un jeton et serait envoyé à capd, qui le
        // refuserait — mais la requête aurait franchi une étape qu'elle ne devait pas franchir.
        assert!(jeton(&[]).is_none());
        assert!(jeton(&[("Proxy-Authorization".to_owned(), "Bearer x".to_owned())]).is_none());
        assert!(jeton(&[("Proxy-Authorization".to_owned(), "Prophet !!!".to_owned())]).is_none());
    }

    #[test]
    fn la_decision_hors_ligne_reste_celle_de_la_bibliotheque() {
        let requete = parse_request(
            "GET http://collecteur.example.com/x HTTP/1.1\r\nHost: collecteur.example.com\r\n\r\n",
        )
        .unwrap();
        let trace = hors_ligne(Policy::allowing(["*.exemple.fr"]), &requete);
        assert!(matches!(trace.verdict, Verdict::Deny { .. }));
    }
    #[test]
    fn la_chaine_de_requete_ne_se_perd_pas_en_route() {
        // `path()` la retire — c'est bon pour le journal, qui n'a pas à garder ce qu'elle peut
        // porter. La transmettre amputée changerait le sens de la requête.
        assert_eq!(
            cible_relative("http://api.exemple.fr/chercher?q=confidentiel&p=2"),
            "/chercher?q=confidentiel&p=2"
        );
        assert_eq!(cible_relative("http://api.exemple.fr"), "/");
        assert_eq!(cible_relative("http://api.exemple.fr/"), "/");
    }

    #[test]
    fn un_cadrage_ambigu_est_refuse_et_non_interprete() {
        let deux = vec![
            ("Content-Length".to_owned(), "0".to_owned()),
            ("content-length".to_owned(), "42".to_owned()),
        ];
        assert!(cadrage_ambigu(&deux).is_some());

        let decoupe = vec![("Transfer-Encoding".to_owned(), "chunked".to_owned())];
        assert!(cadrage_ambigu(&decoupe).is_some());

        let clair = vec![("Content-Length".to_owned(), "12".to_owned())];
        assert!(cadrage_ambigu(&clair).is_none());
    }
}
