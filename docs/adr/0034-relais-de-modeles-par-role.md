# ADR 0034 — Relais de modèles par rôle : réfléchir cher, exécuter bon marché, compter tout

Date : 2026-09-13. Statut : accepté, première livraison.

## Contexte

Une mission est menée par un modèle, du premier tour au dernier. Or les tours ne se valent
pas : découper un objectif et vérifier un résultat demandent le modèle le plus capable ;
écrire un fichier dont le contenu est déjà décidé n'en demande pas tant. La délégation (ADR 0029)
permet déjà à un modèle d'en faire travailler un autre, mais elle exige de **nommer** le modèle,
et personne ne compte ce que chacun a coûté : le budget d'une mission est un total.

L'objectif de Prophet OS est explicite : les modèles les plus capables (Claude, GPT via leurs
clients officiels ; Qwen en local) doivent avancer **ensemble** sur une tâche, avec une
économie de tokens maximale, la réflexion profonde à l'un, l'intégration à un modèle bon
marché, le code à un modèle entraîné pour cela, et l'humain qui supervise. Cela demande trois
choses que le système n'avait pas : une notion de *rôle* indépendante du nom des modèles, un
compte par modèle, et une réduction de ce qui est renvoyé au modèle à chaque tour.

## Décision

1. **Des rôles dans le manifeste.** `model.roles` associe à chacun des rôles `reflect`,
   `execute`, `code` et `review` une liste ordonnée de références de modèles, toutes
   ⊆ `model.preferred`.
   Le manifeste vérifie le nom des rôles et la forme des références ; le catalogue de missions
   d'agentd vérifie l'inclusion dans le plafond. Un rôle ne donne aucun droit et n'élargit rien :
   il choisit parmi ce que le profil admet déjà.
2. **Une délégation par rôle.** `task.delegate {intent, profile, role?, model?}` : avec `role`,
   le service prend le premier modèle que le contexte visé admet pour ce rôle **et que le moteur
   sert en ce moment** (interrogé au moment de déléguer) ; un modèle nommé l'emporte ; un rôle
   que le contexte ne définit pas est une erreur d'outil nommée, sans sous-mission. capd tranche
   `task.spawn` et délègue le jeton exactement comme avant.
3. **Chaque mission connaît son rôle** (`task.role`) : celui demandé pour une sous-mission,
   sinon celui que son profil donne à son modèle. Une mission qui a un rôle, ou qui peut
   confier un contexte à rôles, reçoit une **consigne de système** à chaque tour : ce qu'on
   attend de son rôle, et les contextes qu'elle peut confier avec les rôles qu'ils savent jouer.
   Sans relais, aucune consigne : la boucle native reste ce qu'elle était. La consigne est une
   instruction, pas une autorité ; elle ne nomme ni chemin ni droit.
4. **Un compte par modèle.** Chaque tour est imputé au modèle qui l'a joué (`task.usage` :
   tours, tokens en entrée, tokens en sortie), en plus du budget global. Une sous-mission
   terminée impute son compte à son parent, dans l'état du service et dans celui que le fil du
   parent publie. `task.inspect`, `task.result`, `task.list`, l'événement final du journal
   (`by_model`, `role`), `prophet task show` et l'atelier le montrent ; la mesure honnête du
   relais est **la part des tokens prise en charge hors du modèle de la mission**, sans table
   de prix inventée.
5. **Condensation avant envoi.** Les résultats d'outils plus anciens que les deux derniers et
   plus longs que 1 024 octets sont remplacés, dans ce qui part au moteur, par un résumé
   (`ok`, taille, empreinte blake3, 160 premiers caractères, et la mention qu'un nouvel appel
   rend le détail). L'historique de la boucle, ses points de reprise et son rejeu ne changent
   pas ; la condensation est déterministe et se mesure.

## Complément du 14 septembre 2026 : la relecture

Un quatrième rôle, `review`, juge un travail rendu sans le refaire : sa consigne est de lire ce
qui a changé, de chercher ce qui est faux, manquant, dangereux ou non vérifié, et de rendre un
verdict court et argumenté, sans rien modifier ; celle de la réflexion l'invite à faire relire
tout code ou document important. Le sens est la collaboration entre fournisseurs : dans le
profil « Atelier des agents », Codex code et Claude Code relit (puis Codex, puis le modèle
local), pour qu'un autre regard que celui de l'auteur passe sur ce qui compte, au prix d'une
lecture et non d'une seconde production. Le rôle n'ajoute aucun droit : la relecture reçoit
les outils du contexte comme toute sous-mission, et capd tranche chaque appel.

