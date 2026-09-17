# 0025 — Jev décide vite, le modèle écrit, capd garde tout

- **Statut** : accepté ; protocole vérifié contre un proxy simulé, premier appel réel ouvert
- **Date** : 2026-09-17
- **Tâches liées** : M8-T8 (boucle native), M8-T9 (sélection de pilote), M10-T3 (pont navigateur),
  M6-T1 (proxy de sortie)

## Contexte

Le plan promet un « routage par appel » entre modèles et une interface pilotée par l'arbre
plutôt que par les pixels. Jusqu'ici, un tour de boucle coûtait un tour de LLM — plusieurs
secondes, même pour décider d'un clic — et le pilote d'une mission était la première
préférence disponible du manifeste, quelle que soit la demande. Le 15 septembre 2026,
TypeSafe AI a ouvert Jev, un modèle qui ne génère pas de texte mais répond à des questions
fermées sur un état structuré, avec une probabilité calibrée, en quelques centaines de
millisecondes ([spécification](../specs/jev-decisions.md)). L'arbre SUP est exactement l'état
qu'il sait lire. L'utilisateur a demandé une intégration qui route les demandes vers les LLM et
fait le computer use directement.

Trois invariants du dépôt encadrent la réponse : aucune dépendance à une clé d'API dans le
chemin principal ; toute sortie réseau passe par egress ; aucun secret ne transite par un
modèle. Et une limite de fait : Jev ne sait pas écrire, ne voit pas d'image, et est un
service distant.

## Décision

**Jev est un décideur, pas un pilote.** Il n'apparaît pas dans `model.preferred` et n'a aucun
droit propre. Il intervient à deux endroits de la boucle existante, avec le jeton de la tâche :

1. **L'opérateur** (`providers::jev::operator`) est un `ModelClient` : il lit l'arbre SUP rendu
   par `web.open`, `web.tree` et `web.act`, offre à Jev les éléments actionnables comme options
   nommées, et traduit le choix en `web.act` — un appel d'outil ordinaire, contrôlé par capd et
   journalisé. Il ne remplit un champ qu'avec une valeur connue d'avance (guillemets de
   l'intention). Quand il ne sait pas — texte à écrire, confiance basse, mur, boucle, échecs —
   il rend la main (`DriverError::HandOver`) et la `Cascade` donne la même histoire au modèle
   génératif, qui garde la main jusqu'à la page suivante. Le rapide décide, le lent écrit.
2. **Le routeur** (`providers::jev::router`) départage, à la planification, les candidats déjà
   admissibles (`selection::eligible`) et note difficulté et risque. Une intention `local-only`
   ne part jamais chez Jev. Toute panne ou hésitation rend la sélection statique, avec sa raison.

**Le transport est le proxy.** La requête part sur le socket d'egress avec le jeton de la tâche
et `Authorization: Bearer prophet-secret:<nom>` ; le coffre ne rend la valeur qu'au proxy. Pour
que cela soit possible en HTTPS, egress termine désormais TLS lui-même vers un amont `https://`
en forme absolue, avec les racines de la machine — un tunnel `CONNECT` ne laisserait rien
substituer (ADR-0007). Et parce qu'une décision est une question, pas un effet,
l'administrateur nomme des **hôtes d'interrogation** (`PROPHET_EGRESS_QUERY_HOSTS`) pour
lesquels, et pour `POST` seulement, le proxy demande à capd une lecture plutôt qu'une action
externe ; jeton, grant sur l'hôte, détection d'exfiltration et journal restent entiers.

**Jev reste optionnel.** Il n'existe que si le service nomme un secret (`PROPHET_JEV_SECRET`,
module NixOS `prophet.jev`), si un navigateur est configuré, si le manifeste admet un service
distant et si le jeton de la tâche autorise `net.egress` sur `api.typesafe.ai`. Sinon, rien ne
change. Ses tokens sont imputés au budget de la mission comme ceux d'un modèle, par un compteur
commun placé devant la cascade.

## Alternatives écartées

- **Faire de Jev un pilote de `model.preferred`** : il ne rend jamais de texte final et ne
  peut pas porter une mission seul ; il ne serait qu'un pilote qui échoue dès qu'il faut écrire.
- **Approuver chaque `POST` vers Jev par l'humain** : la règle générale du proxy ; une boucle à
  plusieurs décisions par seconde n'existerait pas, et une approbation donnée cent fois par
  minute n'en est plus une. Les hôtes d'interrogation sont explicites et bornés à `POST`.
- **Laisser agentd parler TLS à Jev par un tunnel du proxy** : il faudrait lui donner la clé,
  ce que l'invariant du coffre interdit ; et le proxy ne verrait plus le corps à inspecter.
- **Laisser Jev inventer une valeur de champ** : il ne le peut pas, et un décideur qui
  devinerait un texte n'est plus calibré. La cascade existe pour cela.
- **Envoyer les captures d'écran** : Jev ne lit pas d'image, et le plan n'en veut pas.

## Conséquences

Le premier appel réel reste à faire, avec une clé déposée par `prophet secret put typesafe
--host api.typesafe.ai` et `prophet.jev.enable = true` ; un écart de protocole se corrige dans
`providers::jev` seul. La sortie réseau propre du navigateur piloté n'est toujours pas relayée
par egress (ADR 0024) : l'opérateur observe et agit par le pont, la page se charge par
Chromium. Les outils `ui.*` (applications natives) ne sont pas encore offerts à l'opérateur ;
il vise `web.*` seulement. L'état envoyé à Jev contient les textes de la page et l'intention :
c'est une sortie de données vers un service distant, journalisée par le proxy, interdite en
`local-only`, et soumise à la détection d'exfiltration. Le compteur commun impute désormais
les tours valides des deux décideurs au même endroit ; les générations incomplètes du modèle
restent comptées par le pilote local.
