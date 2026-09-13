//! Adaptateur des applications existantes.
//!
//! Une application qui n'expose pas SUP nativement peut souvent être lue par son arbre
//! d'accessibilité, que les cadres graphiques (GTK, Qt) publient déjà. La conversion se fait ici.
//!
//! Deux choses comptent. La correspondance des rôles est une **traduction**, pas une équivalence :
//! l'accessibilité décrit ce qu'un lecteur d'écran doit annoncer, pas ce qu'un agent peut faire.
//! Ce qui ne se traduit pas est marqué comme tel plutôt que deviné. Et la **confiance** est
//! annoncée : un agent doit savoir qu'il travaille sur une lecture approchée.

use serde::{Deserialize, Serialize};

use crate::tree::{Action, ArgType, Node, Role, Tree};

/// Origine d'un arbre, et donc la confiance qu'on peut lui accorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// L'application publie SUP elle-même : l'arbre dit ce que l'application sait d'elle-même.
    Native,
    /// Arbre dérivé du document d'une page web.
    Dom,
    /// Arbre dérivé de l'accessibilité d'une application de bureau.
    Accessibility,
    /// Arbre déduit d'une image. Dernier recours.
    Vision,
}

impl Provenance {
    /// Confiance indicative, de 0 à 1.
    ///
    /// Elle n'est pas décorative : un agent qui sait qu'il lit une approximation doit vérifier
    /// davantage, et un système qui ne le lui dit pas le laisse agir à l'aveugle en croyant voir.
    #[must_use]
    pub const fn confidence(self) -> f32 {
        match self {
            Self::Native => 1.0,
            Self::Dom => 0.9,
            Self::Accessibility => 0.75,
            Self::Vision => 0.4,
        }
    }

    /// Avertissement destiné au modèle, à joindre à l'arbre.
    #[must_use]
    pub const fn caveat(self) -> &'static str {
        match self {
            Self::Native => "L'application publie cet état elle-même.",
            Self::Dom => {
                "État dérivé du document de la page ; les éléments purement visuels peuvent manquer."
            }
            Self::Accessibility => {
                "État dérivé de l'accessibilité de l'application : incomplet par nature. \
                 Vérifiez le résultat de chaque action plutôt que de le supposer."
            }
            Self::Vision => {
                "État déduit d'une image : lecture approximative. \
                 N'effectuez aucune action irréversible sur cette seule base."
            }
        }
    }
}

/// Un nœud d'accessibilité, tel que GTK ou Qt le publient.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessibleNode {
    /// Chemin de l'objet, unique dans l'application.
    pub path: String,
    /// Rôle d'accessibilité, dans le vocabulaire AT-SPI.
    pub role: String,
    /// Nom accessible.
    #[serde(default)]
    pub name: String,
    /// Valeur textuelle.
    #[serde(default)]
    pub value: Option<String>,
    /// États d'accessibilité (`enabled`, `focused`, `checked`…).
    #[serde(default)]
    pub states: Vec<String>,
    /// Actions offertes par l'objet.
    #[serde(default)]
    pub actions: Vec<String>,
    /// Enfants.
    #[serde(default)]
    pub children: Vec<AccessibleNode>,
}

