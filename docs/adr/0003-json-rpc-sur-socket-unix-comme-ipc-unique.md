# ADR-0003 — JSON-RPC 2.0 sur socket Unix comme IPC unique

- **Statut** : accepté
- **Date** : 2026-09-11
- **Tâche liée** : M1-T5

## Contexte

Les daemons doivent se parler entre eux et avec les agents. MCP, le protocole des outils, est déjà du JSON-RPC 2.0 délimité par lignes sur stdio.

## Décision

Un seul format de message dans tout le système : JSON-RPC 2.0, un message par ligne, sur sockets Unix (`/run/prophet/<daemon>.sock`), pair authentifié par `SO_PEERCRED`, jeton de tâche dans `params._auth`. Spécification : `docs/specs/ipc.md`.

## Alternatives écartées

- Cap'n Proto, protobuf : plus rapides, mais un second codec à maintenir et à exposer aux agents.
- D-Bus : conservé pour la compatibilité avec le bureau existant (AT-SPI), pas pour l'IPC interne.
- varlink : proche, mais sans écosystème côté agents.

## Conséquences

Performance suffisante pour v0 (objectif : 100 000 messages en moins de 5 s). Si un chemin devient critique (flux d'événements haute fréquence), envisager un transport binaire pour ce chemin seul, par ADR.
