# ADR-0059 — Le journal ne perd plus les événements du service, et ne les écrit qu'une fois

- **Statut** : accepté
- **Date** : 2026-09-24
- **Tâche liée** : FRONTIER, « journalisation durable avec reprise et idempotence » ; ADR 0013
  (« une outbox durable reste à construire »)

## Contexte

agentd consigne au journal ce qui arrive aux missions — création, plan, lancement, fin,
publication, annulation. Il gardait ces événements en mémoire et les poussait vers `ledger`
après chaque commande ; si le journal était injoignable, ou qu'un envoi échouait, ils étaient
perdus (« des événements ne sont pas écrits »), et un redémarrage d'agentd perdait ceux qui
n'étaient pas encore partis. Renvoyer, à l'inverse, risquait d'écrire deux fois un événement
dont seule la réponse s'était perdue.

Les événements d'outil d'une mission ne sont pas concernés : leur écriture précède l'action, et
un journal injoignable arrête la mission (essai « une panne du journal avant l'action interdit
l'écriture »).

## Décision

- Chaque événement d'agentd reçoit, à sa création, une **clé d'idempotence** (`agentd:<ULID>`).
- La **file** des événements que le journal n'a pas encore reçus fait partie de l'état persistant
  d'agentd (`taches.json`, champ `journal`, absent quand vide). Elle est bornée à 10 000
  événements : au-delà, le plus ancien est abandonné, et le service le dit.
- L'envoi se fait **dans l'ordre**, un envoi à la fois ; un événement ne quitte la file qu'une
  fois reçu. Une panne arrête l'envoi jusqu'au prochain essai : après chaque commande, et toutes
  les deux secondes. Seul un refus définitif du journal (`-32602`, charge utile refusée) retire
  l'événement, bruyamment, pour ne pas bloquer les suivants.
- `ledger.append` accepte `idem` (1 à 128 caractères). Le magasin garde l'index des clés écrites,
  reconstruit à l'ouverture ; une clé déjà écrite rend l'événement existant sans rien écrire ni
  diffuser. La clé fait partie de l'événement et de son empreinte ; absente, rien ne change pour
  les événements antérieurs.

## Alternatives écartées

- **Laisser agentd horodater** : le journal reste le seul maître de l'heure et de l'ordre. Un
  événement retardé par une panne porte l'heure de son écriture ; l'ordre entre les événements
  d'agentd est préservé.
- **Dédoublonner par contenu** : deux événements identiques peuvent être légitimes (deux
  tentatives) ; seule une clé donnée par l'émetteur dit « c'est le même ».
- **Une file dans un fichier à part** : l'état d'agentd est déjà écrit d'un coup après chaque
  commande ; la file y est cohérente avec les tâches qu'elle raconte.

## Conséquences

- Un journal arrêté, puis relancé, reçoit ce qui s'est passé entre-temps, une seule fois — y
  compris à travers un redémarrage d'agentd (essai du service).
- Le magasin garde en mémoire une entrée par clé ; les événements d'agentd sont de l'ordre de
  quelques dizaines par mission.
- Les autres émetteurs (egress, memoryd…) peuvent adopter la même clé ; ils ne le font pas
  encore.
