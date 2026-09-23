# ADR-0050 — Un refus de chemin revient au modèle ; la révocation et le troisième refus arrêtent

- **Statut** : accepté
- **Date** : 2026-09-23
- **Tâche liée** : M4 (boucle native d'agentd), M13-T1 (banc M13, deuxième passage)

## Contexte

Depuis la première boucle locale, une mission s'arrêtait au premier appel d'outil refusé
(`PolicyDenied`) ou en panne (`Internal`). Le deuxième passage du banc M13 (`6d01c38`, quinze
tâches, trois exécutions chacune) montre ce que cela coûte à un petit modèle : 19 des 45
exécutions par Prophet s'arrêtent sur un refus (12) ou une « panne » (7), souvent dès la première étape,
quand la boucle nue, qui reçoit les mêmes erreurs, continue et se corrige parfois. Les causes :

- le manifeste du banc n'accordait pas `fs.list` (les profils locaux du système l'accordent) :
  chaque `fs.list` était refusé ;
- un modèle qui passe un fichier comme racine de `fs.search`, ou lit un répertoire, recevait
  `Internal` (`ENOTDIR`) ou `PolicyDenied` (type de fichier), deux codes qui arrêtaient tout ;
- un chemin hors de la portée (`~/fournisseur.txt` au lieu de `~/achats/out/fournisseur.txt`)
  arrêtait la mission, sans que le modèle sache où il pouvait agir.

Le refus reste la bonne décision de capd. Ce qui n'allait pas, c'est d'en faire la fin de la
mission, et de le dire sans dire où agir.

## Décision

- **Un refus de chemin revient au modèle.** Le résultat d'outil garde son code `PolicyDenied`
  et dit où la mission peut agir, d'après les motifs `fs` de son propre jeton (« … ; la mission
  peut agir sous ~/achats ») — ce que `task.status` lui rend déjà. Le refus est journalisé
  comme avant (`policy.deny`, `tool.result`), et l'action n'a pas lieu.
- **La révocation arrête toujours.** Après chaque refus, agentd redemande à capd le droit
  d'appeler l'outil lui-même (`tool.call` sur son nom) : refusé — jeton révoqué, expiré —, la
  mission s'arrête sur-le-champ (« droits retirés »).
- **Trois refus arrêtent la mission** (`REFUS_MAX`) : un modèle qui s'est trompé se corrige vite ;
  un modèle détourné qui sonde le système ne sonde pas longtemps.
- **Une panne (`Internal`) arrête toujours** : son état est inconnu.
- **Une erreur de nature de chemin n'est ni une panne ni un refus** : lire un répertoire ou
  lister un fichier rend `Invalid` avec l'outil qui convient (« listez-le avec fs.list »,
  « lisez-le avec fs.read ») ; `fs.search` accepte un fichier comme racine et le fouille seul ;
  un nom trop long ou invalide rend `Invalid`.
- **Le banc accorde ce que le système accorde** : `fs.list` sur le dossier de la tâche, comme
  les profils locaux (`examples/missions/profils-locaux.json`).

## Alternatives écartées

- **Garder l'arrêt au premier refus** : sûr, mais la sécurité n'en dépend pas — capd refuse
  l'action quoi qu'il arrive — et chaque erreur de chemin d'un petit modèle coûte la mission.
- **Aucune borne** : un modèle détourné par un contenu pourrait sonder indéfiniment ; trois
  refus suffisent à se corriger.
- **Dire la portée du plan plutôt que celle du jeton** : l'outil ne connaît que le jeton, qui
  est la vérité de capd ; un jeton plus large que la portée du plan reste borné par agentd, et
  le modèle recevra le refus suivant.

## Conséquences

- Une mission dont une écriture est refusée peut finir `done` sans cette écriture : le
  créateur le voit au diff et au journal, comme toute action refusée.
- Le banc refait son deuxième passage à conditions égales : `fs.list` accordé, erreurs de
  chemin corrigibles des deux côtés (mêmes implémentations d'outils).
