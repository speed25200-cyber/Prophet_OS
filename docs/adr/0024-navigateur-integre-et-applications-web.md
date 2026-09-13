# 0024 — Un navigateur pour l'agent, un navigateur pour l'humain, et X installé par défaut

- **Statut** : accepté ; sortie réseau du navigateur piloté par egress ouverte
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

La sortie réseau propre du navigateur piloté (sous-ressources, scripts) n'est pas relayée par
egress : Chromium n'accepte pas un proxy sur socket Unix. C'est pourquoi l'outil est désactivé
par défaut et pourquoi l'image installée ne le configure pas encore. Le relais reste à construire
(un mandataire TCP local adossé à egress, ou un espace réseau sans route). Le confinement du
navigateur au niveau 2 attendu par M10-T3 n'est pas livré : il tourne sous l'identité du service.
Le paquet Chromium alourdit l'image ; son cache binaire évite une compilation. X et le navigateur
partagé sont vérifiés par la liste du lanceur et la présence du binaire, pas par une session
authentifiée sur x.com.
