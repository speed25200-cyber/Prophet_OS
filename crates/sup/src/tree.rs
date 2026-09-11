//! L'arbre sémantique et les actions typées.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Version du protocole.
pub const SUP_VERSION: u32 = 0;

/// Rôle sémantique d'un nœud. Fermé et volontairement court : un agent doit pouvoir raisonner
/// dessus sans apprendre un vocabulaire d'application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Regroupement sans sémantique propre.
    Group,
    /// Texte affiché.
    Text,
    /// Champ de saisie.
    Field,
    /// Bouton ou commande.
    Button,
    /// Lien.
    Link,
    /// Case à cocher ou interrupteur.
    Toggle,
    /// Choix parmi une liste.
    Select,
    /// Liste d'éléments.
    List,
    /// Élément de liste.
    Item,
    /// Tableau.
    Table,
    /// Ligne de tableau.
    Row,
    /// Cellule.
    Cell,
    /// Image, avec son texte de remplacement.
    Image,
    /// Zone d'édition riche.
    RichText,
    /// Indicateur d'état ou message.
    Status,
}

/// Niveau de détail demandé.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Detail {
    /// Uniquement la structure et les éléments actionnables.
    Summary,
    /// Structure, textes courts et valeurs.
    #[default]
    Normal,
    /// Tout, y compris les textes longs.
    Full,
}

/// Type d'un argument d'action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ArgType {
    /// Chaîne libre.
    String,
    /// Entier.
    Integer,
    /// Booléen.
    Boolean,
    /// Chemin de fichier.
    File,
    /// Choix parmi une liste fermée.
    Enum {
        /// Valeurs admises.
        values: Vec<String>,
    },
}

impl ArgType {
    /// Vrai si la valeur est conforme au type.
    ///
    /// C'est ce contrôle qui remplace « le clic est-il tombé au bon endroit ? » : une action
    /// invalide est refusée avant d'être tentée, avec une raison.
    #[must_use]
    pub fn accepts(&self, value: &Value) -> bool {
        match self {
            Self::String => value.is_string(),
            Self::Integer => value.is_i64() || value.is_u64(),
            Self::Boolean => value.is_boolean(),
            Self::File => value.as_str().is_some_and(|s| !s.is_empty()),
            Self::Enum { values } => value
                .as_str()
                .is_some_and(|s| values.iter().any(|v| v == s)),
        }
    }
}

/// Argument d'une action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionArg {
    /// Nom.
    pub name: String,
    /// Type attendu.
    #[serde(flatten)]
    pub kind: ArgType,
    /// L'argument est-il obligatoire ?
    #[serde(default)]
    pub required: bool,
}

/// Action offerte par une fenêtre.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    /// Nom, unique dans la fenêtre.
    pub name: String,
    /// Description courte, écrite pour un modèle.
    pub description: String,
    /// Arguments.
    #[serde(default)]
    pub args: Vec<ActionArg>,
    /// L'action est-elle irréversible ?
    #[serde(default)]
    pub irreversible: bool,
    /// A-t-elle un effet hors de la machine ?
    #[serde(default)]
    pub external: bool,
    /// Nœud auquel elle s'applique, s'il y en a un.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// Capacité exigée, au-delà de `ui.act`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<String>,
}

impl Action {
    /// Action simple, sans argument ni effet.
    #[must_use]
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            args: Vec::new(),
            irreversible: false,
            external: false,
            node: None,
            requires: None,
        }
    }

    /// Ajoute un argument.
    #[must_use]
    pub fn arg(mut self, name: impl Into<String>, kind: ArgType, required: bool) -> Self {
        self.args.push(ActionArg {
            name: name.into(),
            kind,
            required,
        });
        self
    }

    /// Marque l'action comme irréversible.
    #[must_use]
    pub const fn irreversible(mut self) -> Self {
        self.irreversible = true;
        self
    }

    /// Marque l'action comme ayant un effet externe.
    #[must_use]
    pub const fn external(mut self) -> Self {
        self.external = true;
        self
    }

    /// Rattache l'action à un nœud.
    #[must_use]
    pub fn on(mut self, node: impl Into<String>) -> Self {
        self.node = Some(node.into());
        self
    }

    /// Vérifie les arguments fournis.
    ///
    /// # Errors
    /// Nomme le premier argument manquant, inconnu ou mal typé.
    pub fn validate(&self, provided: &Value) -> Result<(), String> {
        let object = provided
            .as_object()
            .ok_or_else(|| "les arguments doivent former un objet".to_owned())?;
        for arg in &self.args {
            match object.get(&arg.name) {
                None if arg.required => {
                    return Err(format!("argument obligatoire manquant : {}", arg.name));
                }
                None => {}
                Some(value) if !arg.kind.accepts(value) => {
                    return Err(format!(
                        "argument {} de type inattendu : {:?} attendu",
                        arg.name, arg.kind
                    ));
                }
                Some(_) => {}
            }
        }
        for key in object.keys() {
            if !self.args.iter().any(|a| &a.name == key) {
                return Err(format!("argument inconnu : {key}"));
            }
        }
        Ok(())
    }
}

