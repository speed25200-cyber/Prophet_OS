# Spécification — Jeton de capacité (v0)

- **Version** : 0.1
- **Types Rust** : `prophet_types::cap`
- **Émetteur unique** : `capd`
- **Signature** : ed25519 sur la sérialisation canonique

## Rôle

Un jeton de capacité est la seule preuve de droit dans le système. Il est porté par une tâche, présenté à chaque appel d'outil ou de daemon, et vérifié par `capd.check`. Il ne dit pas *qui* est l'agent, il dit *ce que* cette tâche peut faire, sous quelles contraintes, jusqu'à quand.

## Structure

```jsonc
{
  "v": 0,
  "iss": "capd@<machine-id>",
  "sub": "task:<ulid>",
  "agent": "org.exemple.analyste-ventes",
  "user": "hakik",
  "parent": null,                                  // ou "<blake3 du jeton parent>"
  "grants": [
    { "res": "fs",   "act": "read",   "match": "~/ventes/**" },
    { "res": "fs",   "act": "write",  "match": "~/ventes/out/**" },
    { "res": "net",  "act": "egress", "match": "driver:claude-code" },
    { "res": "tool", "act": "call",   "match": "mail.send",
      "constraints": { "to": ["marie@exemple.fr"], "max_calls": 1, "approval": "required" } }
  ],
  "iat": "2026-09-11T14:03:11Z",
  "exp": "2026-09-11T14:33:11Z",
  "nonce": "BASE64-16-octets",
  "sig": "ed25519:BASE64"
}
```

## Ressources et actions (v0)

| `res` | `act` | `match` | Contraintes admises |
|---|---|---|---|
| `fs` | `read`, `write`, `list` | glob de chemin | `max_bytes` |
| `net` | `egress` | domaine, `*.domaine`, `domaine:port`, `driver:<pilote>` | `methods`, `max_bytes_out`, `rate_per_min` |
| `tool` | `call` | nom d'outil ou glob `domaine.*` | `max_calls`, `approval` ∈ {`none`, `required`}, champs spécifiques à l'outil |
| `proc` | `exec` | glob de chemin de binaire | `level` (niveau de sandbox minimal) |
| `ui` | `act`, `read`, `vision` | identifiant d'application ou `*` | `windows` |
| `ledger` | `read`, `read_all` | `*` | |
| `memory` | `read`, `write` | nom d'espace | |
| `model` | `use` | `local:*`, `driver:*`, `api:*` | `max_tokens` |
| `task` | `spawn` | `*` | `max_depth`, `max_children` |
| `cap` | `delegate` | `*` | |

## Sérialisation canonique

JSON sans espaces, clés triées récursivement, chaînes en NFC, nombres sans exposant ni zéros de tête, `sig` absent. La signature couvre exactement ces octets. Implémentation de référence : `prophet_types::canon::to_canonical_bytes`.

## Délégation

`child ⊆ parent` si et seulement si :

1. `child.exp ≤ parent.exp`, `child.user == parent.user`, `child.parent == blake3(parent)`.
2. Pour chaque `grant` enfant, il existe un `grant` parent de même `res` et `act` tel que le `match` enfant est couvert par le `match` parent (tout chemin, domaine ou nom accepté par l'enfant l'est par le parent), et chaque contrainte de l'enfant est au moins aussi stricte que celle du parent (les contraintes absentes chez le parent peuvent être ajoutées par l'enfant ; une contrainte présente chez le parent ne peut pas être retirée ni relâchée).

La profondeur de délégation est limitée par la contrainte `max_depth` du grant `task.spawn` du jeton racine (défaut 3).

## Vérification (`cap.check`)

Entrée : jeton, `res`, `act`, `target`, `context` (outil, arguments digest, niveau de sandbox courant). Ordre :

1. Format et version.
2. Signature et chaîne de parents (chaque parent est retrouvé dans le cache de `capd` ; un parent révoqué invalide l'enfant).
3. Expiration.
4. Politique Cedar : `allow` requis.
5. Existence d'un `grant` couvrant `res`, `act`, `target`, contraintes satisfaites.

Sortie : `{ "decision": "allow" }` ou `{ "decision": "deny", "reason": "<code>", "rule": "<id de politique ou de grant>" }`. Codes : `expired`, `bad_signature`, `revoked_parent`, `policy_denied`, `no_grant`, `constraint_violated`, `approval_required`.

## Révocation

`cap.revoke {sub}` ajoute le `sub` à une liste en mémoire persistée ; tous les jetons de cette tâche et de ses enfants sont refusés. Publié sur le bus (`policy.revoked`).

## Vecteurs de test

`docs/specs/vectors/cap/` : clé de test, 8 jetons valides, 8 invalides (signature, expiration, délégation trop large, contrainte relâchée, parent révoqué, version inconnue, clé inconnue, glob illégal), chacun avec la sortie attendue de `cap.check`.
