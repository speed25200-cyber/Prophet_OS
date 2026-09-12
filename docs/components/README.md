# Les composants en service

Un fichier par daemon : où est son socket, ce qu'il sert, ce qu'il refuse, et ce qui se passe quand
il n'est pas là. C'est ce dernier point qui compte le plus : un système de sept services se
diagnostique par ses absences.

| Composant | Socket | Sans lui |
|---|---|---|
| [`capd`](capd.md) | `capd.sock` | plus rien n'obtient de droit |
| [`ledger`](ledger.md) | `ledger.sock` | le système agit sans laisser de trace |
| [`vault`](vault.md) | `vault.sock` | aucun secret n'est substitué |
| [`egress`](egress.md) | `egress.sock` | aucune sortie réseau |
| [`sandboxd`](sandboxd.md) | `sandboxd.sock` | aucune tâche non fiable ne démarre |
| [`memoryd`](memoryd.md) | `memoryd.sock` | les agents n'ont pas de mémoire longue |
| [`agentd`](agentd.md) | `agentd.sock` | aucune tâche n'est planifiée |
| [`surface`](surface.md) | — (écran) | l'écran reste noir ; la ligne de commande fonctionne |

Tous les sockets sont en `0660`, dans `/run/prophet` qui est en `0770` pour le groupe
`prophet-system`. Un pair est accepté dans quatre cas : il est le service lui-même ; son groupe
principal est `prophet-system` ; l'administrateur l'a déclaré membre de ce groupe dans
`/etc/group` ; ou il est `root`. Ce dernier fichier n'est consulté que pour un pair qui serait
sinon refusé, et il n'est jamais mis en cache : un compte créé après le démarrage d'un daemon
est servi tout de suite, sans qu'il faille redémarrer les sept.

La troisième règle n'est pas un assouplissement mais une correction. `SO_PEERCRED` n'atteste que le
groupe **principal** du pair : un compte mis dans `prophet-system` par `extraGroups` y appartient
réellement, et voyait pourtant chaque daemon le refuser. La surface était dans ce cas — elle aurait
affiché un champ vide sur une machine parfaitement saine.

`root`, lui, passait autrefois pour un pair comme un autre. Le refus ne protégeait rien : `root`
lit les clés de signature dans `/var/lib/prophet` et peut émettre les jetons qu'il veut sans jamais
toucher à ces sockets. Il ne coûtait qu'une chose — `prophet status` inutilisable pour le
propriétaire de la machine. Une tâche isolée ne peut pas s'en servir : `SO_PEERCRED` traduit les
identifiants dans l'espace de noms du destinataire, et un `uid 0` non projeté y arrive en
`overflowuid`.

## Voir l'ensemble

```sh
systemctl --failed                      # ce qui ne tourne pas
systemctl list-units 'prophet-*'        # ce qui tourne
prophet status                          # ce que cette machine sait isoler
prophet task ls                         # ce qui travaille en ce moment
```
