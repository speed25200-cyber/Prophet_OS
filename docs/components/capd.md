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
| `cap.revoke` | Révoque un sujet ; ses jetons et leurs enfants cessent d'être valides, y compris après un redémarrage |
| `cap.public_key` | La clé publique, pour vérifier un jeton sans repasser par ici |
| `approval.request` | Soumet à un humain une action refusée faute de décision |
| `approval.pending` | Les demandes en attente, les plus anciennes d'abord |
| `approval.status` | L'état d'une demande, en attente ou tranchée depuis moins d'une heure (ADR 0041) |
| `approval.explain` | Joint à une demande en attente le motif du modèle (`reason`, une phrase, 400 caractères au plus) |
| `approval.rules` | Les règles permanentes issues des décisions de portée `task` ou `agent` |
| `approval.resolve` | Tranche une demande ; portée `once` (défaut), `task` ou `agent`. Accorder exige, sous le compte de l'humain, un `ticket` de présence ou le `code` (ADR 0057) |
| `approval.presence` | Échange le code d'approbation contre un ticket de présence, valable dix minutes pour ce compte |
| `approval.set_code` | Définit le code d'approbation, puis le change en donnant l'ancien (`current`) ; `root` le remplace sans |
| `approval.code_status` | Dit si le code est défini, et le verrou qui reste après cinq codes faux (`defined`, `locked_s`) |
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
- Un humain qui voudrait **accorder** sans preuve de présence : sans `ticket` valide ni `code`
  juste, l'erreur `-32001` porte `presence` — `undefined` (aucun code encore choisi),
  `required`, `wrong` (avec les essais restants) ou `locked` (avec les secondes restantes). Un
  programme de sa session ne connaît pas le code et ne peut plus accorder à sa place ;
  **refuser** reste ouvert sans code, pour que la voix « refuse » coupe toujours. Voir
  l'[ADR 0057](../adr/0057-accorder-exige-le-code-d-approbation.md).
- Un jeton signé par une autre clé — motif `bad_signature`.
- Une portée d'approbation inconnue ; le défaut est la plus étroite, jamais la plus large.

## Ce qui survit au redémarrage

La clé de signature, le code d'approbation et les **révocations** : `cap.revoke` inscrit le
sujet dans `/var/lib/prophet/capd/revocations.jsonl` (une ligne par sujet, ajoutée puis
synchronisée, `0600`) avant de répondre, et capd relit ce registre avant d'accepter son premier
appel. Sans lui, le jeton racine d'une mission révoquée redevenait valide jusqu'à son
expiration. Une dernière ligne interrompue (révocation jamais confirmée) est retirée ; toute
autre ligne illisible empêche le démarrage : capd ne devine pas ce qu'il a révoqué. Les jetons
délégués avant un redémarrage cessent de valoir, leurs parents n'étant plus au registre des
jetons émis : c'est le sens sûr.

## Le code d'approbation

capd n'en garde que l'empreinte (sel aléatoire, dérivation blake3 répétée) dans
`/var/lib/prophet/capd/code-approbation`, en `0600` : la session de l'humain ne la lit pas. Six
caractères au moins. Cinq codes faux verrouillent la preuve cinq minutes ; les tickets de
présence vivent en mémoire et meurent avec le service. `prophet cap code` le définit ou le
change ; un code oublié se remplace par l'administrateur (`sudo prophet cap code --remplacer`).

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

Une action irréversible ne s'autorise qu'une fois : capd tient pour ponctuelle toute autorisation
d'une demande irréversible, quelle que soit la portée demandée, et aucune autorisation permanente
ne couvre une action irréversible ; un refus peut valoir pour la tâche ou l'agent (ADR 0054).
