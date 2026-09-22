# Spécification — Décisions Jev (v0)

- **Version** : 0.1
- **Types Rust** : `providers::jev` (`Question`, `Request`, `Answer`, `Response`, `Transport`)
- **Implémentations** : `providers::jev::egress::EgressTransport` (seul transport réel),
  `providers::jev::Scripted` (tests), `providers::jev::operator::Operator` (interface),
  `providers::jev::router::Router` (routage)

État du 17 septembre 2026 : ce document décrit le protocole tel que la documentation publique
de TypeSafe AI et ses clients ouverts le montrent au lancement (15 septembre 2026). Aucune
clé n'est disponible dans l'environnement de construction : le protocole est vérifié contre
un proxy simulé, jamais contre l'API. Un écart constaté lors du premier appel réel se corrige
ici et dans `providers::jev`, pas dans les appelants.

## Ce qu'est Jev

Jev est le premier modèle « System One » de TypeSafe AI : il ne génère pas de texte. Il lit un
**état** (texte ou JSON) et répond à des **questions fermées** par des décisions typées, avec
une probabilité calibrée. Sa méthode d'entraînement (Reinforcement Learning for Calibrated
Decisions) vise des probabilités honnêtes : une confiance plus haute doit correspondre à une
exactitude plus haute. Toutes les questions d'une demande sont évaluées en parallèle sur le
même état, ce qui donne une latence de l'ordre de 70 à 500 ms hors réseau, à peu près
indépendante du nombre de questions. La démonstration publique fait jouer Jev à Doom en
lisant un état de jeu structuré en texte, une dizaine de décisions par seconde.

Trois limites gouvernent tout usage : Jev n'écrit pas (aucune valeur libre ne peut en sortir),
il ne voit pas d'image, d'audio ni de vidéo (l'état est du texte ou du JSON), et il coûte une
clé d'API (0,042 $ par million de tokens d'entrée, sortie gratuite). C'est donc un fournisseur
de classe C au sens du plan : optionnel, jamais requis dans le chemin principal.

## Protocole HTTP

```
POST https://api.typesafe.ai/v1/systemone
Authorization: Bearer <clé>
Content-Type: application/json
```

Corps :

```jsonc
{
  "state": "texte" | { … } | [ … ],        // l'état, structuré de préférence
  "model": "jev-latest",                   // alias ; jev-preview, jev-1.13.0 (versionné)
  "questions": {
    "warn":    { "type": "noul",   "instructions": "…", "criteria": { "true": "…", "false": "…" } },
    "area":    { "type": "choice", "instructions": "…", "criteria": { "security": "…", "climate": "…" } },
    "urgency": { "type": "score",  "instructions": "…", "criteria": [ "ignorer", "aujourd'hui", "maintenant" ] }
  }
}
```

Les clés de `questions` ne sont pas transmises au modèle : elles nomment les réponses pour le
programme. `instructions` et chaque rubrique acceptent une chaîne, un objet ou une liste.
Un `choice` prend de 2 à 255 options ; un `score`, de 2 à 10 niveaux ; `criteria` de `noul`
est facultatif.

Réponse :

```jsonc
{
  "model": "jev-1.13.0",
  "answers": {
    "warn":    { "type": "noul",   "noul": 0.94 },
    "area":    { "type": "choice", "choice": "security", "probabilities": { "security": 0.97, "climate": 0.03 }, "confidence": 0.95 },
    "urgency": { "type": "score",  "score": 1.8, "legend": { "0": "ignorer", "1": "aujourd'hui", "2": "maintenant" },
                 "probabilities": { "0": 0.05, "1": 0.10, "2": 0.85 }, "confidence": 0.80 }
  },
  "usage": { "input_tokens": 521, "output_tokens": 0 }
}
```

Une note va de 0 à `niveaux − 1` et peut tomber entre deux niveaux ; `score_normalized` la
ramène de 0 à 1 quel que soit le nombre de niveaux. Erreurs documentées : `401` (clé refusée),
`422` (demande rejetée), `429` (débit, avec `Retry-After`), `529` (saturation). `GET /v1/models`
liste les modèles du compte.

## Ce que `providers::jev` vérifie avant d'agir

