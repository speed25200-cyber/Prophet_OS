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
| 2 | microVM Firecracker | `/dev/kvm` **ouvrable**, `firecracker`, et des images d'invité |

## Méthodes

| Méthode | Ce qu'elle fait |
|---|---|
| `sandbox.capabilities` | Ce que cette machine sait isoler, et ce qui lui manque |
| `sandbox.min_level` | Le niveau exigé : le plus élevé du manifeste et du jeton, au moins 2 si la tâche exécute du code |
| `sandbox.start` | Lance un programme sous sandbox |
| `sandbox.freeze` / `sandbox.thaw` | Gèle et dégèle — une tâche gelée se reprend, une tâche tuée a perdu son état |
| `sandbox.kill` | Termine |
| `sandbox.status`, `sandbox.list` | Ce qui vit |

## La règle qui fait la différence

**Le niveau demandé est un plancher, pas un souhait.** Si la machine ne sait pas isoler au niveau
exigé, la tâche ne démarre pas — elle ne démarre pas « à un niveau plus bas en attendant ». Un
agent qui se croit en microVM alors qu'il tourne dans un espace de noms agirait avec une confiance
qui ne correspond à rien.

Le refus porte le rapport complet de ce que la machine sait faire, pour qu'on le comprenne sans se
connecter à la machine.

## Distinguer « absent » de « présent mais refusé »

`sandbox.capabilities` rend séparément `user_namespaces`, `userns_restreint_par_politique` et
`kvm`. Les confondre a coûté une demi-journée (ADR-0006) : Ubuntu 24.04 laisse créer un espace de
noms mais interdit d'y exécuter, et `/dev/kvm` peut exister sans être ouvrable. Une sonde qui
constate une présence n'a rien vérifié.
