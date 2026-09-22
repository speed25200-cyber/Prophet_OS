# `prophet-agentd` — runtime des missions

- Socket : `/run/prophet/agentd.sock`
- Utilisateur : `agentd`, groupe `prophet-system`
- État : `/var/lib/prophet/agentd/taches.json`
- Dépendances du lancement local : capd, ledger, home autorisé et moteur HTTP local configuré
- Sortie réseau des outils : le socket d'egress (`PROPHET_EGRESS_SOCKET`, `/run/prophet/egress.sock` par défaut)
- Navigateur piloté : absent sauf `PROPHET_BROWSER` ; profils par tâche sous l'état du service ;
`PROPHET_SUP_SOCKET` nomme le socket de l'adaptateur d'accessibilité de la session (ADR 0027) ; sans lui, aucun outil `ui.*`. `doc.read` lit tout format sous `fs.read` et emploie `pdftotext`, `pdfinfo`, `ffprobe` et `tesseract` s'ils sont sur le chemin du service (ADR 0028). `task.delegate` crée, lance et attend une sous-mission sous un jeton délégué par capd, pour un profil qui accorde `task.spawn` sur un contexte nommé (ADR 0029). Avec un rôle (`role` : `reflect`, `execute`, `code`), le service choisit le modèle que le contexte visé admet pour ce rôle parmi ceux que le moteur sert, briefe chaque mission sur son rôle, condense les anciens résultats d'outils avant chaque envoi, et compte les tokens par modèle (ADR 0034). Si `PROPHET_PILOT_SOCKET` nomme le lanceur de pilotes de la session, un rôle peut désigner un client officiel (`driver:claude-code`, `driver:codex`, `driver:gemini`) : la sous-mission devient une séance d'outils que le client rejoint par le pont, lancé sous l'identité de l'humain, et son texte revient au parent (ADR 0035) ; sans lanceur ou client connecté, le rôle retombe sur le modèle local suivant. `task.delegate {model}` nomme un modèle local ou un client officiel connecté, avec son palier de modèle s'il y a lieu (`claude-code@haiku`, ADR 0040) ; sans modèle ni rôle, une mission menée par un client confie à ce même client. Une sous-mission part de l'espace de travail de son parent et, finie, y rapporte son diff (`carried` dans le résultat de `task.delegate`) ; elle ne se publie pas seule, le parent publie le tout (ADR 0039).
  sondé une fois au démarrage, verdict rendu par `task.options` (`browser`)

## Méthodes

