# ADR 0033 — Un modèle local par défaut dans le système installé

Date : 2026-09-13. Statut : accepté.

## Contexte

`prophet.localEngine.weights` valait `null` : une machine installée n'avait aucun modèle, donc
aucun agent tant que son propriétaire n'avait pas connecté un compte ou posé un fichier GGUF à la
main. Pour « l'OS des agents », c'est une machine vide. Les missions réelles du dépôt sont
prouvées avec Qwen3-1.7B en Q8_0 (trois sur trois, en boucle native et en graphique) ; un
processeur de 2012 à huit cœurs le fait tourner.

## Décision

- La configuration de référence `nixosConfigurations.prophet` pointe `prophet.localEngine.weights`
  sur `pkgs.fetchurl` de Qwen3-1.7B-Q8_0 (1,83 Go, empreinte figée dans le flake). Le
  téléchargement a lieu à l'installation, par `nixos-install`, comme le reste du système ; le
  fichier vit dans `/nix/store`, où le module l'admet.
- L'installeur vérifie que huggingface.co répond avant d'effacer le disque, comme il le fait pour
  le cache Nix, et dit ce qu'il télécharge.
- La variante d'intégration continue `prophet-ci` et le support d'amorçage ne portent pas le
  modèle : la CI ne paie pas 1,83 Go par exécution, et l'ISO reste sous le gigaoctet et demi.
  Le module `local-engine.nix` lui-même ne télécharge toujours rien.

## Conséquences

- Une machine installée a un agent dès le premier démarrage, sans compte ni clé d'API :
  l'invariant « aucune dépendance à une clé API dans le chemin principal » devient vrai sur la
  machine, pas seulement dans le code.
- L'installation dépend d'un site de plus. Une machine qui ne le joint pas est refusée avant
  formatage, avec la raison ; la suite naturelle est un miroir du modèle sur le support
  d'amorçage, quand l'ISO acceptera de grossir d'autant.
- Changer de modèle par défaut se fait en un endroit (`modeleParDefaut` dans `flake.nix`), et la
  même valeur sert à l'essai en machine virtuelle avec poids.
