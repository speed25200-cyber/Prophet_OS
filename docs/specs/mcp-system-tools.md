# Spécification — Outils MCP système (v0)

- **Version** : 0.1
- **Crate** : `mcp-system`
- **Registre généré** : `/etc/prophet/mcp/system.json`

## Conventions

- Nom : `<domaine>.<verbe>`, en anglais, `snake_case`.
- Chaque outil déclare dans ses métadonnées : `requires` (res, act, cible dérivée des arguments), `irreversible` (bool), `external` (bool), `sandbox_level_min` (0, 1, 2 ou null).
- Descriptions destinées aux modèles : une phrase, précise, avec les contraintes ; exemples d'arguments dans le schéma.
- Résultats : toujours structurés (`content` de type `text` contenant du JSON, plus `structuredContent`), avec `truncated: true` et `total_bytes` quand un résultat a été coupé.
- Erreurs : `isError: true` avec `{ "code": "<PolicyDenied|ApprovalRequired|NotFound|SandboxError|BudgetExceeded|Invalid>", "detail": "…" }`. Un refus d'accès fichiers dit dans `detail` où la mission peut agir (motifs `fs` de son jeton) ; lire un répertoire ou lister un fichier rend `Invalid` avec l'outil qui convient (ADR 0050).
- Transport : stdio et socket Unix. Le registre déclare la commande stdio pour les clients d'éditeurs.

## Liste normative v0

