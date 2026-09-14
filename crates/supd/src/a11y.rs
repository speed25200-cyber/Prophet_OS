//! Lecture et pilotage par AT-SPI.
//!
//! Chaque nœud coûte quelques appels D-Bus (nom, rôle, états, interfaces, enfants, texte) ; la
//! lecture est bornée en nœuds et en profondeur, et dit combien elle a laissé de côté. Un arbre
//! partiel annoncé vaut mieux qu'un arbre complet qui ne vient jamais.

use atspi_common::{Interface, ObjectRefOwned, State};
use atspi_proxies::accessible::{AccessibleProxy, ObjectRefExt};
use atspi_proxies::action::ActionProxy;
use atspi_proxies::bus::BusProxy;
use atspi_proxies::proxy_ext::ProxyExt;
use atspi_proxies::text::TextProxy;
use atspi_proxies::value::ValueProxy;
use sup::adapter::{AccessibleNode, convert_tree};
use sup::session::{ActRequest, AppView, Observation, Outcome};
use sup::tree::Tree;

/// Nombre maximal de nœuds lus pour une fenêtre.
pub const MAX_NODES: usize = 500;
/// Profondeur maximale de lecture.
pub const MAX_DEPTH: usize = 24;
/// Enfants lus au plus par nœud : au-delà, une liste de fichiers ou une table n'apprend plus rien.
pub const MAX_CHILDREN: usize = 80;
/// Longueur maximale d'une valeur textuelle rendue.
pub const MAX_TEXT: i32 = 4000;
/// Délai par défaut au-delà duquel une application qui ne répond plus sur le bus est déclarée
/// muette ; `PROPHET_SUP_TIMEOUT_SECS` l'ajuste pour une machine lente.
pub const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn timeout() -> std::time::Duration {
    std::env::var("PROPHET_SUP_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map_or(TIMEOUT, std::time::Duration::from_secs)
}

const ROOT_PATH: &str = "/org/a11y/atspi/accessible/root";
const OBJECT_PREFIX: &str = "/org/a11y/atspi/accessible/";
const REGISTRY: &str = "org.a11y.atspi.Registry";

/// Ce qui peut empêcher de lire ou d'agir.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Le bus de session ou le bus d'accessibilité ne répond pas.
    #[error("bus d'accessibilité injoignable : {0}")]
    Bus(String),
    /// Aucune application de ce nom n'expose d'interface.
    #[error("application inconnue : {0}")]
    UnknownApp(String),
    /// Aucune fenêtre ne correspond.
    #[error("fenêtre introuvable : {0}")]
    UnknownWindow(String),
    /// L'élément visé n'existe pas ou plus.
    #[error("élément introuvable : {0}")]
    UnknownNode(String),
    /// L'application ne déclare pas cette action pour cet élément.
    #[error("action impossible : {0}")]
    Unsupported(String),
    /// Paramètre absent ou invalide.
    #[error("paramètre invalide : {0}")]
    Invalid(String),
    /// L'application n'a pas répondu dans le délai.
    #[error("l'application ne répond pas sur le bus d'accessibilité")]
    Timeout,
}

async fn borne<T>(f: impl std::future::Future<Output = Result<T, Error>>) -> Result<T, Error> {
    tokio::time::timeout(timeout(), f)
        .await
        .map_err(|_| Error::Timeout)?
}

impl From<zbus::Error> for Error {
    fn from(e: zbus::Error) -> Self {
        Self::Bus(e.to_string())
    }
}

impl From<atspi_common::AtspiError> for Error {
    fn from(e: atspi_common::AtspiError) -> Self {
        Self::Bus(e.to_string())
    }
}

/// La session de l'humain, vue par son bus d'accessibilité.
#[derive(Debug, Clone)]
pub struct Desktop {
    a11y: zbus::Connection,
}

/// Une application sur le bus.
#[derive(Debug, Clone)]
struct Application {
    /// Identifiant tranché par capd : le nom que l'application se donne, en minuscules.
    app: String,
    /// Nom unique de l'application sur le bus d'accessibilité.
    bus_name: String,
}

/// Une fenêtre de premier niveau d'une application.
#[derive(Debug, Clone)]
struct Window {
    id: String,
    path: String,
    title: String,
    active: bool,
}

