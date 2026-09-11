//! Différentiel d'arbre.
//!
//! Un agent qui réobserve une fenêtre n'a pas besoin de tout relire : il lui faut ce qui a changé.
//! C'est ce qui fait tomber le coût d'une boucle agentique, bien plus que la compression de
//! l'arbre lui-même.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::tree::{Node, Tree};

/// Un changement dans l'arbre.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    /// Nœud apparu.
    Added {
        /// Identifiant.
        id: String,
        /// Nœud complet.
        node: Node,
    },
    /// Nœud disparu.
    Removed {
        /// Identifiant.
        id: String,
    },
    /// Valeur modifiée.
    ValueChanged {
        /// Identifiant.
        id: String,
        /// Ancienne valeur.
        from: Option<String>,
        /// Nouvelle valeur.
        to: Option<String>,
    },
    /// Nom modifié.
    NameChanged {
        /// Identifiant.
        id: String,
        /// Nouveau nom.
        to: String,
    },
    /// Disponibilité modifiée.
    AvailabilityChanged {
        /// Identifiant.
        id: String,
        /// Le nœud est-il désormais désactivé ?
        disabled: bool,
    },
    /// Le focus a bougé.
    FocusMoved {
        /// Nouveau nœud ayant le focus.
        to: Option<String>,
    },
    /// Le titre a changé.
    TitleChanged {
        /// Nouveau titre.
        to: String,
    },
}

/// Différence entre deux versions d'un arbre.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeDiff {
    /// Version de départ.
    pub from_version: u64,
    /// Version d'arrivée.
    pub to_version: u64,
    /// Changements.
    pub changes: Vec<Change>,
}

impl TreeDiff {
    /// Vrai si rien n'a changé.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Rendu lisible, destiné au journal et à l'humain.
    #[must_use]
    pub fn render(&self) -> String {
        if self.is_empty() {
            return "aucun changement\n".to_owned();
        }
        let mut out = String::new();
        for change in &self.changes {
            match change {
                Change::Added { id, .. } => out.push_str(&format!("+ {id}\n")),
                Change::Removed { id } => out.push_str(&format!("- {id}\n")),
                Change::ValueChanged { id, to, .. } => {
                    out.push_str(&format!("~ {id} = {}\n", to.as_deref().unwrap_or("(vide)")))
                }
                Change::NameChanged { id, to } => out.push_str(&format!("~ {id} « {to} »\n")),
                Change::AvailabilityChanged { id, disabled } => out.push_str(&format!(
                    "~ {id} {}\n",
                    if *disabled { "désactivé" } else { "activé" }
                )),
                Change::FocusMoved { to } => out.push_str(&format!(
                    "→ focus sur {}\n",
                    to.as_deref().unwrap_or("(aucun)")
                )),
                Change::TitleChanged { to } => out.push_str(&format!("≡ titre « {to} »\n")),
            }
        }
        out
    }
}

fn index(node: &Node, map: &mut BTreeMap<String, Node>) {
    let mut shallow = node.clone();
    shallow.children = Vec::new();
    map.insert(node.id.clone(), shallow);
    for child in &node.children {
        index(child, map);
    }
}

