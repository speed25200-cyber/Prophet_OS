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

Tous les sockets sont en `0660`, dans `/run/prophet` qui est en `0750` pour le groupe
`prophet-system`. Un pair est accepté s'il appartient à ce groupe ou s'il est le service lui-même.
`root` n'y passe pas par faveur : le jour où un programme tourne en root sans qu'on l'ait voulu, on
préfère qu'il soit refusé comme n'importe qui.

## Voir l'ensemble

```sh
systemctl --failed                      # ce qui ne tourne pas
systemctl list-units 'prophet-*'        # ce qui tourne
prophet status                          # ce que cette machine sait isoler
prophet task ls                         # ce qui travaille en ce moment
```
