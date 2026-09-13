# Spécification — Contrat Agent Driver (v0)

- **Version** : 0.1
- **Types Rust** : `prophet_types::driver`
- **Implémentations** : `providers::official`, `providers::native`, `providers::local`, `providers::local_async`, `providers::mock`

État du 12 septembre 2026 : ce document décrit le contrat cible. Le socket `providers.sock`
et l'exécution des clients officiels ne sont pas encore implémentés. Le diagnostic CLI est
opérationnel ; voir [les preuves et limites](../components/providers.md).

## Rôle

Un pilote enveloppe une façon d'obtenir une boucle agentique : un client officiel d'éditeur connecté par abonnement, ou la boucle native de l'OS sur un modèle local ou une API. Le reste du système (`agentd`, le shell, le Ledger, les approbations) ne voit que ce contrat.

## Principes

1. Un client officiel tourne **sans modification**, dans une sandbox, avec son répertoire de configuration privé. Le pilote ne lit jamais ses fichiers d'identifiants.
2. Le pilote n'utilise que des mécanismes documentés du client : mode non interactif, sortie structurée en flux, configuration des serveurs MCP, hooks, délégation des demandes de permission, reprise de session.
3. Tout ce que le client fait passe par les serveurs MCP système, `sandboxd` et `egress`. Le pilote ne donne jamais au client un accès direct au disque ou au réseau hors sandbox.

## Méthodes JSON-RPC (socket `/run/prophet/providers.sock`, multiplexé par `driver`)

### `driver.capabilities`

Entrée : `{driver}`. Sortie :

```jsonc
{
  "driver": "claude-code",
  "kind": "official-client" | "native",
  "auth": "subscription" | "none" | "api-key",
  "supports": { "resume": true, "checkpoint": false, "fork": false,
                "token_usage": true, "quota_estimate": true, "cost": false,
                "streaming_events": true, "permission_delegation": true },
  "logged_in": true,
  "models": ["default"]                   // ou liste de modèles locaux
}
```

### `driver.start`

Entrée :

```jsonc
{
  "driver": "claude-code",
  "task": "task:<ulid>",
  "intent": "…",
  "workdir": "/home/u/.prophet/tasks/<ulid>/work",
  "mcp_config": "/run/prophet/tasks/<ulid>/mcp.json",
  "token": "<jeton de capacité base64>",
  "sandbox": { "level": 1, "profile": "base" },
  "limits": { "wall_time_s": 1200, "max_steps": 200 },
  "resume": null                           // ou identifiant de session opaque
}
```

Sortie : `{ "run": "<ulid>", "session_ref": "<opaque>" }`.

### `driver.events`

Entrée : `{run}`. Flux (notifications JSON-RPC `driver.event`) :

| `type` | Champs |
|---|---|
| `step` | `n`, `summary?` |
| `tool_call` | `tool`, `args_digest`, `via` ∈ {`mcp`, `builtin`} |
| `tool_result` | `tool`, `ok`, `error?` |
| `text` | `role`, `text` (texte final ou intermédiaire destiné à l'humain) |
| `permission_request` | `id`, `tool`, `args_digest`, `reason?` |
| `usage` | `tokens_in?`, `tokens_out?`, `cost_eur?`, `quota_pct?` |
| `checkpoint` | `ref` |
| `done` | `status` ∈ {`ok`, `failed`, `cancelled`}, `reason?`, `session_ref` |

### `driver.approve` / `driver.deny`

Entrée : `{run, id, note?}`. Le pilote transmet la décision au client par le mécanisme de délégation de permission. Sortie : `{ok}`.

### `driver.pause` / `driver.resume` / `driver.cancel`

Entrée : `{run}`. `pause` gèle la sandbox (`sandbox.freeze`) ; `resume` dégèle ; `cancel` envoie un arrêt propre puis tue après 5 s. Sortie : `{ok}`.

### `driver.login`

Entrée : `{driver}`. Lance le flux de connexion **du client** (navigateur ou code d'appareil) dans son répertoire privé et retourne `{instructions, url?}` pour l'humain. L'OS ne voit pas les identifiants.

## Correspondance avec les clients officiels

| Besoin | Claude Code | Codex CLI | Gemini CLI | Prophet Agent |
|---|---|---|---|---|
| non interactif + flux | mode `-p` avec sortie JSON en flux | `codex exec` avec sortie JSON | mode non interactif | natif |
| serveurs MCP | fichier de configuration MCP | section `mcp_servers` de sa configuration | configuration MCP | natif |
| journalisation d'outils | hooks avant et après outil → `prophet-hook` | événements de sa sortie structurée | hooks ou sortie structurée | natif |
| délégation de permission | outil de demande de permission externe → `prophet-permission` | politique d'approbation + serveur d'application | équivalent | natif |
| reprise | identifiant de session | identifiant de session | selon version | checkpoint complet |
| sandbox interne du client | conservée ; `sandboxd` ajoute les contraintes de l'OS | idem | idem | sans objet |

Les noms exacts des options se vérifient dans la documentation du client au moment de l'implémentation (M8) et sont consignés dans `docs/components/providers.md` avec la version testée.

## Suite de conformité (`driver-conformance`)

Scénarios obligatoires pour tout pilote : démarrage et fin normale ; appel d'outil MCP journalisé ; demande de permission → approbation → poursuite ; demande de permission → refus → fin propre ; annulation en cours ; pause et reprise ; dépassement de `wall_time_s` → `done{failed, reason:"timeout"}` ; `driver.capabilities` cohérent avec le comportement observé.
