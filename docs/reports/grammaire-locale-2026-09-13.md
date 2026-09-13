# Diagnostic du protocole Qwen3 et llama.cpp — 13 septembre 2026

## Défaut isolé

Le serveur produit un appel `fs.write`, puis répète des retours à la ligne jusqu'au plafond
de génération. Sa grammaire autorise cette répétition ainsi que plusieurs appels malgré
`parallel_tool_calls=false`. Le correctif est porté par le paquet Nix du moteur, sans
assouplir les validations du pilote Prophet.

Le diagnostic utilise exclusivement l'intention et le catalogue synthétiques de l'essai
agentd. Aucun outil n'est exécuté pendant ces comparaisons HTTP. Les trois services et
les vrais accès fichiers sont réservés à l'essai de mission distinct.

## Comparaisons avant correction

Même modèle Qwen3-0.6B-Q8_0, empreinte
`9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031`, llama.cpp
`b10809-5266f24`, CPU quatre threads, un slot et contexte 4096. Mode sans raisonnement,
température 0,7, top-p 0,8, top-k 20, min-p 0, pénalité de présence 1,5, graine 2026.
Ces temps décrivent des observations unitaires sous WSL2, pas un benchmark.

| Variante HTTP | Plafond | Observation | Durée |
|---|---:|---|---:|
| Catalogue original, lecture et écriture | 512 | `length`, identifiant altéré | 17,553 s |
| Noms avec `_` au lieu de `.` | 512 | `length`, identifiant exact mais réponse incomplète | 17,763 s |
| Option `parallel_tool_calls` omise | 512 | `length`, identifiant altéré | 18,375 s |
| Même prompt rendu, génération brute `/completion` | 512 | Arrêt EOS en 41 tokens, identifiant altéré, réponse anticipée | 5,053 s |
| Catalogue original, réponse `verbose` | 128 | Appel suivi de retours à la ligne jusqu'à `length` | 6,682 s |
| Écriture seule | 128 | Fin d'appel normale, contenu erroné `Done` | 4,799 s |
| Écriture avant lecture | 128 | Deux appels d'écriture malgré le mode séquentiel | 5,669 s |
| Lecture sans argument optionnel | 128 | Deux appels, identifiant altéré | 5,374 s |

La génération brute est un diagnostic : elle n'est pas utilisée comme chemin d'exécution
dans Prophet. Supprimer un outil ou changer son ordre ne constitue pas une correction.

## Cause dans le moteur

La dérivation Nix identifie la source exacte, sans patch amont supplémentaire. Dans
`common/chat-auto-parser-generator.cpp`, `build_tool_parser_json_native` répète un
`standard_json_tools` optionnel indépendamment de la cardinalité demandée. La grammaire
observée contient une répétition d'appels optionnels suivis d'espaces. Le test C++ construit
la grammaire du template Qwen3 puis essaie des chaînes complètes ; il ne compare pas
simplement le texte du correctif à une valeur attendue.

Après adaptation du test aux préfixes du mode `required` et au parseur public des réponses
finales, **huit assertions échouent sur le moteur original** : cardinalité de la grammaire
et du parseur en mode séquentiel, espaces sans borne et séquence vide en mode auto.

