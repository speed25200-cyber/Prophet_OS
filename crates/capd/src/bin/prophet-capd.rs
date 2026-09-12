//! `prophet-capd` — le broker de capacités, en service.
//!
//! La bibliothèque `capd` sait tout décider ; ce programme lui donne une porte. Il ouvre
//! `/run/prophet/capd.sock`, et répond aux méthodes que `docs/specs/ipc.md` nomme : `cap.mint`,
//! `cap.check`, `approval.request`, `approval.resolve`, `approval.pending`.
//!
//! Pourquoi ce fichier n'existait pas. `image/modules/prophet.nix` déclarait sept services dont
//! l'`ExecStart` nommait sept programmes — et l'atelier n'en produisait aucun. Une machine
//! installée aurait démarré avec sept unités en échec, pendant que tout le reste paraissait
//! fonctionner. `tools/verifier-les-services.sh` refuse désormais cet écart.
//!
//! La clé de signature vit dans l'état du service, en mode `0600`, et n'en sort jamais : les
//! appelants reçoivent des jetons signés, jamais la clé. C'est le même principe que le Vault, qui
//! rend des poignées et non des valeurs.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use capd::{ApprovalScope, Broker, PolicyEngine};
use ed25519_dalek::SigningKey;
use prophet_ipc::{Error, ErrorCode, Handler, PeerIdentity, Server};
use serde_json::{Value, json};
use time::OffsetDateTime;
use tokio::sync::Mutex;

/// Groupe dont les membres peuvent appeler les méthodes système.
///
/// `SO_PEERCRED` donne un identifiant numérique ; le nom est résolu au démarrage, une fois, et
/// l'absence du groupe est fatale. Servir sans savoir à qui l'on parle serait pire que ne pas
/// servir.
const GROUPE_SYSTEME: &str = "prophet-system";

struct Capd {
    broker: Mutex<Broker>,
    /// Identifiant du groupe système, ou `None` quand il n'existe pas sur cette machine — cas des
    /// développements hors image, où l'on sert alors le seul utilisateur qui a lancé le service.
    gid_systeme: Option<u32>,
    uid_propre: u32,
}

impl Capd {
    /// Le pair a-t-il le droit d'appeler une méthode système ?
    ///
    /// Deux réponses acceptables, et une seule règle : soit le pair appartient au groupe système,
    /// soit il *est* le service lui-même. Rien d'autre.
    fn pair_autorise(&self, peer: PeerIdentity) -> bool {
        match self.gid_systeme {
            Some(gid) => peer.gid == gid || peer.uid == self.uid_propre,
            None => peer.uid == self.uid_propre,
        }
    }
}

impl Handler for Capd {
    async fn call(
        &self,
        peer: PeerIdentity,
        _auth: Option<String>,
        method: String,
        params: Value,
    ) -> Result<Value, Error> {
        if method != "ping" && !self.pair_autorise(peer) {
            tracing::warn!(uid = peer.uid, gid = peer.gid, %method, "pair refusé");
            return Err(Error::new(
                ErrorCode::Unauthorized,
                "ce pair n'appartient pas au groupe système",
            ));
        }

        let maintenant = OffsetDateTime::now_utc();
        match method.as_str() {
            "ping" => Ok(json!("pong")),

            // La clé publique, pour qu'un service vérifie un jeton sans repasser par ici.
            "cap.public_key" => {
                let broker = self.broker.lock().await;
                Ok(json!({
                    "key": format!(
                        "ed25519:{}",
                        base64_standard(broker.verifying_key().as_bytes())
                    )
                }))
            }

            "approval.pending" => {
                let broker = self.broker.lock().await;
                let attente = broker.approvals().pending();
                Ok(serde_json::to_value(attente)
                    .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))?)
            }

            "approval.resolve" => {
                let id = champ_texte(&params, "id")?;
                let decision = match champ_texte(&params, "decision")?.as_str() {
                    "allow" | "approve" => capd::ApprovalDecision::Allow,
                    "deny" | "refuse" => capd::ApprovalDecision::Deny,
                    autre => {
                        return Err(Error::new(
                            ErrorCode::InvalidParams,
                            format!("décision inconnue : {autre} (attendu « allow » ou « deny »)"),
                        ));
                    }
                };
                // La portée par défaut est la plus étroite. Élargir une autorisation est une
                // décision en soi : elle se demande, elle ne se déduit pas d'un champ absent.
                let portee = match params.get("scope").and_then(Value::as_str) {
                    None | Some("once") => ApprovalScope::Once,
                    Some("task") => ApprovalScope::Task,
                    Some("agent") => ApprovalScope::Agent {
                        days: u16::try_from(
                            params.get("days").and_then(Value::as_u64).unwrap_or(7),
                        )
                        .unwrap_or(7),
                    },
                    Some(autre) => {
                        return Err(Error::new(
                            ErrorCode::InvalidParams,
                            format!("portée inconnue : {autre}"),
                        ));
                    }
                };
                let mut broker = self.broker.lock().await;
                let tranchee = broker
                    .approvals_mut()
                    .resolve(&id, decision, portee, maintenant)
                    .ok_or_else(|| {
                        Error::new(
                            ErrorCode::NotFound,
                            "demande inconnue ou déjà tranchée".to_owned(),
                        )
                    })?;
                tracing::info!(%id, ?decision, "approbation tranchée");
                Ok(serde_json::to_value(tranchee)
                    .map_err(|e| Error::new(ErrorCode::InternalError, e.to_string()))?)
            }

