# `sandboxd` — isolation graduée

Trois niveaux, du moins coûteux au plus isolant : 0, confiné (espaces de noms, racine minimale,
seccomp) pour les outils système de confiance ; 1, noyau utilisateur (gVisor) pour
les agents qui manipulent des données non fiables ; 2, microVM (Firecracker sur KVM) pour toute
exécution de code arbitraire. Le niveau atteignable dépend de la machine : la sonde le dit, et
le gestionnaire refuse d'exécuter à un niveau qu'il ne peut pas garantir plutôt que de dégrader.

La sonde Landlock constate l'ABI disponible, mais `apply_landlock` ne pose actuellement
aucune règle et retourne `false`. La racine minimale et seccomp restent les protections
appliquées ; Landlock n'est pas une garantie livrée.

Les commandes synchrones `sandbox.run` restent visibles et contrôlables pendant leur
exécution, comme celles de `sandbox.start` : gel global, dégel et arrêt. La régression
`une_commande_run_est_visible_gelable_et_arretable` exige les espaces de noms utilisateur.

Les tests matériels portent les marqueurs `needs_kvm`, `needs_gvisor` ; la CI les exerce sur
un coureur muni de KVM. Voir le [contrat du service](../../docs/components/sandboxd.md).

```sh
cargo test -p sandboxd
cargo test -p sandboxd -- --ignored   # niveaux 1 et 2, sur une machine qui les offre
```
