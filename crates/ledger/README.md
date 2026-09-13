# `ledger` — journal d'audit

Stockage en ajout seul, chaîné par hachage et scellé périodiquement : toute modification,
suppression ou insertion rompt la chaîne, une réécriture complète est détectée par les sceaux
signés, et une tâche se rejoue étape par étape. `prophet-ledger` est le seul écrivain ; les
services lui poussent leurs événements (`ledger.append`) et la surface, la CLI et agentd les
relisent (`ledger.query`, `ledger.verify`).

Voir la [spécification des événements](../../docs/specs/ledger-event.md) et le
[contrat du service](../../docs/components/ledger.md).

```sh
cargo test -p ledger
```
