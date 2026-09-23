# Pilotes locaux, ChatGPT/Codex et Claude Code

État vérifié le 12 septembre 2026. L'intégration complète de ChatGPT et Claude Code fait partie
des exigences de Prophet OS ; elle n'est pas encore livrée.

| Composant | Preuve acquise | Reste à livrer |
|---|---|---|
| LLM local | HTTP, flux annulable et réponse Qwen3 réelle | Cycle de vie des poids, GPU et chaîne agentique installée |
| Codex CLI 0.153.4 | Paquet Nix officiel, version et diagnostic en profil vierge | Connexion utilisateur et exécution contrôlée depuis l'interface |
| Claude Code 2.1.266 | Paquet Nix officiel, version et diagnostic en profil vierge | Connexion utilisateur, outils, permissions et reprise de bout en bout |
| ChatGPT graphique | Paquet officiel FHS, écran de connexion visible en VM, plugins initialisés | Erreur Fontconfig secondaire, session de bureau et parcours authentifiés |
| Gemini CLI | Profil de commande | Version et protocole à revalider ; connexion non vérifiée |

Le paquet officiel ChatGPT Linux téléchargé pour préparer la compatibilité est la version
`26.908.40834`, architecture amd64. Empreinte SHA-256 du `.deb` :
`da37b8e7bcefaaea019c478cacbe6c73ee1ddd15e0e1ebb3c7ef0a42dd818ac2`.
Il n'est pas installé dans l'image. L'attribut `pkgs.chatgpt` du nixpkgs épinglé vise seulement
macOS ; l'ajouter directement ne fournirait pas l'application Linux.

## Jev, le décideur rapide

[Jev](../specs/jev-decisions.md) (TypeSafe AI) ne génère pas de texte : il répond à des
questions fermées sur un état structuré, avec une probabilité calibrée, en quelques centaines
de millisecondes. `providers::jev` porte le protocole, un transport par le proxy de sortie
(jeton de la tâche, référence de secret `prophet-secret:<nom>`, jamais la clé), un opérateur
d'interface qui décide `web.act` sur l'arbre SUP et rend la main au modèle génératif quand il
faut écrire, et un routeur qui départage les modèles admissibles à la planification. Voir
l'[ADR 0042](../adr/0042-jev-decideur-rapide.md) et le [rapport](../reports/jev-2026-09-17.md).

| Composant | Preuve acquise | Reste à livrer |
|---|---|---|
| Protocole et validation | corps documenté, réponses confrontées à la demande, bornes appliquées (tests unitaires) | premier appel réel, sur un hôte où les daemons tournent sous leurs comptes (`tools/lancer-sur-l-hote.sh`, puis `prophet secret put` et `prophet jev route`) |
| Transport par egress | requête sur socket avec jeton et référence, refus du proxy et codes 401/422/429/529 nommés (faux proxy) | mesure de latence réelle |
| Opérateur d'interface | page réelle opérée sous Chromium avec capd et ledger réels, sans modèle génératif (`cargo test -p agentd --test jev`) | outils `ui.*`, valeurs au-delà des guillemets |
| Routeur | choix parmi les admissibles, `local-only` jamais envoyé, repli statique (tests avec proxy simulé) | calibration observée sur de vraies demandes |

Configuration : `prophet.jev.enable = true` sur l'image, ou `PROPHET_JEV_SECRET` pour agentd et
`PROPHET_EGRESS_QUERY_HOSTS=api.typesafe.ai` pour egress (sur un hôte : `/etc/prophet/agentd.env`
et `/etc/prophet/egress.env`) ; puis `prophet secret put typesafe --host api.typesafe.ai < clé`
et `prophet jev status`. `prophet jev route mission.json` montre la route sans planifier ; Jev
n'est consulté que s'il y a au moins deux candidats admissibles. Le coffre ne révèle la clé
qu'au compte `egress` : sans lui, la route retombe en le disant, et rien ne part.

## Diagnostic utilisable

```sh
prophet provider ls
prophet --json provider doctor codex
prophet --json provider doctor claude-code
prophet provider login codex
prophet provider login claude-code
```

`login` affiche les commandes de préparation et de connexion à exécuter dans un terminal humain.
Il ne lance pas de connexion en arrière-plan. Le répertoire est
`$HOME/.local/state/prophet/providers/<pilote>/<utilisateur>` ; le compte humain peut le créer
sans privilège. Les instructions demandent un mode 0700. Le client gère exclusivement le contenu.

