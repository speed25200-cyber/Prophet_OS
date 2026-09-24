//! Navigation par l'arbre sémantique : l'agent ouvre, lit et agit sur une page sans pixels.
//!
//! Le navigateur est celui du pont CDP, avec un profil par tâche. La page est observée comme un
//! arbre SUP et manipulée par des actions typées désignant des éléments par identifiant. Aucune
//! capture d'écran n'intervient. L'hôte ouvert est contrôlé par capd comme une sortie réseau ;
//! lire l'arbre et agir dessus sont contrôlés comme des accès d'interface.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use browser_bridge::cdp::Session;
use browser_bridge::{Browser, Page};
use serde_json::{Value, json};
use sup::tree::Detail;

use crate::protocol::{CallResult, ErrorCode, ToolMeta, ToolSpec};
use crate::registry::{Tool, ToolContext};
use crate::tools::http::host_of;

/// La session de navigation d'une tâche : un navigateur, un profil privé, une page.
///
/// Le navigateur n'est lancé qu'au premier `web.open`, et meurt avec la session. Les trois
/// outils partagent cette valeur ; chacun n'existe que si le service a configuré un programme.
pub struct Browsing {
    program: PathBuf,
    profile_root: PathBuf,
    /// Socket d'egress ; quand il est donné, tout le trafic du navigateur y est relayé.
    egress: Option<PathBuf>,
    /// Le jeton de la tâche, encodé pour l'en-tête interne du proxy, posé au premier `web.open`.
    token: Arc<Mutex<Option<String>>>,
    relay: Mutex<Option<crate::tools::web_relay::Relay>>,
    live: Mutex<Option<Live>>,
}

struct Live {
    runtime: tokio::runtime::Runtime,
    _browser: Browser,
    page: Page,
    url: String,
}

impl std::fmt::Debug for Browsing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Browsing")
            .field("program", &self.program)
            .finish_non_exhaustive()
    }
}

