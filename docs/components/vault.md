# `prophet-vault` — coffre à secrets

- **Socket** : `/run/prophet/vault.sock`
- **Utilisateur** : `vault`, groupe `prophet-system`
- **État** : `/var/lib/prophet/vault/` — `secrets.json` chiffré et `vault.key` (0600)
- **Crate** : `crates/vault`

Un invariant tient ce composant : **le Vault rend des poignées, jamais des valeurs**. Un agent
reçoit `prophet-secret:<nom>` ; la valeur n'est substituée qu'au dernier moment, dans la requête
sortante, par le proxy.

## Méthodes

| Méthode | Qui peut l'appeler |
|---|---|
| `secrets.list_refs` | le groupe système — noms, domaines, en-tête ; jamais de valeur |
| `secrets.allowed_for` | le groupe système — « ce secret peut-il être présenté à cet hôte ? » |
| `vault.put`, `vault.rotate`, `vault.delete` | le groupe système |
| `secrets.use` / `vault.reveal` | **le seul compte `egress`**, vérifié par `SO_PEERCRED` |

La dernière ligne est la raison d'être du daemon. Appartenir au groupe système ne suffit pas :
ni `root`, ni le compte des agents, ni la surface n'obtiennent une valeur.

Conséquence voulue : **sur une machine où le compte `egress` n'existe pas, personne ne peut rien
révéler**. Un coffre qui refuse tout le monde vaut mieux qu'un coffre qui s'ouvre parce qu'il n'a
pas su à qui il parlait.

## Ce qui n'est jamais journalisé

Le nom d'un secret l'est, sa valeur ne l'est jamais — pas même dans un message d'erreur. Un secret
qui passe par un journal a cessé d'en être un.

## Limite connue

Dans un tunnel `CONNECT`, le proxy ne voit rien et ne peut donc rien substituer. Ce n'est pas un
manque d'implémentation mais la propriété qui rend TLS utile. Voir ADR-0007.
