# Spécification — Outils MCP système (v0)

- **Version** : 0.1
- **Crate** : `mcp-system`
- **Registre généré** : `/etc/prophet/mcp/system.json`

## Conventions

- Nom : `<domaine>.<verbe>`, en anglais, `snake_case`.
- Chaque outil déclare dans ses métadonnées : `requires` (res, act, cible dérivée des arguments), `irreversible` (bool), `external` (bool), `sandbox_level_min` (0, 1, 2 ou null).
- Descriptions destinées aux modèles : une phrase, précise, avec les contraintes ; exemples d'arguments dans le schéma.
- Résultats : toujours structurés (`content` de type `text` contenant du JSON, plus `structuredContent`), avec `truncated: true` et `total_bytes` quand un résultat a été coupé.
- Erreurs : `isError: true` avec `{ "code": "<PolicyDenied|ApprovalRequired|NotFound|SandboxError|BudgetExceeded|Invalid>", "detail": "…" }`.
- Transport : stdio et socket Unix. Le registre déclare la commande stdio pour les clients d'éditeurs.

## Liste normative v0

| Outil | `requires` | irreversible | external | Notes |
|---|---|---|---|---|
| `fs.read` | fs.read chemin | non | non | `max_bytes` défaut et plafond 256 Kio |
| `fs.write` | fs.write chemin | non (réversible via sfs) | non | remplacement atomique dans le travail, contenu ≤ 1 Mio |
| `fs.list` | fs.list chemin et descendants | non | non | fusion travail/origine ; ≤ 2000 résultats |
| `fs.stat` | fs.read chemin | non | non | |
| `fs.search` | fs.read racine et descendants | non | non | nom et contenu ; ≤ 200 résultats |
| `fs.diff_task` | task courante | non | non | |
| `proc.exec` | proc.exec binaire | selon commande | non | niveau de sandbox forcé à 2 hors liste blanche |
| `proc.kill` | task courante | non | non | |
| `http.fetch` | net.egress hôte | selon méthode (POST/PUT/DELETE = irréversible) | selon méthode | via egress, implémenté |
| `task.status` | task courante | non | non | |
| `task.diff` | task courante | non | non | |
| `task.commit_request` | task courante | oui | non | déclenche approbation si fichiers sensibles |
| `task.spawn_sub` | task.spawn | non | non | grants ⊆ |
| `approval.request` | task courante | non | non | |
| `approval.wait` | task courante | non | non | |
| `ledger.query` | ledger.read (ou read_all) | non | non | |
| `ledger.replay_summary` | ledger.read | non | non | |
| `memory.remember` | memory.write espace | non | non | |
| `memory.search` | memory.read espace | non | non | |
| `memory.forget` | memory.write espace | oui | non | |
| `memory.list` | memory.read espace | non | non | |
| `secrets.list_refs` | task courante | non | non | noms seulement |
| `secrets.use` | tool.call secrets.use | non | non | rend un handle |
| `clock.now` | aucune | non | non | horloge de tâche (rejouable) |
| `notify.human` | tool.call notify.human | non | non | hors bande |
| `ui.tree` | ui.read app | non | non | SUP |
| `ui.act` | ui.act app + `requires` de l'action | selon action | selon action | SUP |
| `ui.screenshot` | ui.vision | non | non | confiance basse, hors défaut |
| `model.list` | aucune | non | non | |
| `model.status` | aucune | non | non | |

Un outil sans `requires` explicite (autre que `aucune`) est refusé par le test `mcp_system::registry::all_tools_declare_requires`.

## Accès fichiers actuellement implémentés

Les chemins logiques sont relatifs au home de la tâche, sous la forme `~/docs/note.txt`,
`docs/note.txt` ou d'un chemin absolu inclus dans ce home. Les `..`, chemins hors du home,
noms `.prophet` et `.prophet-write-*` sont refusés. Les noms non UTF-8 sont omis des parcours.
Le contexte vient du lanceur de confiance : son `workdir` doit correspondre exactement à
`home/.prophet/tasks/<tâche>/work`. Une cohérence de chemins ne prouve pas l'identité du lanceur.

