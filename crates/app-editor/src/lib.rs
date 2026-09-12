//! Éditeur de référence.
//!
//! Cette application existe pour montrer ce que « publier SUP nativement » veut dire, et pour
//! servir de banc d'essai aux approbations. Elle n'a pas d'interface graphique : son cœur est
//! l'état du document et les actions qui le modifient, et c'est précisément ce qu'un agent
//! manipule. Un habillage graphique se branche dessus sans rien changer à ce qui suit.
//!
//! Ce qu'elle démontre concrètement :
//! - un arbre sémantique publié par l'application elle-même, donc exact par construction ;
//! - des actions typées dont les arguments sont validés avant exécution ;
//! - une action **irréversible et externe** (`send`) qui déclenche le circuit d'approbation, à
//!   côté d'actions ordinaires qui ne le déclenchent pas.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::path::{Path, PathBuf};

use serde_json::Value;
use sup::tree::{Action, ArgType, Detail, Node, Role, Tree};

/// Erreur de l'éditeur.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EditorError {
    /// Action inconnue.
    #[error("action inconnue : {0}")]
    UnknownAction(String),
    /// Arguments invalides.
    #[error("arguments invalides : {0}")]
    BadArguments(String),
    /// Aucun fichier associé.
    #[error("aucun fichier associé : enregistrez d'abord sous un nom")]
    NoPath,
    /// Erreur d'entrée-sortie.
    #[error("erreur d'entrée-sortie : {0}")]
    Io(String),
}

/// Un document ouvert.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    /// Contenu.
    pub text: String,
    /// Chemin, s'il y en a un.
    pub path: Option<PathBuf>,
    /// Modifications non enregistrées.
    pub dirty: bool,
    /// Destinataire préparé, pour l'action d'envoi.
    pub recipient: String,
    /// Version de l'état, incrémentée à chaque changement.
    pub version: u64,
}

/// L'éditeur.
#[derive(Debug, Default)]
pub struct Editor {
    document: Document,
    /// Ce que l'application a réellement envoyé, pour que les tests le constatent.
    sent: Vec<(String, String)>,
}

/// Identifiant de la fenêtre.
pub const WINDOW: &str = "editeur-1";
/// Identifiant de l'application.
pub const APP: &str = "prophet.editeur";

impl Editor {
    /// Éditeur vide.
    #[must_use]
    pub fn new() -> Self {
        Self {
            document: Document {
                version: 1,
                ..Document::default()
            },
            sent: Vec::new(),
        }
    }

    /// Document courant.
    #[must_use]
    pub const fn document(&self) -> &Document {
        &self.document
    }

    /// Envois réellement effectués.
    #[must_use]
    pub fn sent(&self) -> &[(String, String)] {
        &self.sent
    }

    /// Publie l'état sous forme d'arbre sémantique.
    ///
    /// L'arbre est produit par l'application à partir de son propre état : il ne peut donc pas se
    /// désynchroniser de ce qu'elle fait, contrairement à une lecture externe.
    #[must_use]
    pub fn tree(&self) -> Tree {
        let titre = self
            .document
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(
                || "Document sans nom".to_owned(),
                |n| n.to_string_lossy().to_string(),
            );
        let titre = if self.document.dirty {
            format!("{titre} — modifié")
        } else {
            titre
        };

        let mut tree = Tree::new(
            APP,
            WINDOW,
            titre,
            Node::new("racine", Role::Group, "Éditeur").children([
                Node::new("corps", Role::RichText, "Contenu")
                    .value(&self.document.text)
                    .actionable(),
                Node::new("destinataire", Role::Field, "Destinataire")
                    .value(&self.document.recipient)
                    .actionable(),
                Node::new(
                    "etat",
                    Role::Status,
                    if self.document.dirty {
                        "Modifications non enregistrées"
                    } else {
                        "À jour"
                    },
                ),
            ]),
        )
        .actions(vec![
            Action::new("set_text", "Remplace le contenu du document.")
                .arg("value", ArgType::String, true)
                .on("corps"),
            Action::new("append", "Ajoute du texte à la fin du document.")
                .arg("value", ArgType::String, true)
                .on("corps"),
            Action::new("set_recipient", "Renseigne le destinataire.")
                .arg("value", ArgType::String, true)
                .on("destinataire"),
            // Enregistrer est réversible : le système de fichiers sémantique garde l'état d'avant.
            Action::new("save", "Enregistre le document."),
            Action::new("save_as", "Enregistre le document sous un nom.").arg(
                "path",
                ArgType::File,
                true,
            ),
            // Envoyer sort de la machine et ne se défait pas : c'est ce qui déclenche
            // l'approbation humaine.
            Action::new("send", "Envoie le document au destinataire.")
                .irreversible()
                .external(),
        ]);
        tree.version = self.document.version;
        tree.focus = Some("corps".to_owned());
        tree
    }