/// Nœud de l'arbre sémantique.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    /// Identifiant stable dans la fenêtre. C'est lui que l'agent cite, jamais une coordonnée.
    pub id: String,
    /// Rôle.
    pub role: Role,
    /// Nom accessible.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Valeur courante.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Le nœud est-il actionnable ?
    #[serde(default)]
    pub actionable: bool,
    /// Le nœud est-il désactivé ?
    #[serde(default)]
    pub disabled: bool,
    /// Enfants.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

impl Node {
    /// Nœud minimal.
    #[must_use]
    pub fn new(id: impl Into<String>, role: Role, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role,
            name: name.into(),
            value: None,
            actionable: false,
            disabled: false,
            children: Vec::new(),
        }
    }

    /// Fixe la valeur.
    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Marque le nœud comme actionnable.
    #[must_use]
    pub const fn actionable(mut self) -> Self {
        self.actionable = true;
        self
    }

    /// Ajoute des enfants.
    #[must_use]
    pub fn children(mut self, children: impl IntoIterator<Item = Self>) -> Self {
        self.children = children.into_iter().collect();
        self
    }

    /// Parcourt le nœud et ses descendants.
    pub fn walk(&self, visit: &mut impl FnMut(&Self)) {
        visit(self);
        for child in &self.children {
            child.walk(visit);
        }
    }

    /// Cherche un descendant par identifiant.
    #[must_use]
    pub fn find(&self, id: &str) -> Option<&Self> {
        if self.id == id {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find(id))
    }

    /// Nombre de nœuds de la sous-arborescence.
    #[must_use]
    pub fn count(&self) -> usize {
        1 + self.children.iter().map(Self::count).sum::<usize>()
    }

    /// Réduit l'arbre au niveau de détail demandé.
    ///
    /// Le résumé conserve ce sur quoi on peut agir et la structure, et jette le reste : c'est ce
    /// qui permet à un agent de s'orienter dans une page complexe pour quelques centaines de
    /// tokens.
    #[must_use]
    pub fn at_detail(&self, detail: Detail) -> Self {
        let mut node = self.clone();
        match detail {
            Detail::Full => {}
            Detail::Normal => {
                if let Some(value) = &node.value
                    && value.chars().count() > 200
                {
                    node.value = Some(format!(
                        "{}… ({} caractères)",
                        value.chars().take(200).collect::<String>(),
                        value.chars().count()
                    ));
                }
            }
            Detail::Summary => {
                if node.role == Role::Text && !node.actionable {
                    node.name = truncate(&node.name, 60);
                }
                node.value = node.value.as_ref().map(|v| truncate(v, 60));
            }
        }
        node.children = self
            .children
            .iter()
            .filter(|child| detail != Detail::Summary || child.is_interesting())
            .map(|child| child.at_detail(detail))
            .collect();
        node
    }

    /// Vrai si le nœud mérite de figurer dans un résumé.
    fn is_interesting(&self) -> bool {
        self.actionable
            || matches!(
                self.role,
                Role::Field
                    | Role::Button
                    | Role::Link
                    | Role::Toggle
                    | Role::Select
                    | Role::Status
            )
            || self.children.iter().any(Self::is_interesting)
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    format!(
        "{}…",
        text.chars().take(max.saturating_sub(1)).collect::<String>()
    )
}

/// Arbre d'une fenêtre.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tree {
    /// Version du protocole.
    pub v: u32,
    /// Application.
    pub app: String,
    /// Fenêtre.
    pub window: String,
    /// Titre.
    pub title: String,
    /// Numéro de version de l'état, incrémenté à chaque changement.
    pub version: u64,
    /// Racine de l'arbre.
    pub root: Node,
    /// Actions offertes.
    pub actions: Vec<Action>,
    /// Nœud ayant le focus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
}

impl Tree {
    /// Arbre minimal.
    #[must_use]
    pub fn new(
        app: impl Into<String>,
        window: impl Into<String>,
        title: impl Into<String>,
        root: Node,
    ) -> Self {
        Self {
            v: SUP_VERSION,
            app: app.into(),
            window: window.into(),
            title: title.into(),
            version: 1,
            root,
            actions: Vec::new(),
            focus: None,
        }
    }

    /// Ajoute des actions.
    #[must_use]
    pub fn actions(mut self, actions: impl IntoIterator<Item = Action>) -> Self {
        self.actions = actions.into_iter().collect();
        self
    }

    /// Cherche une action par nom.
    #[must_use]
    pub fn action(&self, name: &str) -> Option<&Action> {
        self.actions.iter().find(|a| a.name == name)
    }

    /// Empreinte de l'état, pour détecter un changement sans comparer l'arbre entier.
    #[must_use]
    pub fn digest(&self) -> String {
        let canonical = serde_json::to_vec(&self.root).unwrap_or_default();
        format!("blake3:{}", blake3::hash(&canonical).to_hex())
    }

