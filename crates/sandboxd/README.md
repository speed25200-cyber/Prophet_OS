# `sandboxd` — isolation graduée

Trois niveaux, du moins coûteux au plus isolant : 0, confiné (espaces de noms, racine minimale,
Landlock, seccomp) pour les outils système de confiance ; 1, noyau utilisateur (gVisor) pour
les agents qui manipulent des données non fiables ; 2, microVM (Firecracker sur KVM) pour toute
exécution de code arbitraire. Le niveau atteignable dépend de la machine : la sonde le dit, et
le gestionnaire refuse d'exécuter à un niveau qu'il ne peut pas garantir plutôt que de dégrader.

Les tests matériels portent les marqueurs `needs_kvm`, `needs_gvisor` ; la CI les exerce sur
un coureur muni de KVM. Voir le [contrat du service](../../docs/components/sandboxd.md).

```sh
cargo test -p sandboxd
cargo test -p sandboxd -- --ignored   # niveaux 1 et 2, sur une machine qui les offre
```
