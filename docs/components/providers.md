# Pilotes locaux, ChatGPT/Codex et Claude Code

État vérifié le 12 septembre 2026. L'intégration complète de ChatGPT et Claude Code fait partie
des exigences de Prophet OS ; elle n'est pas encore livrée.

| Composant | Preuve acquise | Reste à livrer |
|---|---|---|
| LLM local | HTTP, flux annulable et réponse Qwen3 réelle | Cycle de vie des poids, GPU et chaîne agentique installée |
| Codex CLI 0.153.4 | Paquet Nix officiel, version et diagnostic en profil vierge | Connexion utilisateur et exécution contrôlée depuis l'interface |
| Claude Code 2.1.266 | Paquet Nix officiel, version et diagnostic en profil vierge | Connexion utilisateur, outils, permissions et reprise de bout en bout |
| ChatGPT graphique | Documentation officielle Linux vérifiée | Paquet compatible NixOS, session de bureau et tests fonctionnels |
| Gemini CLI | Profil de commande | Version et protocole à revalider ; connexion non vérifiée |

Le paquet officiel ChatGPT Linux téléchargé pour préparer la compatibilité est la version
`26.908.40834`, architecture amd64. Empreinte SHA-256 du `.deb` :
`da37b8e7bcefaaea019c478cacbe6c73ee1ddd15e0e1ebb3c7ef0a42dd818ac2`.
Il n'est pas installé dans l'image. L'attribut `pkgs.chatgpt` du nixpkgs épinglé vise seulement
macOS ; l'ajouter directement ne fournirait pas l'application Linux.

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
le compte humain. Une exécution VM de la nouvelle révision est nécessaire pour valider l'image.

Pour la suite : [décision de bureau](../adr/0009-clients-officiels-et-bureau.md) et
[exigences de version complète](../FRONTIER.md). Aucune conversation authentifiée ni mesure
de performance de ces clients dans Prophet OS installé n'est revendiquée ici.

Le [rapport du paquet ChatGPT Linux](../reports/chatgpt-linux-2026-09-12.md) décrit la source
épinglée, le runtime de compatibilité et la vérification graphique dédiée.
