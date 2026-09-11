# ADR-0005 — Projeter les identifiants depuis le gestionnaire, par poignée de main

- **Statut** : accepté
- **Date** : 2026-09-11
- **Tâche liée** : M5-T1

## Contexte

L'amorçage de sandbox crée ses espaces de noms puis doit se projeter sur la racine de son nouvel espace utilisateur. La voie directe, écrire `/proc/self/uid_map` après `unshare`, échoue avec `EPERM` dans les environnements conteneurisés courants : le noyau réserve cette écriture à un processus de l'espace de noms parent.

C'est exactement pour cette raison que `unshare --map-root-user` et bubblewrap écrivent la carte depuis le parent.

## Décision

Le gestionnaire et l'amorçage se synchronisent par deux tubes nommés, un par sens :

1. L'amorçage appelle `unshare`, puis annonce sur le tube `ready`.
2. Le gestionnaire écrit `setgroups`, `uid_map` et `gid_map` du processus enfant.
3. Le gestionnaire libère l'amorçage par le tube `go`.
4. L'amorçage poursuit : racine minimale, Landlock, seccomp, puis exécution du programme.

Un refus à l'étape 2 libère l'amorçage avec un signal négatif, qui s'arrête proprement ; la sandbox ne démarre pas. Le gestionnaire ne dégrade jamais l'isolation en silence.

## Alternatives écartées

- Écriture par l'enfant : refusée par le noyau dans les environnements conteneurisés, donc inutilisable.
- Déléguer à `unshare` de util-linux : ajoute une dépendance externe et retire au gestionnaire la maîtrise des étapes suivantes.
- Descripteurs hérités plutôt que tubes nommés : exigerait du code `unsafe` autour de `pre_exec`, que le projet s'interdit hors sonde documentée.

## Conséquences

Le gestionnaire doit pouvoir écrire dans `/proc/<pid>/uid_map` d'un enfant, ce qui suppose `CAP_SETUID` dans son espace de noms. Sur Prophet OS, `sandboxd` est un service système qui l'obtient. Sur une machine de développement sans ce droit, le lancement échoue avec un message explicite plutôt que de démarrer moins isolé qu'annoncé.

Le coût de la poignée de main est mesuré : le lancement d'une sandbox de niveau 0 reste sous la milliseconde en médiane.
