# ChatGPT Linux : paquet de compatibilité NixOS

Le paquet expérimental `packages.x86_64-linux.chatgpt-linux` utilise l'application officielle
OpenAI 26.908.40834. La source est le paquet amd64 versionné du dépôt de l'éditeur :

`https://persistent.oaistatic.com/codex-app-prod/linux/deb/pool/main/c/chatgpt/chatgpt_26.908.40834_amd64.deb`

SHA-256 : `da37b8e7bcefaaea019c478cacbe6c73ee1ddd15e0e1ebb3c7ef0a42dd818ac2`.
L'index Packages de ce dépôt annonce la même version et la même empreinte. L'archive téléchargée
a passé le contrôle de Nix. Aucun script d'installation Debian n'est exécuté sur l'hôte.

Les fichiers sont extraits sans correction ELF, remplacement d'Electron ou modification de
l'application. `buildFHSEnv` fournit les bibliothèques et chemins Linux usuels au lanceur d'origine.
La comparaison du binaire principal extrait de l'archive et de celui du magasin Nix donne
la même empreinte : `a5cc07d23381d8e9dcd2d91d6ff350a927a8c844cb99151bce6e937def39d6a6`.
L'environnement FHS assure la compatibilité ; il n'apporte pas les garanties de sandboxd, capd
et egress. Ce paquet n'est pas activé dans l'image installée tant que le bureau humain reste
à intégrer.

## Commandes et périmètre du test

```sh
nix build .#chatgpt-linux
nix build .#checks.x86_64-linux.chatgpt-desktop --print-build-logs
```

Le test construit une VM NixOS avec un compte ordinaire, Sway, XWayland et un rendu logiciel.
La connexion automatique est limitée à cette fixture. Le test bloque les sorties Internet de
la VM avant de lancer ChatGPT, ne connecte aucun compte et ne lit aucun fichier d'identifiants.
Il exige une fenêtre ChatGPT visible dans l'arbre du compositeur et un processus appartenant au
compte ordinaire. Il capture l'écran puis demande au compositeur de fermer la fenêtre.

Résultat de l'exécution graphique : en cours de vérification.

Cette vérification ne couvre pas une conversation authentifiée, les outils Codex, l'ouverture
de projets, les portails, le trousseau, les mises à jour, la reprise ou les performances GPU.
Ces parcours restent des conditions de livraison. L'application Linux est en aperçu et NixOS
ne figure pas parmi les distributions officiellement prises en charge ; XWayland est utilisé
ici conformément au mode de compatibilité documenté. Voir la
[documentation officielle](https://learn.chatgpt.com/docs/linux/linux-app) et l'
[ADR 0009](../adr/0009-clients-officiels-et-bureau.md).
