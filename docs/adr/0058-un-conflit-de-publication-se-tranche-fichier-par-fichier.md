# ADR-0058 — Un conflit de publication se tranche fichier par fichier

- **Statut** : accepté
- **Date** : 2026-09-24
- **Tâche liée** : FRONTIER, « journalisation durable… undo qui respecte les modifications
  intervenues depuis le commit » et « résolution graphique des conflits » ; ADR 0022, 0023

## Contexte

L'ADR 0022 arrête une publication sur le premier fichier que l'humain a changé pendant qu'elle
avançait (état `conflict`), sans restauration aveugle : c'est juste, mais rien ne permettait
d'en sortir. La reprise rejouait le même pas, qui échouait tant que l'humain ne remettait pas
son fichier comme avant ; l'annulation n'était permise qu'à une publication terminée ; agentd
répondait « elle demande une résolution explicite » sans commande pour la donner. Le lot restait
à moitié publié, indéfiniment.

De même, annuler une publication dont l'humain avait retouché un seul fichier était refusé en
bloc : ses changements étaient respectés, mais le reste de la mission ne pouvait plus être
défait.

Un cas plus fin : quand l'humain édite un fichier à l'instant où la mission l'échange, l'échange
atomique emporte **son** édition dans les fichiers déplacés et laisse la version de la mission à
sa place. L'état `conflict` le détecte ; il ne le réparait pas.

## Décision

Le journal de publication retient les fichiers **laissés à l'humain** (`kept`, par rang dans
l'index). Un fichier laissé n'est plus jamais ni publié ni rétabli par ce lot. Vide, le champ
ne s'écrit pas : l'empreinte des journaux antérieurs reste juste.

Une publication arrêtée sur un conflit attend une décision de son créateur, `task.resolve` :

- **Garder sa version** (`keep_mine`) : le fichier en conflit lui est laissé et le lot se
  poursuit dans le même sens — publication ou annulation. Si l'échange a emporté son édition
  alors que la version de la mission est intacte, l'échange est défait : son édition revient à
  sa place, la version de la mission part aux fichiers déplacés.
- **Tout annuler** (`roll_back`) : ce qui avait atteint ses documents est rétabli ; le fichier en
  conflit et ce que la publication n'avait pas encore atteint lui sont laissés. Une annulation
  arrêtée ne s'« annule » pas ; elle se poursuit en gardant sa version.

Un pas n'est tenu pour accompli que s'il a lui-même déplacé les fichiers (identités du journal)
et que chacun porte la version attendue ; sinon le fichier revient à l'humain. Relancer une
décision interrompue est sûr : l'échange déjà défait ne correspond plus aux identités du pas.

`task.undo` accepte `keep_changes` : les fichiers changés depuis la publication sont laissés à
l'humain, le reste est rétabli. Sans lui, l'annulation reste stricte. Si tous les fichiers ont
changé, rien n'est rétabli et l'annulation le dit ; la publication reste publiée.

Poursuivre une publication écrit encore les versions de la mission : capd retranche avant,
comme pour `task.apply`. Rétablir n'écrit que les versions de l'humain, sans nouveau jeton. Ce
que l'humain a gardé se lit au journal (`kept` dans `fs.commit` ou `fs.undo`), dans l'inspection
et dans la réponse.

La surface montre le conflit dans la mission (« CONFLIT DE PUBLICATION » : le fichier, ce qui
est sûr, « Garder ma version et poursuivre », « Tout annuler ») et, après un refus de
l'annulation stricte, « Annuler en gardant mes changements ». La CLI : `prophet task resolve
<id> --keep-mine | --roll-back`, `prophet task undo <id> --keep-changes`.

## Alternatives écartées

- **Fusion à trois voies** : le système ne sait pas fusionner n'importe quel format, et une
  fusion automatique décide à la place de l'humain. Il garde sa version ; il peut rouvrir une
  mission pour reprendre le reste.
- **Écraser par la version de la mission** (« prendre la leur ») : c'est la restauration
  aveugle que l'ADR 0022 refuse ; l'humain qui la veut republie par une nouvelle mission,
  examinée.
- **Annuler en gardant ses changements par défaut** : défaire une partie d'un lot peut laisser
  des fichiers qui ne vont plus ensemble ; l'humain le choisit en connaissance de cause, après
  le refus qui le lui dit.

## Conséquences

- Aucun état de publication n'est plus une impasse : chacun a une commande, dans la surface et
  à la CLI.
- Une publication peut finir publiée ou annulée **en partie** ; ce qui a été laissé est dit
  partout où l'état se lit.
- Les essais de la bibliothèque couvrent l'édition emportée puis ramenée, l'annulation totale
  après un conflit, la poursuite d'une annulation arrêtée, l'annulation qui garde les
  changements et le journal antérieur ; l'essai du service couvre `keep_changes`, les refus de
  `task.resolve` et le journal.
