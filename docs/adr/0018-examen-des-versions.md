# ADR-0018 — Conserver et vérifier les versions examinées par l'humain

- **Statut** : accepté
- **Date** : 2026-09-13
- **Tâche liée** : exigences FRONTIER, supervision des fichiers et exécution installée

## Contexte

L'inspecteur indiquait les fichiers préparés, sans permettre de lire leurs modifications.
L'empreinte SFS initiale ne permet pas de reconstruire un document qui a changé depuis.
Une nouvelle lecture de l'original aurait présenté une comparaison avec une autre version.
Les aperçus ajoutent aussi un accès à des contenus que la liste des tâches ne fournit pas.

## Décision

La capture autorisée écrit, dans un même passage, le travail et une copie initiale privée
`base`. Les contrôles de capd et les bornes de capture restent appliqués. À la fin normale
d'une mission, agentd conserve un index des changements et de leurs empreintes avec le résultat,
hors du répertoire `work`. Cet index produit aussi les métadonnées du diff.

`task.change({id,path})` lit les deux versions par descripteurs avec `openat2`, sans suivre les
liens, traverser un montage ni lire un fichier spécial. Les liens physiques sont refusés.
Chaque version est vérifiée contre l'empreinte conservée avant d'être rendue. L'original
actuel n'est pas consulté. Un fichier supprimé qui réapparaît provoque un refus.

La méthode exige l'UID réellement observé sur le socket lors de la création de la mission.
Ce lien est persistant et ne provient pas du champ `user` déclaré. Ni le groupe système ni
l'UID 0 ne reçoivent de dérogation à cette vérification. Les anciennes missions sans propriétaire
observé ou sans versions conservées restent consultables par leurs métadonnées, sans cet aperçu.
La protection des autres méthodes demeure un chantier distinct ; cette décision ne résout pas
leur autorisation globale ni l'accès privilégié de l'administrateur au disque.

Le service autorise deux lectures simultanées, réalisées hors du verrou du runtime. Le client
graphique attend au maximum cinq secondes, garde une seule lecture en vol et écarte les réponses
d'une ancienne sélection. L'actualisation retire immédiatement l'ancien aperçu. Un échec ne
déclenche pas de nouvelle lecture automatique.

Un aperçu contient le texte UTF-8 complet jusqu'à 64 Kio, avec ses espaces et fins de ligne.
Un contenu binaire ou plus grand reçoit une explication explicite. La comparaison de lignes
se calcule hors de la boucle de dessin, conserve le préfixe et le suffixe communs et borne sa
matrice centrale à un million de cellules. Au-delà, le bloc central est présenté dans ses deux
versions avec une indication de comparaison simplifiée. Le rendu ne compose que les lignes
visibles ; la copie restitue le texte exact de la proposition.

## Compatibilité du service installé

La CI de `1640e1c` a chargé Qwen3, puis la capture a échoué avec `ENOSYS`, avant toute génération.
La cause a été reproduite avec le test SFS sous systemd : `RestrictSUIDSGID=yes` refuse
`openat2`, y compris une ouverture en lecture. Le même test passe avec cette seule restriction
désactivée. Le [code de systemd](https://github.com/systemd/systemd/blob/v259/src/shared/seccomp-util.c#L2274)
décrit ce filtrage, ses arguments indirects ne pouvant pas être inspectés par seccomp.

Seul agentd désactive `RestrictSUIDSGID`. Son UID statique non privilégié, ses capacités vides,
`NoNewPrivileges`, les autres filtres et les permissions privées restent en place. Cela retire
un contrôle sur la création des bits SUID/SGID : ce compromis est explicite. La capture assainit
les modes du travail et les outils exposés ne proposent pas de création avec ces bits. Aucun
programme non fiable ne doit être lancé dans agentd ; il doit passer par sandboxd. La nouvelle
exécution de la VM reste nécessaire pour valider l'ensemble des unités, au-delà de cette sonde.

## Alternatives écartées

- Relire l'original au moment de l'examen : il peut avoir changé indépendamment de la mission.
- Accepter la seule liste des chemins : elle ne lie pas le texte aux versions produites.
- Lire sur le thread graphique : un disque lent ou un long texte bloquerait la supervision.
- Remplacer `openat2` par un contrôle de chemin suivi d'une ouverture : cela réintroduirait des
  courses sur les liens. Le refus reste appliqué si l'ouverture sûre est indisponible.

## Conséquences

La capture conserve deux copies du contenu, dans la limite initiale de 512 Mio : l'espace disque
nécessaire augmente. L'index final est borné à 10 000 objets, 64 niveaux et 512 Mio de contenu.
Une lecture peut vérifier jusqu'à 512 Mio par version, avec deux lectures simultanées au maximum.
La persistance du résultat ne constitue pas encore un protocole de récupération des fichiers
après coupure électrique : toute incohérence détectée interdit l'aperçu.

L'examen est en lecture seule. L'application approuvée, la détection des conflits avec les
originaux, l'undo robuste, les checkpoints et la vérification sémantique restent à réaliser.
Cette vue fonctionnelle ne certifie pas la direction graphique finale ni l'objectif SOTA.
