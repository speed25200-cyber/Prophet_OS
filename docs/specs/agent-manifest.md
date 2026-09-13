# Spécification — Manifeste d'agent (v0)

- **Version** : 0.1 (gelée à la fin de M1-T1 ; toute modification incrémente `schema_version`)
- **Format** : TOML, signé par l'éditeur de l'agent (signature détachée `<nom>.toml.sig`, ed25519)
- **Types Rust** : `prophet_types::manifest`
- **Schéma JSON généré** : `docs/specs/schemas/agent-manifest.schema.json`

## Rôle

Le manifeste décrit un agent installable : son identité, ses préférences de fournisseur, le **plafond** de capacités qu'il pourra jamais demander, son niveau de sandbox minimal, ses budgets par défaut et les actions qui exigent une approbation. `capd` ne délivre jamais un jeton qui dépasse `capabilities.max`.

## Structure

```toml
schema_version = 0

[agent]
id = "org.exemple.analyste-ventes"      # DNS inversé, [a-z0-9.-], unique
version = "1.2.0"                        # semver
name = "Analyste ventes"
description = "Prépare des rapports à partir de fichiers CSV."
publisher_key = "ed25519:BASE64"         # clé publique de l'éditeur

[model]
preferred = ["local:qwen3-14b", "driver:claude-code", "driver:codex"]
privacy = "local-preferred"              # local-only | local-preferred | any
min_capability = "standard"              # small | standard | frontier

[model.roles]                            # facultatif : le relais de modèles (ADR 0034)
reflect = ["driver:claude-code"]         # réflexion profonde, découpage, vérification
code    = ["driver:codex", "local:qwen3-14b"]
execute = ["local:qwen3-14b"]            # étapes simples, au modèle le moins coûteux

[capabilities.max]
"fs.read"  = ["~/ventes/**", "~/modeles/**"]
"fs.write" = ["~/ventes/out/**"]
"net.egress" = ["driver:claude-code", "driver:codex", "*.exemple.fr"]
"tool.call" = ["fs.*", "sheet.*", "doc.render", "mail.send"]
"proc.exec" = []                         # vide = jamais
"ui.act" = ["prophet.mail", "prophet.sheet"]

[sandbox]
min_level = 1                            # 0 | 1 | 2
code_execution = "microvm"               # forbidden | microvm

[budget.default]
tokens = 400000
wall_time = "20m"                        # durée ISO-like : 30s, 20m, 2h
approvals = 3
cost_eur = 2.00                          # classe C uniquement

[actions]
"mail.send" = { require_approval = true, max_calls = 1 }

[memory]
spaces = ["work"]
```

## Règles de validation (toutes vérifiées par `Manifest::validate`)

1. `agent.id` : 3 à 128 caractères, DNS inversé, au moins deux segments.
2. `agent.version` : semver strict.
3. `publisher_key` : préfixe `ed25519:`, 32 octets en base64.
4. `model.preferred` : au moins un élément ; chaque élément est `local:<nom>`, `driver:<pilote>` ou `api:<fournisseur>:<modèle>`. `model.roles`, facultatif, n'admet que les clés `reflect`, `execute` et `code`, chacune avec au moins une référence valide ; un catalogue de missions (agentd) exige de plus que chaque référence de rôle figure dans `preferred`, le rôle ne pouvant qu'y choisir.
5. `capabilities.max` : au moins une clé. Les chemins commencent par `~/` ou `/`. Les globs suivent `globset`. `**` n'est autorisé qu'en fin de segment.
6. `net.egress` : éléments de la forme `domaine`, `*.domaine`, `domaine:port` ou `driver:<pilote>` (signifie « les domaines que ce pilote a besoin de joindre, gérés par l'OS »). Jamais d'adresse IP en v0.
7. `sandbox.min_level` ∈ {0, 1, 2}. Si `capabilities.max."proc.exec"` est non vide, `code_execution` doit être `microvm`.
8. Budgets strictement positifs ; `wall_time` ≤ 24h.
9. Toute clé inconnue est une erreur (pas de champ ignoré silencieusement).

## Sémantique

- `capabilities.max` est un plafond, pas une demande. Une tâche demande un sous-ensemble ; `capd` calcule l'intersection.
- `privacy = local-only` interdit tout pilote non local même si listé dans `preferred`.
- `actions.<outil>.require_approval` s'ajoute aux politiques Cedar ; il ne peut jamais les affaiblir.

## Jeux de test (M1-T1)

`crates/prophet-types/tests/manifests/valid/*.toml` (6) et `invalid/*.toml` (6, chacun avec un fichier `.error` contenant la sous-chaîne attendue du message d'erreur).
