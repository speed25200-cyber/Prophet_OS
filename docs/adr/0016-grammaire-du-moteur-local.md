# ADR 0016 — Corriger la cardinalité des appels dans le moteur local

- Date : 13 septembre 2026
- Statut : adopté

## Contexte

Trois missions Qwen3-0.6B échouaient avec `finish_reason=length`. Le pilote refusait
correctement les générations incomplètes. Une trace synthétique détaillée montre que le
moteur écrit un appel, puis produit des retours à la ligne jusqu'au plafond.

Le paquet llama.cpp du nixpkgs épinglé (`0.4.0`, build `b10809-5266f24`) construit une
grammaire dont la répétition porte sur un appel optionnel. Cette répétition autorise une
suite vide d'appels et des espaces sans borne. Elle est aussi appliquée quand le client
demande `parallel_tool_calls=false`. Des tests indépendants du modèle reproduisent ces
violations sur le parseur et sur la grammaire du moteur.

## Décision

Publier `packages.llama-cpp` avec un correctif limité au générateur de grammaire JSON
à balises. Chaque élément répété exige un appel. Le mode séquentiel admet un seul appel ;
le mode parallèle conserve plusieurs appels. Le caractère optionnel porte sur la séquence
complète en mode `auto`, pour conserver les réponses finales sans outil.

Conserver les poids, le template, les backends et les paramètres de compilation de nixpkgs.
Le test `checks.x86_64-linux.llama-tool-grammar` compile contre les sources du paquet et
exerce ses bibliothèques réelles : modes auto/required et séquentiel/parallèle, appels
complets ou tronqués, répétitions d'espaces et cardinalité du parseur. Les templates
Qwen3 et Qwen2.5 fournis par la même source sont exercés.

Le pilote Prophet continue à refuser les générations tronquées et les appels multiples.
Aucun appel brut n'est extrait pour contourner ce refus. Les contrôles de capd, du journal,
des budgets et des fichiers ne changent pas.

## Conséquences et limites

Le correctif doit être réévalué lors d'une mise à jour de nixpkgs. Le test contractuel
permettra de retirer le patch quand le moteur amont satisfait le même contrat.

Une terminaison correcte ne garantit pas des arguments exacts : le diagnostic observe
aussi un identifiant altéré par le petit modèle. Les résultats d'inférence et ceux du
parseur sont des preuves distinctes. Le catalogue installé, le démarrage du moteur,
les poids, les GPU et les autres familles restent à intégrer et à mesurer.

Voir le [rapport de diagnostic](../reports/grammaire-locale-2026-09-13.md).
