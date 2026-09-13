# ADR 0014 — Examiner et commander une mission depuis l'espace natif

- Date : 13 septembre 2026
- Statut : adopté

## Contexte

La scène graphique regroupait plusieurs états distincts et écartait les missions terminées.
Le filtre « Terminées » ne pouvait donc pas montrer les résultats réels. Le lancement et
l'arrêt introduits par l'ADR 0013 n'étaient disponibles que par la CLI et le socket.

## Décision

`task.inspect` fournit une lecture du plan, de la tâche et du résultat sous le verrou du
runtime. Le jeton de capacité reste dans le service. Les indicateurs de commandes expriment
ce que le service sait autoriser au moment de la lecture ; la commande recontrôle son état.
Cette inspection ne réserve ni travailleur ni ressources.

L'interface présente le plan et les accès, l'exécution, puis la proposition et les métadonnées
des fichiers préparés. Elle conserve les états exacts et les missions terminées. La fin du
runtime ne signifie pas que l'objectif a été vérifié ni que les fichiers ont été appliqués.

Un contrôleur hors de la boucle de rendu envoie une commande par geste explicite, sur une
connexion neuve. Les réponses de lecture portent une révision de sélection. Un changement de
mission ou un acquittement de commande invalide les lectures antérieures. Les erreurs sont
visibles et aucun échec de transport ne déclenche un nouvel envoi automatique. Le contrôleur
attend une relecture après commande ; une demande d'arrêt n'est pas un état final optimiste.
La liste et l'inspecteur réconcilient les transitions attestées par l'historique. Une liste plus
récente invalide l'ancien inspecteur et ses commandes ; une inspection plus récente met à jour
l'état affiché dans la liste. Une mission absente de la source n'est jamais réintroduite ainsi.
Le rang de la transition dans l'historique départage les réponses, y compris dans un cycle
pause/reprise où le même état apparaît plusieurs fois.

Le client IPC borne également ses requêtes et réponses à 8 Mio, vérifie la terminaison de ligne,
la version JSON-RPC, l'identifiant de requête et l'exclusivité résultat/erreur. La valeur
`result: null` demeure valide. Ce contrôle ne remplace pas les droits par méthode et par pair.

## Conséquences

Les lectures et commandes de l'inspecteur ont un délai de cinq secondes. La source globale
interroge les trois services simultanément avec ce même délai, puis s'arrête lorsque sa source
est détruite. Les tâches terminées restent consultables ; les titres longs de la liste sont
abrégés visuellement et conservés dans le contexte.

La conversation ne crée pas encore de plan agentique. Le pilote isolé, le contenu des diffs,
leur validation, la reprise et le journal détaillé d'actions restent à intégrer. Le contrat
historique d'approbation capd demeure distinct : son acquittement visible et l'identité explicite
de la demande dans la scène restent à corriger. Cette évolution ne valide pas un OS SOTA.
