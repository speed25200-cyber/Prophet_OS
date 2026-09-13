# Missions locales par agentd — 13 septembre 2026

## Verdict

Le service possède maintenant un lancement local réel, un arrêt pendant l'inférence et des
résultats persistants. **Le parcours avec Qwen3-0.6B n'est pas validé : trois essais échouent.**
Ce jalon ne rend pas Prophet OS entièrement fonctionnel et ne démontre aucune supériorité
sur une distribution Linux existante. Les critères complets de FRONTIER.md restent ouverts.

## Changements

- `task.start` exécute la mission en arrière-plan ; `task.result` rend le résultat conservé.
  La CLI propose `task new`, `start`, `result` et l'annulation effective.
- Les outils passent par les vrais capd et ledger, en processus séparés. La racine et la
  cible utilisées par capd ne peuvent pas être redéfinies dans le contexte envoyé par un pair.
- Les droits sont revérifiés avant les accès. Les scopes du plan bornent aussi les chemins,
  même si le jeton comporte des droits plus larges. La capture SFS vérifie chaque descendant,
  refuse les liens et ne crée aucun dossier utilisateur pour un périmètre absent.
- Les budgets comptent les tokens d'entrée et de sortie avant une action, y compris ceux
  d'une génération tronquée. La durée repose sur une horloge monotone. Une annulation abandonne
  la requête HTTP et bloque les prochains accès contrôlés.
- Un échec de journal avant l'action empêche celle-ci. Un échec après écriture bloque les
  appels suivants : le résultat est incertain et aucune répétition automatique n'est autorisée.
- La persistance crée le fichier en 0600 avant d'écrire, synchronise et renomme sous verrou.
  Une corruption conserve le fichier et bloque le démarrage ; un redémarrage transforme une
  mission active en échec explicite. Les résultats terminés restent disponibles.

Le contrat exact et ses limites sont dans l'[ADR 0013](../adr/0013-missions-locales-agentd.md)
et le [guide agentd](../../crates/agentd/README.md).

## Vérifications de composants et de service

La commande initiale de test a échoué avec `MethodNotFound` sur `task.start`. Le service
dispose désormais de cette méthode et d'un travailleur qui utilise un transport HTTP annulable.

Les tests de `local_daemon` utilisent de vrais daemons capd, ledger et agentd, dans des
répertoires temporaires. Le moteur des tests ordinaires est un serveur HTTP contrôlé ; ce
serveur ne mesure pas les capacités d'un LLM. Les cas couvrent :

1. moteur absent, échec visible et autres commandes disponibles ;
2. modèle en attente, consultation des tâches et annulation ;
3. révocation par le vrai capd entre l'inférence et l'action ;
4. budget dépassé avant l'écriture ;
5. panne de ledger avant l'écriture ;
6. réponse tronquée, consommation comptée et action refusée ;
7. refus d'un accès extérieur aux scopes malgré les grants ;
8. CLI de planification, lancement et résultat, fichier préparé, diff et relecture après redémarrage ;
9. redémarrage pendant l'inférence, sans tâche fantôme en cours.

Quatre tests SFS vérifient la copie et le diff initial, les liens et droits des descendants,
une racine privée détournée, et l'absence de création dans le home. Un test MCP vérifie la
panne du journal après une écriture : la première action est présente, la seconde est refusée.
Le test de corruption d'agentd a été corrigé : il exigeait auparavant la perte silencieuse
de l'état au redémarrage.

Le test de CLI a également révélé un binaire périmé : `cargo test --workspace` ne reconstruisait
pas le programme `prophet` destiné à être lancé depuis un autre crate. `just check` et la CI
construisent désormais les binaires du workspace avant les tests ; une ancienne CLI ne peut
plus fausser ce parcours.

