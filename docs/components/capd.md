# `prophet-capd` — broker de capacités

- **Socket** : `/run/prophet/capd.sock`
- **Utilisateur** : `capd`, groupe `prophet-system`
- **État** : `/var/lib/prophet/capd/signing.key`, mode 0600
- **Crate** : `crates/capd`

Rien n'est permis dans Prophet OS qui ne soit passé par ici. Un jeton signé par `capd` **et** une
politique Cedar qui l'autorise : les deux doivent dire oui.

## Méthodes

| Méthode | Ce qu'elle fait |
|---|---|
| `cap.mint` | Émet le jeton racine d'une tâche : l'intersection de ce qu'elle demande et de ce que son manifeste plafonne |
| `cap.check` | Contrôle d'accès complet — signature, chaîne de parents, expiration, politique, grant, classe d'action |
| `cap.revoke` | Révoque un sujet ; ses jetons et leurs enfants cessent d'être valides |
| `cap.public_key` | La clé publique, pour vérifier un jeton sans repasser par ici |
| `approval.request` | Soumet à un humain une action refusée faute de décision |
| `approval.pending` | Les demandes en attente, les plus anciennes d'abord |
| `approval.status` | L'état d'une demande, en attente ou tranchée depuis moins d'une heure (ADR 0041) |
| `approval.explain` | Joint à une demande en attente le motif du modèle (`reason`, une phrase, 400 caractères au plus) |
| `approval.rules` | Les règles permanentes issues des décisions de portée `task` ou `agent` |
| `approval.resolve` | Tranche une demande ; portée `once` (défaut), `task` ou `agent` |
| `approval.expire` | Retire les demandes périmées |

Un **refus est une réponse**, pas une erreur de protocole : `cap.check` rend une décision avec son
motif. L'appelant a besoin de savoir *pourquoi* pour décider s'il demande une approbation ou s'il
abandonne. Un paramètre manquant, lui, est bien une erreur (`-32602`) — les confondre ferait passer
un appel malformé pour une politique appliquée.

## Ce qu'il refuse

- Un pair hors du groupe `prophet-system` — membre déclaré dans `/etc/group` compris. `root`, lui,
  est accepté : le refuser ne protégeait rien, puisqu'il lit déjà la clé de signature.
- Un pair de la session humaine qui voudrait émettre, déléguer ou vérifier un droit, demander ou
  expirer une approbation : ces méthodes reviennent aux services (groupe principal
  `prophet-system`). Un service qui voudrait trancher une approbation : cela revient à l'humain.
  Voir l'[ADR 0044](../adr/0044-les-methodes-reservees-par-classe-de-pair.md).
- Un jeton signé par une autre clé — motif `bad_signature`.
- Une portée d'approbation inconnue ; le défaut est la plus étroite, jamais la plus large.

## Quand il n'est pas là

Plus rien ne peut obtenir de droit. `prophet-egress` ferme la sortie réseau, `prophet-agentd`
refuse de planifier. C'est voulu : une machine qui ne peut pas vérifier un droit ne doit pas en
accorder.

## Diagnostic

```sh
systemctl status prophet-capd
journalctl -u prophet-capd -n 50
```

Une clé de signature qui change au redémarrage invaliderait tous les jetons déjà émis. Si le
service redémarre en boucle, vérifiez que `/var/lib/prophet/capd/signing.key` fait 32 octets : le
daemon refuse de deviner une clé abîmée plutôt que de la compléter.