impl Desktop {
    /// Joint le bus d'accessibilité de la session, dont l'adresse est demandée au bus de session.
    ///
    /// # Errors
    /// Si l'un des deux bus ne répond pas.
    pub async fn connect() -> Result<Self, Error> {
        let session = zbus::Connection::session().await?;
        let address = BusProxy::new(&session).await?.get_address().await?;
        let a11y = zbus::connection::Builder::address(address.as_str())?
            .build()
            .await?;
        Ok(Self { a11y })
    }

    /// Les applications qui exposent une interface, et leur nombre de fenêtres.
    ///
    /// # Errors
    /// Si le bus ne répond pas.
    pub async fn applications(&self) -> Result<Vec<AppView>, Error> {
        borne(self.applications_inner()).await
    }

    async fn applications_inner(&self) -> Result<Vec<AppView>, Error> {
        let mut vues = Vec::new();
        for app in self.list_applications().await? {
            let windows = self.windows(&app).await.map(|w| w.len()).unwrap_or(0);
            vues.push(AppView {
                app: app.app,
                windows,
            });
        }
        vues.sort_by(|a, b| a.app.cmp(&b.app));
        vues.dedup_by(|a, b| a.app == b.app);
        Ok(vues)
    }

    /// L'arbre SUP d'une fenêtre d'application : la fenêtre demandée, ou la fenêtre active.
    ///
    /// # Errors
    /// Application ou fenêtre inconnue, ou bus muet.
    pub async fn tree(&self, app: &str, window: Option<&str>) -> Result<Observation, Error> {
        borne(async {
            let application = self.find_application(app).await?;
            let fenetre = self.select_window(&application, window).await?;
            self.observe(&application, &fenetre).await
        })
        .await
    }

    /// Exécute une action typée et rend l'arbre qui en résulte.
    ///
    /// # Errors
    /// Élément inconnu, action que l'application ne déclare pas, paramètre manquant.
    pub async fn act(&self, request: &ActRequest) -> Result<Outcome, Error> {
        borne(self.act_inner(request)).await
    }

