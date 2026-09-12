# ADR-0007 — Un tunnel chiffré ne reçoit pas de secret injecté

- **Statut** : accepté
- **Date** : 2026-09-12
- **Tâche liée** : M6-T1, M6-T4

## Contexte

`docs/PLAN.md` décrit l'injection de secrets comme le mécanisme qui dispense le modèle de jamais
voir une clé : « l'agent demande *appelle GitHub avec mon identité*, le proxy injecte le secret ».
M6-T4 le précise : le proxy remplace `Authorization: Prophet-Secret <handle>` par la vraie valeur
au moment de la sortie.

Cela suppose que le proxy puisse lire et réécrire les en-têtes de la requête sortante. En écrivant
le relais de `prophet-egress`, cette supposition s'est révélée fausse pour le cas le plus fréquent.

Une tâche qui parle à un service HTTPS n'envoie pas une requête au proxy : elle envoie
`CONNECT hôte:443`, puis ouvre une session TLS **avec le serveur distant, à travers** le proxy. Ce
qui transite ensuite est chiffré de bout en bout. Le proxy voit des octets opaques. Il ne peut ni
lire l'en-tête `Authorization`, ni le remplacer.

Ce n'est pas un manque d'implémentation : c'est la propriété qui rend TLS utile. Un proxy qui
pourrait lire à l'intérieur d'un tunnel serait un proxy qui casse le chiffrement.

## Décision

**L'injection de secrets ne s'applique qu'aux requêtes que le proxy peut lire**, c'est-à-dire au
HTTP en clair. Dans un tunnel `CONNECT`, le proxy contrôle l'hôte, journalise la connexion, compte
les octets — et n'injecte rien.

Une tâche qui a besoin qu'un secret soit injecté doit donc passer par un chemin où le proxy est
l'interlocuteur TLS, pas un simple tuyau. Deux formes sont acceptables, et aucune ne casse le
chiffrement vu de la tâche :

1. **L'outil MCP `http.fetch`** : la tâche décrit sa requête, le proxy l'exécute en son nom et
   établit lui-même le TLS vers le serveur. C'est le chemin normal, et celui que les agents
   utilisent.
2. **Une terminaison TLS au proxy**, avec une autorité interne installée dans la sandbox. Écartée
   pour l'instant (voir ci-dessous).

## Alternatives écartées

- **Terminer le TLS au proxy avec une CA interne.** C'est techniquement faisable et c'est ce que
  font les proxys d'entreprise. Écartée en v0 : cela installe dans chaque sandbox une autorité
  capable de se faire passer pour n'importe quel site, ce qui est exactement la capacité qu'un
  agent compromis voudrait. Le gain — injecter un secret dans un tunnel — ne vaut pas ce risque
  tant qu'`http.fetch` fait le même travail sans lui.
- **Interdire `CONNECT`.** Cela forcerait tout le trafic par `http.fetch`, donc l'injection
  marcherait partout. Écartée : les clients officiels (Claude Code, Codex CLI, Gemini CLI) parlent
  HTTPS directement, et Prophet OS s'est engagé à les faire tourner **sans modification**. Leur
  couper `CONNECT` reviendrait à les modifier.
- **Injecter « au mieux », en silence.** La pire des trois : la tâche croirait son secret
  substitué, enverrait le handle littéral au serveur, et découvrirait le problème sous la forme
  d'une authentification échouée — ou pas du tout.

## Conséquences

- Le relais `CONNECT` de `prophet-egress` ne tente aucune substitution, et son commentaire dit
  pourquoi plutôt que de laisser croire à un oubli.
- Un handle envoyé dans un tunnel **sort tel quel**. C'est inoffensif — un handle ne vaut rien sans
  le coffre — mais c'est une authentification qui échouera. M6-T4 doit donc refuser explicitement
  une demande d'injection sur un tunnel, au lieu de la laisser passer sans effet.
- L'invariant « aucun secret ne transite par un modèle » n'est pas affaibli : il porte sur ce que
  le modèle voit, et le modèle ne voit ni l'un ni l'autre chemin.
- À revisiter si un client officiel se met à exiger une injection dans un tunnel. La réponse serait
  alors la CA interne, et il faudrait d'abord décider comment une sandbox peut se défendre contre
  l'autorité qu'on lui installe.
