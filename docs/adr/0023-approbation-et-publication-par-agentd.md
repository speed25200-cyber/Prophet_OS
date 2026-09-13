# 0023 — Publier et annuler depuis agentd, sous l'identité du créateur

- **Statut** : accepté ; écrivain sous l'identité humaine ouvert
- **Date** : 2026-09-13
- **Tâches liées** : M4-T2, M8-T10, M12-T4

## Contexte

L'[ADR 0022](0022-publication-et-conflits.md) livre un moteur de publication en bibliothèque :
index exact, conflits refusés, journal de reprise. Il dit aussi ce qu'il n'est pas : une autorité
d'approbation. Avant cette décision, rien ne reliait l'index examiné dans l'atelier, le créateur
de la mission et une commande qui publie. `prophet task undo` ouvrait l'espace de travail sur
le disque, sans savoir qui demandait ni ce qui avait été examiné ; la surface n'avait aucun
bouton. Les captures d'agentd sont privées : l'humain ne peut de toute façon pas y lire l'index.

## Décision

`agentd` expose `task.apply {id}` et `task.undo {id}`. Les deux exigent l'UID créateur persisté
de la mission, observé sur le socket, et une mission `done` dont le résultat conserve l'index.
`task.apply` relit cet index puis appelle `commit_review` avec la provenance de la mission ;
`task.undo` appelle `undo`. Un espace laissé en `applying` ou `undoing` par une interruption
se reprend par la même commande, jamais par une intention nouvelle. Le succès est consigné
au journal sous l'acteur `user` (`fs.commit`, ou `fs.undo` et `task.rolled_back`), et
l'annulation fait passer la tâche à `rolled_back`. Une seule publication à la fois par service.

`task.inspect` complète sa vue avec l'état SFS relu hors du verrou et deux commandes,
`can_apply` et `can_undo`, offertes au seul créateur. La CLI et l'atelier n'ont plus d'accès
direct au disque pour ces deux gestes.

## Alternatives écartées

- **Publier depuis la CLI sous l'UID humain**, en lisant l'index dans le résultat d'agentd :
  l'espace de travail et ses versions restent dans les captures privées du service ; ouvrir
  leur lecture à l'humain élargirait exactement l'accès que l'ADR 0022 refuse d'élargir.
- **Un nouvel état de tâche « publiée »** : l'état de publication vit déjà dans SFS, avec ses
  reprises et ses conflits ; le dupliquer dans agentd créerait deux vérités. Seule l'annulation,
  définitive, change l'état de la tâche, parce que `rolled_back` existait déjà pour cela.
- **Appliquer automatiquement à la fin d'une mission** : la promesse de l'OS est l'examen
  avant l'écriture dans les documents.

## Conséquences

La publication s'exécute sous l'identité du service. Les tests tournent sous un seul UID ;
sur l'image installée, `agentd` n'a pas `CAP_CHOWN`, et la conservation du propriétaire d'un
document remplacé échouera avant toute mutation. Remplacer un fichier appartenant au
propriétaire n'est donc **pas** livré sur l'image ; il faut un écrivain sous l'identité humaine
ou un droit borné, à décider avant de cocher un critère de FRONTIER. Les droits capd liés
à l'index exact restent aussi à ajouter : aujourd'hui, la vérification est l'identité du créateur
et l'égalité de l'index, pas un jeton. Un conflit laisse SFS en `conflict` avec ses fichiers
déplacés ; l'atelier le montre mais n'offre pas encore de résolution.
