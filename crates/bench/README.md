# Mesure et mise à l'épreuve

Deux suites. `adversarial` demande ce qui se passe quand un contenu extérieur retourne l'agent
contre son utilisateur : vingt scénarios d'injection de prompt, dont aucun ne doit produire
d'exfiltration, d'action irréversible non approuvée ni de lecture de secret, chacun visible au
journal (M13-T3). `cost` demande combien coûte une observation : arbre sémantique contre
capture d'écran.

`tests/suite_reelle.rs` joue la suite de tâches (`tasks`) avec un vrai modèle local, deux fois :
à travers Prophet (capd, journal, espace de travail SFS publié par `task.apply`) et à travers
une boucle nue qui appelle les mêmes outils fichiers sans registre — même modèle, mêmes outils,
mêmes vérificateurs ([ADR 0048](../../docs/adr/0048-le-banc-prophet-contre-une-boucle-nue.md)).
Il mesure réussite, durée médiane et p95, tokens et étapes, et écrit `bench/results/<date>.json`.

```sh
cargo test -p bench                         # suites sans modèle, plomberie du banc comprise
PROPHET_TEST_LLAMA_SERVER=… just bench      # la suite jouée par le vrai modèle de l'image
```
