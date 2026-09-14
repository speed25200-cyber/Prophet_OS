# ADR-0041 — Les approbations de bout en bout : le registre demande, le modèle attend, l'humain tranche

- **Statut** : accepté
- **Date** : 2026-09-14
- **Tâche liée** : M2-T4, M8-T7

## Contexte

capd sait refuser une action « faute de décision humaine » (`ApprovalRequired` : une action
irréversible et externe, une écriture sensible, une élévation), tenir une file de demandes
(`approval.request`, `approval.pending`, `approval.resolve`) et la surface sait les montrer et
les trancher. Mais personne ne créait la demande : les outils `approval.request` et
`approval.wait` étaient des coquilles, et la boucle de mission tenait `ApprovalRequired` pour
une interruption. Une mission qui touchait à une action engageante mourait, sans que l'humain
ait rien vu. Le superviseur, c'est pourtant l'humain.

## Décision

1. **Le registre demande.** Quand capd refuse un appel d'outil faute de décision, le registre
   soumet la demande lui-même (`approval.request`, avec la requête exacte que capd a jugée et
   un résumé lisible), la consigne au journal (`approval.requested`), et rend au modèle
   l'erreur `ApprovalRequired` avec l'identifiant de la demande et la marche à suivre. Une
   demande couverte par une règle (portée tâche ou agent) ou par une décision « une fois »
   rendue pour cette même action est tranchée sur-le-champ : l'appel passe, ou est refusé
   par « décision humaine ».
2. **Le modèle attend.** `approval.wait {id, timeout_s}` attend la décision, au plus 45 s
   (un client qui attend par sa séance a son propre délai), et rend `allowed`, `denied`,
   `expired` ou `pending` — auquel cas on attend encore. Attendre n'exige aucun droit. Puis le
   modèle réessaie le même appel. La boucle de mission ne meurt plus d'une demande ; la
   consigne du relais dit quoi faire ; `approval.request` explique que la demande se fait
   d'elle-même.
3. **L'humain tranche, et sa décision se lit.** capd garde une demande tranchée une heure
   (`approval.status`) pour que celui qui attend la lise, et garde une décision « une fois »
   jusqu'à la demande identique — même tâche, même action, même cible — qui la consomme. Une
   décision de tâche ou d'agent reste une règle, comme avant.

## Conséquences

- Une action engageante n'est plus une impasse : elle attend l'humain, qui décide dans la
  surface, et la mission continue ou s'arrête selon lui. Preuves : la file (une décision « une
  fois » consommée par la demande identique suivante, lisible par son état, oubliée après une
  heure) ; le registre avec un vrai broker (un outil irréversible et externe : refusé avec
  l'identifiant, attente en cours, décision rendue sur un autre fil vue par l'attente, le même
  appel passe une fois puis redemande ; une décision de tâche couvre toute la tâche ; un refus
  est définitif).
- Limites : la demande porte l'action et la cible, pas les arguments — le résumé dit « appeler
  tel outil » ou « tel outil sur telle cible » ; le modèle ne peut pas y ajouter son propre
  résumé. Un client officiel qui attend par sa séance doit réitérer `approval.wait` toutes les
  45 s. La surface tranche ; la CLI aussi (`prophet cap approvals`, `approve`, `deny`, `rules`).