    async fn act_inner(&self, request: &ActRequest) -> Result<Outcome, Error> {
        let application = self.find_application(&request.app).await?;
        let fenetre = self
            .select_window(&application, request.window.as_deref())
            .await?;
        let node = self.node_proxy(&application, &request.node).await?;
        let proxies = node.proxies().await?;
        let role = node.get_role_name().await.unwrap_or_default();
        let name = node.name().await.unwrap_or_default();
        let message = match request.action.as_str() {
            "click" => {
                let action = proxies.action().await.map_err(|_| {
                    Error::Unsupported(format!(
                        "l'élément {} ({role} « {name} ») ne déclare aucune action",
                        request.node
                    ))
                })?;
                let actions = action.get_actions().await?;
                let index = actions
                    .iter()
                    .position(|a| {
                        matches!(
                            a.name.to_lowercase().as_str(),
                            "click" | "activate" | "press" | "jump" | "open" | "select"
                        )
                    })
                    .or(if actions.is_empty() { None } else { Some(0) })
                    .ok_or_else(|| {
                        Error::Unsupported(format!(
                            "l'élément {} ({role} « {name} ») n'a pas d'action de clic",
                            request.node
                        ))
                    })?;
                // Sans attendre la réponse : une entrée de menu qui ouvre un dialogue modal ne
                // répond qu'à sa fermeture, et l'agent doit voir ce dialogue, pas attendre.
                action
                    .inner()
                    .call_noreply("DoAction", &(i32::try_from(index).unwrap_or(0)))
                    .await?;
                format!(
                    "« {name} » activé ({role}, action {}).",
                    actions[index].name
                )
            }
            "set_field" => {
                let value = request
                    .value
                    .as_deref()
                    .ok_or_else(|| Error::Invalid("set_field exige value".into()))?;
                let editable = proxies.editable_text().await.map_err(|_| {
                    Error::Unsupported(format!(
                        "l'élément {} ({role} « {name} ») n'est pas un champ modifiable",
                        request.node
                    ))
                })?;
                if let Ok(component) = proxies.component().await {
                    let _ = component.grab_focus().await;
                }
                let done = editable.set_text_contents(value).await?;
                if !done {
                    return Err(Error::Unsupported(format!(
                        "l'application a refusé le nouveau contenu de {}",
                        request.node
                    )));
                }
                format!(
                    "Champ « {name} » renseigné ({} caractères).",
                    value.chars().count()
                )
            }
            "toggle" => {
                let action = proxies.action().await.map_err(|_| {
                    Error::Unsupported(format!(
                        "l'élément {} ({role} « {name} ») ne déclare aucune action",
                        request.node
                    ))
                })?;
                let actions = action.get_actions().await?;
                let index = actions
                    .iter()
                    .position(|a| {
                        matches!(
                            a.name.to_lowercase().as_str(),
                            "toggle" | "click" | "activate"
                        )
                    })
                    .ok_or_else(|| {
                        Error::Unsupported(format!(
                            "l'élément {} ({role} « {name} ») ne se bascule pas",
                            request.node
                        ))
                    })?;
                action
                    .inner()
                    .call_noreply("DoAction", &(i32::try_from(index).unwrap_or(0)))
                    .await?;
                format!("« {name} » basculé ({role}).")
            }
            other => {
                return Err(Error::Invalid(format!(
                    "action inconnue : {other} (click, set_field, toggle)"
                )));
            }
        };
        // L'application met à jour son arbre après l'action, pas pendant : on lui laisse un
        // instant avant de relire, sinon l'agent verrait l'état d'avant et douterait de l'effet.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let observation = match self.select_window(&application, None).await {
            Ok(fenetre_apres) => self.observe(&application, &fenetre_apres).await.ok(),
            Err(_) => self.observe(&application, &fenetre).await.ok(),
        };
        Ok(Outcome {
            message,
            observation,
        })
    }

    /// L'arbre d'accessibilité brut d'une fenêtre, avant traduction : rôles, états, interfaces
    /// et actions tels que l'application les déclare. Pour comprendre ce que la traduction a
    /// laissé de côté, pas pour piloter.
    ///
    /// # Errors
    /// Application ou fenêtre inconnue, ou bus muet.
    pub async fn inspect(
        &self,
        app: &str,
        window: Option<&str>,
    ) -> Result<(AccessibleNode, usize), Error> {
        borne(async {
            let application = self.find_application(app).await?;
            let fenetre = self.select_window(&application, window).await?;
            let mut budget = Budget {
                remaining: MAX_NODES,
                truncated: 0,
            };
            let source = self
                .read_node(&application, &fenetre.path, 0, &mut budget)
                .await?
                .ok_or_else(|| Error::UnknownWindow(fenetre.id.clone()))?;
            Ok((source, budget.truncated))
        })
        .await
    }

    /// Le nombre d'applications joignables, pour dire si l'adaptateur voit quelque chose.
    ///
    /// # Errors
    /// Si le bus ne répond pas.
    pub async fn count(&self) -> Result<usize, Error> {
        Ok(self.list_applications().await?.len())
    }

    async fn root(&self) -> Result<AccessibleProxy<'_>, Error> {
        Ok(AccessibleProxy::builder(&self.a11y)
            .destination(REGISTRY)?
            .path(ROOT_PATH)?
            .build()
            .await?)
    }

    async fn list_applications(&self) -> Result<Vec<Application>, Error> {
        let root = self.root().await?;
        let mut apps = Vec::new();
        for child in root.get_children().await? {
            let Some(bus_name) = child.name_as_str().map(str::to_owned) else {
                continue;
            };
            let Ok(proxy) = child.as_accessible_proxy(&self.a11y).await else {
                continue;
            };
            let name = proxy.name().await.unwrap_or_default();
            let app = app_name(&name);
            if app.is_empty() {
                continue;
            }
            apps.push(Application { app, bus_name });
        }
        Ok(apps)
    }

    async fn find_application(&self, app: &str) -> Result<Application, Error> {
        let voulu = app_name(app);
        self.list_applications()
            .await?
            .into_iter()
            .find(|a| a.app == voulu)
            .ok_or_else(|| Error::UnknownApp(app.to_owned()))
    }

    async fn windows(&self, application: &Application) -> Result<Vec<Window>, Error> {
        let root = AccessibleProxy::builder(&self.a11y)
            .destination(application.bus_name.clone())?
            .path(ROOT_PATH)?
            .build()
            .await?;
        let mut fenetres = Vec::new();
        for child in root.get_children().await? {
            let Ok(proxy) = child.as_accessible_proxy(&self.a11y).await else {
                continue;
            };
            let role = proxy.get_role_name().await.unwrap_or_default();
            if !matches!(
                role.as_str(),
                "frame"
                    | "window"
                    | "dialog"
                    | "file chooser"
                    | "alert"
                    | "color chooser"
                    | "font chooser"
            ) {
                continue;
            }
            let states = proxy.get_state().await.unwrap_or_default();
            if !states.contains(State::Showing) && !states.contains(State::Visible) {
                continue;
            }
            let path = child.path_as_str().to_owned();
            fenetres.push(Window {
                id: node_id(&path),
                path,
                title: proxy.name().await.unwrap_or_default(),
                active: states.contains(State::Active),
            });
        }
        Ok(fenetres)
    }

    async fn select_window(
        &self,
        application: &Application,
        wanted: Option<&str>,
    ) -> Result<Window, Error> {
        let fenetres = self.windows(application).await?;
        if let Some(id) = wanted {
            return fenetres
                .into_iter()
                .find(|w| w.id == id || w.title == id)
                .ok_or_else(|| Error::UnknownWindow(id.to_owned()));
        }
        // La fenêtre active d'abord : un dialogue ouvert par une action précédente est ce que
        // l'agent doit voir. Sinon la dernière fenêtre, celle que l'application a ouverte en
        // dernier.
        fenetres
            .iter()
            .rev()
            .find(|w| w.active)
            .or(fenetres.last())
            .cloned()
            .ok_or_else(|| Error::UnknownWindow("aucune fenêtre ouverte".into()))
    }

    async fn node_proxy(
        &self,
        application: &Application,
        node: &str,
    ) -> Result<AccessibleProxy<'static>, Error> {
        if node.is_empty() || !node.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(Error::UnknownNode(node.to_owned()));
        }
        let path = format!("{OBJECT_PREFIX}{node}");
        let proxy = AccessibleProxy::builder(&self.a11y)
            .destination(application.bus_name.clone())?
            .path(path)?
            .build()
            .await?;
        // Un identifiant qui ne désigne plus rien répond par une erreur au premier appel.
        proxy
            .get_role_name()
            .await
            .map_err(|_| Error::UnknownNode(node.to_owned()))?;
        Ok(proxy)
    }

    async fn observe(
        &self,
        application: &Application,
        fenetre: &Window,
    ) -> Result<Observation, Error> {
        let mut budget = Budget {
            remaining: MAX_NODES,
            truncated: 0,
        };
        let source = self
            .read_node(application, &fenetre.path, 0, &mut budget)
            .await?
            .ok_or_else(|| Error::UnknownWindow(fenetre.id.clone()))?;
        let tree = convert_tree(&application.app, &fenetre.id, &fenetre.title, &source)
            .unwrap_or_else(|| {
                Tree::new(
                    &application.app,
                    &fenetre.id,
                    &fenetre.title,
                    sup::tree::Node::new(&fenetre.id, sup::tree::Role::Group, &fenetre.title),
                )
            });
        Ok(Observation::accessibility(tree, budget.truncated))
    }

    async fn read_node(
        &self,
        application: &Application,
        path: &str,
        depth: usize,
        budget: &mut Budget,
    ) -> Result<Option<AccessibleNode>, Error> {
        if budget.remaining == 0 || depth > MAX_DEPTH {
            budget.truncated += 1;
            return Ok(None);
        }
        budget.remaining -= 1;
        let proxy = AccessibleProxy::builder(&self.a11y)
            .destination(application.bus_name.clone())?
            .path(path.to_owned())?
            .build()
            .await?;
        let role = proxy.get_role_name().await.unwrap_or_default();
        let name = proxy.name().await.unwrap_or_default();
        let states = proxy.get_state().await.unwrap_or_default();
        let interfaces = proxy.get_interfaces().await.unwrap_or_default();

        let mut actions = Vec::new();
        if interfaces.contains(Interface::Action)
            && let Ok(action) = ActionProxy::builder(&self.a11y)
                .destination(application.bus_name.clone())
                .and_then(|b| b.path(path.to_owned()))
                .map(|b| b.build())
            && let Ok(action) = action.await
            && let Ok(list) = action.get_actions().await
        {
            // GTK rend des noms d'action capitalisés (« Click ») ; le vocabulaire SUP est en
            // minuscules, et la traduction s'y fie.
            actions.extend(list.into_iter().map(|a| a.name.to_lowercase()));
        }
        if interfaces.contains(Interface::EditableText) && states.contains(State::Editable) {
            actions.push("set-text".to_owned());
        }
        let mut value = None;
        if interfaces.contains(Interface::Text)
            && let Ok(text) = TextProxy::builder(&self.a11y)
                .destination(application.bus_name.clone())
                .and_then(|b| b.path(path.to_owned()))
                .map(|b| b.build())
            && let Ok(text) = text.await
        {
            let count = text.character_count().await.unwrap_or(0).min(MAX_TEXT);
            if count > 0 {
                value = text.get_text(0, count).await.ok();
            }
        } else if interfaces.contains(Interface::Value)
            && let Ok(v) = ValueProxy::builder(&self.a11y)
                .destination(application.bus_name.clone())
                .and_then(|b| b.path(path.to_owned()))
                .map(|b| b.build())
            && let Ok(v) = v.await
            && let Ok(current) = v.current_value().await
        {
            value = Some(format!("{current}"));
        }

        let mut children = Vec::new();
        if interfaces.contains(Interface::Accessible) {
            let refs: Vec<ObjectRefOwned> = proxy.get_children().await.unwrap_or_default();
            let total = refs.len();
            for child in refs.into_iter().take(MAX_CHILDREN) {
                let child_path = child.path_as_str().to_owned();
                if !child_path.starts_with(OBJECT_PREFIX) {
                    continue;
                }
                if let Some(node) =
                    Box::pin(self.read_node(application, &child_path, depth + 1, budget)).await?
                {
                    children.push(node);
                }
            }
            if total > MAX_CHILDREN {
                budget.truncated += total - MAX_CHILDREN;
            }
        }
        Ok(Some(AccessibleNode {
            path: node_id(path),
            role,
            name,
            value,
            states: states.iter().map(|s| s.to_string()).collect(),
            actions,
            children,
        }))
    }
}

