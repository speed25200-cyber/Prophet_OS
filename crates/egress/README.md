# `egress` — proxy de sortie

L'unique voie réseau d'une tâche : les sandboxes n'ont pas de pile réseau, seul le socket de
ce proxy leur est monté. Pour chaque requête, dans l'ordre : qui demande (jeton dans l'en-tête
interne, retiré avant la sortie), a-t-il le droit (capd tranche sur l'hôte réellement joint),
est-ce une exfiltration (volume, entropie, motifs de secrets), puis seulement le relais. Les
méthodes modifiantes exigent une décision humaine. Les secrets sont substitués au dernier
moment, hors de portée du modèle, et jamais dans un tunnel (ADR 0007).

Les hôtes d'interrogation (`PROPHET_EGRESS_QUERY_HOSTS`) sont ceux dont un `POST` est une
question et non un effet, comme l'API de décision Jev : pour eux seulement, `POST` est contrôlé
comme une lecture. Un amont `https://` en forme absolue est joint sous TLS terminé par le proxy,
avec les racines de la machine, pour que la substitution reste possible (ADR 0042).

Le client de l'outil `http.fetch` de `mcp-system` parle à ce socket. Voir le
[contrat du service](../../docs/components/egress.md).

```sh
cargo test -p egress
```