impl Browsing {
    /// Une session dont le programme et le répertoire de profils viennent du service.
    #[must_use]
    pub fn new(program: PathBuf, profile_root: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            program,
            profile_root,
            egress: None,
            token: Arc::new(Mutex::new(None)),
            relay: Mutex::new(None),
            live: Mutex::new(None),
        })
    }

    /// Une session dont tout le trafic du navigateur passe par le socket d'egress, sous le
    /// jeton de la tâche : c'est la forme que le service emploie. Sans egress joignable, le
    /// navigateur n'a aucune route.
    #[must_use]
    pub fn via_egress(program: PathBuf, profile_root: PathBuf, egress: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            program,
            profile_root,
            egress: Some(egress),
            token: Arc::new(Mutex::new(None)),
            relay: Mutex::new(None),
            live: Mutex::new(None),
        })
    }

    /// Vrai si le navigateur est relayé par egress.
    #[must_use]
    pub fn relayed(&self) -> bool {
        self.egress.is_some()
    }

    fn bind_token(&self, token: &prophet_types::cap::Token) -> Result<(), String> {
        use base64::Engine as _;
        let json = serde_json::to_string(token).map_err(|e| e.to_string())?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(json);
        *self
            .token
            .lock()
            .map_err(|_| "jeton de navigation indisponible".to_owned())? = Some(encoded);
        Ok(())
    }

    /// Les trois outils de cette session.
    #[must_use]
    pub fn tools(self: &Arc<Self>) -> [Arc<dyn Tool>; 3] {
        [
            Arc::new(Open(Arc::clone(self))),
            Arc::new(Observe(Arc::clone(self))),
            Arc::new(Act(Arc::clone(self))),
        ]
    }

    fn with_live<T>(
        &self,
        task: &str,
        launch: bool,
        f: impl FnOnce(&mut Live) -> Result<T, String>,
    ) -> Result<T, String> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err("la navigation exige un thread de travail".into());
        }
        let mut guard = self
            .live
            .lock()
            .map_err(|_| "session de navigation indisponible".to_owned())?;
        if guard.is_none() {
            if !launch {
                return Err("aucune page ouverte : appelez d'abord web.open".into());
            }
            *guard = Some(self.launch(task)?);
        }
        let live = guard.as_mut().ok_or("session de navigation absente")?;
        f(live)
    }

    /// Fichier où la dernière observation d'une tâche est déposée pour la supervision.
    ///
    /// Adresse, titre et taille de l'arbre, jamais l'arbre lui-même : l'humain voit où l'agent
    /// est et ce que la page dit d'elle-même, pas ce qu'elle contient.
    #[must_use]
    pub fn observation_path(profile_root: &std::path::Path, task: &str) -> PathBuf {
        profile_root.join(task).join("observation.json")
    }

    fn record(&self, task: &str, url: &str, tree: &sup::tree::Tree) {
        let path = Self::observation_path(&self.profile_root, task);
        let value = json!({
            "url": url,
            "title": tree.title,
            "nodes": tree.root.count(),
            "at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        });
        let tmp = path.with_extension("json.tmp");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(&tmp, value.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    /// Sonde le navigateur : le lance sur une page vierge dans un profil jetable sous `root`
    /// et rend la version qu'il annonce.
    ///
    /// C'est ce que le service fait au démarrage, sous ses propres contraintes, pour dire
    /// d'avance si le programme configuré tourne, plutôt que de le découvrir au premier outil
    /// d'une mission déjà lancée. Aucune sortie réseau : la page vierge n'en demande pas.
    ///
    /// # Errors
    /// Lancement, point d'écoute ou protocole en échec ; le message dit lequel.
    pub fn probe(program: &Path, root: &Path) -> Result<String, String> {
        let profile = root.join("sonde");
        std::fs::create_dir_all(&profile).map_err(|e| format!("profil de sonde : {e}"))?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let program = program.display().to_string();
        let version = runtime.block_on(async {
            let browser = Browser::launch_auto(&program, &profile)
                .await
                .map_err(|e| format!("lancement : {e}"))?;
            let endpoint = browser
                .page_endpoint()
                .await
                .map_err(|e| format!("point d'écoute : {e}"))?;
            let mut session = Session::connect(&endpoint)
                .await
                .map_err(|e| format!("session : {e}"))?;
            let version = session
                .call("Browser.getVersion", json!({}))
                .await
                .map_err(|e| format!("protocole : {e}"))?;
            Ok::<_, String>(
                version["product"]
                    .as_str()
                    .unwrap_or("version non annoncée")
                    .to_owned(),
            )
        });
        // Le navigateur est tué à la sortie du bloc, mais ses processus auxiliaires finissent
        // d'écrire dans le profil quelques instants encore : on insiste brièvement.
        for _ in 0..40 {
            if std::fs::remove_dir_all(&profile).is_ok() || !profile.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        version
    }

    fn launch(&self, task: &str) -> Result<Live, String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let profile = self.profile_root.join(task).join("browser");
        let program = self.program.display().to_string();
        let proxy = match &self.egress {
            Some(egress) => {
                let mut relay = self
                    .relay
                    .lock()
                    .map_err(|_| "relais indisponible".to_owned())?;
                if relay.is_none() {
                    *relay = Some(
                        crate::tools::web_relay::Relay::start(egress.clone(), self.token.clone())
                            .map_err(|e| format!("relais vers egress impossible : {e}"))?,
                    );
                }
                relay.as_ref().map(|r| r.address())
            }
            None => None,
        };
        let (browser, page) = runtime.block_on(async {
            let browser = Browser::launch_auto_with(&program, &profile, proxy.as_deref())
                .await
                .map_err(|e| e.to_string())?;
            let endpoint = browser.page_endpoint().await.map_err(|e| e.to_string())?;
            let session = Session::connect(&endpoint)
                .await
                .map_err(|e| e.to_string())?;
            let page = Page::attach(session).await.map_err(|e| e.to_string())?;
            Ok::<_, String>((browser, page))
        })?;
        Ok(Live {
            runtime,
            _browser: browser,
            page,
            url: String::new(),
        })
    }
}

fn detail_of(args: &Value) -> Result<Detail, CallResult> {
    match args.get("detail").and_then(Value::as_str) {
        None | Some("normal") => Ok(Detail::Normal),
        Some("summary") => Ok(Detail::Summary),
        Some("full") => Ok(Detail::Full),
        Some(other) => Err(CallResult::error(
            ErrorCode::Invalid,
            format!("détail inconnu : {other} (summary, normal ou full)"),
        )),
    }
}

/// Nœuds rendus au plus d'une page, et caractères au plus d'un nom ou d'une valeur : les bornes
/// de l'accessibilité du bureau (supd). Une page démesurée ne remplit pas le contexte du modèle.
const WEB_NOEUDS_MAX: usize = 500;
const WEB_TEXTE_MAX: usize = 4_000;

/// L'arbre tel qu'on le rend au modèle, borné, et le nombre de nœuds laissés de côté.
fn borne(tree: &sup::tree::Tree) -> (sup::tree::Tree, usize) {
    let (root, laisses) = tree.root.borner(WEB_NOEUDS_MAX, WEB_TEXTE_MAX);
    (
        sup::tree::Tree {
            root,
            ..tree.clone()
        },
        laisses,
    )
}

fn observation(url: &str, tree: &sup::tree::Tree, detail: Detail) -> CallResult {
    let (rendu, laisses) = borne(tree);
    CallResult::structured(json!({
        "url": url,
        "title": tree.title,
        "nodes": tree.root.count(),
        "observation_bytes": rendu.observation_size(detail),
        "truncated": laisses,
        "tree": rendu,
    }))
}

/// `web.open` : ouvre une adresse et rend l'arbre de la page.
#[derive(Debug)]
pub struct Open(Arc<Browsing>);

impl Tool for Open {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "web.open".into(),
            description: "Ouvre une page web dans le navigateur de la tâche et rend son arbre sémantique : éléments, rôles, textes, champs et actions possibles. Aucune capture d'écran. L'hôte doit être autorisé.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string"},
                    "detail": {"type": "string", "enum": ["summary", "normal", "full"], "default": "normal"}
                },
                "required": ["url"],
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "net.egress".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, args: &Value, _context: &ToolContext) -> Option<String> {
        host_of(args.get("url")?.as_str()?)
    }

    fn call(&self, args: &Value, context: &ToolContext) -> CallResult {
        let Some(url) = args.get("url").and_then(Value::as_str) else {
            return CallResult::error(ErrorCode::Invalid, "argument `url` manquant");
        };
        if host_of(url).is_none() || !(url.starts_with("http://") || url.starts_with("https://")) {
            return CallResult::error(ErrorCode::Invalid, format!("adresse illisible : {url}"));
        }
        let detail = match detail_of(args) {
            Ok(d) => d,
            Err(e) => return e,
        };
        if let Err(e) = self.0.bind_token(&context.token) {
            return CallResult::error(ErrorCode::Internal, e);
        }
        let url = url.to_owned();
        let result = self.0.with_live(&context.task, true, |live| {
            let target = url.clone();
            let tree = live.runtime.block_on(async {
                live.page
                    .navigate(&target)
                    .await
                    .map_err(|e| e.to_string())?;
                live.page.tree(detail).await.map_err(|e| e.to_string())
            })?;
            live.url = url.clone();
            Ok(tree)
        });
        match result {
            Ok(tree) => {
                self.0.record(&context.task, &url, &tree);
                observation(&url, &tree, detail)
            }
            Err(e) => CallResult::error(ErrorCode::SandboxError, e),
        }
    }
}

