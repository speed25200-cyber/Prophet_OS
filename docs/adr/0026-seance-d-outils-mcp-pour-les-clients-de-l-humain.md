# 0026 — Une séance d'outils tenue par agentd pour les clients MCP de l'humain

- **Statut** : accepté
- **Date** : 2026-09-13
- **Tâches liées** : M7-T10 (registre consommable par les clients d'éditeurs), M8-T4 et M8-T5 (pilotes Claude Code et Codex), M8-T1 (cycle de vie)
- **Complète** : ADR 0015 (profils de mission), ADR 0023 (publication par agentd)

## Contexte

Le plan prévoit que les clients d'éditeurs reçoivent les serveurs MCP du système, et la
spécification des outils décrivait un `prophet-mcp` lancé par le client avec le jeton de la
tâche dans un fichier. Ce binaire refusait de servir, et le schéma posait deux problèmes sur
la machine installée : le jeton d'une mission aurait quitté le service, et le travail SFS de la
mission, privé à `agentd`, n'est pas lisible par le processus de l'humain. Claude Code et Codex
tournent pourtant sur le bureau, sous le compte humain, avec leurs propres outils natifs et
sans aucune des garanties de Prophet OS : ni droits tranchés par capd, ni journal, ni versions
à examiner avant publication.

## Décision

**Le service tient la séance ; le client ne reçoit que les outils.** Une mission préparée par
son créateur (`task.prepare`, ou « Nouvel objectif ») accueille un client par `task.attach`.
`agentd` ouvre alors exactement ce qu'il ouvre pour une mission native : le jeton émis par capd,
le travail SFS capturé sur les périmètres, le registre d'outils sous contrôle et journal, sans
modèle. `task.tools` rend la liste des outils que le jeton couvre ; `task.call` exécute un
appel, compté comme une étape du budget ; `task.detach` scelle les versions et conclut la
mission en `done`, prête pour `task.change`, `task.apply` et `task.undo`. Une annulation
(`task.cancel`) conclut la séance en `cancelled` et le client l'apprend à son prochain appel.
Toutes ces méthodes exigent l'UID créateur persisté, comme la publication.

**`prophet-mcp` est un pont, pas un serveur.** Lancé par le client avec `PROPHET_TASK`, il parle
MCP sur son entrée et sa sortie standard : `initialize` attache la mission avec le nom du
client, `tools/list` et `tools/call` sont relayés, la fin de l'entrée retire le client. Il ne
tient aucun jeton, n'ouvre aucun fichier et n'exécute aucun outil. `prophet task mcp-config
<mission>` rend la configuration à donner au client (`--mcp-config` de Claude Code ;
`mcp_servers` de Codex). Le paramètre `PROPHET_TASK_AUTH_FILE` disparaît.

**Ce que le client voit.** Seuls les outils que le jeton couvre lui sont proposés ; un appel
hors périmètre est refusé par le registre, avec le motif, sans que le contenu visé n'apparaisse ;
une écriture va dans le travail de la mission, jamais dans le home ; une action externe ou
irréversible demande une décision humaine, comme pour une mission native. Le journal porte
`provider.started` avec `driver: mcp-client` et le nom du client, puis chaque `tool.call` et
sa cible.

## Conséquences et limites

Le client reste un processus de l'humain, avec ses propres outils natifs : la séance ajoute
des outils contrôlés, elle ne confine pas le client. C'est le pilote de l'ADR M8-T4, lancé par
`agentd` dans une sandbox avec la même séance, qui apportera le confinement ; cette décision
en pose la moitié service. Une séance vit avec le service : un redémarrage la perd et la
mission passe en échec, comme une mission native. La capacité de travail (deux missions
actives) est partagée entre séances et missions natives. Le bureau ne configure pas encore
ses lanceurs pour ouvrir une séance ; `prophet task mcp-config` est la porte d'entrée.
