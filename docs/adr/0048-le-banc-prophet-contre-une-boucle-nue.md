# ADR-0048 — Mesurer Prophet contre une boucle nue, même modèle, mêmes outils

- **Statut** : accepté
- **Date** : 2026-09-23
- **Tâche liée** : M13-T1, M13-T4 (FRONTIER : « comparaison avec une base Linux utilisant les
  mêmes modèles et les mêmes tâches »)

## Contexte

La suite de tâches (M13-T1, `bench::tasks`) existe depuis le 12 septembre, chaque tâche avec son
vérificateur, mais n'avait jamais été jouée par un vrai modèle : `just bench` répondait « pas
encore disponible ». M13-T2 prévoit une ligne de base « pixels » (un agent par captures d'écran
sur Ubuntu) ; elle exige un agent de vision et un compte, et reste bloquée. FRONTIER demande,
lui, une comparaison avec une base Linux utilisant les mêmes modèles et les mêmes tâches, en
mesurant réussite, latences médiane et p95, tokens et interventions humaines.

## Décision

- **Deux chemins, un seul modèle.** Le banc (`crates/bench/tests/suite_reelle.rs`) joue chaque
  tâche sans navigateur deux fois, avec le modèle de l'image (Qwen3 1.7B en Q8_0, tiré du
  catalogue par egress) servi par le llama-server épinglé comme l'image le lance :
  - **par Prophet** : une chaîne capd, ledger et agentd neuve par tâche ; la mission est
    planifiée (`task.spawn`, portée limitée au dossier de la tâche), lancée, attendue, puis
    publiée (`task.apply`) comme un humain le ferait, et le vérificateur lit le répertoire
    personnel ;
  - **par une boucle nue** : la même boucle native, les **mêmes implémentations** des outils
    fichiers (`mcp-system`), appelées sans registre (un contrôleur qui autorise tout), sans
    capd, journal, consigne ni condensation ; l'espace de travail est recopié dans le
    répertoire personnel, et le même vérificateur tranche.
- **Ce qui diffère est exactement ce que Prophet ajoute** : jeton et contrôle de chaque accès,
  journal, espace de travail scellé puis publié, condensation des anciens résultats d'outils
  (une mission du banc n'a ni rôle ni relais : agentd ne lui donne pas de consigne). Le banc
  mesure donc le prix et le gain de ces couches, pas un autre agent.
- **Mesures** : réussite par tâche (vérificateur), durée, tokens, étapes (Prophet) ou tours
  (boucle nue), outils appelés et réponse finale du modèle ; par côté, taux de réussite, durée
  médiane et p95, tokens moyens. Le modèle échantillonne comme l'image le règle (température
  0,7) : chaque tâche se rejoue `PROPHET_BENCH_REPETITIONS` fois (trois en CI) et le bilan
  compte les réussites par tâche sur ces passages. `just bench`
  les écrit dans `bench/results/<date>.json` ; le travail « Poids du catalogue servis (réels) »
  de la CI les joue à chaque poussée et les publie (résumé et artefact `banc-m13`).
- **Le banc mesure, il ne juge pas** : il n'échoue que si rien n'a pu être joué. Un essai sans
  modèle (`le_banc_joue_une_tache_des_deux_cotes_avec_un_faux_moteur`, dans `just check`)
  éprouve sa plomberie avec un moteur scripté.

## Alternatives écartées

- **Attendre la ligne de base « pixels »** : bloquée faute d'agent de vision et de compte ; elle
  mesurerait autre chose (l'observation par captures), qui reste à faire quand elle sera
  possible.
- **Une boucle nue écrite à part, avec ses propres outils** : ses différences d'outils se
  confondraient avec celles de Prophet ; réutiliser les implémentations isole les couches.
- **Écrire directement dans le répertoire personnel côté boucle nue** : les outils de
  `mcp-system` écrivent par construction dans un espace de travail ; le recopier ensuite rend le
  même résultat sans changer leurs sémantiques.

## Conséquences

- Les chiffres ne valent que pour ce modèle, ce processeur et ces sept tâches : ils se relisent
  à chaque passage, et d'autres modèles du catalogue se mesurent par `PROPHET_BENCH_MODEL`.
- La suite compte sept tâches sans navigateur sur les trente que M13-T1 prévoit ; l'étendre est
  la suite naturelle, avec des tâches web servies localement.
- Les interventions humaines se comptent à zéro dans les deux chemins (aucune approbation
  demandée par ces tâches) ; une tâche qui en demande devra les compter.