Codex emploie `codex login` et `codex login status`. Claude Code emploie `claude auth login` et
`claude auth status`. Le diagnostic différencie `client_missing`, `login_required`, `connected`,
`unknown` et `probe_failed`. `connected` signifie que le client annonce une connexion ; cela ne
prouve ni le type d'abonnement, ni un quota disponible, ni la réussite d'une génération.
Sources : [authentification Codex](https://learn.chatgpt.com/docs/auth),
[commandes Claude Code](https://code.claude.com/docs/en/cli-reference).

La sortie JSON contient `agent_execution_ready: false` tant que le raccordement n'est pas livré.
Les capacités du pilote ne promettent ni flux, ni reprise, ni estimation de quota opérationnels.

## Options vérifiées sur les vrais binaires

Le profil Codex construit `codex exec --json -- <intention>` ou
`codex exec resume --json -- <session> <intention>`. Les arguments positionnels restent après
le séparateur, même lorsqu'ils commencent par un tiret. Le profil Claude Code inclut `-p`,
`--output-format stream-json`, `--verbose` et `--include-partial-messages`, et place le prompt
après les options. Aucun contournement des permissions n'est ajouté.
Les valeurs de configuration MCP et de reprise Claude sont liées à leur option avec `=` :
une valeur commençant par `--` ne devient pas une nouvelle option du client.

L'intégration Codex interactive visera son
[App Server](https://learn.chatgpt.com/docs/app-server), pour pouvoir acheminer les événements
et les décisions d'approbation. Ce transport n'est pas encore implémenté dans Prophet.

## Vérification reproductible

Les paquets viennent du nixpkgs épinglé dans `flake.lock`, révision
`8ce4ef6cb6f871616146b9fe26d2a5ae594e94fe`. L'attribut `codex` désigne bien
`https://github.com/openai/codex`. Le module d'image exige maintenant ces deux clients.

Après construction des paquets correspondants, exécuter depuis le shell Nix :

```sh
PROPHET_TEST_CODEX=/chemin/du/paquet/bin/codex \
PROPHET_TEST_CLAUDE=/chemin/du/paquet/bin/claude \
cargo test -p providers --lib needs_official_clients_versions_et_sessions_vierges -- --ignored --nocapture
```

Résultat local : un test réussi ; versions `codex-cli 0.153.4` et `2.1.266 (Claude Code)` ; les
deux profils vierges renvoient `login_required`. Les tests de substitution vérifient aussi les
codes de retour, l'absence de sortie privée publiée, le délai et la borne de taille.
La CLI compilée a également passé `doctor`, `login` et `ls` au format JSON dans des répertoires
utilisateur temporaires, avec les vrais clients ; un pilote inconnu est refusé.
Le test NixOS `checks.x86_64-linux.services` exige les deux binaires et le diagnostic depuis
le compte humain. Ce contrôle a réussi dans la CI des révisions `0f3f307` et `f8e263e`.

Pour la suite : [décision de bureau](../adr/0009-clients-officiels-et-bureau.md) et
[exigences de version complète](../FRONTIER.md). Aucune conversation authentifiée ni mesure
de performance de ces clients dans Prophet OS installé n'est revendiquée ici.

Le [rapport du paquet ChatGPT Linux](../reports/chatgpt-linux-2026-09-12.md) décrit la source
épinglée, le runtime de compatibilité et la vérification graphique dédiée.

## Catalogue des poids

`providers::weights` lit l'en-tête GGUF de chaque fichier de poids — architecture, nom, taille
annoncée, quantification (`general.file_type`), fenêtre de contexte et nombre de couches —
sans charger les poids : les tableaux du tokeniseur sont sautés, chaque compte et chaque
longueur est borné, un fichier corrompu ou fabriqué est refusé avec sa raison plutôt que de
faire allouer ou boucler. Le catalogue d'une machine réunit le dossier des poids et les
fichiers que `PROPHET_WEIGHTS` nomme ; le module du moteur local pose cette variable pour les
poids qu'il sert. `prophet model ls` et la page Modèles de l'atelier le lisent ;
`LocalModel::served` lit dans `/props` de llama-server le fichier chargé et la fenêtre servie,
que `prophet model ls` place en face du fichier (un routeur de modèles n'est pas interrogé
modèle par modèle : cela en chargerait un).

## Fenêtre de contexte du moteur

`AsyncLocalModel` lit le refus `exceed_context_size_error` de llama-server — ses deux nombres,
jamais le reste du corps —, resserre les résultats d'outils de ce qui part au moteur
(`local::fit` : anciens condensés, puis réduits à leur issue, dernier tronqué avec un avis) et
renvoie le tour, trois envois au plus ; la fenêtre apprise sert ensuite d'emblée. La
conversation en flux (`ChatClient`) oublie de même ses plus anciens messages, consigne et
dernière question gardées, et `Completion::forgotten` dit combien ; la page Conversation
l'affiche. Prouvé contre des serveurs de test qui rendent le refus exact de llama-server
(`tests/local.rs`, `tests/stream.rs`) ; l'essai NixOS du moteur vérifie la forme de ce refus
sur le moteur épinglé (ADR 0034, complément du 22 septembre).

## Paquet du moteur local et essai agentd

`nix build .#llama-cpp` construit le moteur du nixpkgs épinglé avec un correctif du
générateur de grammaire. `nix build .#checks.x86_64-linux.llama-tool-grammar` vérifie
le parseur et la grammaire sans charger de poids. La CI possède un travail distinct
« Moteur local (protocole) » ; sa réussite ne prouve pas une inférence réelle.

Pour exercer plusieurs missions avec les mêmes vrais poids, après construction des
binaires du workspace et dans `nix develop` :

```sh
python3 tools/verifier-moteur-local.py \
  --llama-server /chemin/du/paquet/bin/llama-server \
  --weights /chemin/Qwen3-0.6B-Q8_0.gguf --model qwen3-0.6b \
  --output /chemin/rapport-nouveau --repetitions 3
```

Le dossier de sortie doit être nouveau. Le script calcule l'empreinte des poids, lance
le serveur local qu'il possède, attend sa disponibilité, puis exerce le test agentd
avec capd, ledger, fichier exact et résultat relu après redémarrage. Il conserve les
échecs et arrête le serveur à la fin. Ses réglages de génération correspondent à
l'essai Qwen3 décrit dans le [rapport](../reports/grammaire-locale-2026-09-13.md) ;
ils ne constituent pas des réglages optimaux pour toutes les familles de modèles.
Le moteur et les poids ne sont pas encore provisionnés dans la session humaine installée.
