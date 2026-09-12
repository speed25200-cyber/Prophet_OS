# 0010 — Direction visuelle Iris pour l'espace natif

Date : 13 septembre 2026. Statut : implémenté et vérifié localement à la demande de l'utilisateur.

L'interface précédente présente les commandes comme un tableau d'administration, avec de
grandes zones vides et une hiérarchie visuelle trop uniforme. L'accueil devient un espace de
création : composition centrée, typographie Inter, sculpture irisée dessinée par le moteur
natif, dock flottant avec pictogrammes, fond lumineux et surfaces aux contours doux. Les pages de conversation,
de modèles et d'activité partagent le même vocabulaire.

La zone de saisie est intégrée à l'accueil ; une conversation conserve son compositeur en bas.
Les décorations n'indiquent aucune mesure de charge ou capacité inventée. Les états du moteur
et les limites de session restent lisibles. Le mouvement réduit fige la sculpture ; le dessin
ne doit effectuer aucune requête réseau ni charger une ressource graphique distante.

Vérification exécutée avant livraison :

```sh
nix develop --command cargo test -p surface -- --ignored --test-threads=1 --nocapture
nix develop --command cargo run -p surface --bin prophet-surface -- \
  --capture iris-1440.png --largeur 1440 --hauteur 900 --demonstration
nix develop --command cargo run -p surface --bin prophet-surface -- \
  --capture iris-640.png --largeur 640 --hauteur 480 --demonstration
nix develop --command just check
```

Les captures ont été relues après rendu, y compris une conversation réelle, les modèles
indisponibles et une décision. Les essais conservent la navigation, la saisie Unicode,
le collage et les contrôles d'approbation. Les dix tests graphiques passent ; `just check`
réussit avec 576 tests, aucun échec et 18 ignorés. Le
[rapport Iris](../reports/interface-iris-2026-09-13.md) contient les captures, les conditions
de reproduction et les limites de cette validation.
