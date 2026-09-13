# SFS — travail de mission et publication des versions

`Workspace::begin_authorized` capture les périmètres autorisés dans un travail privé et conserve
leur contenu initial dans `base`. Les ouvertures Linux refusent les liens et les traversées de
montage ; les droits sont recontrôlés pendant la copie. Le parcours est borné à 10 000 objets,
64 niveaux et 512 Mio de contenu initial. Le moteur utilise des copies, même sur btrfs.

Après les outils natifs, `Workspace::seal_review` produit un `ReviewIndex` à conserver hors du
travail. `ReviewIndex::diff` donne les changements ; `read(home, task, path)` restitue leurs deux
versions après vérification des empreintes, tailles et permissions. L'original actuel n'est
jamais relu par cette méthode. Le texte intégral est limité à 64 Kio par version ; les contenus
binaires ou trop grands sont signalés explicitement.

`commit_review(index, now, provenance)` publie uniquement les versions correspondant encore
à l'index fourni. Il contrôle tout le lot, conserve les originaux et propositions, puis écrit
un journal synchronisé avant chaque échange de noms. `undo()` compare les fichiers aux versions
réellement publiées : une modification humaine ultérieure provoque un refus. Les métadonnées
humaines sont conservées, dont les attributs, ACL, UID, GID et date de modification initiale.

`recover_publication()` reprend l'intention enregistrée après interruption. `Workspace::open`
et `list` lisent son état, y compris `applying`, `undoing` et `conflict`. Une reprise déjà terminée
ne réécrit pas les documents. Les anciennes publications sans journal vérifiable ne sont pas annulées.

Ces méthodes ne délivrent aucune autorisation. Le consentement, l'identité de l'appelant et les
droits doivent être contrôlés avant publication. Le raccordement installé à agentd et aux commandes
graphiques reste à faire sous l'identité humaine. Aucun droit d'écriture supplémentaire n'est accordé
à agentd. Les anciennes API `Transaction` restent distinctes et ne doivent pas recevoir de chemins
non fiables ni être présentées comme une transaction multi-fichiers atomique.

Un échange de fichier est atomique ; l'ensemble du lot ne l'est pas. Un conflit détecté avant
publication laisse les documents en place. Lors d'une édition arrivée pendant l'échange, le
fichier humain déplacé est conservé dans `displaced/`, mais une partie du lot peut être visible.
La récupération demande alors un examen explicite. Les répertoires parents créés et les copies
privées restent présents après annulation ; leur nettoyage n'est pas implémenté.

```sh
nix develop --command cargo test -p sfs
```

Voir les [versions examinées](../../docs/adr/0018-examen-des-versions.md), la
[publication et ses limites](../../docs/adr/0022-publication-et-conflits.md) et le
[rapport de validation](../../docs/reports/publication-2026-09-13.md).
