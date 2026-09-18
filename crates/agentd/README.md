# Runtime des missions

`prophet-agentd` planifie et suit les tâches. Le lancement local utilise le modèle HTTP
configuré, les outils fichiers MCP, le vrai `capd` et le vrai `ledger`, sur un travail SFS.
Les fichiers de l'utilisateur restent à valider explicitement ; aucune validation n'est
effectuée par cette boucle.

Le service n'exécute actuellement que les plans `local:*` au niveau 0, avec des outils natifs
de confiance. Il ne lance aucun programme fourni par le modèle. Les plans qui exigent les
niveaux 1 ou 2 et les clients officiels sont refusés par `task.start`. Ce niveau ne constitue
pas la preuve d'un lancement de processus sous `sandboxd`.

## Configuration de développement

Configurer le même `PROPHET_HOME` absolu pour agentd et capd. Les services doivent disposer de
répertoires d'état privés et de sockets accessibles aux seuls pairs de confiance.
`PROPHET_LOCAL_ENDPOINT` définit le serveur local compatible Chat Completions, par exemple
`http://127.0.0.1:8080/v1`. Les redirections, proxys d'environnement et serveurs distants sont
refusés. Le moteur doit déjà être lancé. Cette configuration n'est pas encore activée dans
la session de l'image installée.

Les adresses de services sont `PROPHET_CAPD_SOCKET`, `PROPHET_LEDGER_SOCKET`,
`PROPHET_EGRESS_SOCKET` et, pour la CLI, `PROPHET_AGENTD_SOCKET`. `STATE_DIRECTORY` définit
l'état de chaque daemon, séparément. `PROPHET_BROWSER` nomme un navigateur Chromium pour les
outils `web.*` ; sans lui, une mission n'a pas de navigateur.

`PROPHET_JEV_SECRET` nomme le secret du coffre qui porte la clé de Jev et active le décideur
rapide : routage des missions à la planification (`task.spawn`, `task.route`) et opérateur
d'interface en cascade devant le modèle génératif, quand le jeton autorise `net.egress` sur
`api.typesafe.ai` et que le manifeste n'est pas `local-only`. `PROPHET_JEV_MODEL` choisit le
modèle. Le proxy de sortie doit déclarer l'hôte d'interrogation (`PROPHET_EGRESS_QUERY_HOSTS`).
`cargo test -p agentd --test jev` exerce la chaîne avec un proxy simulé et, si un navigateur
est présent, une page réelle opérée par Jev seul. Un test monte aussi toute la chaîne sans aucun
faux — capd, ledger, coffre, proxy de sortie, agentd — et vérifie que, sous un compte qui n'est
pas `egress`, le coffre refuse, le proxy arrête la requête et la route retombe en nommant la
raison, sans que le secret n'apparaisse en clair sur disque. Le premier appel réel demande les
comptes de service : `tools/lancer-sur-l-hote.sh` les crée et son en-tête explique comment
activer Jev sur un hôte. Voir l'[ADR 0025](../../docs/adr/0025-jev-decideur-rapide.md).

## Préparer depuis l'interface

`PROPHET_MISSION_PROFILES` désigne un fichier JSON de configuration chargé au démarrage.
L'[exemple de profils](../../examples/missions/profils-locaux.json) fournit le format : adapter
les modèles et répertoires au service local, puis créer le répertoire de documents choisi.
Sa clé d'éditeur est factice, comme dans l'exemple CLI ; ce fichier de développement ne constitue
pas un manifeste d'éditeur authentifié. Le modèle cité est un exemple de référence de moteur,
pas une recommandation de qualité ni une validation de la chaîne agentd avec Qwen3-0.6B.

Le catalogue est une configuration de confiance de l'administrateur. Il ne doit pas être
modifiable par un agent. Une configuration invalide bloque le démarrage avec une erreur ;
son absence rend une liste de profils vide. La limite est de 32 profils et 1 Mio.

La surface lit `task.options`, choisit un profil et un modèle effectivement présent, puis
envoie `task.prepare {id, intent, profile, model}`. Le service fixe l'identité à partir du pair
Unix et obtient le jeton auprès de capd. Les autres champs sont refusés. La préparation ne
lance ni génération ni outil. L'humain examine le plan et commande son lancement séparément.
Une référence existante produit un conflit ; elle se relit par `task.inspect`.

Ce raccordement utilise `PROPHET_LOCAL_ENDPOINT` du service, même si le dialogue de la surface
emploie un autre moteur. Les profils ne sont pas encore installés automatiquement dans l'image.
Voir l'[ADR 0015](../../docs/adr/0015-intention-et-profils-de-mission.md).

## Parcours CLI

Adapter le modèle, l'identifiant, l'utilisateur et le périmètre de
[`note-locale.json`](../../examples/missions/note-locale.json) à la configuration de test.
Le manifeste de cet exemple est une déclaration de développement : sa clé factice n'est
pas une attestation d'éditeur vérifiée. La validation des éditeurs et des manifestes reste
un critère de livraison.

```sh
prophet task new examples/missions/note-locale.json
prophet task start note-locale
prophet --json task ls
prophet task result note-locale
prophet task cancel note-locale
prophet task apply note-locale
prophet task undo note-locale
```

`new` rend le plan sans démarrer. `start` accuse réception du lancement en arrière-plan.
`cancel` accuse réception de la demande ; attendre l'état final pour confirmer l'arrêt.
`result` rend le texte, la raison d'arrêt, le budget et, en cas de fin normale, le diff SFS.
Un identifiant déjà utilisé ne peut pas être relancé. Le jeton expire à partir de la
planification ; un long délai de relecture peut donc nécessiter un nouveau plan.
`apply` publie dans le home l'index exact conservé par le service, pour le créateur de la
mission ; `undo` annule cette publication si les documents n'ont pas changé depuis. Les deux
refusent un conflit sans rien écraser, et reprennent une intention interrompue. `show` affiche
l'état de publication et la commande suivante.

La surface appelle aussi `task.change` avec `{id,path}` pour lire les versions initiale et
proposée d'un fichier du diff. Cette méthode exige une mission terminée, son index conservé
et l'UID créateur observé sur le socket. Elle vérifie les octets, refuse les versions altérées
et ne relit pas les originaux actuels. Les anciennes missions sans versions ou sans propriétaire
observé n'offrent pas cet aperçu. Voir l'[ADR 0018](../../docs/adr/0018-examen-des-versions.md).

## Validation

```sh
nix develop --command just check
# Pour un essai ciblé, construire aussi les autres programmes utilisés par les tests :
nix develop --command cargo build -p capd -p ledger -p prophet-cli
nix develop --command cargo test -p agentd --test local_daemon
PROPHET_TEST_ENDPOINT=http://127.0.0.1:18080/v1 PROPHET_TEST_MODEL=qwen3-0.6b \
  nix develop --command cargo test -p agentd --test local_daemon une_mission_reelle \
  -- --ignored --nocapture
```

Le dernier test exige un modèle réel et n'est pas lancé par `just check`. Les autres utilisent
une réponse HTTP contrôlée, avec les daemons et la CLI réels. Voir le
[contrat de service](../../docs/components/agentd.md), l'[ADR 0013](../../docs/adr/0013-missions-locales-agentd.md)
et le [rapport d'intégration](../../docs/reports/missions-locales-2026-09-13.md).
