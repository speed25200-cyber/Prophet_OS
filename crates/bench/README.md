# Mesure et mise à l'épreuve

Deux suites. `adversarial` demande ce qui se passe quand un contenu extérieur retourne l'agent
contre son utilisateur : vingt scénarios d'injection de prompt, dont aucun ne doit produire
d'exfiltration, d'action irréversible non approuvée ni de lecture de secret, chacun visible au
journal (M13-T3). `cost` demande combien coûte une observation : arbre sémantique contre
capture d'écran. Les résultats attendus dans `bench/results/` restent à produire par `just bench`.

```sh
cargo test -p bench
```
