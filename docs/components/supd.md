# supd — adaptateur d'accessibilité de la session

**Programme** : `prophet-supd`, service utilisateur de la session graphique (`sway-session.target`).
**Socket** : `/run/prophet/sup.sock` (`PROPHET_SUP_SOCKET`), groupe `prophet-system`, appelant
admis : `agentd` (`PROPHET_SUP_CLIENT`), plus le propriétaire si `PROPHET_SUP_ALLOW_OWNER=1`.
**Délai** : `PROPHET_SUP_TIMEOUT_SECS` (10 s) par opération sur le bus.

| Méthode | Paramètres | Rend |
|---|---|---|
| `sup.status` | — | `{ready, detail}` : le bus d'accessibilité répond, combien d'applications |
| `sup.apps` | — | `[{app, windows}]`, sans titre |
| `sup.tree` | `{app, window?}` | `{tree, provenance, confidence, caveat, truncated?}` |
| `sup.act` | `{app, window?, action, node, value?}` | `{message, observation?}` |

L'identifiant d'une application est le nom qu'elle se donne sur le bus, en minuscules. Une
fenêtre est désignée par son identifiant de nœud ou son titre ; sans indication, la fenêtre
active, sinon la dernière ouverte. Les nœuds portent le dernier segment de leur chemin d'objet
AT-SPI ; une action vise ce nœud. `click` emploie la première action `click`, `activate`,
`press`, `jump`, `open` ou `select` déclarée ; `set_field` exige l'interface `EditableText` et
l'état `editable` ; `toggle` emploie `toggle`, `click` ou `activate`. L'adaptateur n'attend pas
la réponse d'une action : un dialogue modal ne la rendrait qu'à sa fermeture.

Ce qu'il ne fait pas : décider d'un droit (agentd et capd), prendre une capture d'écran,
simuler une touche ou un clic, lire une application qui n'expose pas d'accessibilité.

Essai local sans Nix : `tools/bureau-local.sh` monte un X virtuel, un bus de session et un bus
d'accessibilité, et lance `cargo test -p supd` contre Mousepad. Voir l'[ADR 0027](../adr/0027-pilotage-des-applications-par-l-accessibilite.md).
