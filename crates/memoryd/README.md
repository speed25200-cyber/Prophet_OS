# `memoryd` — mémoire

Trois mémoires, un seul magasin : de travail (tenue par le runtime), épisodique (ce qui s'est
passé, tâche par tâche, avec un lien vers le journal) et sémantique (ce que le système sait de
l'utilisateur et de sa machine). Une entrée porte toujours sa provenance et vit dans un
espace : un agent de travail ne lit pas la mémoire personnelle, et rien ne quitte la machine.

Voir le [contrat du service](../../docs/components/memoryd.md).

```sh
cargo test -p memoryd
```
