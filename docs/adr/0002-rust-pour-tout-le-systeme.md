# ADR-0002 — Écrire tous les nouveaux composants en Rust

- **Statut** : accepté
- **Date** : 2026-09-11
- **Tâche liée** : M0-T2

## Contexte

Les composants nouveaux (broker de capacités, sandbox, FS sémantique, proxy, runtime d'agents) sont des composants système où la sûreté mémoire et la concurrence comptent.

## Décision

Rust édition 2024 pour tout code nouveau. C uniquement pour les patches noyau. Scripts d'outillage en shell ou Python, jamais en production.

## Alternatives écartées

- Go : ramasse-miettes et empreinte inadaptés aux composants à latence contrainte (`cap.check` < 200 µs).
- C/C++ : surface de bugs mémoire incompatible avec les invariants de sécurité.

## Conséquences

Écosystème : `tokio`, `serde`, `rmcp`, `cedar-policy`, `landlock`, `seccompiler`, `rusqlite`. `unsafe` interdit sans justification écrite.
