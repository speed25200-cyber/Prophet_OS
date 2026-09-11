//! Registre des fenêtres.
//!
//! Les applications y publient leur arbre ; les agents le lisent et y agissent. Le registre fait
//! deux choses qu'aucune application ne peut faire seule : il **cloisonne** (un agent ne voit que
//! les fenêtres de sa tâche ou celles qu'on lui a ouvertes) et il **historise** (il garde la
//! version précédente, donc il peut répondre en différentiel).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::diff::{self, TreeDiff};
use crate::tree::{Detail, Tree};

/// Identifiant d'une fenêtre : application et fenêtre.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WindowId {
    /// Application.
    pub app: String,
    /// Fenêtre.
    pub window: String,
}

impl WindowId {
    /// Construit un identifiant.
    #[must_use]
    pub fn new(app: impl Into<String>, window: impl Into<String>) -> Self {
        Self {
            app: app.into(),
            window: window.into(),
        }
    }
}

impl std::fmt::Display for WindowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.app, self.window)
    }
}

/// Erreur du registre.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RegistryError {
    /// Fenêtre inconnue.
    #[error("fenêtre inconnue : {0}")]
    UnknownWindow(String),
    /// La tâche n'a pas accès à cette fenêtre.
    #[error("la tâche {task} n'a pas accès à la fenêtre {window}")]
    Forbidden {
        /// Tâche demandeuse.
        task: String,
        /// Fenêtre visée.
        window: String,
    },
    /// Action inconnue.
    #[error("action inconnue sur {window} : {action}")]
    UnknownAction {
        /// Fenêtre.
        window: String,
        /// Action demandée.
        action: String,
    },
    /// Arguments invalides.
    #[error("arguments invalides pour {action} : {detail}")]
    BadArguments {
        /// Action.
        action: String,
        /// Explication.
        detail: String,
    },
}

/// Résultat d'une action, tel que l'agent le reçoit.
///
/// Le nouvel état accompagne toujours le résultat : c'est ce qui supprime le cycle
/// « agir, recapturer, deviner si ça a marché ».
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionOutcome {
    /// Succès.
    pub ok: bool,
    /// Code d'erreur, le cas échéant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Ce qui a changé du fait de l'action.
    pub diff: TreeDiff,
}

#[derive(Debug, Clone)]
struct Entry {
    current: Tree,
    previous: Option<Tree>,
    /// Tâche propriétaire, s'il y en a une.
    owner: Option<String>,
    /// Tâches auxquelles la fenêtre a été ouverte explicitement.
    shared_with: Vec<String>,
}

/// Registre des fenêtres.
#[derive(Debug, Default)]
pub struct Registry {
    windows: BTreeMap<WindowId, Entry>,
}

impl Registry {
    /// Registre vide.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Publie ou met à jour l'arbre d'une fenêtre.
    ///
    /// La version précédente est conservée, pour pouvoir répondre en différentiel.
    pub fn publish(&mut self, tree: Tree, owner: Option<String>) {
        let id = WindowId::new(&tree.app, &tree.window);
        match self.windows.get_mut(&id) {
            Some(entry) => {
                entry.previous = Some(entry.current.clone());
                entry.current = tree;
            }
            None => {
                self.windows.insert(
                    id,
                    Entry {
                        current: tree,
                        previous: None,
                        owner,
                        shared_with: Vec::new(),
                    },
                );
            }
        }
    }

    /// Ouvre une fenêtre à une tâche qui n'en est pas propriétaire.
    pub fn share(&mut self, id: &WindowId, task: impl Into<String>) -> bool {
        self.windows.get_mut(id).is_some_and(|entry| {
            let task = task.into();
            if !entry.shared_with.contains(&task) {
                entry.shared_with.push(task);
            }
            true
        })
    }

    /// Vrai si la tâche peut voir la fenêtre.
    ///
    /// Une fenêtre sans propriétaire est publique : c'est le cas des applications que l'humain a
    /// ouvertes lui-même et qu'il accepte de partager.
    #[must_use]
    pub fn visible_to(&self, id: &WindowId, task: &str) -> bool {
        self.windows
            .get(id)
            .is_some_and(|entry| match &entry.owner {
                None => true,
                Some(owner) => owner == task || entry.shared_with.iter().any(|t| t == task),
            })
    }

    /// Fenêtres visibles par une tâche.
    #[must_use]
    pub fn list_for(&self, task: &str) -> Vec<WindowId> {
        self.windows
            .keys()
            .filter(|id| self.visible_to(id, task))
            .cloned()
            .collect()
    }

