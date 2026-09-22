# ADR-0044 — Réserver l'émission des droits et l'écriture du journal aux services, la décision à l'humain

- **Statut** : accepté ; vérifié par tests unitaires, essai NixOS sous le compte de l'humain en CI
- **Date** : 2026-09-22
- **Tâche liée** : FRONTIER, « permissions interservices par méthode et identité » ; lève le
  blocage « l'autorisation au niveau du socket est grossière » de STATUS

## Contexte

Un pair admis par un daemon l'était pour toutes ses méthodes. Or l'humain est membre de
`prophet-system` (`extraGroups`), pour que sa session, la surface et la CLI lisent les tâches et
tranchent les approbations ; et sous son identité tournent aussi, depuis l'ADR 0035, Claude Code
et Codex, ses applications et tout ce qu'il lance. Chacun de ces processus pouvait donc demander à
`capd` un jeton pour un manifeste qu'il écrivait lui-même (`cap.mint`), puis sortir par egress
sous ce jeton ; ou inscrire au journal un événement qui n'avait pas eu lieu (`ledger.append`).
STATUS le notait : « à faire avant qu'un programme moins fiable qu'un afficheur ne parle à un
daemon ». C'est désormais le cas.

`SO_PEERCRED` suffit à faire la différence sans rien ajouter à `prophet-ipc` : les sept daemons
ont `prophet-system` pour **groupe principal** (le `Group=` de leur unité), l'humain et la surface
n'en sont que **membres déclarés**.

## Décision

`prophet-daemon` classe chaque pair admis : **soi** (le service lui-même, ou `root`), **service**
(groupe principal `prophet-system`), **humain** (membre déclaré seulement). Une méthode s'ouvre à
**tous**, aux **services** ou aux **humains** ; *soi* passe partout, ce qui garde les tests, qui
lancent tout sous un même compte, et l'administration par `root`.

- `capd` : `cap.mint`, `cap.delegate`, `cap.check`, `approval.request`, `approval.explain`,
  `approval.expire` → services ; `approval.resolve` → humain ; `cap.public_key`, `cap.revoke`,
  `approval.pending`, `approval.rules`, `approval.status` → tous. Révoquer reste ouvert :
  cela ne fait que retirer, et l'arrêt d'urgence de l'humain en dépend.
- `ledger` : `ledger.append`, `ledger.seal` → services ; lire, vérifier, résumer → tous.

Un pair admis d'une autre classe reçoit `-32001` avec le nom de la méthode et à qui elle revient ;
un pair non admis reçoit le refus d'avant.

## Alternatives écartées

- **Une liste d'identifiants par méthode dans la configuration** : un réglage de plus à tenir
  juste sur chaque machine, alors que le groupe principal dit déjà qui est un daemon.
- **`SO_PEERGROUPS` ou l'exécutable du pair** : changerait le type traversant les sept daemons ou
  lirait `/proc` à chaque appel, pour une distinction que le groupe principal donne.
- **Retirer l'humain du groupe** : la surface, la CLI et les décisions humaines ne joindraient
  plus aucun daemon.

## Conséquences

Un processus de la session humaine n'émet plus de droit ni n'écrit au journal ; un service ne
tranche plus une approbation à la place de l'humain. Ce qui reste ouvert, dit franchement : un
processus de l'humain peut toujours **trancher** une approbation, comme l'humain lui-même par la
CLI. Un client officiel lancé par `prophet-pilotd` sous cette identité, avec ses propres outils,
le pourrait donc s'il exécutait un programme qui appelle `approval.resolve` ; distinguer la
surface d'un autre programme du même compte demande un chemin de confiance que l'OS n'a pas
encore, ou le confinement du client que l'ADR 0026 laisse ouvert. Les autres daemons (agentd,
vault, egress, sandboxd, memoryd) gardent leurs propres contrôles, à passer au même crible.
L'essai NixOS des services vérifie, sous le compte de l'humain, la lecture permise et les refus
de `cap.mint` et de `ledger.append`.
