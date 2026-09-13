# Pont navigateur

Un agent navigue par l'arbre sémantique d'une page, jamais par des pixels. `Browser` lance un
Chromium en mode sans tête avec un profil dédié, `Session` parle le protocole DevTools, et
`Page` rend la page comme un arbre SUP (`tree`) et exécute des actions typées (`act`) :
`click`, `set_field`, `submit`, `navigate`. Chaque action rend le nouvel arbre, pour que
l'agent sache immédiatement ce qu'elle a produit.

Le navigateur choisit et lie lui-même son port de débogage (`--remote-debugging-port=0`) et
l'annonce dans `DevToolsActivePort` ; le pont le lit au lieu de réserver un port qu'un autre
processus pourrait prendre entre-temps. Plusieurs navigateurs peuvent donc démarrer ensemble.
Aucune télémétrie, aucune synchronisation, aucun proxy hérité de l'environnement.

Les outils `web.open`, `web.tree` et `web.act` de `mcp-system` s'appuient sur ce pont, sous
le contrôle de capd. Ce que le pont ne fait pas : relayer la sortie réseau du navigateur par
egress ni le confiner au niveau 2 ; voir l'[ADR 0024](../../docs/adr/0024-navigateur-integre-et-applications-web.md).

```sh
# Les tests se taisent sans navigateur ; exiger sa présence pour qu'un vert veuille dire vrai.
PROPHET_EXIGER_NAVIGATEUR=1 cargo test -p browser-bridge
PROPHET_BROWSER=/chemin/vers/chromium cargo test -p browser-bridge
```
