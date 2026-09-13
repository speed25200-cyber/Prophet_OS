# Le web pour les agents, le navigateur pour l'humain — 13 septembre 2026

Travail après `0654fcc`, lié à M7-T3, M10-T3 et M9. Il rend réel ce que la liste normative des
outils annonçait, et donne au bureau un navigateur à profil Prophet et l'application X.

## Ce qui est livré

- `http.fetch` relaie par le socket d'egress, sous le jeton de la tâche placé dans l'en-tête
  interne que le proxy retire. `GET` et `HEAD` sont automatiques ; les autres méthodes sont
  irréversibles et externes, donc soumises à décision, dans le registre et dans le proxy. La
  réponse est recomposée si elle est segmentée et bornée à 256 Kio par défaut, 1 Mio au plus.
- Le registre demande à chaque outil les effets de l'appel précis (`Tool::effects`) au lieu
  de lire des drapeaux fixes : lire une page n'engage pas la même décision qu'y poster.
- `web.open`, `web.tree`, `web.act` : un Chromium par tâche, à profil privé, observé par son
  arbre SUP et manipulé par `click`, `set_field`, `submit`. L'hôte ouvert est une sortie réseau
  contrôlée par capd ; lire l'arbre et agir sont des accès d'interface (`ui.read`, `ui.act`
  sur `browser`) ; `submit` exige une décision humaine. Les outils n'existent que si le service
  nomme un programme (`PROPHET_BROWSER`).
- L'humain voit où l'agent navigue : les outils web déposent l'adresse, le titre et la taille
  de la page courante dans l'état privé du service, `task.inspect` les rend dans `browsing`,
  et l'inspecteur affiche « Sur le web : titre · adresse » avec un bouton qui ouvre l'adresse
  dans le navigateur de l'humain. L'arbre lui-même n'est jamais déposé ni rendu ainsi.
- Le bureau ouvre Chromium avec un profil Prophet (Super+N) et X en fenêtre d'application à
  profil propre (Super+X) ; `prophet-ouvrir --liste` rend la liste sans session graphique et le
  test `desktop-session` la vérifie. Cette partie n'a pas été construite dans cette session,
  faute de Nix : elle attend la CI.

## Vérifications

| Contrôle | Résultat local |
| --- | --- |
| `cargo test -p agentd --test local_daemon` | tous réussis, 1 ignoré (modèle réel) ; deux nouveaux tests web |
| `cargo test -p mcp-system --test web` avec `PROPHET_EXIGER_NAVIGATEUR=1` | 1 réussi, Chromium 1194 réel |
| `cargo test -p mcp-system --lib` | recomposition d'une réponse segmentée, refus nommé du proxy, aucune émission sans proxy |
| `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings` | réussis |
| `cargo test --workspace --no-fail-fast` | voir le décompte final ci-dessous |

Le premier test agentd lance capd, ledger, egress et agentd réels, un serveur HTTP témoin et une
réponse de modèle contrôlée qui appelle `http.fetch`. Il vérifie qu'une seule requête atteint
le témoin, qu'elle ne porte pas l'en-tête du jeton, que le journal contient l'appel et un
résultat `ok`, et que le contenu lu n'y figure pas. Le second vérifie que sans proxy aucune
requête ne part et que le résultat journalisé est un échec.

Le test navigateur ouvre une page de réservation servie localement : un hôte hors droits est
refusé avant tout lancement ; la page est ouverte et son titre lu ; un champ est rempli puis
relu à `Paris` ; `submit` est refusé `ApprovalRequired` ; un identifiant absent donne
`NotFound` ; le journal ne contient pas le texte de la page.

## Le navigateur piloté ne sort que par egress

Un relais local, propre à la session de navigation, écoute sur l'adresse de bouclage et remet
chaque requête et chaque tunnel du navigateur au socket d'egress, avec le jeton de la tâche
dans l'en-tête interne que le proxy retire. Chromium est lancé avec ce mandataire, sans
exception pour le bouclage et sans QUIC. Deux tests avec les vrais capd, ledger, egress, agentd
et un vrai Chromium le prouvent : la page demandée par `web.open` arrive au serveur témoin
par le proxy, sans le jeton, avec `web.open` journalisé sur l'hôte contrôlé ; sans egress,
aucune requête n'atteint le serveur. Trois tests unitaires couvrent la réécriture de la tête,
le refus sans egress et le refus sans jeton.

## Limites de cette preuve

Le navigateur tourne sous l'identité du service, pas au niveau 2 attendu par M10-T3, et le
relais est une route de bouclage joignable par tout processus du même hôte, bornée par le jeton
de la tâche ; les outils web restent donc désactivés par défaut. WebRTC n'est pas exercé. L'humain ne voit pas en direct l'arbre que l'agent observe ;
il lit ses appels au journal. X et le navigateur partagé sont vérifiés par la liste du lanceur
et la présence du binaire, sans session sur x.com. Les tests tournent sous un seul UID, sans VM.
Aucun critère de FRONTIER n'est coché.
