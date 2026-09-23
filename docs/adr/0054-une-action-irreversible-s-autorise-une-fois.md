# ADR-0054 — Une action irréversible s'autorise une fois

- **Statut** : accepté
- **Date** : 2026-09-23
- **Tâche liée** : M6 (approbations, ADR 0041) ; FRONTIER (« approbations humaines liées à
  l'action exacte »)

## Contexte

L'audit visuel de la décision humaine montre, pour « Envoyer le paiement de 87,40 € à SNCF
Connect ? — action irréversible », un bouton « Autoriser pour toute la mission ». capd en faisait
une règle permanente : même action, même cible, même mission, autorisées d'office. Pour un
paiement, c'est autoriser à le **refaire** — un second paiement, un troisième — sans nouvel
accord, par un modèle qu'un contenu aurait pu détourner. Et une règle née d'une action
*réversible* couvrait aussi une action *irréversible* de même nom et de même cible.

## Décision

- **capd n'autorise une action irréversible qu'une fois.** Une décision « autoriser » sur une
  demande irréversible est tenue pour ponctuelle, quelle que soit la portée demandée (tâche ou
  agent) : elle vaut pour la reprise de l'appel autorisé, puis la même action redemande.
- **Une autorisation permanente ne couvre jamais une action irréversible**, même née d'une
  action réversible de même nom et de même cible.
- **Un refus, lui, peut valoir pour la tâche ou l'agent** : refuser d'office n'expose à rien.
- **La surface ne propose plus « Autoriser pour toute la mission »** pour une action
  irréversible, et dit pourquoi : « si l'agent veut la refaire, il vous la redemandera ».

## Alternatives écartées

- **Garder la portée et avertir** : un avertissement ne protège pas d'un clic ; la garantie
  doit être dans capd, pas dans l'interface.
- **Refuser la portée par une erreur** : l'humain a voulu autoriser ; l'autorisation vaut, pour
  l'action qu'il a vue.

## Conséquences

- Une mission qui doit payer deux fois demande deux accords. C'est voulu.
- La CLI, la voix et tout client d'`approval.resolve` héritent de la règle : elle est appliquée
  par capd.
