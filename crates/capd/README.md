# `capd` — broker de capacités et moteur de politique

Aucun droit n'existe sans un jeton émis ici **et** une politique Cedar qui l'autorise. Trois
couches, dans cet ordre : la politique (avec ses interdits absolus), le jeton (signature, chaîne
de parents, expiration, grant couvrant la demande), puis les approbations humaines pour les
actions irréversibles ou externes. Le binaire `prophet-capd` est le seul émetteur de jetons du
système : `cap.mint`, `cap.check`, `cap.revoke`, `approval.*`.

Voir le [contrat du service](../../docs/components/capd.md), la
[spécification des jetons](../../docs/specs/capability-token.md) et `policies/default.cedar`.

```sh
cargo test -p capd
```
