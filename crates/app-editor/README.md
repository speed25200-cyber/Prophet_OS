# Éditeur de référence

L'application qui montre ce que « publier SUP nativement » veut dire. Elle n'a pas d'interface
graphique : son cœur est l'état d'un document et les actions typées qui le modifient, validées
avant exécution, exactement ce qu'un agent manipule. `save` est marqué réversible, `send_email`
(factice) externe, pour exercer les approbations de bout en bout (M10-T5).

```sh
cargo test -p app-editor
```
