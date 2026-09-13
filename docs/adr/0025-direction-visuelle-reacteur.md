# 0025 — Direction visuelle Réacteur : la nuit, un accent au choix, et un champ qui ne ment pas

- **Statut** : accepté ; à apprécier par l'utilisateur sur écran physique
- **Date** : 2026-09-13
- **Tâches liées** : FRONTIER, critère « Interface native » ; remplace la présentation de l'ADR 0019

## Contexte

L'utilisateur demande un espace « jamais vu dans aucun OS », tourné vers la science-fiction et
le futur, fluide et optimisé. Une première référence donnait un fond de nuit, un mot-marque à
l'œil, une vague de particules d'or et des cartes de verre sombre. La seconde demande précise :
plus d'air, plus professionnel, plus spectaculaire, l'impression d'un tableau de bord de
science-fiction, et une couleur personnalisable. L'atelier de l'ADR 0019 était clair, graphite
et minéral : lisible, mais il ne ressemblait à rien de ce qui est demandé.

Les références de ce genre montrent aussi des chiffres inventés. Prophet OS n'affiche rien que
les services ne fournissent. La direction retient la forme et refuse la fiction.

## Décision

**La nuit est le fond, l'accent signale, l'alerte est réservée à l'humain.** Les neutres
(`theme::palette`) ne changent jamais : une nuit à peine froide, des plaques de verre qui
laissent passer le champ, une encre claire, une seule couleur d'alerte pour ce qui exige un
humain, une menthe pour ce que le service dit accompli. L'**accent** (`theme::Accent`) colore
tout ce qui signale — champ, fils, crochets, lueurs, ce qui est actif ou commande — et il est au
choix de la personne : *Arc* (cyan, par défaut), *Or*, *Plasma*, *Jade*, *Nacre*. Un test
vérifie que chaque accent contraste sur le verre et qu'aucun ne se confond avec l'alerte.
Le choix se fait dans la page Système, se conserve dans `$XDG_CONFIG_HOME/prophet/surface.json`,
et se force par `--accent` ou `PROPHET_SURFACE_ACCENT`. Un fichier illisible rend l'accent par
défaut plutôt qu'une erreur : une faute de frappe ne doit pas empêcher l'écran de s'ouvrir.

**Le champ est vivant, et il est vrai.** Derrière les plaques, le module GPU `champ` dessine
trois populations dans la même passe que l'interface, avant elle : une grille de points fixes,
plus présente au centre ; une voûte de grains fixes, dense le long d'une houle ; et les rubans
des missions reçues des services, au plus dix, la mission choisie et ce qui réclame l'humain
d'abord, puis les plus vives. Chaque ruban est tissé de treize fils qui avancent à la vitesse
réelle des étapes ; sa clarté est le budget qui reste ; sa teinte passe à l'alerte quand une
décision attend ; la mission choisie s'éclaire. Un ruban arrêté est immobile : l'arrêt se voit,
il ne se lit pas. Le grain d'un fil suit la particule, jamais l'horloge, pour qu'une mission
arrêtée ne scintille pas. Sous mouvement réduit, l'horloge du champ est figée. Le bureau ne
demande un redessin à la cadence de l'écran que tant qu'un ruban avance.

**Le tableau de bord dit tout par des formes réelles.** Le module `hud` fournit les plaques de
verre à crochets d'angle et bord gradué, les jauges à couronne de graduations, le cadran d'une
mission — une graduation par étape franchie, l'arc du budget consommé, le nombre d'étapes au
centre —, les relevés de la barre du système, les boutons en capitales espacées et les lueurs.
La barre du système ne relève que des comptes reçus : missions actives, missions à examiner,
niveau d'isolation, modèles découverts. Le rail porte l'anneau des missions actives autour de
leur nombre. Inter est la seule police, en trois graisses du même fichier variable : fine pour
les grands chiffres et les titres, régulière pour lire, demi-grasse pour ce qui doit tenir.

**La composition respire.** La liste des missions, virtualisée, tient à gauche sur sa plaque ;
l'espace de mission à droite, avec de l'air entre les deux ; la Focale retire la liste. À petite
largeur, la liste et l'espace de mission se remplacent. L'espace vide est une seule plaque, à
gauche, avec la question « Que voulez-vous accomplir ? ». Les filtres, la recherche Ctrl+K,
l'inspecteur, ses onglets, l'examen d'une décision et toutes les commandes gardent leurs
identifiants et leur logique : les parcours existants les vérifient inchangés.

## Conséquences et limites

Le champ ajoute au plus 3 000 grains, 2 800 points de grille et 60 000 particules par image,
tracés en un seul appel instancié ; le tampon des rubans a une taille fixe et le groupe de
liaison n'est jamais reconstruit. Sur un rastériseur logiciel, que wgpu déclare comme
périphérique de type processeur, le champ s'allège de lui-même (1 600 particules par ruban,
1 200 grains, 30 images par seconde) ; `--champ-complet` rétablit le champ entier. La fenêtre
ne redessine un écran que si l'empreinte de la scène a changé ou si l'interface l'a demandé.
`prophet-surface --mesure N` donne, sur n'importe quelle machine, les temps par image et la
mémoire résidente attendus par le critère d'interface ; les chiffres de cette session, en
rendu logiciel, sont dans le rapport. La fluidité et la consommation sur une carte graphique
réelle restent à mesurer avec cette commande.

Rien n'est affiché qui ne vienne d'un service. Les captures de démonstration portent la mention
« DÉMONSTRATION » et leur relevé de modèles est un tiret. L'anneau du rail, les relevés, le
cadran et les rubans ne comptent que des missions réellement reçues. La graduation du bord d'une
plaque et la grille sont des repères d'échelle fixes, pas des mesures. Cette direction ne prouve
ni une équivalence avec une interface commerciale, ni un statut SOTA ; sa qualité visuelle sur
un écran physique reste à apprécier par l'utilisateur. Le renderer historique (`--observation`)
et ses tests de propriétés sont conservés tels quels.