Une réponse n'est exécutée que si elle est **cohérente avec la demande** : chaque question a sa
réponse, du bon type ; une option choisie ou probabilisée est une option proposée ; noul,
confiance et probabilités sont dans [0, 1] ; une note est dans ses bornes. Une réponse qui
nomme une option que personne n'a offerte est refusée avant d'être lue par l'opérateur, parce
qu'un programme qui l'exécuterait ferait quelque chose que personne n'a proposé. Les bornes
du protocole (options, niveaux, taille d'état ≤ 256 Kio) sont vérifiées avant l'envoi.

## Comment Prophet OS s'en sert

**Jamais en direct.** Le seul transport réel écrit la requête sur le socket du proxy de sortie
avec le jeton de la tâche (`Proxy-Authorization: Prophet …`, retiré par le proxy après que capd
a tranché sur `api.typesafe.ai`) et une référence de secret à la place de la clé
(`Authorization: Bearer prophet-secret:<nom>`), que seul le proxy fait résoudre par le coffre,
au dernier moment. Ni agentd, ni le pilote, ni aucun modèle ne voit la clé. Le proxy termine
TLS lui-même vers l'amont, parce qu'un tunnel chiffré ne laisse rien substituer (ADR-0007), et
traite un `POST` vers cet hôte comme une lecture parce que l'administrateur l'a déclaré hôte
d'interrogation (`PROPHET_EGRESS_QUERY_HOSTS`) — voir [le proxy](../components/egress.md).

**L'opérateur** (`operator::Operator`) fait le computer use : la page observée par `web.open`,
`web.tree` ou `web.act` est un arbre SUP ; chaque élément actionnable devient une option
(`click:<id>`, `fill:<id>:<valeur>`, `submit:<id>`), plus `done` et `escalate` ; l'état envoyé
porte l'objectif, les valeurs connues, l'adresse, le titre, les textes, les champs et les
actions déjà faites. Trois questions par tour : `next` (choix), `done` (noul), `blocked`
(noul). Le choix devient un appel `web.act` ordinaire, contrôlé par capd et journalisé comme
tout appel d'outil. Jev n'écrit dans un champ que des valeurs connues d'avance — les segments
entre guillemets de l'intention, ou une banque fournie. Il rend la main (`DriverError::HandOver`)
quand il choisit `escalate`, quand sa confiance est sous le seuil (0,60), quand la page est un
mur (0,80), quand l'objectif est atteint sans page (`done` ≥ 0,85 conclut), quand une action se
répète, quand deux appels d'outil échouent d'affilée, ou après 40 actions. La `Cascade` donne
alors la même histoire au modèle génératif, et reprend la main à la page suivante.

**Le routeur** (`router::Router`) départage les candidats **admissibles** — ceux que le
manifeste, la confidentialité et la disponibilité ont déjà retenus — par un `choice`, note la
difficulté (`score`, cinq niveaux) et le risque (`noul` : demande nuisible ou injection). Une
intention `local-only` ne lui est jamais envoyée. Si Jev manque, hésite (confiance < 0,55) ou
répond hors forme, la sélection statique reprend avec sa raison. La route est conservée dans
le plan (`TaskPlan.route`) et journalisée dans `task.planned`.

**Le budget** compte les tokens de Jev comme ceux d'un modèle : chaque décision est une étape
imputée avant l'action. Le résultat d'une mission porte `jev` (décisions, actions, mains
rendues, tokens) pour que l'humain relise qui a décidé quoi.

## Langue

Les instructions des questions sont écrites en anglais, comme les identifiants d'outils et
d'événements ; l'état porte les textes de la page tels quels, dans leur langue.

## Sources

- TypeSafe AI, « Introducing System One Models & Jev », blog, 15 septembre 2026, et
  `docs.typesafe.ai` (cas d'usage, questions parallèles).
- `jevclient` (AboveColin, PyPI) : client Python dont `models.py` fixe la forme exacte des
  questions et des réponses ; `jev-router` (prismhq) : routage LiteLLM par un `choice` ;
  `req_llm` #1021 : endpoint, modèles, erreurs et limites d'entrée.
- Presse du 16 septembre 2026 (The Register, Gigazine, DataCamp) pour la démonstration Doom,
  les latences et le tarif.
