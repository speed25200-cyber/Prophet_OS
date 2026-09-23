# ADR-0053 — `calc.eval` : le calcul exact, rendu au modèle par le système

- **Statut** : accepté
- **Date** : 2026-09-23
- **Tâche liée** : M3 (outils système), M13-T1 (banc M13)

## Contexte

Sur cinq passages du banc M13, « total-des-ventes » n'a jamais réussi, d'aucun côté : le modèle
lit bien la colonne `montant` (100, 125, 200) et écrit 325, 330, 3 305 ou 33 450 au lieu de 425.
« le-plus-gros-achat » échoue de même sur une comparaison. Un petit modèle recopie les nombres
et les combine mal ; le calcul exact est une tâche que le système sait faire.

## Décision

- **`calc.eval {expression?, numbers?}`**, outil pur de `mcp-system` : une expression
  arithmétique (nombres, `+ - * /`, parenthèses, signes, `×` et `÷`, virgule décimale acceptée)
  rend sa valeur ; une liste de nombres rend `sum`, `count`, `mean`, `min` et `max`. Les
  entiers restent entiers, le reste est arrondi à douze chiffres significatifs (pour ne pas
  montrer 0,30000000000000004). Une expression mal formée, une division par zéro, un résultat
  non fini ou plus de 64 parenthèses imbriquées sont dits (`Invalid`), sans rien calculer.
- **Il ne touche à rien** : ni fichier, ni réseau, ni état ; il n'exige que `tool.call` sur son
  nom, comme `clock.now`.
- **Offert là où l'on travaille** : les contextes de l'image et le catalogue d'exemple
  l'accordent avec les outils fichiers, le catalogue de l'image l'admet, et le banc l'offre des
  deux côtés.

## Alternatives écartées

- **Une commande sandboxée (`proc.exec` d'`awk`)** : juste, mais elle exige un contexte qui
  exécute des programmes, un niveau d'isolation et une approbation ; pour une somme, c'est
  disproportionné.
- **Un interpréteur d'expressions complet** (fonctions, variables) : plus de surface, pour un
  besoin qui se limite aux quatre opérations et aux agrégats d'une liste.

## Conséquences

- Le banc dira si « total-des-ventes » et « le-plus-gros-achat » deviennent possibles quand le
  modèle peut déléguer le calcul.