| Outil | `requires` | irreversible | external | Notes |
|---|---|---|---|---|
| `fs.read` | fs.read chemin | non | non | `max_bytes` défaut et plafond 256 Kio ; `offset` et `next_offset` pour lire par morceaux, sans couper de caractère ; `lines`, nombre de lignes du contenu rendu |
| `fs.write` | fs.write chemin | non (réversible via sfs) | non | remplacement atomique dans le travail, contenu ≤ 1 Mio |
| `fs.list` | fs.list chemin et descendants | non | non | fusion travail/origine ; ≤ 2000 résultats |
| `fs.edit` | fs.write chemin (lecture sous fs.read) | non | non | `old` exact remplacé par `new`, une seule occurrence sauf `all: true` ; écrit dans l'espace de travail comme `fs.write` ; texte UTF-8, 1 Mio au plus ; rend `replaced` (ADR 0051) |
| `calc.eval` | tool.call | non | non | `expression` (nombres, + - * /, parenthèses, virgule décimale) ou `numbers` (somme, compte, moyenne, min, max) ; ne lit ni n'écrit rien (ADR 0053) |
| `fs.copy` | fs.read source, fs.write destination | non | non | `from` copié octet pour octet vers `path` dans l'espace de travail, dossiers créés ; 64 Mio au plus (ADR 0051) |
| `fs.stat` | fs.read chemin | non | non | |
| `fs.search` | fs.read racine et descendants | non | non | nom et contenu ; une racine qui est un fichier est fouillée seule ; un `name_contains` seul qu'aucun nom ne porte est cherché dans le contenu (`note`) ; ≤ 200 résultats ; par contenu, le nombre exact de lignes trouvées de chaque fichier (`matching_lines`) et les cinq premières (`matches` : numéro et extrait de 200 caractères au plus, `more_matches`) |
| `fs.diff_task` | task courante | non | non | |
| `proc.exec` | proc.exec binaire | selon commande | non | niveau de sandbox forcé à 2 hors liste blanche |
| `proc.kill` | task courante | non | non | |
| `http.fetch` | net.egress hôte | selon méthode (POST/PUT/DELETE = irréversible) | selon méthode | via egress, implémenté |
| `task.status` | task courante | non | non | |
| `task.diff` | task courante | non | non | |
| `task.commit_request` | task courante | oui | non | déclenche approbation si fichiers sensibles |
| `task.delegate` | task.spawn contexte | non | non | sous-mission à droits ⊆, autre contexte ou modèle, résultat rendu (ADR 0029) |
| `approval.request` | task courante | non | non | |
| `approval.wait` | task courante | non | non | |
| `ledger.query` | ledger.read (ou read_all) | non | non | |
| `ledger.replay_summary` | ledger.read | non | non | |
| `memory.remember` | memory.write espace | non | non | |
| `memory.search` | memory.read espace | non | non | |
| `memory.forget` | memory.write espace | oui | non | |
| `memory.list` | memory.read espace | non | non | |
| `secrets.list_refs` | task courante | non | non | noms seulement |
| `secrets.use` | tool.call secrets.use | non | non | rend un handle |
| `clock.now` | aucune | non | non | horloge de tâche (rejouable) |
| `notify.human` | tool.call notify.human | non | non | hors bande |
| `doc.read` | fs.read chemin | non | non | texte et métadonnées d'un PDF, document bureautique, image, média, page, archive ; format reconnu aux octets |
| `ui.apps` | tool.call ui.apps | non | non | applications et nombre de fenêtres, sans titre |
| `ui.tree` | ui.read app | non | non | SUP, provenance accessibilité |
| `ui.act` | ui.act app + `requires` de l'action | selon action | selon action | SUP |
| `ui.screenshot` | ui.vision | non | non | confiance basse, hors défaut |
| `model.list` | aucune | non | non | clients officiels et poids locaux (architecture, quantification, fenêtre d'entraînement) ; pour chacun, `memory` : ce que le moteur réservera à la fenêtre `local_context` (`weights`, `kv_cache`, `compute`, `total`, en octets) et `fit` face à `system_memory` (`fits`, `tight`, `too_large`) ; `template` : ce que le gabarit de conversation du fichier déclare (`tool_calls`, `reasoning`) ; `resident` : ce que l'instance du moteur qui le sert tient en mémoire (`rss`, `anonymous`, `file`), s'il est servi ; `recommended_local_weight` : le plus gros poids qui déclare les outils et tient dans la mémoire disponible, sinon `null` |
| `model.status` | aucune | non | non | |

Un outil sans `requires` explicite (autre que `aucune`) est refusé par le test `mcp_system::registry::all_tools_declare_requires`.

## Accès fichiers actuellement implémentés

Les chemins logiques sont relatifs au home de la tâche, sous la forme `~/docs/note.txt`,
`docs/note.txt` ou d'un chemin absolu inclus dans ce home. Les `..`, chemins hors du home,
noms `.prophet` et `.prophet-write-*` sont refusés. Les noms non UTF-8 sont omis des parcours.
Le contexte vient du lanceur de confiance : son `workdir` doit correspondre exactement à
`home/.prophet/tasks/<tâche>/work`. Une cohérence de chemins ne prouve pas l'identité du lanceur.

Les accès relatifs utilisent `openat2` avec `RESOLVE_BENEATH`, `RESOLVE_NO_SYMLINKS` et
`RESOLVE_NO_XDEV`. Les racines sont ouvertes sans lien symbolique. Les fichiers spéciaux et
les lectures de fichiers ayant plusieurs liens physiques sont refusés. L'absence de cette
primitive noyau provoque un échec ; aucun repli par canonicalisation/réouverture n'est permis.
Le travail a priorité sur l'origine, et seule une absence autorise le repli. Les écritures
créent leurs parents dans le travail, publient par renommage atomique et synchronisent fichier
et répertoire. Elles ne valident pas la transaction SFS.

La lecture de contenu est bornée à 256 Kio rendus (un octet supplémentaire sert à détecter la
troncature). `max_bytes` doit être un entier positif ou nul et ne peut relever le plafond.
La recherche lit au plus 8 Mio cumulés. Liste et recherche limitent les visites à 10 000, les
résultats sérialisés à 512 Kio et vérifient un budget de deux secondes entre les opérations ;
une opération noyau lente peut dépasser ce temps. La recherche descend jusqu'à 32 niveaux
après sa racine. Les plafonds de résultats sont respectivement 2000 et 200. Les omissions
dues aux plafonds donnent `truncated: true`. `scoped: true` signifie que les descendants sans
capacité ne sont pas énumérés dans la réponse ; aucun total de fichiers interdits n'est rendu.
Les limites du transport s'appliquent en plus de ces limites de contenu.

Le contexte fiable, les racines privées, le confinement des processus et les commits/undo
SFS concurrents restent des conditions d'intégration, détaillées dans l'[ADR 0012](../adr/0012-acces-fichiers-mcp.md).

## Ce que le journal retient d'un appel

Chaque `tool.call` porte le nom de l'outil, la capacité exigée, la cible contrôlée (hôte,
chemin absolu ou fenêtre) et une empreinte des arguments avec leur taille ; jamais le
contenu des arguments. Chaque `tool.result` porte l'issue et une empreinte du résultat. C'est
ainsi que l'humain lit où l'agent est allé sans que le journal contienne ce qu'il a lu ou écrit.

