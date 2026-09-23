# `prophet-sandboxd` — gestionnaire d'isolation

- **Socket** : `/run/prophet/sandboxd.sock`
- **Utilisateur** : `root` — le seul service qui en a besoin
- **Crate** : `crates/sandboxd`

Tout processus non fiable tourne sous ce daemon, au niveau requis. Il est le seul à porter
`CAP_SETUID`, `CAP_SETGID`, `CAP_SYS_ADMIN` et l'accès à `/dev/kvm`, et il ne les prête à personne.

## Les trois niveaux

| Niveau | Moyens | Ce qu'il faut sur la machine |
|---|---|---|
| 0 | espaces de noms, Landlock, seccomp, cgroups v2 | un noyau ≥ 5.13 et des espaces de noms utilisables |
| 1 | gVisor (`runsc`) | `runsc` installé |
| 2 | microVM Firecracker | `/dev/kvm` **ouvrable**, `firecracker`, e2fsprogs (`mkfs.ext4`, `debugfs`), et des images d'invité — sur la machine installée, l'invité du dépôt (`invite-microvm` : noyau publié par Firecracker, racine busybox + Python, ADR 0038), nommé à sandboxd par `PROPHET_MICROVM_KERNEL` et `PROPHET_MICROVM_ROOTFS`. Le répertoire de travail part dans un disque ext4 avec `.prophet/exec.sh`, l'invité le monte au même chemin que sur l'hôte (surcouche overlay en mémoire sur sa racine en lecture seule), l'exécute, la console porte la sortie entre « PROPHET_INVITE_PRET » et « PROPHET_INVITE_FIN code=N », et le disque revient |

### La réserve du niveau 2

Au démarrage, sandboxd démarre un invité **sans tâche**, qui attend son disque de travail, en
fait un instantané, et garde `PROPHET_MICROVM_POOL` machines (2 par défaut, 0 pour aucune)
restaurées depuis cet instantané, en pause (ADR 0045). Une exécution de niveau 2 prend l'une
d'elles : le disque de la tâche remplace le disque d'attente, la machine reprend, l'invité lit
son chemin de travail sur le disque et exécute sous les mêmes marques qu'à froid. Une machine
ne sert qu'une fois, la réserve en restaure une autre ; vide, ou refusée, l'exécution démarre à
froid, toujours en microVM. `sandbox.capabilities` rend `reserve` : cible, machines prêtes,
dernière durée de restauration, erreur. L'instantané (1 Gio de mémoire) va sous
`PROPHET_MICROVM_RESERVE`, sinon le répertoire temporaire du service.

## Méthodes

| Méthode | Ce qu'elle fait |
|---|---|
| `sandbox.capabilities` | Ce que cette machine sait isoler, et ce qui lui manque |
| `sandbox.min_level` | Le niveau exigé : le plus élevé du manifeste et du jeton, au moins 2 si la tâche exécute du code |
| `sandbox.start` | Lance un programme sous sandbox |
| `sandbox.run` | Lance, attend (délai borné, tue au-delà) et rend code de retour, sortie et erreur bornées ; c'est `proc.exec` (ADR 0031) |
| `sandbox.freeze` / `sandbox.thaw` | Gèle et dégèle le groupe de processus entier — une tâche gelée se reprend, une tâche tuée a perdu son état |
| `sandbox.kill` | Termine |
| `sandbox.status`, `sandbox.list` | Ce qui vit |

## La règle qui fait la différence

**Le niveau demandé est un plancher, pas un souhait.** Si la machine ne sait pas isoler au niveau
exigé, la tâche ne démarre pas — elle ne démarre pas « à un niveau plus bas en attendant ». Un
agent qui se croit en microVM alors qu'il tourne dans un espace de noms agirait avec une confiance
qui ne correspond à rien.

Le refus porte le rapport complet de ce que la machine sait faire, pour qu'on le comprenne sans se
connecter à la machine.

## Les capacités qu'il reçoit, et celle qui ne se devine pas

`CAP_SETUID`, `CAP_SETGID`, `CAP_SYS_ADMIN` — et `CAP_SETFCAP`.

La dernière surprend, et c'est pour cela qu'elle est écrite ici. Depuis Linux 5.12, projeter
l'**uid 0** dans un espace de noms utilisateur exige `CAP_SETFCAP` dans l'espace parent, et non
`CAP_SETUID` comme le laisse croire la lecture du code de projection. Sans elle, l'écriture de
`uid_map` rend `EPERM` — et « Operation not permitted » ne renvoie à rien qu'on puisse chercher.

Le refus nomme donc maintenant sa cause : il lit l'espace de noms de l'enfant, celui du
gestionnaire, la carte déjà écrite et ses propres capacités, et dit laquelle des quatre causes
possibles s'applique. `tools/verifier-le-durcissement.sh` refuse par ailleurs, en une seconde, un
service qui reçoit `CAP_SETUID` sans `CAP_SETFCAP`.

## Pourquoi son filtre d'appels système n'est pas celui des autres

Les six autres daemons tournent avec `@system-service` moins `@privileged` et `@resources`. Ce
filtre ne convient pas à celui-ci : `@system-service` ne contient pas `@mount`, et `~@privileged`
retire `setuid`, `setgid`, `setgroups` et `pivot_root` — c'est-à-dire exactement le travail de ce
service. Lui accorder `CAP_SETUID` d'une main et lui interdire `setuid` de l'autre est une
contradiction qui ne se voit qu'à l'exécution, et que `sandbox.capabilities` ne peut pas signaler
puisqu'il sonde le noyau et non ses propres entraves.

Elle s'est vue en toutes lettres au premier démarrage sous systemd :

```
confinement impossible : écriture de uid_map : Operation not permitted
```

Son filtre ajoute donc `@mount` — monter la racine minimale, puis `pivot_root` — et
`@privileged` — `setuid`, `setgid`, `setgroups`, `capset`. Ce qui en est aussitôt retiré est plus
long que ce qui est ajouté : `@privileged` est un fourre-tout qui contient de quoi charger un
module noyau, changer l'heure, arrêter la machine ou lire la mémoire d'un autre processus. Un
service capable de charger un module rendrait tout le reste décoratif.

Ce que cela n'élargit **pas** : la sandbox. Le filtre qu'une tâche subit est posé par `sandboxd`
dans son enfant, après le confinement, et il est bien plus étroit. Le filtre du service borne le
gestionnaire ; celui de la sandbox borne la tâche. Les confondre revenait à borner le gardien avec
les règles du prisonnier.

## Distinguer « absent » de « présent mais refusé »

`sandbox.capabilities` rend séparément `user_namespaces`, `userns_restreint_par_politique` et
`kvm`. Les confondre a coûté une demi-journée (ADR-0006) : Ubuntu 24.04 laisse créer un espace de
noms mais interdit d'y exécuter, et `/dev/kvm` peut exister sans être ouvrable. Une sonde qui
constate une présence n'a rien vérifié.
