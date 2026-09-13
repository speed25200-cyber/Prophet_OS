# `prophet-agentd` — runtime des missions

- Socket : `/run/prophet/agentd.sock`
- Utilisateur : `agentd`, groupe `prophet-system`
- État : `/var/lib/prophet/agentd/taches.json`
- Dépendances du lancement local : capd, ledger, home autorisé et moteur HTTP local configuré

## Méthodes

| Méthode | Comportement |
|---|---|
| `task.spawn` | Planifie, demande le jeton à capd, persiste et rend le plan |
| `task.start` | Lance en arrière-plan une mission locale native de niveau 0 |
| `task.list` | Rend les tâches et leurs budgets observés |
| `task.status` | Rend une tâche par identifiant |
| `task.result` | Rend le résultat conservé ; erreur tant qu'il n'est pas disponible |
| `task.cancel` | Demande l'arrêt d'une mission active ou annule un plan non lancé |

`task.start`, `task.status`, `task.result` et `task.cancel` prennent `{"id":"…"}`.
Le format de planification complet est illustré dans
[`examples/missions/note-locale.json`](../../examples/missions/note-locale.json).
Les méthodes sont réservées aux pairs de confiance. Le contrôle existant est global au
socket ; les droits propres à chaque méthode ne sont pas encore appliqués.

## Exécution

Deux missions au maximum peuvent être actives. Les plans et jetons restent côté service.
Le moteur reçoit l'intention et le catalogue d'outils autorisés ; il ne fournit aucune racine
de fichiers ni aucun niveau d'isolation. Les accès doivent aussi rester dans les scopes du
plan. La capture SFS limite son parcours à 10 000 objets, 64 niveaux et 512 Mio de contenu.
Un dépassement ou une source instable fait échouer la préparation.

Les tokens sont débités avant une action ; les générations incomplètes restent comptées
quand leur réponse contient les compteurs. Si l'inférence est annulée ou la connexion rompue
sans compteurs, sa consommation partielle n'est pas connue. Le plafond de génération est
actuellement de 2 048 tokens par tour ; le budget total peut donc être dépassé par le tour
en cours, mais l'action suivante est refusée. La réservation prédictive de contexte et de
tokens reste à implémenter.

Le statut d'annulation final confirme la sortie du travailleur. Les fichiers déjà préparés
restent dans le travail SFS. `result` expose le diff à une fin normale, sans appliquer les
changements. Une fin normale n'est pas une vérification sémantique de l'objectif.

Le registre confirme les événements auprès du vrai ledger. Un échec de journal arrête les
outils ; une écriture peut déjà avoir eu lieu si sa confirmation finale a échoué. Aucun
rejeu automatique n'est tenté. La file ancienne des événements de planification reste
distincte et peut perdre des événements lors d'une panne de journal : la reprise durable
de bout en bout n'est donc pas livrée.

## Persistance et reprise

Plans, tâches, jetons et résultats sont enregistrés sous le verrou du runtime, dans un fichier
temporaire créé en 0600 avant toute donnée, synchronisé et renommé. Le répertoire parent est
ensuite synchronisé. Un état illisible bloque le démarrage et reste intact pour réparation.
Une mission active lors de l'arrêt du service devient `failed` à la reprise ; son résultat
explique l'interruption. Les résultats terminés restent consultables. Les jetons conservés
restent soumis à l'expiration et aux vérifications de capd.

Le [guide du crate](../../crates/agentd/README.md) décrit la configuration et les commandes.
L'[ADR 0013](../adr/0013-missions-locales-agentd.md) détaille les limites du confinement natif,
des chemins de métadonnées, des commits SFS et des clients officiels.
