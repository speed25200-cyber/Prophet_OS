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
| `fs.read` | fs.read chemin | non | non | `max_bytes` défaut 256 Kio |
| `fs.write` | fs.write chemin | non (réversible via sfs) | non | via transaction sfs |
| `fs.list` | fs.list chemin | non | non | |
| `fs.stat` | fs.read chemin | non | non | |
| `fs.search` | fs.read racine | non | non | nom et contenu |
| `fs.diff_task` | task courante | non | non | |
| `proc.exec` | proc.exec binaire | selon commande | non | niveau de sandbox forcé à 2 hors liste blanche |
| `proc.kill` | task courante | non | non | |
| `http.fetch` | net.egress hôte | selon méthode (POST/PUT/DELETE = irréversible) | oui | via egress |
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
