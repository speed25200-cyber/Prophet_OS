# `prophet-agentd` — runtime des missions

- Socket : `/run/prophet/agentd.sock`
- Utilisateur : `agentd`, groupe `prophet-system`
- État : `/var/lib/prophet/agentd/taches.json`
- Dépendances du lancement local : capd, ledger, home autorisé et moteur HTTP local configuré
- Sortie réseau des outils : le socket d'egress (`PROPHET_EGRESS_SOCKET`, `/run/prophet/egress.sock` par défaut)
- Navigateur piloté : absent sauf `PROPHET_BROWSER` ; profils par tâche sous l'état du service

## Méthodes

| Méthode | Comportement |
|---|---|
| `task.spawn` | Planifie, demande le jeton à capd, persiste et rend le plan |
| `task.options` | Rend les profils configurés et leurs modèles réellement disponibles |
| `task.prepare` | Prépare une intention avec un profil du service, sans génération ni exécution |
| `task.start` | Lance en arrière-plan une mission locale native de niveau 0 |
| `task.list` | Rend les tâches et leurs budgets observés |
| `task.status` | Rend une tâche par identifiant |
| `task.inspect` | Rend tâche, plan, résultat et commandes possibles, sans jeton |
| `task.result` | Rend le résultat conservé ; erreur tant qu'il n'est pas disponible |
| `task.change` | Rend les versions vérifiées d'un fichier au créateur de la mission terminée |
| `task.cancel` | Demande l'arrêt d'une mission active ou annule un plan non lancé |
| `task.apply` | Publie dans le home l'index exact examiné, pour le créateur d'une mission `done` |
| `task.undo` | Annule cette publication si les documents n'ont pas changé depuis |

`task.start`, `task.status`, `task.inspect`, `task.result`, `task.cancel`, `task.apply` et
`task.undo` prennent `{"id":"…"}`.
Le format de planification complet est illustré dans
[`examples/missions/note-locale.json`](../../examples/missions/note-locale.json).
Les méthodes sont réservées aux pairs de confiance. Le contrôle existant est global au
socket ; la nouvelle méthode `task.change` exige en plus l'UID créateur persisté. Elle prend
`{id,path}`, où `path` est relatif au home et appartient au diff final. La mission doit être
`done` et posséder ses versions conservées ; aucun propriétaire n'est déduit des anciennes
identités déclarées. Deux lectures simultanées sont admises. Les autres méthodes conservent
leur contrôle global et restent à traiter individuellement.

`task.apply` et `task.undo` exigent le même UID créateur et la même mission `done`. La
bibliothèque SFS relit l'index et les originaux avant la première mutation et refuse un
document retouché ; le service n'ajoute que l'identité, la sérialisation (une publication à la
fois) et le journal : `fs.commit`, ou `fs.undo` puis `task.rolled_back`, sous l'acteur `user`.
Une intention interrompue (`applying`, `undoing`) se reprend par la même commande. `task.inspect`
rend l'état SFS dans `publication`, et `can_apply` / `can_undo` au seul créateur. Avant de
publier, le service conserve le manifeste de la mission, demande à capd un jeton de deux minutes
borné aux chemins de l'index exact et soumet chaque chemin à `cap.check` ; un refus est
journalisé (`policy.deny`, étape `publish`) sans rien écrire. La publication s'exécute ensuite
sous l'identité du service ; voir les limites de l'[ADR 0023](../adr/0023-approbation-et-publication-par-agentd.md).

`task.prepare` prend uniquement `{id, intent, profile, model}`. Son utilisateur provient du pair
Unix. `PROPHET_MISSION_PROFILES` fixe les manifestes et périmètres au démarrage ; les modèles sont
redécouverts au moment de préparer. Le modèle du dialogue ne fournit aucune autorité à ce chemin.
Une référence déjà connue est refusée et se relit par `task.inspect`. Le plan devient exécutable
par une commande distincte après examen. Le catalogue est une configuration locale de confiance,
pas une validation des signatures d'éditeurs. Voir l'[ADR 0015](../adr/0015-intention-et-profils-de-mission.md).

## Exécution

Deux missions au maximum peuvent être actives. Les plans et jetons restent côté service.
Les outils offerts au modèle sont les accès fichiers, `http.fetch` par egress et, si un
navigateur est nommé, `web.open`, `web.tree` et `web.act` ; voir l'[ADR 0024](../adr/0024-navigateur-integre-et-applications-web.md).
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

Plans, manifestes, tâches, jetons et résultats sont enregistrés sous le verrou du runtime, dans un fichier
temporaire créé en 0600 avant toute donnée, synchronisé et renommé. Le répertoire parent est
ensuite synchronisé. Un état illisible bloque le démarrage et reste intact pour réparation.
Une mission active lors de l'arrêt du service devient `failed` à la reprise ; son résultat
explique l'interruption. Les résultats terminés restent consultables. Les jetons conservés
restent soumis à l'expiration et aux vérifications de capd.

Le [guide du crate](../../crates/agentd/README.md) décrit la configuration et les commandes.
L'[ADR 0013](../adr/0013-missions-locales-agentd.md) détaille les limites du confinement natif,
des chemins de métadonnées, des commits SFS et des clients officiels.