/// Calcule la différence entre deux arbres de la même fenêtre.
#[must_use]
pub fn compute(before: &Tree, after: &Tree) -> TreeDiff {
    let mut old = BTreeMap::new();
    let mut new = BTreeMap::new();
    index(&before.root, &mut old);
    index(&after.root, &mut new);

    let mut changes = Vec::new();
    if before.title != after.title {
        changes.push(Change::TitleChanged {
            to: after.title.clone(),
        });
    }
    if before.focus != after.focus {
        changes.push(Change::FocusMoved {
            to: after.focus.clone(),
        });
    }
    for (id, node) in &new {
        match old.get(id) {
            None => {
                // Le nœud ajouté est rendu avec ses enfants, pour que l'agent n'ait pas à
                // réobserver l'arbre entier après un simple ajout.
                let full = after.root.find(id).cloned().unwrap_or_else(|| node.clone());
                changes.push(Change::Added {
                    id: id.clone(),
                    node: full,
                });
            }
            Some(previous) => {
                if previous.value != node.value {
                    changes.push(Change::ValueChanged {
                        id: id.clone(),
                        from: previous.value.clone(),
                        to: node.value.clone(),
                    });
                }
                if previous.name != node.name {
                    changes.push(Change::NameChanged {
                        id: id.clone(),
                        to: node.name.clone(),
                    });
                }
                if previous.disabled != node.disabled {
                    changes.push(Change::AvailabilityChanged {
                        id: id.clone(),
                        disabled: node.disabled,
                    });
                }
            }
        }
    }
    for id in old.keys() {
        if !new.contains_key(id) {
            changes.push(Change::Removed { id: id.clone() });
        }
    }
    // Un ajout dont le parent est lui aussi ajouté est déjà couvert par celui-ci.
    let added: Vec<String> = changes
        .iter()
        .filter_map(|c| match c {
            Change::Added { id, node } if node.children.is_empty() => None,
            Change::Added { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    changes.retain(|change| match change {
        Change::Added { id, .. } => !added.iter().any(|parent| {
            parent != id
                && after
                    .root
                    .find(parent)
                    .is_some_and(|node| node.find(id).is_some())
        }),
        _ => true,
    });

    TreeDiff {
        from_version: before.version,
        to_version: after.version,
        changes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Role;

    fn base() -> Tree {
        Tree::new(
            "app",
            "w1",
            "Titre",
            Node::new("root", Role::Group, "Racine").children([
                Node::new("a", Role::Field, "A").value("1").actionable(),
                Node::new("b", Role::Button, "B").actionable(),
            ]),
        )
    }

    #[test]
    fn aucun_changement() {
        let diff = compute(&base(), &base());
        assert!(diff.is_empty());
        assert_eq!(diff.render(), "aucun changement\n");
    }

    #[test]
    fn valeur_modifiee() {
        let mut apres = base();
        apres.version = 2;
        apres.root.children[0].value = Some("2".into());
        let diff = compute(&base(), &apres);
        assert_eq!(
            diff.changes,
            vec![Change::ValueChanged {
                id: "a".into(),
                from: Some("1".into()),
                to: Some("2".into())
            }]
        );
        assert_eq!(diff.to_version, 2);
    }

    #[test]
    fn ajout_et_suppression() {
        let mut apres = base();
        apres.root.children.remove(1);
        apres
            .root
            .children
            .push(Node::new("c", Role::Status, "Envoyé"));
        let diff = compute(&base(), &apres);
        assert!(diff.changes.contains(&Change::Removed { id: "b".into() }));
        assert!(
            diff.changes
                .iter()
                .any(|c| matches!(c, Change::Added { id, .. } if id == "c"))
        );
    }

    #[test]
    fn un_sous_arbre_ajoute_ne_compte_qu_une_fois() {
        let mut apres = base();
        apres
            .root
            .children
            .push(Node::new("liste", Role::List, "Résultats").children([
                Node::new("r1", Role::Item, "Premier"),
                Node::new("r2", Role::Item, "Deuxième"),
            ]));
        let diff = compute(&base(), &apres);
        let ajouts: Vec<&str> = diff
            .changes
            .iter()
            .filter_map(|c| match c {
                Change::Added { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            ajouts,
            vec!["liste"],
            "les enfants sont portés par le nœud ajouté, pas répétés"
        );
        // Et ils sont bien présents dans la charge utile.
        let Change::Added { node, .. } = diff
            .changes
            .iter()
            .find(|c| matches!(c, Change::Added { .. }))
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(node.children.len(), 2);
    }

    #[test]
    fn focus_et_titre() {
        let mut apres = base();
        apres.focus = Some("a".into());
        apres.title = "Autre titre".into();
        let diff = compute(&base(), &apres);
        assert!(diff.changes.contains(&Change::TitleChanged {
            to: "Autre titre".into()
        }));
        assert!(diff.changes.contains(&Change::FocusMoved {
            to: Some("a".into())
        }));
    }

    #[test]
    fn desactivation() {
        let mut apres = base();
        apres.root.children[1].disabled = true;
        let diff = compute(&base(), &apres);
        assert_eq!(
            diff.changes,
            vec![Change::AvailabilityChanged {
                id: "b".into(),
                disabled: true
            }]
        );
    }

    #[test]
    fn le_differentiel_est_bien_plus_petit_que_l_arbre() {
        let mut apres = base();
        // Un arbre volumineux dont un seul champ change.
        let gros: Vec<Node> = (0..300)
            .map(|i| Node::new(format!("n{i}"), Role::Text, format!("ligne {i}")))
            .collect();
        let mut avant = base();
        avant.root.children.extend(gros.clone());
        apres.root.children.extend(gros);
        apres.root.children[0].value = Some("2".into());

        let taille_arbre = serde_json::to_vec(&apres).unwrap().len();
        let taille_diff = serde_json::to_vec(&compute(&avant, &apres)).unwrap().len();
        assert!(
            taille_diff * 20 < taille_arbre,
            "différentiel {taille_diff} octets contre {taille_arbre} pour l'arbre"
        );
    }

    #[test]
    fn rendu_lisible() {
        let mut apres = base();
        apres.root.children[0].value = Some("2".into());
        let rendu = compute(&base(), &apres).render();
        assert!(rendu.contains("~ a = 2"), "{rendu}");
    }
}
