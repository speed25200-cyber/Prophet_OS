# 0024 — Un navigateur pour l'agent, un navigateur pour l'humain, et X installé par défaut

- **Statut** : accepté ; sortie réseau du navigateur piloté relayée par egress ; confinement au niveau 2 ouvert
- **Date** : 2026-09-13
- **Tâches liées** : M7-T3 (`http.fetch`), M10-T3 (pont navigateur), M9 (image)

## Contexte

L'OS promet qu'un agent peut consulter Internet et que l'humain voit et peut aussi s'y rendre.
Avant cette décision, `http.fetch` répondait toujours « proxy injoignable » : l'outil existait
dans la liste normative, pas dans les faits. Le pont CDP de M10-T3 savait ouvrir une page et la
rendre en arbre SUP, mais aucun outil ne l'offrait à une mission. Le bureau ouvrait Firefox sans
profil Prophet, et aucune application web n'était installée par défaut.

## Décision

**Lire le web passe par egress.** `http.fetch` envoie la requête au socket du proxy avec le jeton
de la tâche dans l'en-tête interne ; le proxy fait trancher capd sur l'hôte réellement joint,
retire le jeton, puis relaie. L'outil recompose une réponse segmentée et la borne (256 Kio par
défaut, 1 Mio au plus). Une lecture (`GET`, `HEAD`) est automatique et journalisée ; toute autre
méthode est irréversible et externe, donc soumise à décision humaine, dans l'outil comme dans le
proxy. Le registre demande désormais à chaque outil les effets de l'appel précis, parce que lire
une page n'engage pas la même décision qu'y poster.

**L'agent navigue par l'arbre.** Trois outils s'adossent au pont CDP : `web.open {url}` contrôlé
comme une sortie réseau sur l'hôte, `web.tree {detail}` contrôlé comme une lecture d'interface
(`ui.read browser`), `web.act {action, node, value}` contrôlé comme une action d'interface
(`ui.act browser`), avec `click`, `set_field` et `submit` ; seul `submit` engage un effet
extérieur et exige une décision. Le navigateur a un profil par tâche dans l'état privé du service
et meurt avec la session. Ces outils n'existent que si l'administrateur nomme un programme
(`PROPHET_BROWSER`) : par défaut, une mission n'a pas de navigateur.

**Tout le trafic du navigateur passe par egress.** Chromium ne parle qu'à un mandataire TCP et
egress n'écoute que sur un socket Unix : un relais local, propre à la session de navigation,
écoute sur l'adresse de bouclage, reçoit chaque requête et chaque tunnel `CONNECT`, y pose le
jeton de la tâche dans l'en-tête interne que le proxy retire, et remet le tout au socket
d'egress, qui fait trancher capd sur l'hôte. Le navigateur est lancé avec ce mandataire, sans
exception pour le bouclage et sans QUIC, qui le contournerait. Sans egress joignable, le
relais répond une erreur et le navigateur n'a aucune route. Le relais ne décide de rien.

**L'humain a le sien.** Le bureau ouvre Chromium avec un profil Prophet distinct (`Super+N`),
jamais confondu avec les profils privés de Claude Code, Codex ou ChatGPT. X est installé par
défaut, en fenêtre d'application dédiée avec son propre profil (`Super+X`). Le lanceur expose sa
liste sans session graphique (`prophet-ouvrir --liste`), ce que le test du bureau vérifie.

## Alternatives écartées

- **Un seul navigateur partagé entre l'humain et l'agent**, piloté par CDP dans la session
  humaine : l'humain verrait l'agent agir, mais tout processus de la session pourrait alors
  piloter le navigateur, et une page ouverte par l'humain deviendrait le contexte de l'agent.
  Ce que l'agent voit se lit dans la supervision par ses appels d'outils ; une vue en direct de
  son arbre reste à construire.
- **Laisser `http.fetch` joindre le réseau directement** quand le proxy manque : c'est exactement
  la route que l'invariant interdit. Sans proxy, l'outil échoue et le dit.
- **Marquer `web.act` toujours externe** : chaque clic demanderait une décision, et une décision
  donnée cent fois n'en est plus une.

## Conséquences

Le navigateur piloté tourne sous l'identité du service, pas au niveau 2 attendu par M10-T3,
et le relais est une route de bouclage que tout processus du même hôte peut joindre : il
n'accorde rien de plus que ce que capd accorde au jeton de la tâche qui l'a ouvert, mais
c'est pourquoi l'outil reste désactivé par défaut tant que le navigateur n'est pas confiné.
Un test avec les vrais capd, ledger, egress et agentd prouve que la page demandée arrive au
serveur témoin par le proxy sans le jeton, et qu'aucune requête n'atteint le serveur quand
egress est absent. La résolution de noms passe par le proxy (forme absolue et `CONNECT`) ;
WebRTC n'est pas exercé et devra être vérifié avant de confier des pages hostiles.
Le paquet Chromium alourdit l'image ; son cache binaire évite une compilation. X et le navigateur
partagé sont vérifiés par la liste du lanceur et la présence du binaire, pas par une session
authentifiée sur x.com.
