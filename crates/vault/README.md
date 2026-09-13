# `vault` — coffre à secrets

Règle unique : un modèle ne voit jamais la valeur d'un secret. Un agent demande « appelle ce
service avec mon identité » et reçoit une référence ; le proxy de sortie substitue la valeur au
dernier moment, hors de portée du modèle. Le coffre chiffre au repos (ChaCha20-Poly1305), avec
une clé en fichier 0600, scellée par le TPM là où il y en a un. Les fichiers d'identifiants des
clients officiels n'y entrent jamais : ils restent aux clients.

Voir le [contrat du service](../../docs/components/vault.md).

```sh
cargo test -p vault
```
