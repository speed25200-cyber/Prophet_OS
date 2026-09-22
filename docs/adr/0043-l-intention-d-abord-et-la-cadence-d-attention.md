# ADR-0043 — Écrire l'objectif dès l'accueil, tout faire au clavier, ralentir le champ quand personne n'agit

- **Statut** : accepté ; à apprécier par l'utilisateur sur écran physique
- **Date** : 2026-09-22
- **Tâche liée** : FRONTIER, critère « Interface native » (navigation clavier, redimensionnement,
  consommation au repos) ; complète l'ADR 0025 (direction Réacteur)

## Contexte

L'audit visuel du 22 septembre, fait sur les captures du binaire en rendu logiciel, relève
trois défauts et deux manques de la direction Réacteur :

- à 1280, 1440 et 1920 px de large, la plaque de l'espace de mission dépassait de 10 à 22 px
  la colonne que définissent les commandes de la page : la rangée horizontale ajoutait son
  espacement après la liste, puis après la colonne du titre, et la largeur ne le comptait pas ;
- le libellé « ÉTAPES » des lignes de mission touchait le trait qui les sépare et les
  crochets de la sélection ;
- l'écran vide demandait « Que voulez-vous accomplir ? » sans permettre d'y répondre :
  il fallait trouver le bouton « Nouvel objectif », puis retaper l'objectif dans un autre écran ;
- seule la recherche avait un raccourci (Ctrl K) : le critère FRONTIER exige une navigation
  au clavier ;
- avec des missions actives, le champ vivant redessine à la cadence de l'écran même quand
  l'humain ne touche à rien. Sur rastériseur logiciel, `--repos` mesurait 32 images/s et
  118 % d'un cœur pour une surface que personne ne manipulait.

## Décision

**L'objectif s'écrit là où la question est posée.** L'écran vide porte un champ
(`intention-accueil`). Entrée, ou « Préparer », ouvre la préparation habituelle avec cet
objectif déjà écrit ; l'humain y choisit le contexte et le modèle et examine le plan avant
tout lancement. Taper ne soumet rien et n'interroge aucun modèle. Une tentative de préparation
gardée pour sa reprise n'est jamais écrasée : le brouillon de l'accueil attend, intact.

**Le clavier suffit.** Ctrl 1 à 4 ouvrent les pages, Ctrl N un nouvel objectif (son champ
reçoit aussitôt le clavier), Échap referme l'examen d'une décision ou la préparation. Échap ne
ferme rien tant qu'un champ avait le focus à l'image précédente : il lui rend d'abord la main,
et le brouillon est conservé. Refermer l'examen n'accorde ni ne refuse rien (ADR 0011). La
barre d'état rappelle ces raccourcis sur les grands écrans.

**Le champ ralentit quand personne n'agit.** Après trente secondes sans geste (pointeur,
clavier, défilement, toucher), la cadence du champ passe de 60 à 20 images/s sur une carte
graphique, et de 30 à 10 sur un rastériseur logiciel (`bureau::cadence_du_champ`). Le premier
geste rétablit la pleine cadence. Le mouvement des rubans suit l'horloge et non le nombre
d'images : l'état montré reste exact, seul son lissé baisse.

**Les plaques tiennent dans leur colonne, au pixel.** La largeur de l'espace de mission compte
l'espacement de la rangée et le trait de la plaque ; la colonne du titre réserve au cadran sa
place et son espacement. Les plaques nommées retiennent leur rectangle à chaque image
(`Bureau::plaque`) et les parcours le vérifient : l'espace de mission s'arrête à 1,5 px près
au bord des commandes, avec ou sans Focale, à trois tailles ; l'écran vide tient entier à
1440 × 1000 (ses trois étapes passent en colonnes sur grand écran).

## Alternatives écartées

- **Préparer le plan dès Entrée sur l'accueil** : l'humain n'aurait vu ni le contexte ni le
  modèle avant la demande au service ; la préparation reste l'écran où l'on choisit.
- **Échap qui ferme tout de suite** : un Échap pour quitter un champ fermerait la préparation
  en cours de rédaction.
- **Couper le champ au repos** : une mission qui avance doit se voir avancer, même sans geste.
- **Un délai plus court avant ralentissement** : l'humain qui regarde ses agents travailler
  sans toucher à rien est le cas d'usage ; trente secondes laissent le temps de lire.

## Conséquences

Les parcours du bureau passent de 11 à 14 : colonne des plaques, objectif de l'accueil,
clavier seul. Les captures Réacteur sont régénérées par `just captures`, qui produit désormais
les noms que le rapport cite. Mesures en rendu logiciel (llvmpipe) dans le
[rapport du jour](../reports/reacteur-seconde-passe-2026-09-22.md) : 60 s de repos avec trois
missions actives passent de 32 à 21,5 images/s en moyenne et de 118 % à 77 % d'un cœur
(environ 36 % une fois ralenti). Les mêmes mesures sur une carte graphique réelle restent à
faire avec `prophet-surface --mesure` et `--repos`.
