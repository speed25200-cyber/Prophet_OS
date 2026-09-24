# ADR-0057 — Accorder une approbation exige le code d'approbation de l'humain

- **Statut** : accepté (décision de l'utilisateur, 24 septembre 2026)
- **Date** : 2026-09-24
- **Tâche liée** : FRONTIER, « approbations humaines liées à l'action exacte » ; ADR 0044, 0054,
  0056 ; STATUS, « Bloqué » (`approval.resolve`, `task.spawn`)

## Contexte

Depuis l'ADR 0044, `approval.resolve` de capd revient à la classe « humain » : les membres
déclarés du groupe système. Depuis l'ADR 0056, un client officiel en mission ne voit plus capd.
Mais tout programme que l'humain lance lui-même hors mission — un script, un paquet installé, un
outil de sa session — tourne sous la même identité et pouvait **accorder** à sa place une
demande en attente, paiement irréversible compris. `SO_PEERCRED` ne distingue pas la surface
d'un autre programme du même compte, et capd, confiné (`ProtectProc=invisible`), ne voit pas les
processus de la session.

Deux décisions étaient laissées à l'utilisateur ; il les a prises :

- `task.spawn` d'agentd (un manifeste fourni par l'appelant, ce que fait `prophet task new`)
  **reste tel quel** : seuls les programmes de l'humain hors mission l'atteignent, la mission qui
  en naît est planifiée, journalisée et montrée à la supervision, et rien n'y est accordé sans
  capd.
- Accorder une approbation **exige une saisie humaine**.

## Décision

capd garde un **code d'approbation** que l'humain choisit (six caractères au moins). Il n'en
garde que l'empreinte — sel aléatoire, dérivation blake3 répétée — dans son état, en `0600`, que
la session de l'humain ne peut pas lire.

- `approval.presence {code}` (humain) : un code juste rend un **ticket de présence** aléatoire,
  valable dix minutes, pour ce compte. La surface le garde en mémoire : l'humain ne retape pas
  son code à chaque décision, et un autre programme ne peut pas lire la mémoire de la surface.
- `approval.resolve` venant d'un humain avec `decision: allow` exige `ticket` valide, ou `code`
  juste. **Refuser** reste ouvert sans code : un refus ne fait que bloquer, et la voix « refuse »
  doit toujours pouvoir couper.
- Cinq codes faux verrouillent la preuve cinq minutes pour tous ; le verrou et les échecs se
  disent (`approval.code_status`).
- `approval.set_code` : l'humain le définit une première fois, puis ne le change qu'en donnant
  l'ancien. `root` (l'administrateur) peut le remplacer sans l'ancien.
- Sans code défini, accorder est refusé en le disant : « définissez votre code d'approbation ».
- Le service lui-même et `root` (classe « soi ») passent sans code, comme pour toute méthode ;
  les services ne peuvent toujours pas trancher.

La surface demande le code dans la décision quand elle n'a pas de ticket valide ; la CLI le
demande au terminal, sans écho ; `prophet cap code` le définit ou le change.

## Alternatives écartées

- **polkit et le mot de passe de session** : absent de l'image, un agent d'authentification de
  plus dans la session, et capd confiné sans bus système.
- **Reconnaître la surface par son exécutable** : capd ne voit pas `/proc` de la session, et un
  programme de l'humain peut lancer le même exécutable.
- **Exiger le code pour refuser aussi** : un refus ne donne rien ; l'exiger retarderait l'arrêt
  d'une action dangereuse.
- **Un ticket lié au numéro de processus** : invérifiable depuis capd confiné, et réutilisable
  après la fin du processus.

## Conséquences

- Un programme de la session qui ne connaît pas le code ne peut plus accorder ; il peut encore
  refuser (déni de service, pas d'élévation), ou définir le code le premier sur une machine où il
  ne l'est pas encore — l'humain le voit (le sien est refusé) et `root` le remplace.
- Sous Wayland, un programme ordinaire ne lit pas les frappes destinées à la surface ; le code
  tapé dans la décision ne lui est pas visible. Un programme qui pourrait tracer la surface
  (même compte, `ptrace`) sortirait de ce modèle, comme de tout autre.
- Un utilisateur qui oublie son code demande à l'administrateur de le remplacer
  (`sudo prophet cap code --remplacer`).
- Les essais qui lancent tout sous un seul compte (classe « soi ») ne changent pas ; la logique
  (empreinte, verrou, tickets) est éprouvée à part, et le refus sans code sous le compte de
  l'humain par l'essai NixOS des services.
