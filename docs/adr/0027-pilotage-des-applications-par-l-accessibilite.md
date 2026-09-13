# ADR 0027 — Pilotage des applications de bureau par leur arbre d'accessibilité

Date : 2026-09-13. Statut : accepté, première livraison.

## Contexte

Un agent doit pouvoir travailler dans les logiciels de l'humain (éditeur, bureautique, dessin,
CAO) sans capture d'écran ni clic en coordonnées, conformément au plan : « l'interface est un
rendu, pas une API ». Les applications GTK et Qt publient déjà un arbre sémantique, celui de
l'accessibilité (AT-SPI 2), avec des rôles, des noms, des états, des valeurs et des actions
déclarées. `crates/sup` sait le traduire en arbre SUP et annonce la confiance qu'une telle
lecture mérite. Ce qui manquait : le lire réellement, y agir, et le mettre sous capd.

Le bus d'accessibilité vit dans la session de l'humain. Rien d'extérieur ne peut le joindre :
son socket est dans le répertoire d'exécution de la session (mode 0700) et son authentification
n'admet que l'identité de la session. `agentd`, service système sous un autre compte, ne peut
donc pas le lire lui-même.

## Décision

1. **Un adaptateur dans la session.** `prophet-supd` (crate `supd`) tourne comme service
   utilisateur de la session graphique. Il joint le bus d'accessibilité à la demande, lit
   l'arbre d'une fenêtre (borné en nœuds, en profondeur et en temps, et disant ce qu'il a laissé
   de côté), le rend en SUP par `sup::adapter`, et exécute trois actions typées : `click` par
   `Action.DoAction`, `set_field` par `EditableText.SetTextContents`, `toggle` par `Action`.
   Il n'emploie que ce que l'application déclare ; il ne simule ni touche ni clic.
2. **Un socket réservé à agentd.** L'adaptateur écoute sur `/run/prophet/sup.sock`, dans le
   répertoire d'exécution des services, que le groupe système de Prophet peut écrire (l'humain
   en est membre) ; il pose ce groupe sur le socket, et n'admet que le compte `agentd`, attesté
   par `SO_PEERCRED`. Il ne décide d'aucun droit : c'est `agentd` qui fait trancher capd avant
   chaque appel.
3. **Des droits par application.** `ui.read` et `ui.act` désignent une application par le nom
   qu'elle se donne sur le bus (`mousepad`) : minuscules, sans joker. `screen`, `desktop`,
   `session` et leurs semblables ne sont pas des applications : un droit sur « tout ce qui
   s'affiche » n'existe pas. Un profil de mission qui nomme une application peut offrir les
   outils `ui.apps`, `ui.tree` et `ui.act` ; sans application nommée, il ne le peut pas.
4. **Trois outils.** `ui.apps` liste les applications et leur nombre de fenêtres, sans titre ;
   `ui.tree {app, window?, detail?}` exige `ui.read` sur l'application et rend l'arbre de la
   fenêtre active (ou désignée) avec sa provenance, sa confiance et sa réserve ; `ui.act {app,
   action, node, value?}` exige `ui.act` et rend ce qui s'est passé puis l'arbre résultant. Ils
   servent aux missions natives comme aux séances MCP (ADR 0026), et `prophet task attach /
   call / detach` les rend accessibles depuis un terminal.
5. **Un contexte « bureau ».** Le catalogue de l'image nomme l'éditeur de texte du bureau
   (Mousepad, GTK 3) avec `ui.read` et `ui.act` sur lui, et le lanceur l'ouvre (« Éditeur »).

## Conséquences

- Le critère du jalon M10-T4 est tenu, en local contre un vrai éditeur GTK sur un vrai bus
  (`tools/bureau-local.sh`) et dans le test du bureau : l'agent lit l'arbre, écrit « bonjour »
  dans le champ, active « Enregistrer » par le menu, et le fichier le contient.
- **Limite connue des applications GTK 3** : une action qui ouvre un dialogue modal
  (« Enregistrer sous… ») bloque leur pont d'accessibilité jusqu'à la fermeture du dialogue,
  parce que libdbus n'est pas réentrant. L'agent obtient alors « l'application ne répond pas »
  au bout du délai, et l'humain ferme le dialogue. Les applications GTK 4 (dialogues
  asynchrones, pont sur GDBus) n'ont pas cette limite ; c'est vers elles que va la suite.
- Lire coûte quelques appels D-Bus par nœud ; une fenêtre riche (sélecteur de fichiers) est
  tronquée et le dit. L'interface `Cache` d'AT-SPI permettrait une lecture en un appel.
- Le co-pilotage n'est pas encore signalé à l'humain dans la fenêtre (« un agent agit ici »),
  et l'adaptateur ne cloisonne pas les fenêtres par tâche : capd tranche par application.
- Les applications sans accessibilité (Blender, la plupart des jeux) restent hors de portée ;
  les logiciels Windows (AutoCAD, Photoshop) ne sont pas concernés par cette voie.