    /// Lit l'arbre d'une fenêtre au niveau de détail demandé.
    ///
    /// # Errors
    /// Fenêtre inconnue, ou hors de portée de la tâche.
    pub fn tree(&self, id: &WindowId, task: &str, detail: Detail) -> Result<Tree, RegistryError> {
        let entry = self.entry_for(id, task)?;
        Ok(Tree {
            root: entry.current.root.at_detail(detail),
            ..entry.current.clone()
        })
    }

    /// Lit ce qui a changé depuis la version indiquée.
    ///
    /// Quand la version demandée n'est pas celle que le registre a gardée, il le dit en rendant un
    /// différentiel depuis la version disponible : mieux vaut un différentiel plus large qu'un
    /// différentiel faux.
    ///
    /// # Errors
    /// Fenêtre inconnue, ou hors de portée de la tâche.
    pub fn diff_since(
        &self,
        id: &WindowId,
        task: &str,
        since_version: u64,
    ) -> Result<TreeDiff, RegistryError> {
        let entry = self.entry_for(id, task)?;
        match &entry.previous {
            Some(previous) if previous.version == since_version => {
                Ok(diff::compute(previous, &entry.current))
            }
            _ if entry.current.version == since_version => Ok(TreeDiff {
                from_version: since_version,
                to_version: since_version,
                changes: Vec::new(),
            }),
            Some(previous) => Ok(diff::compute(previous, &entry.current)),
            None => Ok(TreeDiff {
                from_version: 0,
                to_version: entry.current.version,
                changes: vec![crate::diff::Change::Added {
                    id: entry.current.root.id.clone(),
                    node: entry.current.root.clone(),
                }],
            }),
        }
    }

    /// Vérifie une action sans l'exécuter, et rend ses annotations de sécurité.
    ///
    /// # Errors
    /// Fenêtre ou action inconnue, arguments invalides.
    pub fn check_action(
        &self,
        id: &WindowId,
        task: &str,
        action: &str,
        args: &Value,
    ) -> Result<crate::tree::Action, RegistryError> {
        let entry = self.entry_for(id, task)?;
        let found = entry
            .current
            .action(action)
            .ok_or_else(|| RegistryError::UnknownAction {
                window: id.to_string(),
                action: action.to_owned(),
            })?;
        found
            .validate(args)
            .map_err(|detail| RegistryError::BadArguments {
                action: action.to_owned(),
                detail,
            })?;
        Ok(found.clone())
    }

    /// Applique le nouvel état produit par une action et rend le résultat, différentiel compris.
    pub fn apply_result(
        &mut self,
        id: &WindowId,
        new_tree: Tree,
        ok: bool,
        error: Option<String>,
    ) -> ActionOutcome {
        let before = self.windows.get(id).map(|e| e.current.clone());
        self.publish(new_tree, None);
        let after = self.windows.get(id).map(|e| e.current.clone());
        let diff = match (before, after) {
            (Some(before), Some(after)) => diff::compute(&before, &after),
            _ => TreeDiff {
                from_version: 0,
                to_version: 0,
                changes: Vec::new(),
            },
        };
        ActionOutcome { ok, error, diff }
    }