## Complément du 22 septembre 2026 : la fenêtre du moteur

L'image sert ses modèles avec une fenêtre de 4 096 tokens, et `fs.read` rend jusqu'à 256 Kio :
une seule lecture d'un fichier moyen suffisait à ce que llama-server refuse l'historique
(`exceed_context_size_error`, HTTP 400) et que la mission échoue sur « le moteur local répond
HTTP 400 ». Le pilote lit désormais ce refus, et seulement ses nombres (`n_prompt_tokens`,
`n_ctx`), jamais le reste du corps. Il en déduit le rapport octets par token de ce qu'il vient
d'envoyer, calcule ce qu'il faut retirer pour laisser la place de la réponse (le plafond de
sortie, au plus le quart de la fenêtre), avec une marge de 30 %, puis resserre les résultats
d'outils dans cet ordre : anciens condensés, anciens réduits à leur issue et leur taille, du
plus ancien au plus récent, dernier tronqué à son début avec un avis qui dit au modèle d'en
lire moins (`max_bytes`, une cible plus précise) plutôt que de le relire en entier. Trois envois
au plus par tour ; la fenêtre apprise sert ensuite d'emblée. Un historique que rien ne peut
resserrer (une intention démesurée) rend une erreur qui donne les deux nombres.

La page Conversation de l'atelier heurtait la même limite au bout d'une vingtaine d'échanges.
Là, rien n'est un résultat d'outil : le client oublie les plus anciens messages, à la même
mesure, garde la consigne de système et la dernière question, reprend sur une question de
l'humain, et la page dit combien de messages le modèle n'a pas relus. L'historique affiché
ne change pas.

Ce que l'essai NixOS du moteur vérifie sur le llama-server épinglé : le refus a bien cette
forme, `n_ctx` et `n_prompt_tokens` compris. Le resserrement lui-même est prouvé contre des
serveurs de test qui rendent ce refus ; une mission réelle qui lit un gros fichier coûterait
environ 70 s de préremplissage sur le processeur de la CI, trop près du budget de 90 s du
profil « Documents ».

Écarté : interroger `/props` avant chaque mission (un aller-retour de plus, et un mode routeur
qui ne dit la fenêtre d'un modèle qu'une fois chargé) ; compter les tokens nous-mêmes (il faudrait
le tokeniseur de chaque modèle) ; retirer des messages entiers (le modèle perdrait la trace de
ses propres appels, et le rejeu sa correspondance avec l'historique).

## Conséquences

- Un profil peut dire « qwen3-1.7b réfléchit, qwen3-0.6b exécute » ; le catalogue d'exemple le
  fait, et `task.options` rend pour chaque rôle les modèles réellement servis. Le même jour,
  l'image sert les deux : `prophet.localEngine.executeWeights` (Qwen3-0.6B Q8_0, 640 Mo,
  téléchargé à l'installation comme le modèle principal) fait passer le moteur en mode routeur
  de llama-server (un fichier de préréglages, un modèle par section, chargés à la demande sur le
  même port), et les profils de l'image gagnent les rôles `reflect` et `execute` ; sans second
  poids, un seul modèle fait tout et rien ne change.
- Les clients officiels ne sont pas encore des cibles de rôle exécutables par le service : un
  rôle `driver:claude-code` ou `driver:codex` est un manifeste valide, mais le catalogue de
  missions locales n'admet que `local:` et le lanceur ne lance aucun client (ADR 0026 : la
  séance MCP est ouverte par l'humain). C'est la prochaine marche : le même relais, avec Claude
  Code en réflexion et Codex en code, par la séance qu'ils savent rejoindre.
- Le parent reste bloqué le temps de l'enfant, la profondeur reste bornée à trois, et rien ne
  garantit qu'un modèle de réflexion délègue effectivement : la consigne l'y invite, le compte
  par modèle dit s'il l'a fait. Une mission sans relais ne voit aucune différence, sauf la
  condensation, qui peut amener un modèle à relancer un outil pour relire un ancien résultat.
