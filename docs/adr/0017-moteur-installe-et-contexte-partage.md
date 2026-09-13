# ADR 0017 — Moteur installé et contexte documentaire partagé

Date : 13 septembre 2026. Statut : intégration en cours de vérification.

## Contexte

La chaîne graphique sait préparer et lancer une mission, mais l'image ne configurait aucun
moteur ni profil. Les services supposaient `/home/prophet`, même lorsque l'installation créait
un propriétaire portant un autre nom. Les essais sous un même UID ne vérifiaient pas les
permissions Unix de cette chaîne.

## Décision

Le module `local-engine.nix`, importé par le module Prophet, installe le moteur corrigé de
l'ADR 0016 et un profil `Documents Prophet`. Un fichier GGUF choisi dans la configuration
active une unité `prophet-local-engine`. Le démarrage ne télécharge aucun poids. Sans fichier
configuré, le profil reste visible mais aucun modèle ne doit être inventé dans le catalogue.

Le moteur utilise un compte distinct, une écoute sur la boucle locale et un système de
fichiers en lecture seule. Il n'appartient pas au groupe des services Prophet. Cette première
configuration utilise le CPU, un seul créneau de génération et des paramètres Qwen3 sans
raisonnement. Elle ne représente pas un réglage optimal pour tous les modèles.

Le dialogue et agentd reçoivent le même endpoint. capd et agentd reçoivent le home déclaré du
propriétaire. Le profil n'autorise que `fs.read` et `fs.write`, dans `~/Documents/Prophet`.
Des ACL permettent à agentd de capturer ce contexte ; le travail reste privé dans
`~/.prophet/tasks`, sous l'identité du service. Il ne s'applique pas aux originaux.

Le manifeste du profil est une configuration administrative locale. Sa clé d'éditeur de
structure n'est pas une preuve de signature : la vérification cryptographique et le contrôle
des méthodes par identité restent des exigences ouvertes. Le compte humain peut administrer
le système ; aucun processus non fiable ne reçoit l'identité d'agentd dans ce parcours.

## Vérification prévue

- Test graphique explicite avec poids réels : saisie, préparation, lancement, rendu pendant
  l'inférence et vérification exacte du fichier, avec vrais agentd, capd et ledger.
- VM NixOS : modèle initialement absent, chargement réel sous systemd, propriétaire `pilot`,
  fichier privé inaccessible au service, mission réelle, résultat identique après redémarrage.
- Tests ordinaires et captures contrôlées conservés ; aucun faux serveur dans les essais réels.

Les résultats et commandes sont consignés dans le [rapport d'intégration](../reports/moteur-installe-2026-09-13.md).

## Limites

La session graphique de l'image reste le kiosque Cage. La VM de ce jalon exerce les services,
pas une session humaine graphique complète. Le choix et le téléchargement des poids depuis
l'interface, la gestion GPU, les files de requêtes, les clients authentifiés, le contenu des
diffs et leur validation humaine restent à intégrer. Aucun critère FRONTIER complet n'est acquis.
