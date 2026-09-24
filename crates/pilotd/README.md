# Lanceur de pilotes de la session

`prophet-pilotd` tourne dans la session graphique de l'humain, sous son identité. Quand un
rôle du relais (ADR 0034) désigne un client officiel (`driver:claude-code`, `driver:codex`,
`driver:gemini`), `agentd` lui demande de lancer ce client, sans modification, dans une
mission préparée : le client reçoit la configuration MCP du pont `prophet-mcp`, rejoint la
séance d'outils de la mission, y travaille sous le jeton délégué par capd, se retire, et sa
réponse finale revient au service (ADR 0035). L'OS ne lit jamais les identifiants du client :
ils restent dans son profil privé (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `GEMINI_CONFIG_DIR`).

## Configuration

| Variable | Rôle |
|---|---|
| `PROPHET_PILOT_SOCKET` | Socket d'écoute (`/run/prophet/pilot.sock` par défaut) |
| `PROPHET_PILOT_CLIENT` | Compte admis à appeler (`agentd` par défaut) |
| `PROPHET_PILOT_GROUP` | Groupe posé sur le socket (`prophet-system`) |
| `PROPHET_PILOT_ALLOW_OWNER` | `1` : le propriétaire de la session est admis aussi (essais) |
| `PROPHET_PILOT_STATE` | Racine des profils privés (`$HOME/.local/state/prophet` par défaut) |
| `PROPHET_AGENTD_SOCKET` | Socket d'agentd que le pont joindra |
| `PROPHET_MCP_BRIDGE` | Le pont `prophet-mcp` (voisin du binaire par défaut) |
| `PROPHET_PILOT_CLIENTS` | Clients de remplacement, JSON `{"codex": {"program": "…", "args": ["{intent}"]}}` (essais) |
| `PROPHET_PILOT_CAGE` | La cage des clients (`prophet-pilot-cage`, voisine du lanceur par défaut) |
| `PROPHET_PILOT_READ_ONLY` | Chemins supplémentaires visibles en lecture seule dans la cage, séparés par `:` (un client installé hors du système) |
| `PROPHET_EGRESS_SOCKET` | Le proxy de sortie par lequel passe le réseau des clients (`/run/prophet/egress.sock` par défaut) |

Chaque client tourne dans une cage (ADR 0056) : il ne voit que le système en lecture seule,
son profil privé, les lieux de sa mission et un socket qui ne mène qu'à la séance de sa
mission. Son réseau ne sort que par egress, sous le jeton de sa mission que le lanceur pose sur
chaque requête. Sans espaces de noms utilisateur, la cage ne se pose pas et aucun client n'est lancé.

Les clients sont sondés par leurs propres commandes (`claude auth status`, `codex login
status`) ; un client absent ou non connecté n'est pas lancé, et `task.options` ne propose pas
son rôle. Pour connecter un client, l'humain lance lui-même son flux de connexion dans son
profil privé (`prophet provider login <pilote>`).

## Méthodes

- `pilot.status` → `{drivers: [{driver, connection, executable?, version?}]}` ; répond du dernier
  sondage, rafraîchi par un fil toutes les 60 s et après chaque lancement, resondé s'il a plus de
  deux minutes (`StatusCache`).
- `pilot.run {task, driver, intent, wall_time_s}` → `{exit_code, text, duration_ms, output_bytes}`.
  La configuration MCP est écrite en 0600 sous `$XDG_RUNTIME_DIR/prophet-pilot/`, puis retirée.

## Validation

```sh
nix develop --command cargo test -p pilotd
nix develop --command cargo test -p agentd --test pilot
```

Les essais de la cage (`pilotd --test cage`, `agentd --test pilot`) exigent des espaces de noms
utilisateur : ailleurs, ils le disent et s'arrêtent ; `PROPHET_EXIGER_ESPACES_DE_NOMS=1` les
rend obligatoires, comme sur le coureur d'isolation.

Le second lance capd, ledger, agentd, `prophet-pilotd` et la CLI, avec un script à la place
de Codex ; le vrai client suit le même chemin, mais exige une connexion que seul l'humain peut
faire (`needs_chatgpt_login`, `needs_claude_login`).
