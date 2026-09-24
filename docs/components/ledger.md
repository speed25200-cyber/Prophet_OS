# `prophet-ledger` — journal d'audit

- **Socket** : `/run/prophet/ledger.sock`
- **Utilisateur** : `ledger`, groupe `prophet-system`
- **État** : `/var/lib/prophet/ledger/` — un fichier JSONL par jour, plus `seal.key` (0600)
- **Crate** : `crates/ledger`

Le journal est en ajout seul, chaîné par hachage. Ce daemon en est le **seul écrivain**, et c'est
la condition pour que le chaînage prouve quelque chose : une seule séquence de numéros existe.

## Méthodes

| Méthode | Ce qu'elle fait |
|---|---|
| `ledger.append` | Ajoute un événement ; le journal attribue `seq`, `prev` et `hash` |
| `ledger.query` | Filtre par tâche, types, bornes de séquence, limite |
| `ledger.verify` | Vérifie la chaîne et les sceaux, et dit où elle rompt |
| `ledger.seal` | Scelle immédiatement la tête de chaîne |
| `ledger.replay_summary` | Rejoue une tâche en texte lisible |
| `ledger.head` | Empreinte de tête et nombre d'abonnés |

Un appelant **ne choisit pas sa place dans la chaîne**. `seq`, `prev` et `hash` envoyés dans les
paramètres sont ignorés : le journal les calcule. Sans cela, le chaînage ne prouverait rien.

Le scellement a lieu tous les 256 événements, **après** l'écriture. Sceller une tête qu'on n'a pas
encore écrite signerait une chaîne qui n'existe pas.

`ledger.query` rend les événements dans l'ordre du journal, les plus anciens d'abord ; `limit`
coupe par le début. Pour suivre une tâche, on relit donc à partir du dernier numéro lu
(`since_seq`) : cette lecture ne touche que les fichiers et les lignes que l'index désigne, sans
relire les jours précédents. Une écriture sait où elle tombe sans relire le fichier du jour — egress
inscrit chaque connexion, et l'écriture ne doit pas ralentir au fil des heures (15 000 événements
dans la journée : 100 écritures en 0,5 ms, les 10 derniers relus en 2 ms).

## Ce qu'il refuse

- Un type d'événement hors du vocabulaire de `docs/specs/ledger-event.md`.
- Un scellement quand aucun signataire n'est chargé : il le dit plutôt que de rendre « rien » et de
  laisser croire la chaîne scellée.
- Une écriture ou un scellement demandé depuis la session humaine : `ledger.append` et
  `ledger.seal` reviennent aux services ; la lecture et la vérification restent ouvertes à tout
  pair admis. Voir l'[ADR 0044](../adr/0044-les-methodes-reservees-par-classe-de-pair.md).

## Quand il n'est pas là

`prophet-agentd` continue de travailler mais journalise en erreur les événements qu'il ne peut pas
écrire. Un système qui agit sans laisser de trace a perdu ce qui permet de revenir en arrière :
traitez-le comme un incident, pas comme un désagrément.

Les commandes `prophet log tail|replay|verify` lisent les fichiers directement et fonctionnent
**sans** le daemon — c'est exactement ce qu'on attend d'un journal d'audit, y compris après un
incident qui aurait emporté le service.
