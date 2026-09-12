# 0011 — Faire de la supervision l'espace principal

Date : 13 septembre 2026. Statut : implémentation vérifiée localement, direction visuelle à apprécier par l'utilisateur.

L'utilisateur rejette la direction Iris : la décoration et l'accueil de conversation ne
représentent pas un espace conçu pour les agents et la supervision humaine. Cette demande
remplace la direction de l'ADR 0010.

L'écran principal rassemble les missions réellement reçues. Une mission sélectionnée ouvre
son contexte : agent, état, étapes, activité, budget consommé et demande humaine disponible.
Les filtres distinguent les missions à examiner, en cours et terminées. Les livrables et les
diffs restent explicitement absents tant que les services ne les fournissent pas. Préparer un
objectif ouvre le dialogue local, sans prétendre lancer une mission agentique.

Une décision en attente apparaît dans une bande persistante. L'humain peut continuer à
consulter les missions puis ouvrir l'examen, lire les conséquences et choisir. Ouvrir ou fermer
l'examen n'autorise rien. Une modification de la tâche, du texte, de la conséquence ou de
l'irréversibilité referme l'examen. Cela protège la relecture dans l'interface ; le contrat
d'approbation des services reste à compléter avec une identité explicite et un acquittement
visible. Cette refonte ne prétend pas résoudre ces limites préexistantes.

Le vocabulaire visuel devient clair : surfaces neutres, encre foncée, Inter à graisses explicites,
contours fins et couleur réservée aux états utiles. La sculpture décorative est supprimée.
L'accueil n'impose plus de redessin toutes les 33 ms ; la surveillance existante des services
conserve son intervalle de 250 ms. Les notifications réseau et les événements de fenêtre
continuent de demander leur redessin. Aucune mesure de performances matérielles n'est déduite
de cette seule modification.

Sur petit écran, la sélection ouvre le contexte à la place de la liste, avec un retour aux
missions. Le panneau d'examen reste distinct et ses contrôles doivent être visibles à 640 × 480.

Critères : tests GPU des filtres, sélection, disparition, presse-papiers, examen explicite,
modification de conséquence, navigation, saisie Unicode et tailles ; `just check` ; captures
du binaire relues. Le [rapport de supervision](../reports/supervision-2026-09-13.md) rassemble
les résultats et les limites. Ni une équivalence avec Apple ni un statut révolutionnaire ou
SOTA ne se démontrent par ces tests ; la qualité visuelle reste soumise à la revue de l'utilisateur.
