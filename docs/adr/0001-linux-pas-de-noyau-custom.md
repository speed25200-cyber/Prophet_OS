# ADR-0001 — Utiliser le noyau Linux LTS, ne pas écrire de noyau

- **Statut** : accepté
- **Date** : 2026-09-11
- **Tâche liée** : M0-T5

## Contexte

Le projet vise un OS conçu pour les agents IA. La question d'un noyau custom (Rust, micro-noyau, seL4) s'est posée.

## Décision

Noyau Linux LTS, compilé sur mesure avec une configuration minimale. Tout l'espace utilisateur est écrit de zéro. Voir `docs/PLAN.md` section 3.

## Alternatives écartées

- Noyau custom : 5 à 10 ans de pilotes, aucun gain pour l'agent.
- seL4 : pas de pilotes desktop ni GPU ; conservé comme piste pour l'hyperviseur en phase 4.
- Fuchsia, Redox : écosystème insuffisant pour un PC.

## Conséquences

Compatibilité matérielle égale à celle de Linux. Les mécanismes de sécurité reposent sur Landlock, seccomp, cgroups v2, namespaces, KVM. Revisiter à la phase 4.