/// Traduit un rôle d'accessibilité en rôle sémantique.
///
/// Retourne `None` quand le rôle n'a pas d'équivalent utile : le nœud est alors écarté plutôt que
/// rangé approximativement, ce qui éviterait à un agent de le voir mais lui éviterait surtout de
/// se tromper à son sujet.
#[must_use]
pub fn map_role(atspi: &str) -> Option<Role> {
    Some(match atspi {
        "push button" | "button" | "toggle button" => Role::Button,
        "check box" | "radio button" | "switch" | "check menu item" | "radio menu item" => {
            Role::Toggle
        }
        "entry" | "text" | "password text" | "spin button" | "slider" => Role::Field,
        "combo box" | "list box" => Role::Select,
        "link" => Role::Link,
        "label" | "static" | "heading" | "paragraph" | "caption" | "tooltip" => Role::Text,
        "list" | "menu" | "menu bar" | "tree" | "page tab list" => Role::List,
        "list item" | "menu item" | "tree item" | "page tab" => Role::Item,
        "table" | "tree table" => Role::Table,
        "table row" => Role::Row,
        "table cell" | "table column header" | "table row header" => Role::Cell,
        "image" | "icon" => Role::Image,
        "document text" | "document frame" | "document web" => Role::RichText,
        "status bar" | "notification" | "alert" | "progress bar" => Role::Status,
        "panel" | "filler" | "frame" | "window" | "application" | "scroll pane" | "dialog"
        | "file chooser" | "color chooser" | "font chooser" | "tool bar" | "viewport"
        | "split pane" | "layered pane" | "root pane" | "glass pane" | "section" | "form"
        | "header" | "footer" | "landmark" | "html container" | "desktop frame" | "embedded"
        | "canvas" | "grouping" => Role::Group,
        _ => return None,
    })
}

/// Convertit un nœud d'accessibilité en nœud sémantique.
#[must_use]
pub fn convert_node(source: &AccessibleNode) -> Option<Node> {
    let role = map_role(&source.role)?;
    let enfants: Vec<Node> = source.children.iter().filter_map(convert_node).collect();

    // Un groupe sans nom ni enfant n'apporte rien à un agent.
    if role == Role::Group && source.name.is_empty() && enfants.is_empty() {
        return None;
    }

    let mut node = Node::new(&source.path, role, &source.name);
    node.value.clone_from(&source.value);
    node.actionable = !source.actions.is_empty();
    node.disabled = !source.states.iter().any(|s| s == "enabled");
    node.children = enfants;
    Some(node)
}

/// Construit un arbre SUP à partir d'un arbre d'accessibilité.
///
/// Les actions proposées sont celles que l'accessibilité déclare réellement, jamais un jeu
/// standard supposé : proposer une action qui n'existe pas ferait échouer l'agent sans raison
/// compréhensible.
#[must_use]
pub fn convert_tree(app: &str, window: &str, title: &str, source: &AccessibleNode) -> Option<Tree> {
    let root = convert_node(source)?;
    let mut noms: Vec<String> = Vec::new();
    collect_actions(source, &mut noms);

    let mut actions = Vec::new();
    if noms
        .iter()
        .any(|a| a == "click" || a == "press" || a == "activate")
    {
        actions.push(
            Action::new("click", "Active un élément désigné par son identifiant.").arg(
                "node",
                ArgType::String,
                true,
            ),
        );
    }
    if noms.iter().any(|a| a == "set-text" || a == "insert-text") {
        actions.push(
            Action::new("set_field", "Renseigne un champ.")
                .arg("node", ArgType::String, true)
                .arg("value", ArgType::String, true),
        );
    }
    if noms.iter().any(|a| a == "toggle" || a == "check") {
        actions.push(
            Action::new("toggle", "Change l'état d'une case ou d'un interrupteur.").arg(
                "node",
                ArgType::String,
                true,
            ),
        );
    }

    let mut tree = Tree::new(app, window, title, root).actions(actions);
    tree.focus = find_focused(source);
    Some(tree)
}

fn collect_actions(source: &AccessibleNode, out: &mut Vec<String>) {
    for action in &source.actions {
        if !out.contains(action) {
            out.push(action.clone());
        }
    }
    for child in &source.children {
        collect_actions(child, out);
    }
}

