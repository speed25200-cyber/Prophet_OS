# ADR-0045 — Tenir des microVM prêtes, restaurées d'un instantané d'un invité en attente

- **Statut** : accepté ; vérifié sur le coureur KVM de la CI (`3314b74`, `41def9e`) : microVM rendue en 8,9 ms (médiane de cinq prises, de 8,4 à 10,0 ms)
- **Date** : 2026-09-23
- **Tâche liée** : M5-T4

## Contexte

Le niveau 2 démarre une microVM Firecracker par exécution (ADR 0031, 0038) : un noyau qui
démarre, une racine qui se monte, environ une seconde avant que le programme d'un agent
commence, pour un programme qui dure souvent quelques millisecondes. Le plan demande un pool
de microVM restaurées d'un instantané mémoire, « disponible en moins de 150 ms (p50) quand le
pool est chaud », régénéré en arrière-plan, par profil (`base`, `python`, `node`, `browser`).

Deux contraintes du contrat de l'invité (ADR 0038) : le disque de travail de la tâche n'existe
qu'au moment de l'exécution, et le chemin où le monter est sur la ligne de commande du noyau,
fixée au démarrage.

## Décision

L'invité gagne un mode **réserve** (`prophet.pool=1` sur la ligne de commande) : il démarre
sans tâche, son second disque n'est qu'un disque d'attente de 1 Mio, il dit
« PROPHET_INVITE_ATTENTE » et guette la taille de ce disque. sandboxd (`reserve.rs`) démarre
une fois cet invité, le met en pause, en fait un instantané complet (état et mémoire), et garde
`PROPHET_MICROVM_POOL` machines (2 par défaut) restaurées depuis cet instantané, **en pause** ;
le modèle lui-même est la première. Prendre une machine, c'est construire le disque de la
tâche, le mettre à la place du disque d'attente (`PATCH /drives/travail`) et reprendre la
machine : l'invité voit la taille changer, oublie ce que son noyau gardait du disque d'attente,
lit le chemin de travail écrit sur le disque (`.prophet/workdir`, que l'hôte écrit désormais à
côté du script) et exécute comme à froid, sous les mêmes marques de console. Une machine ne
sert qu'une fois ; le fil de la réserve en restaure une autre. Si la réserve est vide ou qu'une
machine refuse la tâche, l'exécution démarre à froid — toujours une microVM : la réserve
accélère le niveau 2, elle ne le remplace jamais. `sandbox.capabilities` dit l'état de la
réserve (cible, prêtes, dernière durée de restauration, erreur).

Un seul profil, celui de l'invité du dépôt (busybox et Python) : les profils `node` et
`browser` du plan n'ont pas d'invité à restaurer.

## Alternatives écartées

- **Restaurer à la demande** (instantané gardé, aucune machine prête) : chaque exécution
  paierait la restauration et la création d'un moniteur, là où une machine en pause ne coûte
  que la reprise.
- **Garder des machines démarrées sans instantané** : régénérer la réserve coûterait un
  démarrage complet par machine ; l'instantané le fait une fois.
- **Relier le disque de la tâche par un lien symbolique avant la restauration** : le chemin du
  disque est inscrit dans l'instantané, commun à toutes les machines qu'on en tire ; deux prises
  simultanées se le disputeraient.
- **Garder le vsock dans la configuration de réserve** : rien ne l'écoute encore, et un même
  instantané restauré plusieurs fois voudrait lier plusieurs fois le même socket.
- **Un instantané par profil de langage** : il n'existe qu'un invité.

## Conséquences

Toutes les machines de la réserve partent de la même mémoire : même état du générateur
aléatoire du noyau au réveil, sauf si l'hyperviseur signale le clonage (VMGenID) et que le
noyau d'invité le prend en compte — à vérifier sur le noyau épinglé avant d'y confier quoi que
ce soit qui en dépende. L'invité en attente ne détient rien de la tâche ni aucun secret.
L'instantané écrit la mémoire de l'invité (1 Gio) une fois par démarrage de sandboxd, sous
`PROPHET_MICROVM_RESERVE` ou le répertoire temporaire du service ; chaque machine restaurée ne
garde en propre que les pages qu'elle modifie. Le mode réserve suppose que Firecracker accepte
de remplacer un disque sur une machine restaurée ; l'ordre inverse (reprendre, puis remplacer)
est essayé si le moniteur refuse l'ordre direct. La mesure du critère (médiane de cinq prises,
réserve chaude) est l'essai `la_reserve_rend_une_microvm_de_niveau_deux_en_moins_de_150_ms`,
qui n'a de sens que sur une machine où KVM s'ouvre ; sur le coureur de la CI, Firecracker a
accepté le remplacement du disque sur une machine restaurée — l'essai ne dit pas lequel des
deux ordres a servi —, et une prise coûte 8,9 ms, dont l'essentiel pour construire le disque
de la tâche (6,5 ms mesurées sur l'hôte de développement).