La documentation [Qwen sur les appels de fonctions](https://qwen.readthedocs.io/en/latest/framework/function_call.html)
recommande le format Hermes pour Qwen3 et rappelle que la conformité des générations n'est
pas garantie. La documentation [llama.cpp](https://github.com/ggml-org/llama.cpp/blob/master/docs/function-calling.md)
décrit les templates et l'activation des appels ; le diagnostic ci-dessus repose sur la
source exacte du paquet installé, plutôt que sur la branche amont courante.

## Validation du correctif

Le module corrigé compilé séparément passe 60 assertions sur les templates Qwen3 et
Qwen2.5. Dans une comparaison HTTP de développement avec ce module chargé, l'appel
original se termine normalement en 9,174 s. Une consigne système générale sur la
précision donne aussi une fin normale en 9,234 s, mais l'identifiant reste altéré dans
les deux cas. Cette consigne n'est pas ajoutée au pilote. Ces observations ont eu lieu
pendant la compilation du paquet ; elles ne constituent pas des mesures comparatives
de performance du produit final.

Le paquet Nix complet est construit avec succès. **Ses 60 assertions contractuelles
réussissent** avec ses propres bibliothèques, sur les deux templates. Commande :

```sh
nix build .#checks.x86_64-linux.llama-tool-grammar --print-build-logs
```

Le paquet vérifié est
`/nix/store/828hffb09kkgrb27f2pm11k1kc7r7r0m-llama-cpp-0.4.0` et le résultat du contrôle
est `/nix/store/l4zzw5a1nlry0bfhvad26mkq8wgd3xwz-prophet-llama-tool-grammar/result.txt`.
La version amont affichée par le binaire ne change pas : le chemin Nix et le patch du
dépôt identifient cette construction. Le paquet conserve tous les backends et les
variantes CPU du nixpkgs épinglé. Les tests Rust restent inchangés et `just check`
réussit : **622 tests, aucun échec, 25 ignorés**, format, clippy et contrôles du dépôt.

Les [trois échecs précédents](missions-locales-2026-09-13.md) sont conservés. Un test de
grammaire réussi ne démontrerait ni la qualité sémantique des réponses, ni la chaîne agentd
avec un vrai LLM, ni la disponibilité du moteur dans une session installée complète.

Le second modèle de l'essai réel est le [Qwen3-1.7B-Q8_0 officiel](https://huggingface.co/Qwen/Qwen3-1.7B-GGUF),
révision `90862c4b9d2787eaed51d12237eafdfe7c5f6077`, fichier de 1 834 426 016 octets.
Son empreinte téléchargée correspond au SHA-256 publié :
`061b54daade076b5d3362dac252678d17da8c68f07560be70818cace6590cb1a`.
Le modèle de 0,6 milliard de paramètres reste dans la comparaison ; changer de taille
ne doit pas masquer ses échecs.

## Missions réelles avec le paquet final

Le script `tools/verifier-moteur-local.py` a exécuté trois missions par modèle, après
la fin de compilation, avec le paquet Nix final et sans module préchargé. Le test
Rust existant, son intention et ses assertions n'ont pas été modifiés. Chaque mission
utilise un identifiant aléatoire distinct ; aucune réponse ni écriture n'est simulée.

| Modèle | Essai | Résultat du test complet | Durée du test Rust |
|---|---:|---|---:|
| Qwen3-0.6B-Q8_0 | 1 | Échec : le fichier contient une partie de la consigne au lieu de l'identifiant | 9,90 s |
| Qwen3-0.6B-Q8_0 | 2 | Réussi : fichier exact, événements et résultat relu après redémarrage | 13,09 s |
| Qwen3-0.6B-Q8_0 | 3 | Échec : casse et ponctuation de l'identifiant altérées | 11,52 s |
| Qwen3-1.7B-Q8_0 | 1 | Réussi : fichier exact, événements et résultat relu après redémarrage | 12,42 s |
| Qwen3-1.7B-Q8_0 | 2 | Réussi : mêmes exigences | 12,96 s |
| Qwen3-1.7B-Q8_0 | 3 | Réussi : mêmes exigences | 13,02 s |

Dans les six cas, la tâche atteint la fin d'exécution. Les deux échecs du petit modèle
viennent ensuite de l'assertion sur le contenu exact ; ils ne sont pas transformés en
succès parce que l'exécution est terminée. Les quatre tests réussis vérifient agentd,
capd et ledger en processus séparés, le fichier SFS, l'absence de changement dans les
documents d'origine, les événements sans contenu privé et le résultat persistant après
redémarrage. Tous les processus de ces essais sont arrêtés à la fin.

Ces temps excluent la compilation et le démarrage initial du moteur, mais incluent le
parcours Rust complet. Le script conserve aussi une durée englobant l'invocation Cargo :
16,051 / 13,878 / 12,315 s pour le 0.6B et 14,732 / 13,723 / 13,812 s pour le 1.7B.
Trois essais d'une même tâche ne permettent pas d'estimer un taux de réussite général,
une latence p95 ou une supériorité sur un autre OS. Le 1.7B n'a pas été comparé au moteur
original : sa réussite avec le paquet corrigé ne mesure pas à elle seule l'effet du patch.

## État restant

La chaîne de service possède maintenant une preuve réelle sur cette tâche. Le profil
installé, le lancement depuis une session graphique avec ce moteur, plusieurs familles,
les GPU et les tâches variées restent à exercer. Le contrôle automatique du contenu est
encore celui du test : agentd ne vérifie pas lui-même la justesse du résultat produit.
L'interface distingue déjà la fin d'exécution de la validation de l'objectif.

La CI de `a16472a` a réussi composants, isolation et surface ; ChatGPT reste en échec
([run 34733626591](https://github.com/speed25200-cyber/Prophet_OS/actions/runs/34733626591)).
Le travail CI du moteur local est ajouté dans ce jalon et reste à vérifier après sa
publication. Le graphisme, les sessions authentifiées et les autres critères de
[FRONTIER](../FRONTIER.md) demeurent ouverts.
