# Spécification — IPC interne (v0)

- **Version** : 0.1
- **Crate** : `prophet-ipc`
- **Décision** : ADR-0003

## Transport

- Sockets Unix de type flux, chemins `/run/prophet/<daemon>.sock`, mode `0660`, groupe `prophet-system` ; les sockets destinés aux tâches (serveurs MCP) sont montés dans la sandbox de la tâche.
- Un message = un objet JSON-RPC 2.0 sur une ligne terminée par `\n`. Taille maximale d'un message : 8 Mio (au-delà, `-32600`).
- Multiplexage par `id`. Les flux sont des notifications répétées (`method` sans `id`) associées à un `run` ou `subscription`.

## Authentification

1. `SO_PEERCRED` sur chaque connexion : `uid`, `gid`, `pid`. Les méthodes système (`ledger.append`, `vault.put`, `cap.mint`…) exigent `gid == prophet-system`.
2. Méthodes de tâche : `params._auth = "<jeton de capacité base64>"`. Le serveur appelle `cap.check` (ou vérifie localement avec la clé publique de `capd` pour les lectures) avant toute action. Absence → `-32001 unauthorized`.
3. Les serveurs journalisent `uid`, `pid`, `task` de chaque appel dans `tracing` ; les événements du Ledger ne contiennent pas le jeton.

## Codes d'erreur

| Code | Nom | Usage |
|---|---|---|
| -32700 | parse error | JSON invalide |
| -32600 | invalid request | trop grand, forme invalide |
| -32601 | method not found | |
| -32602 | invalid params | validation de schéma |
| -32001 | unauthorized | pas de jeton, pair non autorisé |
| -32002 | policy denied | `cap.check` refuse ; `data.reason`, `data.rule` |
| -32003 | approval required | `data.approval_id` |
| -32004 | budget exceeded | `data.dimension` |
| -32005 | sandbox error | `data.level`, `data.detail` |
| -32006 | not found | tâche, run, fichier |
| -32007 | conflict | état incompatible (ex. `resume` d'une tâche terminée) |

## Conventions

- Noms de méthodes : `<domaine>.<verbe>` en minuscules, `snake_case`.
- Horodatages : RFC 3339 UTC millisecondes.
- Identifiants : ULID, préfixés (`task:`, `run:`, `apr:`).
- Digests : `blake3:HEX`.
- Toute méthode a un schéma JSON d'entrée et de sortie généré par `schemars`, versionné dans `docs/specs/schemas/ipc/`.

## Performance

Objectif v0 : 100 000 aller-retours `ping` en moins de 5 s sur un portable, connexion réutilisée. Mesuré par `crates/prophet-ipc/benches/roundtrip.rs`.