    /// Taille approximative en octets d'une observation à ce niveau de détail.
    ///
    /// Sert à comparer honnêtement le coût d'une observation sémantique à celui d'une capture.
    #[must_use]
    pub fn observation_size(&self, detail: Detail) -> usize {
        let reduced = Self {
            root: self.root.at_detail(detail),
            ..self.clone()
        };
        serde_json::to_vec(&reduced).map(|v| v.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn arbre() -> Tree {
        Tree::new(
            "prophet.mail",
            "compose-42",
            "Nouveau message",
            Node::new("root", Role::Group, "Composer").children([
                Node::new("to", Role::Field, "Destinataire")
                    .value("marie@exemple.fr")
                    .actionable(),
                Node::new("subject", Role::Field, "Objet")
                    .value("Rapport Q3")
                    .actionable(),
                Node::new("body", Role::RichText, "Message").value("x".repeat(500)),
                Node::new("aide", Role::Text, "Appuyez sur Entrée pour envoyer"),
            ]),
        )
        .actions([
            Action::new("set_field", "Remplit un champ du message")
                .arg(
                    "field",
                    ArgType::Enum {
                        values: vec!["to".into(), "subject".into(), "body".into()],
                    },
                    true,
                )
                .arg("value", ArgType::String, true),
            Action::new("send", "Envoie le message")
                .irreversible()
                .external(),
            Action::new("save_draft", "Enregistre un brouillon"),
        ])
    }

    #[test]
    fn recherche_par_identifiant_jamais_par_coordonnee() {
        let t = arbre();
        assert_eq!(
            t.root.find("subject").unwrap().value.as_deref(),
            Some("Rapport Q3")
        );
        assert!(t.root.find("inexistant").is_none());
    }

    #[test]
    fn les_actions_dangereuses_sont_annotees() {
        let t = arbre();
        let envoi = t.action("send").unwrap();
        assert!(envoi.irreversible && envoi.external);
        assert!(!t.action("save_draft").unwrap().irreversible);
    }

    #[test]
    fn une_action_mal_formee_est_refusee_avant_d_etre_tentee() {
        let t = arbre();
        let action = t.action("set_field").unwrap();

        assert!(
            action
                .validate(&json!({"field": "to", "value": "x"}))
                .is_ok()
        );

        let err = action.validate(&json!({"field": "to"})).unwrap_err();
        assert!(err.contains("value"), "{err}");

        let err = action
            .validate(&json!({"field": "inexistant", "value": "x"}))
            .unwrap_err();
        assert!(err.contains("field"), "{err}");

        let err = action
            .validate(&json!({"field": "to", "value": 42}))
            .unwrap_err();
        assert!(err.contains("value"), "{err}");

        let err = action
            .validate(&json!({"field": "to", "value": "x", "surplus": 1}))
            .unwrap_err();
        assert!(err.contains("surplus"), "{err}");
    }

    #[test]
    fn le_resume_garde_l_actionnable_et_jette_le_reste() {
        let t = arbre();
        let complet = t.root.count();
        let resume = t.root.at_detail(Detail::Summary);
        assert!(resume.count() < complet);
        assert!(resume.find("to").is_some(), "un champ reste dans le résumé");
        assert!(
            resume.find("aide").is_none(),
            "un texte non actionnable disparaît du résumé"
        );
    }

    #[test]
    fn le_niveau_normal_tronque_les_valeurs_longues() {
        let t = arbre();
        let normal = t.root.at_detail(Detail::Normal);
        let corps = normal.find("body").unwrap().value.as_ref().unwrap();
        assert!(corps.contains("500 caractères"), "{corps}");
        assert!(corps.chars().count() < 300);

        let complet = t.root.at_detail(Detail::Full);
        assert_eq!(
            complet.find("body").unwrap().value.as_ref().unwrap().len(),
            500
        );
    }

    #[test]
    fn une_observation_semantique_est_bien_plus_legere_qu_une_capture() {
        let t = arbre();
        let complet = t.observation_size(Detail::Full);
        let resume = t.observation_size(Detail::Summary);
        assert!(resume < complet);
        // Une capture d'écran de bureau en PNG pèse typiquement plus d'un mégaoctet.
        const CAPTURE_TYPIQUE: usize = 1_000_000;
        assert!(
            complet < CAPTURE_TYPIQUE / 100,
            "l'arbre complet fait {complet} octets"
        );
    }

    #[test]
    fn l_empreinte_change_avec_l_etat_et_pas_autrement() {
        let t = arbre();
        let avant = t.digest();
        let mut identique = arbre();
        identique.version = 99;
        assert_eq!(
            avant,
            identique.digest(),
            "la version seule ne change pas l'état"
        );

        let mut modifie = arbre();
        modifie.root.children[0].value = Some("autre@exemple.fr".into());
        assert_ne!(avant, modifie.digest());
    }

    #[test]
    fn parcours_complet() {
        let t = arbre();
        let mut roles = Vec::new();
        t.root.walk(&mut |node| roles.push(node.role));
        assert_eq!(roles.len(), t.root.count());
        assert!(roles.contains(&Role::RichText));
    }

    #[test]
    fn aller_retour_json() {
        let t = arbre();
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<Tree>(&json).unwrap(), t);
    }
}
