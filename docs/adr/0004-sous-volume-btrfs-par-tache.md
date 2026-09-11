# ADR-0004 — Un sous-volume btrfs par tâche, overlayfs en mode dégradé

- **Statut** : proposé
- **Date** : 2026-09-11
- **Tâche liée** : M4-T1

## Contexte

Chaque tâche d'agent doit pouvoir être visualisée en diff, validée, abandonnée et annulée après coup, de façon transparente pour les applications.

## Décision

`/home/<u>` est un sous-volume btrfs. Chaque tâche travaille dans un snapshot en écriture (`~/.prophet/tasks/<task>/work`), avec un snapshot en lecture seule de départ (`base`). Le commit copie les changements vers l'espace réel après avoir pris un snapshot de restauration. Sur un système de fichiers non btrfs, overlayfs par tâche avec les mêmes API et un `undo` limité au dernier commit.

## Alternatives écartées

- ZFS : licence incompatible avec une distribution du noyau intégrée.
- bcachefs : trop jeune pour la stabilité requise.
- Git sur tout le home : inadapté aux gros binaires et aux fichiers ouverts.

## Conséquences

L'installeur impose btrfs pour `/home`. Les performances de `find-new` doivent être mesurées sur des homes de 500 Go (M4-T2).