struct Budget {
    remaining: usize,
    truncated: usize,
}

/// L'identifiant SUP d'un nœud : le dernier segment de son chemin d'objet, unique dans
/// l'application et court pour le modèle.
fn node_id(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

/// L'identifiant d'une application, tel que capd le tranche et que les profils le nomment : le
/// nom qu'elle se donne sur le bus, en minuscules — et, quand ce nom est un identifiant en
/// domaine inversé (`org.xfce.mousepad`, `org.gnome.Nautilus` : au moins deux points, aucun
/// espace), son dernier segment. Les applications GTK récentes se présentent ainsi sur le bus
/// d'accessibilité ; le profil « bureau » dit `mousepad`, et l'agent aussi. Un nom à un seul
/// point (`soffice.bin`) reste entier.
fn app_name(name: &str) -> String {
    let name = name.trim().to_lowercase();
    if name.matches('.').count() >= 2 && !name.contains(char::is_whitespace) {
        name.rsplit('.')
            .find(|segment| !segment.is_empty())
            .unwrap_or(&name)
            .to_owned()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l_identifiant_est_le_dernier_segment() {
        assert_eq!(node_id("/org/a11y/atspi/accessible/23"), "23");
        assert_eq!(node_id("/org/a11y/atspi/accessible/root"), "root");
    }

    #[test]
    fn le_nom_d_une_application_est_court_et_en_minuscules() {
        assert_eq!(app_name("mousepad"), "mousepad");
        assert_eq!(app_name(" Mousepad "), "mousepad");
        assert_eq!(app_name("org.xfce.mousepad"), "mousepad");
        assert_eq!(app_name("org.gnome.Nautilus"), "nautilus");
        assert_eq!(app_name("io.github.foo.Bar"), "bar");
        assert_eq!(app_name("soffice.bin"), "soffice.bin");
        assert_eq!(app_name("Mon appli. v2. finale"), "mon appli. v2. finale");
        assert_eq!(app_name(""), "");
    }
}