fn find_focused(source: &AccessibleNode) -> Option<String> {
    if source.states.iter().any(|s| s == "focused") {
        return Some(source.path.clone());
    }
    source.children.iter().find_map(find_focused)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noeud(path: &str, role: &str, name: &str) -> AccessibleNode {
        AccessibleNode {
            path: path.to_owned(),
            role: role.to_owned(),
            name: name.to_owned(),
            value: None,
            states: vec!["enabled".to_owned()],
            actions: Vec::new(),
            children: Vec::new(),
        }
    }

    fn fenetre() -> AccessibleNode {
        AccessibleNode {
            children: vec![
                AccessibleNode {
                    value: Some("brouillon".to_owned()),
                    actions: vec!["set-text".to_owned()],
                    states: vec!["enabled".to_owned(), "focused".to_owned()],
                    ..noeud("/obj/1", "entry", "Message")
                },
                AccessibleNode {
                    actions: vec!["click".to_owned()],
                    ..noeud("/obj/2", "push button", "Envoyer")
                },
                noeud("/obj/3", "label", "Brouillon enregistré"),
                // Un rôle sans équivalent utile : il doit disparaître, pas être rangé au hasard.
                noeud("/obj/4", "redundant object", ""),
            ],
            ..noeud("/obj/0", "frame", "Courrier")
        }
    }

    #[test]
    fn la_confiance_est_annoncee_et_decroissante() {
        assert!(Provenance::Native.confidence() > Provenance::Dom.confidence());
        assert!(Provenance::Dom.confidence() > Provenance::Accessibility.confidence());
        assert!(Provenance::Accessibility.confidence() > Provenance::Vision.confidence());
        for provenance in [
            Provenance::Native,
            Provenance::Dom,
            Provenance::Accessibility,
            Provenance::Vision,
        ] {
            assert!(!provenance.caveat().is_empty());
        }
    }

    #[test]
    fn l_avertissement_de_la_vision_interdit_l_irreversible() {
        assert!(
            Provenance::Vision.caveat().contains("irréversible"),
            "{}",
            Provenance::Vision.caveat()
        );
    }

    #[test]
    fn les_roles_connus_sont_traduits() {
        assert_eq!(map_role("push button"), Some(Role::Button));
        assert_eq!(map_role("entry"), Some(Role::Field));
        assert_eq!(map_role("check box"), Some(Role::Toggle));
    }

    #[test]
    fn un_role_sans_equivalent_est_ecarte_pas_devine() {
        assert_eq!(map_role("redundant object"), None);
        assert_eq!(map_role("un rôle inventé"), None);
    }

    #[test]
    fn conversion_d_une_fenetre() {
        let tree = convert_tree("gtk.courrier", "w1", "Courrier", &fenetre()).unwrap();
        assert_eq!(tree.root.count(), 4, "le nœud sans équivalent a disparu");
        let champ = tree.root.find("/obj/1").unwrap();
        assert_eq!(champ.role, Role::Field);
        assert_eq!(champ.value.as_deref(), Some("brouillon"));
        assert!(champ.actionable);
        assert_eq!(tree.focus.as_deref(), Some("/obj/1"));
    }

    #[test]
    fn seules_les_actions_reellement_offertes_sont_proposees() {
        let tree = convert_tree("gtk.courrier", "w1", "Courrier", &fenetre()).unwrap();
        let noms: Vec<&str> = tree.actions.iter().map(|a| a.name.as_str()).collect();
        assert!(noms.contains(&"click"));
        assert!(noms.contains(&"set_field"));
        assert!(
            !noms.contains(&"toggle"),
            "aucune case à cocher dans cette fenêtre : ne pas proposer l'action"
        );
    }

    #[test]
    fn un_element_desactive_est_signale() {
        let mut source = fenetre();
        source.children[1].states.clear();
        let tree = convert_tree("app", "w1", "t", &source).unwrap();
        assert!(tree.root.find("/obj/2").unwrap().disabled);
    }

    #[test]
    fn un_groupe_vide_et_sans_nom_disparait() {
        let vide = noeud("/obj/9", "panel", "");
        assert!(convert_node(&vide).is_none());
    }

    #[test]
    fn une_racine_sans_equivalent_ne_produit_pas_d_arbre() {
        let source = noeud("/obj/0", "rôle inconnu", "x");
        assert!(convert_tree("app", "w1", "t", &source).is_none());
    }
}
