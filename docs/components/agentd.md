# `prophet-agentd` — runtime d'agents

- **Socket** : `/run/prophet/agentd.sock`
- **Utilisateur** : `agentd`, groupe `prophet-system`
- **Dépend de** : `capd` (obligatoire), `ledger`, `sandboxd`
- **Crate** : `crates/agentd`

Le daemon qui tient les tâches : il les planifie, les suit, les annule. Tout ce que la surface
montre vient d'ici.

## Ce qu'il ne fait pas

**Il n'émet pas de jetons.** Il les demande à `capd`. Une seule clé doit signer les jetons de tout
le système, sans quoi `egress` jugerait contrefaits ceux d'`agentd` — et il aurait raison.

**Il n'écrit pas le journal.** Il pousse ses événements vers `ledger`, seul écrivain, parce que le
chaînage par hachage ne prouve quelque chose que s'il existe une seule séquence de numéros. Les
événements sont retirés de sa file quand ils sont écrits : sans cela, chaque planification
rejouerait tout l'historique, et un journal qui raconte deux fois la même chose ne raconte plus
rien de fiable.

## Méthodes

| Méthode | Ce qu'elle fait |
|---|---|
| `task.spawn` | Planifie : choisit le pilote, calcule le niveau d'isolation, obtient le jeton, rend le plan |
| `task.list` | Les tâches connues |
| `task.status` | Une tâche par identifiant |
| `task.cancel` | Annule — le geste de l'humain aboutit, même si le pilote traîne |

Le plan est rendu entier, avec la raison du choix de pilote : l'humain doit pouvoir dire non en
connaissance de cause, ce qui suppose que tout y soit.

## Quand `capd` n'est pas là

**Aucune tâche n'est planifiée.** Une tâche qui démarrerait sans jeton agirait sans qu'aucune
capacité ne la borne. Le refus nomme la cause.

## Persistance

Les tâches et leurs jetons sont écrits dans `/var/lib/prophet/agentd/taches.json`, en mode 0600 —
un fichier de jetons ne se partage pas. L'écriture passe par un fichier voisin puis un renommage,
pour qu'un arrêt au mauvais moment ne laisse pas un fichier tronqué.

Un état illisible **ne bloque pas le démarrage** : le daemon le signale et repart à vide. Refuser
de démarrer emporterait bien plus que les tâches en cours.

Les jetons repris ne sont pas revalidés : un jeton périmé le reste, et `capd` le refusera au
premier contrôle. C'est lui qui décide, pas `agentd`.