## Lecture des formats actuellement implémentée

`doc.read {path, max_chars?, ocr?}` lit un fichier sous les règles et le droit de `fs.read`
(jusqu'à 64 Mio) et rend `kind`, `text` (au plus 256 Kio, `truncated` sinon), `meta` et
`notes`. Le format vient des premiers octets, jamais du nom : PDF par `pdftotext` et `pdfinfo`
(pages, titre) ; `docx`, `xlsx`, `pptx`, `odt`, `ods`, `odp` par leur archive et leur XML, en
Rust pur ; images par leurs en-têtes (format, largeur, hauteur) et leur texte reconnu par
`tesseract` (`fra+eng`) quand il est là et que `ocr` n'est pas faux ; vidéos et sons par
`ffprobe` (durée, conteneur, flux, étiquettes) ; HTML dépouillé de ses balises, scripts et
styles ; archives zip et tar listées ; texte brut sinon, avec `json`, `csv`, `markdown` et
`xml` distingués. Un programme absent ou trop long (30 s) est dit dans `notes`, jamais
attendu indéfiniment ; les octets passent par un fichier temporaire privé, effacé après. Aucun
réseau, aucun programme choisi par l'agent. Voir l'[ADR 0028](../adr/0028-lecture-des-formats-par-un-outil-natif.md).

## Introspection actuellement implémentée

Dans une mission d'agentd, comme dans la séance d'outils d'un client de l'humain,
`task.status {}` rend la tâche, l'agent, l'étape, le niveau d'isolation, le dossier de travail,
les capacités accordées et le budget : `limits`, `spent` et `remaining` en étapes, tokens et
secondes. `task.diff {}` rend les fichiers ajoutés, modifiés et supprimés dans le travail de la
tâche, et leur rendu, sans rien appliquer. Tous deux exigent `tool.call` sur leur nom, ne lisent
que la mission elle-même et ne changent rien : un agent qui sait ce qu'il lui reste et ce qu'il
a déjà fait choisit ses étapes au lieu de heurter le plafond. Les profils de l'image et des
exemples les accordent.

## Commandes actuellement implémentées

`proc.exec {program, args?, level?, timeout_s?}` exige `proc.exec` sur le programme tel que
demandé : le nom nu, résolu par le chemin du service, ou un chemin absolu (jamais relatif ni
remontant). Il tourne par `sandbox.run` de sandboxd dans l'espace de travail de la tâche, le
home lisible selon le jeton et jamais inscriptible. La liste blanche (`cat`, `ls`, `wc`, `head`,
`tail`, `sort`, `uniq`, `grep`, `rg`, `cut`, `tr`, `diff`, `file`), par le nom nu seulement,
tourne au niveau 0 sans décision humaine ; tout autre programme, et tout chemin même nommé
`cat`, exige le niveau 2 et est irréversible. Rend `exit_code`, `stdout` (256 Kio au plus,
`truncated` sinon), `stderr`, `timed_out`, `elapsed_ms` (lancement compris) et `warm_start`
(la microVM venait de la réserve, ADR 0045). Sans sandboxd, l'outil le dit et ne lance rien. Voir
l'[ADR 0031](../adr/0031-execution-de-programmes-sous-sandboxd.md).

## Sortie réseau et navigation actuellement implémentées

`http.fetch` ne joint jamais le réseau lui-même : il écrit la requête sur le socket du proxy
`egress` avec le jeton de la tâche dans l'en-tête interne `Proxy-Authorization: Prophet …`,
que le proxy retire avant la sortie après avoir fait trancher capd sur l'hôte réellement joint.
Seuls `http://` et `https://` sont relayés. `GET` et `HEAD` sont des lectures réseau
automatiques ; toute autre méthode est irréversible et externe, donc soumise à décision humaine,
dans le registre comme dans le proxy. La réponse est recomposée si elle est segmentée et bornée
à `max_bytes` (256 Kio par défaut, 1 Mio au plus) ; `truncated` le signale. Un refus du proxy
est rendu avec son code (`PolicyDenied` pour 403 et 407, `Invalid` pour 400 et 413, `NotFound`
pour 502, `SandboxError` sinon). Sans socket configuré, l'outil échoue sans émettre de requête.

Les outils `web.*` s'adossent au pont CDP (`browser-bridge`) et n'existent que si le service
nomme un programme de navigateur. `web.open {url, detail?}` exige `net.egress` sur l'hôte et
rend l'arbre SUP de la page ; `web.tree {detail?}` exige `ui.read browser` ; `web.act {action,
node?, value?}` exige `ui.act browser`, avec `click`, `set_field` et `submit` ; seul `submit`
est externe et demande une décision. Le profil du navigateur est propre à la tâche, dans l'état
privé du service, où les outils déposent aussi l'observation courante (adresse, titre, nombre
de nœuds) que `task.inspect` rend à la supervision. Dans le service, tout le trafic du
navigateur passe par un relais local vers le socket d'egress, sous le jeton de la tâche ;
sans egress, le navigateur n'a aucune route. Voir l'[ADR 0024](../adr/0024-navigateur-integre-et-applications-web.md).

## Applications de bureau actuellement implémentées

Les outils `ui.*` s'adossent à l'adaptateur d'accessibilité de la session (`prophet-supd`,
ADR 0027) et n'existent que si le service connaît son socket (`PROPHET_SUP_SOCKET`). `ui.apps`
liste les applications qui exposent une interface et leur nombre de fenêtres, sans titre ;
`ui.tree {app, window?, detail?}` exige `ui.read` sur le nom de l'application et rend l'arbre
SUP de sa fenêtre active (ou désignée), avec `provenance: accessibility`, une confiance
inférieure à 1 et une réserve à l'intention du modèle, plus `truncated` si la lecture a été
bornée ; `ui.act {app, action, node, value?, window?, detail?}` exige `ui.act` sur
l'application, avec `click`, `set_field` et `toggle`, et rend `message` puis `observation`.
L'application est désignée par le nom qu'elle se donne sur le bus (`mousepad`) ; ni joker, ni
écran. Aucune capture d'écran : `ui.screenshot` n'est pas implémenté.

## Fichier de registre

```jsonc
{
  "mcpServers": {
    "prophet": { "command": "/run/current-system/sw/bin/prophet-mcp", "args": [], "env": { "PROPHET_TASK": "<mission>" } }
  }
}
```

`prophet task mcp-config <mission>` rend ce fichier pour une mission préparée par le même
utilisateur ; il est consommé tel quel par les clients d'éditeurs (`--mcp-config` de Claude
Code, `mcp_servers` de Codex). Le pont `prophet-mcp` ne tient aucun jeton : `initialize`
attache la mission (`task.attach`), `tools/list` et `tools/call` sont relayés à `agentd`
(`task.tools`, `task.call`) qui exécute les outils avec le jeton, le travail SFS et le journal
de la mission, et la fin de l'entrée retire le client (`task.detach`), ce qui scelle les
versions pour l'examen du créateur. Voir l'[ADR 0026](../adr/0026-seance-d-outils-mcp-pour-les-clients-de-l-humain.md).
