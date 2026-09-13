# ADR-0019 — Un atelier de missions et une Focale pour les examiner

- **Statut** : accepté
- **Date** : 2026-09-13
- **Tâche liée** : FRONTIER, interface de supervision humaine des agents

## Contexte

La supervision présentait une liste étroite et un inspecteur. Cette organisation accordait peu
de place au résultat et conservait le même niveau de détail pendant toute la mission. La
direction visuelle demandée exige aussi une composition plus affirmée que ces panneaux pâles.

## Décision

Le bureau devient un atelier : navigation verticale graphite, fond minéral statique et surfaces
de travail claires. Les glyphes sont vectoriels et la police Inter reste embarquée. Le décor
ne reçoit aucune animation au repos. À petite largeur, les quatre destinations occupent une
barre inférieure ; le contexte remplace la liste après sélection.

Sur une grande fenêtre, plusieurs missions occupent une galerie horizontale. Chaque objet porte
le titre, le modèle ou pilote, l'état et le nombre d'étapes provenant de la scène. La sélection
pilote le même inspecteur de service. La **Focale** retire la galerie pour agrandir le plan,
la proposition ou les fichiers. Une mission seule reçoit directement cet espace. La préparation
réussie ouvre le plan en Focale et efface la recherche précédente pour ne pas le masquer.

Les filtres et la recherche rétablissent la galerie. **Ctrl+K** concentre le clavier dans une
recherche locale sur le titre, la référence ou le pilote ; ce raccourci ne soumet aucune demande
à un modèle. Le changement de sélection conserve la référence exacte et rend sa carte visible.
La galerie horizontale ne compose que les objets proches du rectangle visible. Le filtrage
et la copie de la scène restent proportionnels au nombre de missions reçues.

La Focale est un changement de présentation. Elle n'accorde aucun droit et n'exécute aucune
action. Le lancement, l'arrêt et les choix humains restent des contrôles distincts reliés aux
contrôleurs existants. Une décision capd reste accessible depuis chaque page.

Le bureau egui mélange ses couleurs prémultipliées dans une vue `Unorm`, comme le prévoit
son renderer. La texture des captures et les surfaces sRGB autorisent une vue compatible sans
conversion sRGB automatique. Le renderer historique conserve sa vue et son mélange linéaire.
Le test GPU de blanc translucide sur blanc rendait `[220, 220, 220, 255]` avant ce changement ;
il exige maintenant un blanc d'au moins 253 par canal. Le choix respecte aussi le format BGRA
ou RGBA négocié avec la fenêtre. Cette correction évite l'assombrissement des transparences
et des pixels d'anticrénelage, sans appliquer un filtre aux images capturées.

## Conséquences et limites

La surface conserve son moteur de rendu natif et ses transports réels. Ce changement ne crée
ni relations fictives entre agents, ni mesure de progression déduite du décor. Le nombre de
modèles indiqué dans la barre désigne leur découverte pour le dialogue ; il ne garantit pas
la disponibilité du moteur des missions, configuré séparément.

Le panorama réduit l'espace vertical de l'inspecteur quand plusieurs missions sont présentes ;
la Focale lui rend cet espace. Les documents longs restent défilables. La liste compacte n'est
pas encore virtualisée. Les tests sur mille objets contrôlent le rendu et la recherche, pas
l'exécution simultanée de mille agents.

Cette direction doit encore être appréciée visuellement par l'utilisateur et mesurée dans la
session installée. Elle ne constitue pas une preuve de qualité équivalente à Apple, de fluidité
sur matériel réel ou d'OS SOTA. Le bureau multiapplication, la validation des fichiers, l'undo
et les sessions authentifiées des clients officiels restent des exigences distinctes.
