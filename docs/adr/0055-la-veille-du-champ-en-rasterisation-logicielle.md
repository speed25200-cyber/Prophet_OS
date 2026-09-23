# ADR-0055 — Figer le champ en veille sur un rastériseur logiciel

- **Statut** : accepté
- **Date** : 2026-09-23
- **Tâche liée** : M13 (audit visuel et fluidité de la surface) ; complète ADR 0043

## Contexte

ADR 0043 ralentit le champ vivant après trente secondes sans geste (60 → 20 images/s sur une
carte graphique, 30 → 10 sur un rastériseur logiciel) et écarte « couper le champ au repos » :
une mission qui avance doit se voir avancer. L'audit de fluidité du 23 septembre mesure ce que
cela coûte sur une machine sans carte graphique (llvmpipe, quatre cœurs, scène d'exemple, trois
missions actives, 1920 × 1080) :

- l'interface elle-même coûte peu : 0,45 à 0,6 ms de processeur par image sur les quatre pages,
  y compris en 2560 × 1440 (composition, tessellation, soumission) ;
- le reste est le tracé logiciel du champ : 24 à 28 ms par image en champ complet, 13 ms en
  champ allégé ;
- au repos, cadence ralentie comprise, le champ garde **37 % d'un cœur** indéfiniment.

Sur cette machine, le moteur local tourne lui aussi sur le processeur : chaque image du champ
est prise aux missions qu'il montre. Sur une carte graphique, le même champ devrait coûter au
processeur de l'ordre de 1 à 2 % d'un cœur à 20 images/s (0,6 ms par image, plus le pilote) :
c'est une estimation, faute de carte dans l'environnement de mesure, mais le problème ne s'y
pose pas dans les mêmes termes.

## Décision

- **Sur un rastériseur logiciel, après deux minutes sans geste (`bureau::VEILLE`), le champ se
  fige** comme sous mouvement réduit. L'écran ne se redessine plus que lorsque l'état des
  missions change (la relecture des services, quatre fois par seconde, compare l'empreinte de
  la scène) : étapes, budget, états et décisions restent exacts à la seconde près ; seul le
  défilement des rubans s'arrête.
- **Le champ se fige là où il est et repart de là** au premier geste (`HorlogeDuChamp`) : son
  horloge est celle de l'interface moins le temps passé en veille, sans saut ni retour à zéro.
- **Sur une carte graphique, rien ne change** : ADR 0043 s'applique, le champ ne se fige jamais.
- `prophet-surface --repos N` détaille désormais les phases (pleine cadence, cadence ralentie,
  veille) avec leurs images et leur temps processeur, et `--mesure` sépare le travail du
  processeur de l'attente du GPU.

## Alternatives écartées

- **Garder 10 images/s indéfiniment** : 37 % d'un cœur pris au moteur local pour un défilement
  que personne ne regarde depuis deux minutes.
- **Baisser encore la cadence (2 à 5 images/s)** : un ruban qui saccade se lit comme une panne ;
  une image immobile et exacte se lit comme un repos.
- **Réduire les particules en veille** : le coût reste proportionnel au temps, et le champ
  changerait d'aspect sous les yeux de qui revient.
- **Figer aussi sur une carte graphique** : le gain estimé est de 1 à 2 % d'un cœur, et la règle
  d'ADR 0043 (« une mission qui avance doit se voir avancer ») y reste tenable.

## Conséquences

- Mesuré (150 s, llvmpipe, 1920 × 1080) : voir `docs/reports/audit-visuel-2026-09-23.md`.
- Le délai de veille est une constante ; s'il faut le régler par machine, il rejoindra le choix
  d'accent dans la configuration de la surface.
- À revisiter si la surface apprend à connaître la charge du moteur local : la veille pourrait
  alors ne se déclencher que lorsque le moteur génère.