            // Les demandes périmées cessent d'attendre. Une approbation qu'on ne peut plus
            // accorder ne doit pas rester à l'écran comme si elle le pouvait.
            "approval.expire" => {
                let mut broker = self.broker.lock().await;
                let perimees = broker.approvals_mut().expire(maintenant);
                Ok(json!({ "expired": perimees.len() }))
            }

            autre => Err(Error::new(
                ErrorCode::MethodNotFound,
                format!("méthode inconnue : {autre}"),
            )),
        }
    }
}

fn champ_texte(params: &Value, nom: &str) -> Result<String, Error> {
    params
        .get(nom)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            Error::new(
                ErrorCode::InvalidParams,
                format!("paramètre « {nom} » attendu, de type chaîne"),
            )
        })
}

fn base64_standard(octets: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(octets)
}

/// Charge la clé de signature, ou en crée une au premier démarrage.
///
/// Le mode `0600` est vérifié après écriture, et non supposé : un `umask` hostile ferait mentir la
/// création seule.
fn clef(etat: &Path) -> std::io::Result<SigningKey> {
    use std::os::unix::fs::PermissionsExt as _;

    let chemin = etat.join("signing.key");
    if chemin.exists() {
        let octets = std::fs::read(&chemin)?;
        let tableau: [u8; 32] = octets.as_slice().try_into().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{} ne fait pas 32 octets", chemin.display()),
            )
        })?;
        return Ok(SigningKey::from_bytes(&tableau));
    }

    std::fs::create_dir_all(etat)?;
    let clef = SigningKey::generate(&mut rand::rngs::OsRng);
    std::fs::write(&chemin, clef.to_bytes())?;
    std::fs::set_permissions(&chemin, std::fs::Permissions::from_mode(0o600))?;
    let mode = std::fs::metadata(&chemin)?.permissions().mode() & 0o777;
    if mode != 0o600 {
        return Err(std::io::Error::other(format!(
            "la clé est en mode {mode:o} après écriture, attendu 600"
        )));
    }
    tracing::info!(chemin = %chemin.display(), "clé de signature créée");
    Ok(clef)
}

/// Charge les politiques de `/etc/prophet/policies`, ou celles par défaut si le répertoire est
/// vide.
///
/// Un fichier de politique illisible arrête le service. Continuer avec les règles par défaut
/// laisserait croire qu'une politique s'applique alors qu'elle a été ignorée, et c'est exactement
/// le genre de silence qui rend une autorisation fausse.
fn politiques(repertoire: &Path) -> anyhow::Result<PolicyEngine> {
    if !repertoire.is_dir() {
        tracing::info!(chemin = %repertoire.display(), "aucune politique locale, défauts appliqués");
        return Ok(PolicyEngine::with_defaults()?);
    }
    let mut fichiers: Vec<PathBuf> = std::fs::read_dir(repertoire)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "cedar"))
        .collect();
    fichiers.sort();
    if fichiers.is_empty() {
        tracing::info!(chemin = %repertoire.display(), "répertoire vide, défauts appliqués");
        return Ok(PolicyEngine::with_defaults()?);
    }
    let mut texte = String::new();
    for fichier in &fichiers {
        texte.push_str(&std::fs::read_to_string(fichier)?);
        texte.push('\n');
    }
    tracing::info!(nombre = fichiers.len(), "politiques locales chargées");
    Ok(PolicyEngine::new(&texte)?)
}

/// L'identifiant d'utilisateur de ce processus, sans passer par `unsafe`.
///
/// `/proc/self` appartient au propriétaire du processus : le noyau le dit, et le lire évite un
/// appel à `libc` pour une information que le système de fichiers porte déjà.
fn uid_propre() -> std::io::Result<u32> {
    use std::os::unix::fs::MetadataExt as _;
    Ok(std::fs::metadata("/proc/self")?.uid())
}

fn gid_du_groupe(nom: &str) -> Option<u32> {
    let contenu = std::fs::read_to_string("/etc/group").ok()?;
    contenu.lines().find_map(|ligne| {
        let mut champs = ligne.split(':');
        (champs.next()? == nom).then(|| champs.nth(1)?.parse().ok())?
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let etat = std::env::var("STATE_DIRECTORY")
        .map_or_else(|_| PathBuf::from("/var/lib/prophet/capd"), PathBuf::from);
    let socket = std::env::var("PROPHET_SOCKET")
        .map_or_else(|_| PathBuf::from("/run/prophet/capd.sock"), PathBuf::from);
    let maison = std::env::var("PROPHET_HOME").unwrap_or_else(|_| "/home/prophet".to_owned());

    let broker = Broker::new(clef(&etat)?, "capd", maison)?
        .with_policy(politiques(Path::new("/etc/prophet/policies"))?);

    let gid_systeme = gid_du_groupe(GROUPE_SYSTEME);
    if gid_systeme.is_none() {
        tracing::warn!(
            groupe = GROUPE_SYSTEME,
            "groupe absent : seul l'utilisateur du service sera servi"
        );
    }

    let serveur = Server::bind(&socket)?;
    tracing::info!(socket = %socket.display(), "capd écoute");

    serveur
        .serve(Arc::new(Capd {
            broker: Mutex::new(broker),
            gid_systeme,
            uid_propre: uid_propre()?,
        }))
        .await?;
    Ok(())
}
