# SFS — travail de mission et versions

`Workspace::begin_authorized` capture les périmètres autorisés dans un travail privé et conserve
leur contenu initial dans `base`. Les ouvertures Linux refusent les liens et les traversées de
montage ; les droits sont recontrôlés pendant la copie. Le parcours est borné à 10 000 objets,
64 niveaux et 512 Mio de contenu initial. La copie initiale supplémentaire augmente l'espace requis.

Après les outils natifs, `Workspace::seal_review` produit un `ReviewIndex` à conserver hors du
travail. `ReviewIndex::diff` donne les changements ; `read(home, task, path)` restitue leurs deux
versions après vérification des empreintes, tailles et permissions. L'original actuel n'est
jamais relu par cette méthode. Le texte intégral est limité à 64 Kio par version ; les contenus
binaires ou trop grands sont signalés explicitement.

L'autorisation du lecteur appartient à agentd. Les anciennes captures sans `base` ne fournissent
pas ces aperçus. Les API historiques de commit/undo ne constituent pas un chemin approuvé pour
les missions installées : leur durabilité, concurrence et respect des changements humains
restent à sécuriser avant raccordement à la surface.

```sh
nix develop --command cargo test -p sfs
```

Voir l'[ADR 0018](../../docs/adr/0018-examen-des-versions.md).
