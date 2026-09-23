# Spécification — Événement du Ledger (v0)

- **Version** : 0.1
- **Types Rust** : `prophet_types::ledger`
- **Stockage** : `ledger` (M3)

## Structure

```jsonc
{
  "v": 0,
  "seq": 184223,                         // entier croissant strict, global à la machine
  "prev": "blake3:HEX",                  // hash de l'événement précédent (ou "genesis")
  "ts": "2026-09-11T14:07:52.118Z",      // horloge monotone recalée, UTC, ms
  "task": "task:<ulid>",                 // ou null pour les événements système
  "step": 18,                            // ou null
  "actor": "agentd" | "capd" | "mcp:fs" | "driver:claude-code" | "user" | "system",
  "kind": "tool.call",
  "payload": { ... },                    // spécifique au kind, voir catalogue
  "hash": "blake3:HEX"                   // blake3 de la sérialisation canonique sans `hash`
}
```

## Règles

1. `seq` est attribué par `ledger` à l'écriture ; un producteur ne le choisit jamais.
2. `prev` = `hash` de l'événement `seq - 1`. Le premier événement a `prev = "genesis"`.
3. Aucune valeur de secret, aucun contenu de fichier complet, aucun corps de requête réseau dans `payload` : uniquement des digests (`blake3`), des tailles, des chemins, des noms d'outils, des codes de décision. Le contenu vit dans `sfs` ou dans les résultats d'outils, pas dans le journal.
4. Les `payload` sont validés par schéma par `kind`. Un `kind` inconnu est refusé.
5. Scellement : toutes les 1 000 entrées ou 60 secondes, un événement `ledger.seal` contient la signature ed25519 (ou TPM) du `hash` courant.

## Catalogue des `kind` et charges utiles minimales

| `kind` | Charge utile |
|---|---|
| `task.created` | `intent_digest`, `agent`, `manifest_version`, `user` |
| `task.planned` | `provider`, `sandbox_level`, `grants_digest`, `budget` |
| `task.started` / `task.done` / `task.failed` / `task.cancelled` / `task.rolled_back` | `reason?`, `stats {steps, tool_calls, approvals, wall_ms, tokens?}`, `by_model? {<modèle>: {turns, tokens_in, tokens_out}}`, `role?` (relais de modèles, ADR 0034) |
| `task.waiting` | `approval_id` |
| `tool.call` | `tool`, `args_digest`, `args_size`, `requires` |
| `tool.result` | `tool`, `ok`, `error_code?`, `result_digest`, `result_size`, `duration_ms` |
| `policy.allow` / `policy.deny` | `res`, `act`, `target`, `rule`, `reason?` |
| `policy.revoked` | `sub` |
| `approval.requested` | `approval_id`, `action`, `context_digest`, `irreversible`, `external` |
| `approval.resolved` | `approval_id`, `decision`, `scope`, `by` |
| `fs.begin` / `fs.commit` / `fs.undo` / `fs.abandon` | `snapshot`, `files {added, modified, deleted}`, `bytes` |
| `net.request` | `host`, `port`, `method`, `bytes_out`, `bytes_in`, `status` |
| `net.deny` / `net.exfil_suspected` | `host`, `reason`, `score?` |
| `provider.started` / `provider.stopped` | `driver`, `model?`, `session_ref_digest` |
| `provider.quota` | `driver`, `estimate_pct`, `window_reset?` |
| `sandbox.started` / `sandbox.frozen` / `sandbox.killed` | `level`, `profile`, `cgroup`, `startup_ms` |
| `ui.tree` / `ui.act` | `app`, `window`, `detail` ou `action`, `irreversible`, `result_code` |
| `memory.write` | `space`, `entry_id`, `source_task` |
| `model.pulled` / `model.removed` | `id`, `file`, `sha256`, `bytes` (ADR 0046) |
| `ledger.seal` | `up_to_seq`, `signature`, `signer` |

## Vérification

`ledger.verify {from, to}` recalcule chaque `hash`, vérifie chaque `prev`, vérifie chaque `ledger.seal` avec la clé publique. Toute divergence renvoie `{ok:false, first_bad_seq, reason}`.
