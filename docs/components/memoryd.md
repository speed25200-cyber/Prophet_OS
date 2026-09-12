# `prophet-memoryd` — mémoire

- **Socket** : `/run/prophet/memoryd.sock`
- **Utilisateur** : `memoryd`, groupe `prophet-system`
- **État** : `/var/lib/prophet/memoryd/memoire.sqlite`
- **Crate** : `crates/memoryd`

Faits, préférences et résumés de tâches passées, rangés par **espace**.

## Les espaces sont des cloisons

Un espace n'est pas une étiquette. Ce qu'un espace contient ne se retrouve pas dans un autre, et
**une recherche sans espace échoue** au lieu de chercher partout : le défaut dangereux serait
« tous les espaces », qui ferait parcourir la mémoire entière à un appel mal formé.

## Méthodes

| Méthode | Ce qu'elle fait |
|---|---|
| `memory.remember` | Enregistre un fait, un épisode ou une préférence |
| `memory.search` | Cherche dans un espace, et dans un seul |
| `memory.list` | Tout ce qu'un espace contient |
| `memory.forget` | Oublie une entrée |
| `memory.forget_space` | Oublie un espace entier, et dit combien |
| `memory.spaces` | Les espaces existants |

« Oublié » sans quantité laisserait croire à une opération sans effet quand il n'y avait rien à
oublier : le nombre est toujours rendu.

## Modifiable par l'humain

La base est un fichier SQLite ordinaire. `prophet memory ls|search|forget` la lit et l'édite sans
passer par le daemon — une mémoire qu'on ne peut pas corriger soi-même n'est pas la sienne.
