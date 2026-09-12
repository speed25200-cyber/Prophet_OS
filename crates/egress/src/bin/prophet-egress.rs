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

use egress::{Detector, Outbound, Policy, Proxy, RequestLog, parse_request};
use prophet_daemon as commun;
use prophet_ipc::Client;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};
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
    detecteur: Detector,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    commun::journaliser();

    let socket = commun::socket("egress");
    if socket.exists() {
        std::fs::remove_file(&socket)?;
    }
    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let ecoute = UnixListener::bind(&socket)?;
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o660))?;
    }

    let capd = std::env::var("PROPHET_CAPD_SOCKET").map_or_else(
        |_| prophet_ipc::socket_path("capd"),
        std::path::PathBuf::from,
    );

    tracing::info!(
        socket = %socket.display(),
        capd = %capd.display(),
        "egress écoute ; rien ne sort sans un jeton que capd approuve"
    );

    let sortie = Arc::new(Sortie {
        capd,
        detecteur: Detector::new(),
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
        let modifiante = egress::policy::MUTATING_METHODS.contains(&requete.method.as_str());
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

        if decision["decision"] != "allow" {
            let motif = decision["reason"].as_str().unwrap_or("refusé").to_owned();
            tracing::info!(hote = %requete.host, %motif, "sortie refusée par capd");
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
            relayer_tunnel(&requete, lecteur, ecriture).await
        } else {
            relayer_http(&requete, ecriture).await
        }
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
        if fin || brut.len() > TETE_MAX {
            break;
        }
    }
    Ok(Some(brut))
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

/// Relaie une requête HTTP en clair.
async fn relayer_http(
    requete: &egress::ParsedRequest,
    mut ecriture: tokio::net::unix::OwnedWriteHalf,
) -> anyhow::Result<()> {
    let port = port_de(&requete.target, 80);
    let mut amont = match TcpStream::connect((requete.host.as_str(), port)).await {
        Ok(flux) => flux,
        Err(erreur) => {
            ecriture
                .write_all(&reponse(502, "Unreachable", &erreur.to_string()))
                .await?;
            return Ok(());
        }
    };

    let mut brut = format!("{} {} HTTP/1.1\r\n", requete.method, requete.path());
    for (nom, valeur) in entetes_sortants(&requete.headers) {
        brut.push_str(&format!("{nom}: {valeur}\r\n"));
    }
    brut.push_str("\r\n");
    amont.write_all(brut.as_bytes()).await?;
    amont.write_all(&requete.body).await?;
    amont.flush().await?;

    tokio::io::copy(&mut amont, &mut ecriture).await?;
    ecriture.flush().await?;
    Ok(())
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
) -> anyhow::Result<()> {
    let port = port_de(&requete.target, 443);
    let amont = match TcpStream::connect((requete.host.as_str(), port)).await {
        Ok(flux) => flux,
        Err(erreur) => {
            ecriture
                .write_all(&reponse(502, "Unreachable", &erreur.to_string()))
                .await?;
            return Ok(());
        }
    };
    ecriture
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    ecriture.flush().await?;

    let (mut amont_lecture, mut amont_ecriture) = amont.into_split();
    let montant = async {
        tokio::io::copy_buf(&mut lecteur, &mut amont_ecriture).await?;
        amont_ecriture.shutdown().await
    };
    let descendant = async {
        tokio::io::copy(&mut amont_lecture, &mut ecriture).await?;
        ecriture.shutdown().await
    };
    let (_, _) = tokio::join!(montant, descendant);
    Ok(())
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
}
