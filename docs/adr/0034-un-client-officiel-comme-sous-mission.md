# ADR 0034 — Un client officiel connecté comme sous-mission

Date : 2026-09-13. Statut : accepté, première livraison.

## Contexte

La délégation (ADR 0029) fait coopérer des modèles sous capd, mais l'enfant était toujours un
modèle du moteur local. Un client officiel connecté par l'abonnement de l'humain (Claude Code,
Codex) ne pouvait être enfant que si l'humain le lançait lui-même sur une séance d'outils
(ADR 0026). Or ce sont ces clients, avec leurs modèles de pointe, qu'un agent local voudrait
appeler pour la part difficile d'une tâche. Deux invariants tiennent la solution : l'OS ne lit,
ne copie ni ne réutilise jamais les identifiants des clients, et les clients tournent sans
modification.

## Décision

1. **La session lance, le service tient.** `prophet-supd`, qui tourne déjà dans la session de
   l'humain pour l'accessibilité, gagne deux méthodes réservées à agentd : `client.status`
   (le client est-il installé et connecté ? demandé au client, jamais lu) et `client.run`
   (lancer le client sur une mission préparée et attendre). Le client tourne sous l'identité de
   l'humain, avec son répertoire de configuration privé (`~/.local/state/prophet/providers/…`)
   et ses propres identifiants, un home éphémère par lancement, et une configuration MCP qui ne
   nomme que le pont `prophet-mcp` de la mission.
2. **Le client n'a que les outils de Prophet.** Claude Code est lancé avec `--tools ""`,
   `--strict-mcp-config`, `--allowedTools mcp__prophet`, `--permission-prompts none` et un
   nombre de tours borné ; Codex avec `--sandbox read-only` et le pont comme seul serveur MCP ;
   Gemini n'a pas de mode équivalent et est refusé. Chaque appel d'outil passe par le pont,
   `task.call`, le registre et capd, sous le jeton délégué de l'enfant ; le travail va dans
   l'espace SFS de l'enfant, à examiner comme toute écriture d'agent.
3. **`task.delegate` accepte un client.** `model = "claude-code"` (ou `codex`) exige que le
   contexte admette `driver:<client>` dans ses modèles et ne soit pas local seulement, que la
   session soit raccordée et le client connecté. L'enfant est planifié avec ce pilote, rattaché
   au parent et à son propriétaire, puis la session le lance ; le parent attend. Le client
   rejoint la mission (`task.attach`), travaille (`task.call`) et se retire (`task.detach`) ;
   son texte final complète le résultat de la séance. Un client qui s'arrête sans avoir rejoint
   la mission laisse un enfant `failed` avec sa raison ; un client arrêté au délai est retiré
   de force, la séance interrompue.
4. **Le lanceur ne juge rien.** Il vérifie qui l'appelle, borne le temps, les sorties et le
   nom de la mission, et tue le groupe de processus du client au délai. Les droits sont tranchés
   ailleurs, comme pour tout le reste.

## Conséquences

- Un agent local peut confier à Claude Code ou Codex une part d'une tâche, sous les mêmes outils
  et les mêmes droits que lui, et recevoir le résultat ; l'humain voit la sous-mission dans la
  supervision, avec son pilote.
- Ce que le client fait de son propre modèle n'est pas sous le contrôle de Prophet : son
  abonnement, son quota, ses conditions. Prophet ne contrôle que ce qu'il touche sur la
  machine, et c'est exactement ce que le pont laisse passer.
- Prouvé avec un faux client qui fait ce que fait le vrai en mode délégué (pont, attache,
  outils, retrait, texte final) ; avec un vrai Claude Code, cela exige un compte connecté sur la
  machine (`needs_claude_login`), ce que la CI n'a pas.
- Le client ne reçoit pas de fichiers : parent et enfant se parlent par l'intention et le
  résultat, comme entre modèles locaux. L'annulation d'une sous-mission déléguée arrête le
  client au prochain appel d'outil, pas immédiatement ; l'arrêt immédiat viendra avec le
  signal du lanceur.