Les accès relatifs utilisent `openat2` avec `RESOLVE_BENEATH`, `RESOLVE_NO_SYMLINKS` et
`RESOLVE_NO_XDEV`. Les racines sont ouvertes sans lien symbolique. Les fichiers spéciaux et
les lectures de fichiers ayant plusieurs liens physiques sont refusés. L'absence de cette
primitive noyau provoque un échec ; aucun repli par canonicalisation/réouverture n'est permis.
Le travail a priorité sur l'origine, et seule une absence autorise le repli. Les écritures
créent leurs parents dans le travail, publient par renommage atomique et synchronisent fichier
et répertoire. Elles ne valident pas la transaction SFS.

La lecture de contenu est bornée à 256 Kio rendus (un octet supplémentaire sert à détecter la
troncature). `max_bytes` doit être un entier positif ou nul et ne peut relever le plafond.
La recherche lit au plus 8 Mio cumulés. Liste et recherche limitent les visites à 10 000, les
résultats sérialisés à 512 Kio et vérifient un budget de deux secondes entre les opérations ;
une opération noyau lente peut dépasser ce temps. La recherche descend jusqu'à 32 niveaux
après sa racine. Les plafonds de résultats sont respectivement 2000 et 200. Les omissions
dues aux plafonds donnent `truncated: true`. `scoped: true` signifie que les descendants sans
capacité ne sont pas énumérés dans la réponse ; aucun total de fichiers interdits n'est rendu.
Les limites du transport s'appliquent en plus de ces limites de contenu.

Le contexte fiable, les racines privées, le confinement des processus et les commits/undo
SFS concurrents restent des conditions d'intégration, détaillées dans l'[ADR 0012](../adr/0012-acces-fichiers-mcp.md).

## Sortie réseau et navigation actuellement implémentées

`http.fetch` ne joint jamais le réseau lui-même : il écrit la requête sur le socket du proxy
`egress` avec le jeton de la tâche dans l'en-tête interne `Proxy-Authorization: Prophet …`,
que le proxy retire avant la sortie après avoir fait trancher capd sur l'hôte réellement joint.
Seuls `http://` et `https://` sont relayés. `GET` et `HEAD` sont des lectures réseau
automatiques ; toute autre méthode est irréversible et externe, donc soumise à décision humaine,
dans le registre comme dans le proxy. La réponse est recomposée si elle est segmentée et bornée
à `max_bytes` (256 Kio par défaut, 1 Mio au plus) ; `truncated` le signale. Un refus du proxy
est rendu avec son code (`PolicyDenied` pour 403 et 407, `Invalid` pour 400 et 413, `NotFound`
pour 502, `SandboxError` sinon). Sans socket configuré, l'outil échoue sans émettre de requête.

Les outils `web.*` s'adossent au pont CDP (`browser-bridge`) et n'existent que si le service
nomme un programme de navigateur. `web.open {url, detail?}` exige `net.egress` sur l'hôte et
rend l'arbre SUP de la page ; `web.tree {detail?}` exige `ui.read browser` ; `web.act {action,
node?, value?}` exige `ui.act browser`, avec `click`, `set_field` et `submit` ; seul `submit`
est externe et demande une décision. Le profil du navigateur est propre à la tâche, dans l'état
privé du service. La sortie réseau propre du navigateur (sous-ressources) n'est pas relayée par
egress : voir l'[ADR 0024](../adr/0024-navigateur-integre-et-applications-web.md).

## Fichier de registre

```jsonc
{
  "mcpServers": {
    "prophet-fs":   { "command": "/run/current-system/sw/bin/prophet-mcp", "args": ["fs"],   "env": { "PROPHET_TASK_AUTH_FILE": "/run/prophet/tasks/<ulid>/auth" } },
    "prophet-proc": { "command": "/run/current-system/sw/bin/prophet-mcp", "args": ["proc"], "env": { "PROPHET_TASK_AUTH_FILE": "/run/prophet/tasks/<ulid>/auth" } }
    // … un serveur par domaine
  }
}
```

Le fichier est généré par tâche par `agentd` (M8) avec le chemin du jeton de cette tâche, et consommé tel quel par les clients d'éditeurs.
