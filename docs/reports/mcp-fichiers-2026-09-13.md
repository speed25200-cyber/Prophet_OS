# Accès fichiers MCP et première liaison au modèle local

Essais du 13 septembre 2026, sur la branche de développement après `bee035d`, sous WSL2 /
Ubuntu 24.04, avec le shell Nix épinglé et Rust 1.97.1. Tous les fichiers manipulés par les
tests sont temporaires. Aucune donnée utilisateur ni session de client officiel n'est utilisée.

## Régressions et comportement corrigé

Avant la correction, les six tests initiaux de `fichiers_isoles` ont tous échoué : lecture
par lien final/ancêtre, repli après lien cassé, écriture par lien, recherche de descendants
sans droit, lecture au plafond fourni par le modèle et liste oubliant les fichiers d'origine.
Après correction, les 12 tests ordinaires de ce fichier passent. Les cas supplémentaires
exercent des opérations valides, les fichiers spéciaux, le refus de confondre home et travail,
la troncature de recherche, 300 remplacements concurrents d'un ancêtre par un lien et la
révocation des droits de l'exécuteur natif.

```sh
nix develop --command cargo test -p mcp-system
```

Résultat : **52 tests réussis, aucun échec, un test avec modèle réel ignoré** dans cette
commande. L'essai ignoré est exécuté séparément ci-dessous. Les appels restent contrôlés par
le Broker, puis journalisés ; les outils fichiers recontrôlent les descendants. Les accès
physiques utilisent des descripteurs et les restrictions noyau décrites dans l'[ADR 0012](../adr/0012-acces-fichiers-mcp.md).

Validation complète : `nix develop --command just check` se termine avec le code 0 : **589
tests réussis, aucun échec, 21 ignorés**, format, clippy avec avertissements interdits,
cohérence des services, durcissement déclaré, documentation des travaux et contrôle gitleaks
réussis. Les tests ignorés conservent leurs prérequis matériels ou de services externes ;
l'essai réel ci-dessous ne remplace pas ces validations.

## Parcours avec des poids réels

Serveur llama.cpp 0.4.0 du nixpkgs épinglé, modèle `Qwen3-0.6B-Q8_0.gguf` déjà vérifié lors
du [premier essai d'inférence](local-inference-2026-09-12.md). Quatre threads CPU et contexte de
4096 tokens ; aucun GPU d'inférence. Le serveur de cet essai écoute seulement sur loopback
et est arrêté après le test.

```sh
llama-server -m /chemin/Qwen3-0.6B-Q8_0.gguf --alias qwen3-0.6b \
  --host 127.0.0.1 --port 18080 -c 4096 -t 4 --jinja --reasoning-budget 0

PROPHET_TEST_ENDPOINT=http://127.0.0.1:18080/v1 PROPHET_TEST_MODEL=qwen3-0.6b \
  nix develop --command cargo test -p mcp-system --test fichiers_isoles \
  un_modele_reel_ecrit -- --ignored --nocapture
```

Le modèle reçoit la description réelle de `fs.write`, filtrée par `RegistryExecutor`, et
l'intention d'écrire un identifiant aléatoire dans `~/docs/note.txt`. Il génère l'appel d'outil,
le registre vérifie un jeton signé par le Broker de l'essai, et l'outil publie le fichier dans
l'espace SFS de la tâche. Le résultat est renvoyé au modèle, qui termine. Le test vérifie une
consommation non nulle, le succès de l'outil, le statut final, le contenu exact du fichier et
le diff calculé par SFS. Le fichier n'existe pas dans le home hors espace de travail.

Résultat observé : **réussite en 10,50 secondes pour la boucle**, huit événements du pilote,
deux entrées du journal, un fichier ajouté de 22 octets, aucun fichier modifié ou supprimé.
La commande de test se termine en 10,57 secondes. Ce résultat unique ne mesure ni une latence
p95 ni un avantage comparatif face à une autre distribution.

## Portée de la preuve

Le parcours vérifie `LocalModel → NativeDriver → RegistryExecutor → Registry/Broker → fs.write`
puis `Workspace::diff`, dans un seul processus de test avec des racines temporaires fiables.
L'inférence est réelle. Le Broker et le journal sont en mémoire ; les sockets de capd/ledger,
la persistance, agentd et les commandes de l'interface ne font pas partie de cet essai.
Le jeton et le contexte ne sont jamais fournis au modèle.

Les racines doivent encore être établies et protégées par le lanceur, et le confinement des
processus reste indispensable. Les commits, undo et diffs SFS concurrents nécessitent leur
propre durcissement. Le binaire `prophet-mcp` reste donc désactivé. Les exigences de livraison
de [FRONTIER.md](../FRONTIER.md) restent ouvertes ; aucun parcours installé complet ni statut
SOTA n'est déduit de ces tests.

## CI de la révision précédente

Les résultats relus pour `bee035d` valident les composants, l'isolation et le rendu de la
surface ; le [travail ChatGPT échoue encore](https://github.com/speed25200-cyber/Prophet_OS/actions/runs/34724773360).
Le [workflow d'image](https://github.com/speed25200-cyber/Prophet_OS/actions/runs/34724770934)
réussit les services, l'installeur, les deux constructions et les deux démarrages. La question
ouverte de racine en lecture seule est ignorée par ce workflow. Le délai de session observé
sur `f132518` ne s'est donc pas reproduit dans cette exécution ; sa cause n'est pas établie.
Ces résultats portent sur `bee035d`, pas sur le nouveau correctif MCP.
