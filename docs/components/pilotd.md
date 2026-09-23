# pilotd — lanceur de pilotes de la session

- Programme : `prophet-pilotd`, service utilisateur de la session graphique (`prophet-pilotd.service`,
  partie de `sway-session.target`), sous l'identité de l'humain
- Socket : `/run/prophet/pilot.sock`, groupe `prophet-system` ; seul `agentd` est admis (`SO_PEERCRED`)
- Rôle : lancer un client officiel non modifié (Claude Code, Codex, Gemini) dans une mission
  préparée, avec son profil privé et la configuration MCP du pont `prophet-mcp`, et rendre sa
  réponse finale (ADR 0035). Il ne décide d'aucun droit.

## Méthodes

| Méthode | Effet |
|---|---|
| `pilot.status` | Chaque client : installé, connecté (sondé par sa propre commande), version, sans lire ses fichiers. Servi d'un cache rafraîchi en arrière-plan (toutes les 60 s, et après chaque lancement) : la réponse ne dépend pas de la durée des sondes, qui peuvent attendre le réseau |
| `pilot.run` | `{task, driver, intent, wall_time_s, model?}` : écrit la configuration MCP en 0600, ouvre le socket filtré de la mission, lance le client **dans sa cage** (ADR 0056) en mode non interactif, dans son propre groupe de processus — avec son palier de modèle s'il est demandé (`--model` pour Claude Code, `-m` pour Codex et Gemini, ADR 0040) —, attend (tué au délai, avec ce qu'il a lancé), rend `{exit_code, text, duration_ms, output_bytes}` |
| `pilot.stop` | `{task}` : tue sur-le-champ le client lancé pour cette mission, et son groupe de processus ; `pilot.run` rend alors « arrêté à la demande ». `{task, stopped}`, `stopped` disant si un client tournait |

## Comment agentd s'en sert

`task.options` interroge le lanceur (trois secondes au plus), propose les clients prêts en tête
des modèles de chaque contexte — ce sont les modèles principaux — et ne propose un rôle
`driver:` que si le client est prêt. Une mission préparée sur un client (`task.prepare {model:
"codex"}`) est lancée par `task.start` exactement comme une délégation, sans parent : `pilot.run`,
séance rejointe par le pont, conclusion par le service si le client part sans se retirer. `task.delegate {role}` résolu en `driver:<client>` prépare la sous-mission
pour une séance d'outils (jeton délégué par capd, filiation, budget prélevé, rôle, même
propriétaire), puis appelle `pilot.run` et attend. Le client rejoint la séance par le pont
(`task.attach`, sous l'identité de l'humain, propriétaire de la mission), appelle ses outils
(`task.call`, chacun tranché par capd et journalisé, compté sous `client:<nom>` sans tokens), se
retire (`task.detach`). Une séance laissée ouverte est conclue par le service avec le texte du
client ; un client qui ne s'attache pas fait échouer la sous-mission en le disant.

## La cage (ADR 0056)

Chaque client lancé en mission passe par `prophet-pilot-cage` : espaces de noms utilisateur,
montage, processus, IPC et nom d'hôte, sous l'uid et le gid de l'humain ; racine minimale où
figurent le système en lecture seule, le profil privé du client en écriture, et une maison, un
temporaire et un répertoire de travail propres à la mission (retirés après elle) ; Landlock
par-dessus. Le client ne voit ni la maison de l'humain, ni `/run/prophet`, ni son bus de
session, ni sway ; il joint agentd par un seul socket, relayé par le lanceur, qui ne laisse
passer que la séance de sa mission (`task.attach`, `task.tools`, `task.call`, `task.detach`).
Sans cage, aucun client n'est lancé (`SandboxError`).

## Limites

- Le réseau de l'hôte reste au client (phase 1 de l'ADR 0056) : il joint son éditeur
  directement, pas par egress. Le relais vers egress et la politique des hôtes de chaque
  éditeur sont la phase 2.
- Les sondes d'état et la connexion (`prophet provider login`) tournent hors cage : ce sont les
  commandes du client, lancées pour l'humain, sans objectif de mission.
- Le vrai Claude Code et le vrai Codex exigent une connexion que seul l'humain effectue ; les
  preuves de ce composant emploient un client de remplacement par le même chemin. Le paramètre
  `-c` de Codex pour ses serveurs MCP suit sa documentation, sans vérification sur le binaire ;
  Gemini n'a pas de configuration MCP raccordée.
- Le parent ne reçoit que le texte final du client, pas ses événements intermédiaires.

Voir le [guide du crate](../../crates/pilotd/README.md) et l'[ADR 0035](../adr/0035-clients-officiels-comme-roles-par-le-lanceur-de-session.md).