Validation complète finale : `nix develop --command just check` réussit avec **604 tests,
aucun échec et 22 ignorés**, ainsi que format, clippy, construction des binaires, vérifications
des services/durcissement/documentation et recherche de secrets. Les neuf cas ordinaires du
parcours agentd passent en 0,37 seconde sur cette exécution. Ce temps avec un serveur contrôlé
n'est pas une mesure de performance d'inférence. Le test de modèle réel est ignoré par défaut
et ses trois exécutions explicites ci-dessous restent en échec.

## Essais avec le vrai modèle

Environnement : Ubuntu 24.04 sous WSL2, Rust 1.97.1 via le shell Nix du dépôt, llama-server
`b10809-5266f24`, Qwen3-0.6B-Q8_0 sur CPU, quatre threads, contexte 4 096.
Empreinte SHA-256 du modèle :
`9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031`.

Le scénario demande une écriture exacte contenant un identifiant aléatoire dans
`~/docs/note.txt`, puis une réponse finale. Le catalogue conserve `fs.read` et `fs.write`.
Le résultat attendu comprend le contenu exact, le diff, les événements du vrai journal et
la relecture du résultat après redémarrage du service.

| Essai | Configuration | Observation |
|---|---|---|
| 1 | Réglages du serveur, `--reasoning-budget 0` | `finish_reason=length`, 2 048 tokens générés ; test échoué en 77,64 s. Le budget restait à zéro : défaut de comptage corrigé ensuite. |
| 2 | `--reasoning off`, température 0,7, top-p 0,8, top-k 20, min-p 0, pénalité de présence 1,5 | Même motif d'échec, 2 424 tokens comptés ; test échoué en 82,91 s. |
| 3 | Désactivation explicite de `enable_thinking`, mêmes réglages, graine 2026 ; capture du protocole synthétique | Appel `fs.write` présent, mais `finish_reason=length` et texte altéré ; 2 422 tokens comptés. Test échoué en 76,44 s. |

Le troisième essai observe le protocole via un relais local de diagnostic réservé au scénario
synthétique. Il ne rend aucune réponse préfabriquée et ne modifie pas le catalogue. Le serveur
signale que `enable_thinking` est désormais une option dépréciée, équivalente au réglage
`--reasoning off`. Cette tentative ne constitue donc pas la preuve d'un autre mode de moteur.

Les paramètres d'échantillonnage suivent la [fiche officielle Qwen3-0.6B](https://huggingface.co/Qwen/Qwen3-0.6B),
qui recommande aussi une pénalité de présence contre les répétitions. Leur application n'a
pas résolu l'échec observé. La cause précise du mauvais arrêt reste à isoler entre modèle,
template et moteur. Accepter un appel malgré `finish_reason=length` masquerait une génération
incomplète ; ce contrôle est conservé. Aucun de ces échecs n'est présenté comme une réussite
agentique. L'ancien essai MCP à un seul outil reste une preuve plus étroite, distincte.

Commande de reproduction : voir le test `une_mission_reelle` du guide agentd. Utiliser le
serveur local et le modèle indiqués, sans interpréter `just check` comme l'exécution de ce
test ignoré par défaut. Tous les processus de ces essais sont arrêtés à la fin.

## État du produit et prochaines preuves

La capture native actuelle présente des missions et des compteurs ; le contenu du travail,
les diffs et les contrôles de lancement/arrêt restent à connecter à la vue. Cette livraison
prépare leurs données et commandes, sans prétendre améliorer à elle seule le graphisme.
L'évaluation visuelle reste ouverte et aucune équivalence avec la finition d'Apple n'est établie.

La CI relue pour `d7b5c90` réussit les composants, l'isolation et la surface ; son travail
ChatGPT échoue encore. [Exécution 34726143307](https://github.com/speed25200-cyber/Prophet_OS/actions/runs/34726143307).
Ce résultat porte sur la révision précédente. Le bureau installé, les sessions authentifiées
ChatGPT/Claude Code, la validation des résultats, la sécurité interservices, la reprise
durable, les GPU et les mesures comparatives restent des critères distincts non satisfaits.
