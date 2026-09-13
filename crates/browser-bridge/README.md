# Pont navigateur

Un agent navigue par l'arbre sémantique d'une page, jamais par des pixels. `Browser` lance un
Chromium en mode sans tête avec un profil dédié, `Session` parle le protocole DevTools, et
`Page` rend la page comme un arbre SUP (`tree`) et exécute des actions typées (`act`) :
`click`, `set_field`, `submit`, `navigate`. Chaque action rend le nouvel arbre, pour que
l'agent sache immédiatement ce qu'elle a produit.

Le navigateur choisit et lie lui-même son port de débogage (`--remote-debugging-port=0`) et
l'annonce dans `DevToolsActivePort` ; le pont le lit au lieu de réserver un port qu'un autre
processus pourrait prendre entre-temps. Plusieurs navigateurs peuvent donc démarrer ensemble.
Aucune télémétrie, aucune synchronisation, aucun proxy hérité de l'environnement. Le
navigateur reçoit un foyer à lui (`HOME` et les répertoires XDG sous `<profil>/home`) : Chromium
y range ses rapports de plantage, sa base de certificats et son cache de polices, jamais sous
`--user-data-dir`, et un service dont le foyer est `/var/empty` verrait sinon son gestionnaire
de plantage échouer et Chromium s'arrêter net avant d'ouvrir son point d'écoute.

Les outils `web.open`, `web.tree` et `web.act` de `mcp-system` s'appuient sur ce pont, sous
le contrôle de capd ; `launch_with` accepte un mandataire HTTP local, par lequel le service
fait passer tout le trafic vers egress. Ce que le pont ne fait pas : confiner le navigateur au
niveau 2 ; voir l'[ADR 0024](../../docs/adr/0024-navigateur-integre-et-applications-web.md).

```sh
# Les tests se taisent sans navigateur ; exiger sa présence pour qu'un vert veuille dire vrai.
PROPHET_EXIGER_NAVIGATEUR=1 cargo test -p browser-bridge
PROPHET_BROWSER=/chemin/vers/chromium cargo test -p browser-bridge
```
