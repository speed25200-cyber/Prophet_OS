# ADR-0052 — Les échecs d'outil qui se répètent : une note au deuxième, l'arrêt au cinquième

- **Statut** : accepté
- **Date** : 2026-09-23
- **Tâche liée** : M4 (boucle native d'agentd), M13-T1 (banc M13, cinquième passage)

## Contexte

Le cinquième passage du banc M13 (`06c432d`) donne 12 réussites sur 45 par Prophet contre 9
pour la boucle nue, mais au prix d'un p95 de 170 s et de 11 588 tokens en moyenne. La cause :
`fs.edit` (ADR 0051) a ouvert un nouveau piège. Le modèle essaie d'*éditer* le fichier qu'il
doit créer, reçoit une erreur, et recommence à l'identique — jusqu'à vingt-quatre fois de suite
(« tableau-de-l-equipe » : 380 s, 68 353 tokens, jusqu'à la limite de génération). Les refus
de capd étaient bornés (ADR 0050, trois au plus) ; les autres échecs ne l'étaient pas.

## Décision

- **`fs.edit` sur un fichier absent le dit** : « ce fichier n'existe pas encore : fs.edit ne
  modifie qu'un fichier existant ; pour le créer, écrivez-le en entier avec fs.write ». Sa
  description le précise (« Ne crée pas de fichier »).
- **Une note au deuxième échec identique** : agentd enveloppe l'exécuteur de la mission
  (`agentd::garde::Garde`). Quand le même outil échoue avec le même code deux fois de suite, le
  résultat rendu au modèle garde son code et son détail, et porte `repeated` (le compte) et une
  note : ne pas rappeler à l'identique, relire l'erreur, changer d'approche ou conclure en
  disant ce qui bloque.
- **L'arrêt au cinquième échec de suite** (`ECHECS_MAX`), quel que soit l'outil : la mission
  échoue en le disant. Un succès remet le compte à zéro ; une décision humaine attendue
  (`ApprovalRequired`) n'est pas un échec.

## Alternatives écartées

- **Retirer `fs.edit`** : il a rendu possible « corriger-une-faute » (0 sur 12 par côté en
  quatre passages, 3 sur 3 des deux côtés au cinquième). Le piège se désamorce par l'erreur
  qu'il rend.
- **Arrêter au deuxième échec identique** : trop tôt ; un modèle se corrige souvent au
  troisième appel, une fois la note lue.
- **Laisser le budget borner** : il borne, mais après avoir dépensé soixante mille tokens et
  six minutes de processeur.

## Conséquences

- La boucle nue du banc n'a pas cette garde : elle fait partie de ce que Prophet ajoute.
- Une mission qui échoue cinq fois de suite se relance par son contexte (`prophet task retry`),
  comme toute mission échouée.