| Méthode | Comportement |
|---|---|
| `task.spawn` | Planifie, demande le jeton à capd, persiste et rend le plan |
| `task.options` | Rend les profils configurés (`web` s'ils exigent le navigateur), leurs modèles réellement disponibles et l'état du navigateur piloté |
| `task.prepare` | Prépare une intention avec un profil du service, sans génération ni exécution ; `model` nomme un modèle local découvert ou un client officiel connecté (`claude-code`, `codex` — les modèles principaux, ADR 0035) ; refuse un contexte web sans navigateur qui répond ; `client: true` dispense du moteur local pour une mission destinée à un client MCP |
| `task.start` | Lance en arrière-plan une mission de niveau 0 : sur le moteur local du service, ou, si le plan désigne un client officiel, par le lanceur de pilotes de la session (le client rejoint la mission par le pont, la réponse revient aussitôt, la mission se suit par `task.inspect`) |
| `task.list` | Rend les tâches et leurs budgets observés |
| `task.status` | Rend une tâche par identifiant |
| `task.inspect` | Rend tâche, plan, résultat et commandes possibles, sans jeton |
| `task.result` | Rend le résultat conservé ; erreur tant qu'il n'est pas disponible |
| `task.change` | Rend les versions vérifiées d'un fichier au créateur de la mission terminée |
| `task.cancel` | Demande l'arrêt d'une mission active ou annule un plan non lancé |
| `task.apply` | Publie dans le home l'index exact examiné, pour le créateur d'une mission `done` |
| `task.undo` | Annule cette publication si les documents n'ont pas changé depuis |
| `task.attach` | Ouvre pour le créateur une séance d'outils sur une mission préparée : jeton, travail SFS, registre, journal, sans modèle (`{id, client?}`) |
| `task.tools` | Rend les outils que le jeton de la séance couvre |
| `task.call` | Exécute un outil de la séance, compté comme une étape (`{id, name, arguments}`) |
| `task.detach` | Retire le client, scelle les versions et conclut la mission en `done` (`{id, text?}`) |
| `task.route` | Dit quel modèle Jev choisirait pour une intention, sans planifier ; sélection statique sans Jev |

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

`task.attach`, `task.tools`, `task.call` et `task.detach` exigent l'UID créateur persisté ; le
pont `prophet-mcp` les relaie pour un client d'éditeur (ADR 0026). Une annulation pendant la
séance la conclut en `cancelled`. `task.apply` et `task.undo` exigent le même UID créateur et la même mission `done`. La
bibliothèque SFS relit l'index et les originaux avant la première mutation et refuse un
document retouché ; le service n'ajoute que l'identité, la sérialisation (une publication à la
fois) et le journal : `fs.commit`, ou `fs.undo` puis `task.rolled_back`, sous l'acteur `user`.
Une intention interrompue (`applying`, `undoing`) se reprend par la même commande. `task.inspect`
rend l'état SFS dans `publication`, `can_apply` / `can_undo` au seul créateur, et dans
`browsing` l'adresse, le titre et la taille de la page où l'agent navigue, déposés par les
outils web, jamais l'arbre. Avant de
publier, le service conserve le manifeste de la mission, demande à capd un jeton de deux minutes
borné aux chemins de l'index exact et soumet chaque chemin à `cap.check` ; un refus est
journalisé (`policy.deny`, étape `publish`) sans rien écrire. La publication s'exécute ensuite
sous l'identité du service ; voir les limites de l'[ADR 0023](../adr/0023-approbation-et-publication-par-agentd.md).

`task.prepare` prend uniquement `{id, intent, profile, model}`. Son utilisateur provient du pair
Unix. `PROPHET_MISSION_PROFILES` fixe les manifestes et périmètres au démarrage ; les modèles sont
redécouverts au moment de préparer. Un client officiel demandé comme modèle doit être admis par
le profil et dit connecté par le lanceur de pilotes ; sinon la préparation le refuse en disant
comment se connecter (`prophet provider login <client>`). Le modèle du dialogue ne fournit aucune autorité à ce chemin.
Une référence déjà connue est refusée et se relit par `task.inspect`. Le plan devient exécutable
par une commande distincte après examen. Le catalogue est une configuration locale de confiance,
pas une validation des signatures d'éditeurs. Voir l'[ADR 0015](../adr/0015-intention-et-profils-de-mission.md).

`task.route` prend `{intent, manifest, availability?}`. Quand Jev est configuré et que le
manifeste admet un service distant et la sortie vers `api.typesafe.ai`, le service demande à
capd un jeton de deux minutes borné à cet hôte, consulte Jev par le proxy et rend la route
(`decider`, probabilités, difficulté, risque, consommation). Sinon, il rend la sélection
statique avec la raison. Aucune tâche n'est créée. `task.spawn` fait la même consultation avec
le jeton de la mission avant de planifier ; la route est conservée dans le plan (`route`) et
journalisée dans `task.planned`. Voir l'[ADR 0042](../adr/0042-jev-decideur-rapide.md).

## Exécution

Deux missions au maximum peuvent être actives. Les plans et jetons restent côté service.
Les outils offerts au modèle sont les accès fichiers, `http.fetch` par egress et, si un
navigateur est nommé, `web.open`, `web.tree` et `web.act` ; voir l'[ADR 0024](../adr/0024-navigateur-integre-et-applications-web.md).
Quand `PROPHET_JEV_SECRET` nomme un secret du coffre, qu'un navigateur est configuré, que le
manifeste n'est pas `local-only` et que le jeton autorise `net.egress` sur `api.typesafe.ai`,
la mission tourne en cascade : l'opérateur Jev lit l'arbre de la page et décide `web.act`
lui-même, en quelques centaines de millisecondes ; il rend la main au modèle génératif quand
il faut écrire ou quand il hésite. `provider.started` porte alors `decider`, et le résultat
porte `jev` (décisions, actions, mains rendues, tokens). Les tokens de Jev sont imputés au
budget comme ceux du modèle. `PROPHET_JEV_MODEL` choisit le modèle (`jev-latest` par défaut).
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

Chaque tour est aussi compté au nom du modèle qui l'a joué (`task.usage`, dans `task.inspect`,
`task.result` et `task.list`, et `by_model` dans l'événement final du journal) ; une sous-mission
terminée impute son compte à son parent, modèle par modèle. C'est la mesure du relais (ADR 0034) :
`prophet task show` dit la part des tokens prise en charge hors du modèle de la mission. Les
tours d'un client MCP sont comptés sous `client:<nom>`, sans tokens, le client ne rendant pas
ses compteurs au service. Avant chaque envoi au moteur, les résultats d'outils plus anciens que
les deux derniers et plus longs que 1 024 octets sont condensés (taille, empreinte, début) ;
l'historique conservé par la boucle ne change pas.

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
