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
noyau d'invité le prend en compte. Sur le coureur de la CI (`4aaeb50`), cinq clones tirent cinq
suites de 16 octets distinctes de `/dev/urandom` : la propriété observable tient ; le mécanisme
qui la donne n'est pas établi par l'essai, qui la vérifie à chaque passage. L'invité en attente ne détient rien de la tâche ni aucun secret.
L'instantané écrit la mémoire de l'invité (1 Gio) sous `PROPHET_MICROVM_RESERVE` ou le
répertoire temporaire du service ; chaque machine restaurée ne garde en propre que les pages
qu'elle modifie. Le mode réserve suppose que Firecracker accepte
de remplacer un disque sur une machine restaurée ; l'ordre inverse (reprendre, puis remplacer)
est essayé si le moniteur refuse l'ordre direct. La mesure du critère (médiane de cinq prises,
réserve chaude) est l'essai `la_reserve_rend_une_microvm_de_niveau_deux_en_moins_de_150_ms`,
qui n'a de sens que sur une machine où KVM s'ouvre ; sur le coureur de la CI, Firecracker a
accepté le remplacement du disque sur une machine restaurée — l'essai ne dit pas lequel des
deux ordres a servi —, et une prise coûte 8,9 ms, dont l'essentiel pour construire le disque
de la tâche (6,5 ms mesurées sur l'hôte de développement). Un autre passage (`4aaeb50`) : réserve
pleine en 5,0 s après le démarrage (amorçage du modèle et instantané de 1 Gio), restauration
d'une machine en 6 ms, prise médiane de 10,9 ms (de 8,8 à 12,1 ms).

## Complément du 23 septembre 2026 : l'instantané survit au redémarrage

Refaire l'instantané à chaque démarrage de sandboxd coûte un démarrage d'invité et l'écriture
d'un gigaoctet, pour un résultat identique tant que rien n'a changé. Sous
`PROPHET_MICROVM_RESERVE` (sur la machine installée, `/var/lib/prophet/sandboxd/reserve`, dans
le répertoire d'état du service, `0700`), la réserve est **persistante** : elle garde son
instantané à l'arrêt, avec son empreinte, écrite en dernier — un amorçage interrompu ne laisse
rien qui se reprenne. L'empreinte dit ce dont l'instantané dépend : chemin, taille et date du
moniteur, du noyau et de la racine d'invité, la configuration de la machine, le noyau et le
processeur de l'hôte (un instantané ne traverse pas à coup sûr un changement de l'un ou de
l'autre). Au démarrage, une empreinte identique fait restaurer la première machine directement
depuis l'instantané gardé ; une empreinte différente, ou une restauration refusée, le fait
refaire, comme avant. Les dossiers des machines d'un démarrage précédent sont retirés, pas
l'instantané. Sans `PROPHET_MICROVM_RESERVE`, rien ne change : tout est temporaire et retiré à
l'arrêt.

Ce que cela ne change pas : l'invité en attente ne détient toujours ni tâche ni secret, et
l'instantané n'est lisible et modifiable que par le service ; qui peut le réécrire peut déjà
remplacer le moniteur du service. L'empreinte n'authentifie pas le contenu, elle évite de
reprendre un instantané périmé. Le critère est l'essai
`la_reserve_reprend_son_instantane_au_redemarrage` (`needs_kvm`) : après un premier
démarrage, l'instantané et son empreinte restent, sans machine ; le second démarrage le reprend
et remplit la réserve en moins d'une seconde, et la machine reprise exécute ; une empreinte
altérée le fait refaire, et la machine refaite exécute aussi. Sur le coureur KVM de la CI
(`094a443`), il est vert : la réserve reprise est pleine en **7,4 ms**, là où le premier
démarrage du même passage la remplit en 4,6 s (amorçage de l'invité et écriture de
l'instantané) ; prise médiane de 9,6 ms, cinq aléas distincts.
