# `prophet-types` — types fondamentaux

Ce crate ne fait rien : il définit les formats sur lesquels tout le système s'accorde et les
règles qui les gouvernent. `canon` (sérialisation canonique, base des signatures et hachages),
`pattern` (motifs de cible et couverture), `cap` (jetons, grants, délégation, décisions),
`manifest` (manifeste d'agent), `ledger` (événements du journal), `driver` (contrat des
pilotes). Les spécifications correspondantes sont dans [`docs/specs/`](../../docs/specs/).

```sh
cargo test -p prophet-types
```
