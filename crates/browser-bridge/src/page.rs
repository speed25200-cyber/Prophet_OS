//! Une page, vue comme une fenêtre SUP.

use serde::{Deserialize, Serialize};
use serde_json::json;
use sup::tree::{Detail, Tree};

use crate::cdp::{CdpError, Session};
use crate::{ACT_JS, EXTRACT_JS};

/// Résultat d'une action sur la page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionResult {
    /// Succès.
    pub ok: bool,
    /// Code d'erreur nommé, le cas échéant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Détail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Une page pilotée.
#[derive(Debug)]
pub struct Page {
    session: Session,
    last_tree: Option<Tree>,
}

impl Page {
    /// Ouvre une page à partir d'une session DevTools.
    ///
    /// # Errors
    /// Si le domaine `Page` refuse de s'activer.
    pub async fn attach(mut session: Session) -> Result<Self, CdpError> {
        session.call("Page.enable", json!({})).await?;
        session.call("Runtime.enable", json!({})).await?;
        Ok(Self {
            session,
            last_tree: None,
        })
    }

    /// Ouvre une adresse et attend le chargement.
    ///
    /// # Errors
    /// Si la navigation échoue.
    pub async fn navigate(&mut self, url: &str) -> Result<(), CdpError> {
        self.session
            .call("Page.navigate", json!({"url": url}))
            .await?;
        // Attente du document prêt, par sondage du document plutôt que par un délai fixe : un
        // délai fixe est soit trop court et instable, soit trop long et coûteux.
        for _ in 0..100 {
            let state = self
                .evaluate("document.readyState")
                .await
                .unwrap_or_default();
            if state.contains("complete") || state.contains("interactive") {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        Err(CdpError::Unexpected(
            "la page n'a pas fini de charger".to_owned(),
        ))
    }

    async fn evaluate(&mut self, expression: &str) -> Result<String, CdpError> {
        let result = self
            .session
            .call(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true,
                    "awaitPromise": true
                }),
            )
            .await?;
        if let Some(details) = result.get("exceptionDetails") {
            return Err(CdpError::Protocol(details.to_string()));
        }
        Ok(result["result"]["value"]
            .as_str()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| result["result"]["value"].to_string()))
    }

    /// Construit l'arbre sémantique de la page.
    ///
    /// # Errors
    /// Si l'extraction échoue ou rend un arbre illisible.
    pub async fn tree(&mut self, detail: Detail) -> Result<Tree, CdpError> {
        let raw = self.evaluate(EXTRACT_JS).await?;
        let tree: Tree =
            serde_json::from_str(&raw).map_err(|e| CdpError::Unexpected(format!("{e} : {raw}")))?;
        self.last_tree = Some(tree.clone());
        Ok(Tree {
            root: tree.root.at_detail(detail),
            ..tree
        })
    }

    /// Exécute une action typée et rend son résultat **avec** le nouvel arbre.
    ///
    /// Rendre le nouvel état supprime le cycle « agir, recapturer, deviner » : l'agent sait
    /// immédiatement ce que son action a produit.
    ///
    /// # Errors
    /// Si l'action est refusée par la page ou si le transport échoue.
    pub async fn act(
        &mut self,
        action: &str,
        node: Option<&str>,
        value: Option<&str>,
    ) -> Result<(ActionResult, Tree), CdpError> {
        if action == "navigate" {
            let url = value.ok_or_else(|| {
                CdpError::Unexpected("l'action navigate exige une adresse".to_owned())
            })?;
            self.navigate(url).await?;
            let tree = self.tree(Detail::Normal).await?;
            return Ok((
                ActionResult {
                    ok: true,
                    error: None,
                    detail: None,
                },
                tree,
            ));
        }

        let expression = format!(
            "({ACT_JS})({}, {}, {})",
            json_str(action),
            node.map_or_else(|| "null".to_owned(), json_str),
            value.map_or_else(|| "null".to_owned(), json_str)
        );
        let raw = self.evaluate(&expression).await?;
        let result: ActionResult =
            serde_json::from_str(&raw).map_err(|e| CdpError::Unexpected(format!("{e} : {raw}")))?;

        // Une action peut déclencher une navigation : on laisse à la page le temps de réagir
        // avant de relire l'arbre, sinon on observerait l'état d'avant.
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        let tree = self.tree(Detail::Normal).await?;
        Ok((result, tree))
    }

    /// Dernier arbre observé, sans nouvelle extraction.
    #[must_use]
    pub const fn last_tree(&self) -> Option<&Tree> {
        self.last_tree.as_ref()
    }
}

fn json_str(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}