    fn entry_for(&self, id: &WindowId, task: &str) -> Result<&Entry, RegistryError> {
        let entry = self
            .windows
            .get(id)
            .ok_or_else(|| RegistryError::UnknownWindow(id.to_string()))?;
        if !self.visible_to(id, task) {
            return Err(RegistryError::Forbidden {
                task: task.to_owned(),
                window: id.to_string(),
            });
        }
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{Action, ArgType, Node, Role};
    use serde_json::json;

    fn arbre(version: u64, valeur: &str) -> Tree {
        let mut t = Tree::new(
            "prophet.mail",
            "compose-1",
            "Nouveau message",
            Node::new("root", Role::Group, "Composer").children([Node::new(
                "to",
                Role::Field,
                "Destinataire",
            )
            .value(valeur)
            .actionable()]),
        )
        .actions([
            Action::new("set_field", "Remplit un champ")
                .arg("field", ArgType::String, true)
                .arg("value", ArgType::String, true),
            Action::new("send", "Envoie").irreversible().external(),
        ]);
        t.version = version;
        t
    }

    fn id() -> WindowId {
        WindowId::new("prophet.mail", "compose-1")
    }

    #[test]
    fn une_tache_ne_voit_pas_la_fenetre_d_une_autre() {
        let mut r = Registry::new();
        r.publish(arbre(1, "a@x.fr"), Some("task:01".into()));
        assert!(r.visible_to(&id(), "task:01"));
        assert!(!r.visible_to(&id(), "task:02"));
        assert_eq!(
            r.tree(&id(), "task:02", Detail::Normal).unwrap_err(),
            RegistryError::Forbidden {
                task: "task:02".into(),
                window: "prophet.mail/compose-1".into()
            }
        );
        assert!(r.list_for("task:02").is_empty());
    }

    #[test]
    fn une_fenetre_peut_etre_ouverte_a_une_autre_tache() {
        let mut r = Registry::new();
        r.publish(arbre(1, "a@x.fr"), Some("task:01".into()));
        assert!(r.share(&id(), "task:02"));
        assert!(r.visible_to(&id(), "task:02"));
        assert_eq!(r.list_for("task:02"), vec![id()]);
    }

    #[test]
    fn une_fenetre_sans_proprietaire_est_publique() {
        let mut r = Registry::new();
        r.publish(arbre(1, "a@x.fr"), None);
        assert!(r.visible_to(&id(), "n'importe quelle tâche"));
    }

    #[test]
    fn lecture_au_niveau_demande() {
        let mut r = Registry::new();
        r.publish(arbre(1, "a@x.fr"), None);
        let complet = r.tree(&id(), "t", Detail::Full).unwrap();
        assert_eq!(
            complet.root.find("to").unwrap().value.as_deref(),
            Some("a@x.fr")
        );
    }

    #[test]
    fn differentiel_depuis_la_version_precedente() {
        let mut r = Registry::new();
        r.publish(arbre(1, "a@x.fr"), None);
        r.publish(arbre(2, "marie@exemple.fr"), None);
        let diff = r.diff_since(&id(), "t", 1).unwrap();
        assert_eq!(diff.changes.len(), 1);
        assert_eq!(diff.to_version, 2);
    }

    #[test]
    fn differentiel_depuis_la_version_courante_est_vide() {
        let mut r = Registry::new();
        r.publish(arbre(1, "a@x.fr"), None);
        r.publish(arbre(2, "b@x.fr"), None);
        assert!(r.diff_since(&id(), "t", 2).unwrap().is_empty());
    }

    #[test]
    fn une_version_trop_ancienne_donne_un_differentiel_plus_large_pas_faux() {
        let mut r = Registry::new();
        r.publish(arbre(1, "a@x.fr"), None);
        r.publish(arbre(2, "b@x.fr"), None);
        r.publish(arbre(3, "c@x.fr"), None);
        // La version 1 n'est plus gardée : le registre rend le différentiel depuis la 2.
        let diff = r.diff_since(&id(), "t", 1).unwrap();
        assert_eq!(diff.from_version, 2);
        assert!(!diff.is_empty());
    }

    #[test]
    fn premiere_observation_rend_l_arbre_entier() {
        let mut r = Registry::new();
        r.publish(arbre(1, "a@x.fr"), None);
        let diff = r.diff_since(&id(), "t", 0).unwrap();
        assert!(matches!(diff.changes[0], crate::diff::Change::Added { .. }));
    }

    #[test]
    fn action_inconnue_et_arguments_invalides() {
        let mut r = Registry::new();
        r.publish(arbre(1, "a@x.fr"), None);
        assert!(matches!(
            r.check_action(&id(), "t", "inexistante", &json!({})),
            Err(RegistryError::UnknownAction { .. })
        ));
        assert!(matches!(
            r.check_action(&id(), "t", "set_field", &json!({"field": "to"})),
            Err(RegistryError::BadArguments { .. })
        ));
    }

    #[test]
    fn les_annotations_de_securite_remontent_avec_l_action() {
        let mut r = Registry::new();
        r.publish(arbre(1, "a@x.fr"), None);
        let envoi = r.check_action(&id(), "t", "send", &json!({})).unwrap();
        assert!(envoi.irreversible && envoi.external);
    }

    #[test]
    fn le_resultat_d_une_action_porte_le_nouvel_etat() {
        let mut r = Registry::new();
        r.publish(arbre(1, "a@x.fr"), None);
        let outcome = r.apply_result(&id(), arbre(2, "marie@exemple.fr"), true, None);
        assert!(outcome.ok);
        assert_eq!(outcome.diff.changes.len(), 1);
        // L'agent sait immédiatement que le champ vaut la nouvelle valeur, sans réobserver.
        assert!(outcome.diff.render().contains("marie@exemple.fr"));
    }

    #[test]
    fn fenetre_inconnue() {
        let r = Registry::new();
        assert!(matches!(
            r.tree(&id(), "t", Detail::Normal),
            Err(RegistryError::UnknownWindow(_))
        ));
    }
}