    /// Arbre au niveau de détail demandé.
    #[must_use]
    pub fn tree_at(&self, detail: Detail) -> Tree {
        let tree = self.tree();
        Tree {
            root: tree.root.at_detail(detail),
            ..tree
        }
    }

    /// Exécute une action.
    ///
    /// Les arguments sont validés contre la description publiée : une action mal formée est
    /// refusée **avant** d'avoir le moindre effet.
    ///
    /// # Errors
    /// Action inconnue, arguments invalides, ou erreur d'entrée-sortie.
    pub fn act(&mut self, action: &str, args: &Value) -> Result<Value, EditorError> {
        let tree = self.tree();
        let description = tree
            .action(action)
            .ok_or_else(|| EditorError::UnknownAction(action.to_owned()))?;
        description
            .validate(args)
            .map_err(EditorError::BadArguments)?;

        let valeur = |clef: &str| args.get(clef).and_then(Value::as_str).unwrap_or_default();
        let resultat = match action {
            "set_text" => {
                self.document.text = valeur("value").to_owned();
                self.document.dirty = true;
                serde_json::json!({"length": self.document.text.len()})
            }
            "append" => {
                self.document.text.push_str(valeur("value"));
                self.document.dirty = true;
                serde_json::json!({"length": self.document.text.len()})
            }
            "set_recipient" => {
                self.document.recipient = valeur("value").to_owned();
                serde_json::json!({"recipient": self.document.recipient})
            }
            "save" => {
                let chemin = self.document.path.clone().ok_or(EditorError::NoPath)?;
                std::fs::write(&chemin, &self.document.text)
                    .map_err(|e| EditorError::Io(e.to_string()))?;
                self.document.dirty = false;
                serde_json::json!({"path": chemin.display().to_string()})
            }
            "save_as" => {
                let chemin = PathBuf::from(valeur("path"));
                if let Some(parent) = chemin.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| EditorError::Io(e.to_string()))?;
                }
                std::fs::write(&chemin, &self.document.text)
                    .map_err(|e| EditorError::Io(e.to_string()))?;
                self.document.path = Some(chemin.clone());
                self.document.dirty = false;
                serde_json::json!({"path": chemin.display().to_string()})
            }
            "send" => {
                if self.document.recipient.is_empty() {
                    return Err(EditorError::BadArguments(
                        "aucun destinataire renseigné".to_owned(),
                    ));
                }
                self.sent
                    .push((self.document.recipient.clone(), self.document.text.clone()));
                serde_json::json!({"sent_to": self.document.recipient})
            }
            autre => return Err(EditorError::UnknownAction(autre.to_owned())),
        };
        self.document.version += 1;
        Ok(resultat)
    }

    /// Ouvre un fichier.
    ///
    /// # Errors
    /// Si le fichier est illisible.
    pub fn open(&mut self, path: &Path) -> Result<(), EditorError> {
        self.document.text =
            std::fs::read_to_string(path).map_err(|e| EditorError::Io(e.to_string()))?;
        self.document.path = Some(path.to_path_buf());
        self.document.dirty = false;
        self.document.version += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn l_arbre_decrit_l_etat_reel() {
        let mut editeur = Editor::new();
        editeur
            .act("set_text", &json!({"value": "bonjour"}))
            .unwrap();
        let tree = editeur.tree();
        assert_eq!(
            tree.root.find("corps").unwrap().value.as_deref(),
            Some("bonjour")
        );
        assert!(tree.title.contains("modifié"));
        assert_eq!(
            tree.root.find("etat").unwrap().name,
            "Modifications non enregistrées"
        );
    }

    #[test]
    fn seul_l_envoi_est_irreversible_et_externe() {
        let tree = Editor::new().tree();
        let envoi = tree.action("send").unwrap();
        assert!(envoi.irreversible && envoi.external);
        for ordinaire in ["set_text", "append", "save", "save_as", "set_recipient"] {
            let action = tree.action(ordinaire).unwrap();
            assert!(
                !action.irreversible && !action.external,
                "{ordinaire} ne devrait pas déclencher d'approbation"
            );
        }
    }

    #[test]
    fn une_action_mal_formee_n_a_aucun_effet() {
        let mut editeur = Editor::new();
        assert!(editeur.act("set_text", &json!({})).is_err());
        assert!(editeur.act("set_text", &json!({"value": 42})).is_err());
        assert!(editeur.act("inexistante", &json!({})).is_err());
        assert_eq!(
            editeur.document().text,
            "",
            "aucun effet ne doit avoir eu lieu"
        );
        assert_eq!(
            editeur.document().version,
            1,
            "la version ne bouge pas non plus"
        );
    }

    #[test]
    fn la_version_change_a_chaque_action_reussie() {
        let mut editeur = Editor::new();
        let avant = editeur.document().version;
        editeur.act("append", &json!({"value": "a"})).unwrap();
        editeur.act("append", &json!({"value": "b"})).unwrap();
        assert_eq!(editeur.document().version, avant + 2);
        assert_eq!(editeur.document().text, "ab");
    }

    #[test]
    fn enregistrer_sans_nom_est_refuse_puis_possible() {
        let dir = tempfile::tempdir().unwrap();
        let mut editeur = Editor::new();
        editeur
            .act("set_text", &json!({"value": "contenu"}))
            .unwrap();
        assert_eq!(editeur.act("save", &json!({})), Err(EditorError::NoPath));

        let chemin = dir.path().join("sous/dossier/note.txt");
        editeur
            .act("save_as", &json!({"path": chemin.display().to_string()}))
            .unwrap();
        assert_eq!(std::fs::read_to_string(&chemin).unwrap(), "contenu");
        assert!(!editeur.document().dirty);

        editeur.act("append", &json!({"value": " ajouté"})).unwrap();
        editeur.act("save", &json!({})).unwrap();
        assert_eq!(std::fs::read_to_string(&chemin).unwrap(), "contenu ajouté");
    }

    #[test]
    fn envoyer_sans_destinataire_est_refuse() {
        let mut editeur = Editor::new();
        editeur.act("set_text", &json!({"value": "x"})).unwrap();
        assert!(editeur.act("send", &json!({})).is_err());
        assert!(editeur.sent().is_empty());
    }

    #[test]
    fn envoyer_avec_destinataire_aboutit() {
        let mut editeur = Editor::new();
        editeur
            .act("set_text", &json!({"value": "le rapport"}))
            .unwrap();
        editeur
            .act("set_recipient", &json!({"value": "marie@exemple.fr"}))
            .unwrap();
        editeur.act("send", &json!({})).unwrap();
        assert_eq!(
            editeur.sent(),
            &[("marie@exemple.fr".to_owned(), "le rapport".to_owned())]
        );
    }

    #[test]
    fn ouvrir_un_fichier() {
        let dir = tempfile::tempdir().unwrap();
        let chemin = dir.path().join("note.txt");
        std::fs::write(&chemin, "déjà écrit").unwrap();
        let mut editeur = Editor::new();
        editeur.open(&chemin).unwrap();
        assert_eq!(editeur.document().text, "déjà écrit");
        assert!(!editeur.document().dirty);
        assert!(editeur.tree().title.starts_with("note.txt"));
    }

    #[test]
    fn le_resume_garde_l_actionnable() {
        let mut editeur = Editor::new();
        editeur
            .act("set_text", &json!({"value": "x".repeat(5000)}))
            .unwrap();
        let complet = editeur.tree_at(Detail::Full);
        let resume = editeur.tree_at(Detail::Summary);
        assert!(
            serde_json::to_vec(&resume).unwrap().len()
                < serde_json::to_vec(&complet).unwrap().len()
        );
        assert!(resume.root.find("destinataire").is_some());
    }

    #[test]
    fn l_arbre_est_utilisable_par_le_registre_sup() {
        let mut registre = sup::Registry::new();
        let editeur = Editor::new();
        registre.publish(editeur.tree(), Some("task:01".into()));
        let id = sup::WindowId::new(APP, WINDOW);
        let envoi = registre
            .check_action(&id, "task:01", "send", &json!({}))
            .unwrap();
        assert!(envoi.irreversible);
        // Une tâche étrangère ne voit pas la fenêtre.
        assert!(registre.tree(&id, "task:02", Detail::Normal).is_err());
    }
}
