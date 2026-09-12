# ADR-0012 — Ancrer les accès MCP sur les répertoires de la tâche

- **Statut** : accepté
- **Date** : 2026-09-13
- **Tâches liées** : M7-T1, M8-T8 ; intégration de FRONTIER.md

## Contexte

Les contrôles de capacités portaient sur le chemin logique, puis les outils ouvraient un
chemin ordinaire. Un lien symbolique pouvait conduire hors du périmètre ; un lien physique
pouvait faire tronquer un fichier externe. La recherche ne recontrôlait pas chaque descendant,
et un `max_bytes` excessif supprimait la borne effective. Six régressions ont d'abord échoué.
Ces défauts interdisaient de raccorder les outils aux missions de l'interface.

## Décision

Utiliser les appels sûrs de `rustix` vers `openat2` et des descripteurs de répertoire conservés
durant l'opération. Ouvrir les racines sans liens, puis résoudre les descendants avec
`BENEATH | NO_SYMLINKS | NO_XDEV`. Refuser fichiers spéciaux et lectures d'inodes ayant plusieurs
liens physiques. Écrire dans un fichier temporaire privé au travail, synchroniser et publier
par `renameat` dans le même répertoire, puis synchroniser le répertoire. Revérifier le droit
avant publication. Ne pas ouvrir de fichier du home en écriture.

Le registre fournit un contrôleur qui revérifie `tool.call`, la capacité de chaque descendant,
le jeton, son expiration et la révocation. Un appel direct aux outils fichiers sans contrôleur
est refusé. Borner lectures, écritures et parcours ; exposer la troncature. Conserver la fusion
des fichiers d'origine et de travail et ne pas suivre un lien cassé pour rechercher une autre
version. Refuser les noms réservés à l'état privé de Prophet.

Fournir un `RegistryExecutor` lié à une tâche pour la boucle native. Son catalogue est filtré
par les grants ; seul l'appel effectue la vérification complète. Le lanceur doit créer une
instance par tâche et fournir un contexte fiable. L'essai Qwen3 utilise cet exécuteur et un
vrai espace SFS temporaire, avec un Broker et un journal en mémoire.

## Alternatives écartées

- Canonicaliser puis ouvrir : un remplacement concurrent entre les deux reste possible.
- Autoriser tous les liens internes : cela demande une politique d'alias qui ne permet pas
  de contourner les grants portant sur les noms ; cette version les refuse.
- Ouvrir la destination avec troncature : un lien physique conserverait l'inode externe.
- Activer directement le daemon MCP : le contexte authentifié et les services ne sont pas
  raccordés ; un test de bibliothèque ne peut pas valider ce transport.

## Conséquences et limites

Linux avec `openat2` est requis, sans repli moins strict. Les montages descendants et les
liens usuels d'un home sont refusés dans ce premier contrat. Le drapeau `O_PATH` peut ouvrir
le dernier lien lui-même avec `O_NOFOLLOW` : le type de l'objet est donc aussi vérifié.

Le lanceur doit protéger la provenance et la durée de vie des racines. Un descripteur reste
attaché au répertoire si un acteur ayant des droits externes le déplace ; les appels relatifs
ne constituent pas un espace de noms privé. La protection du répertoire temporaire contre
des acteurs partageant ses droits fait également partie de cette intégration. Les commits,
undo et lectures de diffs SFS utilisent encore leur propre implémentation : cette décision
ne certifie pas leur résistance aux courses, leur atomicité globale ou la reprise après panne.
Le journal en mémoire ne fournit aucune durabilité. La boucle native reste à raccorder à
agentd avec budgets, arrêt, état persistant et contexte authentifié.

Références : [contrat Linux openat2](https://www.man7.org/linux/man-pages/man2/openat2.2.html),
[résolution des chemins](https://www.man7.org/linux/man-pages/man7/path_resolution.7.html),
[interface rustix 1.1.4](https://docs.rs/rustix/1.1.4/rustix/fs/fn.openat2.html).
