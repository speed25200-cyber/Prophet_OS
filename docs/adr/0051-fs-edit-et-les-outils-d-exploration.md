# ADR-0051 — `fs.edit`, et les contextes de l'image qui explorent leur portée

- **Statut** : accepté
- **Date** : 2026-09-23
- **Tâche liée** : M3 (outils système), M8 (contextes de l'image), M13-T1 (banc M13)

## Contexte

Sur les trois passages du banc M13, deux tâches n'ont jamais réussi, ni par Prophet ni par la
boucle nue : « corriger-une-faute » (remplacer « sincèrment » par « sincèrement » sans toucher au
reste) et « changer-le-port » (passer un port à 9090 dans un fichier TOML). Le modèle répond
qu'il « ne peut pas modifier un fichier » : le seul moyen de changer un fichier est de le
réécrire en entier avec `fs.write`, ce qu'un petit modèle n'ose pas ou fait mal (troncature,
fautes de recopie). Les agents de code ont convergé vers un autre geste : remplacer un passage
exact.

Par ailleurs, les contextes que l'image installe (`image/modules/local-engine.nix`) n'accordent
que `fs.read`, `doc.read` et `fs.write` parmi les outils fichiers : un agent n'y peut ni lister
sa portée, ni chercher un fichier, ni connaître la taille d'un document, alors que le catalogue
d'exemple (`examples/missions/profils-locaux.json`) et le banc les accordent.

## Décision

- **`fs.edit {path, old, new, all?}`** remplace un passage exact d'un fichier texte et écrit le
  résultat dans l'espace de travail de la mission, comme `fs.write` : même droit (`fs.write`
  sur le chemin, vérifié avant toute lecture), même confinement, même journal, même
  publication explicite. Le passage doit apparaître une seule fois ; sinon l'outil le dit (« il
  apparaît 2 fois : allongez-le, ou passez all: true ») sans rien écrire. Introuvable, il le dit
  aussi (« relisez-le avec fs.read »). Fichier de plus de 1 Mio ou qui n'est pas du texte UTF-8 :
  refusé, pour ne jamais réécrire des octets que l'outil n'a pas lus fidèlement. Deux éditions
  successives partent de la copie de travail.
- **Les contextes de l'image explorent leur portée** : chaque contexte qui lit reçoit `fs.list`
  sur les mêmes motifs, et les outils `fs.list`, `fs.stat`, `fs.search` et `fs.edit`. Le
  catalogue de l'image admet `fs.edit` parmi les outils fichiers.
- **Un nom introuvable se cherche dans le contenu** : au quatrième passage du banc, un seul
  `fs.search` qui ne rend rien (« aucun fichier ne contient la référence ZX-99417 », alors
  qu'un fichier la contient) suffit au modèle pour conclure. Quand `name_contains` seul ne
  correspond à aucun nom, `fs.search` cherche le même texte dans le contenu, sous les mêmes
  droits et bornes, et `note` le dit.
- **Le banc l'offre des deux côtés** : même implémentation, même ordre, même description ; il
  relève aussi, pour la boucle nue, les arguments abrégés de chaque appel (le journal de
  Prophet n'en garde que l'empreinte, et c'est voulu).

- **Complément (neuvième passage du banc) : `fs.copy {from, path}`.** « ranger-par-annee »
  n'a jamais réussi : ranger des factures dans des sous-dossiers exige de les copier, et aucun
  outil ne le permettait sans les relire et les réécrire en texte — ce qui corromprait un vrai
  PDF. `fs.copy` copie un fichier octet pour octet dans l'espace de travail : la source se lit
  sous `fs.read`, la destination s'écrit sous `fs.write` (les deux sont vérifiés), les dossiers
  sont créés, 64 Mio au plus ; rien ne quitte l'espace de travail avant la publication. Les
  contextes de l'image, le catalogue d'exemple et le banc l'offrent.

## Alternatives écartées

- **Un diff unifié à appliquer** : exact, mais un petit modèle produit rarement un diff valide ;
  un passage à remplacer est ce qu'il sait recopier.
- **Remplacer par numéro de ligne** : fragile dès que le fichier change entre la lecture et
  l'édition ; le passage exact se vérifie.
- **Laisser `fs.write` seul** : la réécriture complète reste possible, mais c'est elle qui
  échoue.

## Conséquences

- `fs.edit` est un outil de plus dans chaque registre (missions natives, séances MCP des
  clients officiels), offert seulement là où un profil l'accorde.
- Le banc mesurera si la correction de faute et le changement de port deviennent possibles.