/// `web.tree` : relit l'arbre de la page courante.
#[derive(Debug)]
pub struct Observe(Arc<Browsing>);

impl Tool for Observe {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "web.tree".into(),
            description: "Rend l'arbre sémantique de la page courante, au niveau de détail demandé. Utile après une action ou un chargement.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "detail": {"type": "string", "enum": ["summary", "normal", "full"], "default": "normal"}
                },
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "ui.read".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, _args: &Value, _context: &ToolContext) -> Option<String> {
        Some("browser".into())
    }

    fn call(&self, args: &Value, context: &ToolContext) -> CallResult {
        let detail = match detail_of(args) {
            Ok(d) => d,
            Err(e) => return e,
        };
        let result = self.0.with_live(&context.task, false, |live| {
            let tree = live
                .runtime
                .block_on(live.page.tree(detail))
                .map_err(|e| e.to_string())?;
            Ok((live.url.clone(), tree))
        });
        match result {
            Ok((url, tree)) => observation(&url, &tree, detail),
            Err(e) => CallResult::error(ErrorCode::SandboxError, e),
        }
    }
}

/// `web.act` : une action typée sur un élément, avec l'arbre résultant.
#[derive(Debug)]
pub struct Act(Arc<Browsing>);

impl Tool for Act {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "web.act".into(),
            description: "Agit sur la page courante : click sur un élément, set_field pour remplir un champ ou choisir une option (value), submit pour envoyer le formulaire d'un élément. Les éléments sont désignés par leur identifiant dans l'arbre. Rend le résultat et le nouvel arbre. Soumettre engage un effet extérieur et demande une décision humaine. Pour changer d'adresse, utilisez web.open.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["click", "set_field", "submit"]},
                    "node": {"type": "string"},
                    "value": {"type": "string"}
                },
                "required": ["action"],
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "ui.act".into(),
                irreversible: false,
                external: true,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, _args: &Value, _context: &ToolContext) -> Option<String> {
        Some("browser".into())
    }

    /// Remplir un champ ou cliquer restent locaux à la page ; soumettre envoie quelque chose
    /// hors de la machine et exige une décision humaine.
    fn effects(&self, args: &Value, _meta: &ToolMeta) -> (bool, bool) {
        match args.get("action").and_then(Value::as_str) {
            Some("submit") => (false, true),
            _ => (false, false),
        }
    }

    fn call(&self, args: &Value, context: &ToolContext) -> CallResult {
        let Some(action) = args.get("action").and_then(Value::as_str) else {
            return CallResult::error(ErrorCode::Invalid, "argument `action` manquant");
        };
        if !matches!(action, "click" | "set_field" | "submit") {
            return CallResult::error(
                ErrorCode::Invalid,
                format!("action inconnue : {action} ; pour ouvrir une adresse, utilisez web.open"),
            );
        }
        let node = args.get("node").and_then(Value::as_str).map(str::to_owned);
        let value = args.get("value").and_then(Value::as_str).map(str::to_owned);
        let action = action.to_owned();
        let result = self.0.with_live(&context.task, false, |live| {
            let (outcome, tree) = live
                .runtime
                .block_on(live.page.act(&action, node.as_deref(), value.as_deref()))
                .map_err(|e| e.to_string())?;
            let url = live
                .runtime
                .block_on(live.page.current_url())
                .unwrap_or_else(|_| live.url.clone());
            live.url = url.clone();
            Ok((outcome, url, tree))
        });
        match result {
            Ok((outcome, url, tree)) if outcome.ok => {
                self.0.record(&context.task, &url, &tree);
                let (rendu, laisses) = borne(&tree);
                CallResult::structured(json!({
                    "ok": true,
                    "url": url,
                    "title": tree.title,
                    "truncated": laisses,
                    "tree": rendu,
                }))
            }
            Ok((outcome, _, _)) => CallResult::error(
                match outcome.error.as_deref() {
                    Some("NodeNotFound") => ErrorCode::NotFound,
                    _ => ErrorCode::Invalid,
                },
                format!(
                    "{} : {}",
                    outcome.error.unwrap_or_else(|| "ActionFailed".into()),
                    outcome.detail.unwrap_or_default()
                ),
            ),
            Err(e) => CallResult::error(ErrorCode::SandboxError, e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sup::tree::{Node, Role, Tree};

    #[test]
    fn une_page_demesuree_est_rendue_bornee_avec_le_compte_de_ce_qui_manque() {
        let liens: Vec<Node> = (0..1_999)
            .map(|n| Node::new(format!("l{n}"), Role::Link, format!("lien {n}")).actionable())
            .collect();
        let page = Tree::new(
            "prophet.browser",
            "https://exemple.fr/",
            "Mille liens",
            Node::new("root", Role::Group, "").children(liens),
        );
        let rendu = observation("https://exemple.fr/", &page, Detail::Full)
            .structured
            .unwrap();
        assert_eq!(
            rendu["nodes"],
            json!(2_000),
            "le compte de la page reste entier"
        );
        assert_eq!(rendu["truncated"], json!(1_500));
        let arbre: Tree = serde_json::from_value(rendu["tree"].clone()).unwrap();
        assert_eq!(arbre.root.count(), WEB_NOEUDS_MAX);
    }
}
