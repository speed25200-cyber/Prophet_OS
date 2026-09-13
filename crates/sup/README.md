# `sup` — Semantic UI Protocol

La fin des captures d'écran : une application expose ce qu'elle est (un arbre d'état, avec des
rôles, des noms, des valeurs) et ce qu'on peut lui faire (des actions typées, avec `irreversible`,
`external`, `requires`). L'observation pèse des kilooctets au lieu de mégaoctets, et une action
est nommée au lieu d'être un clic en (x, y). Le pont navigateur et l'éditeur de référence
publient cet arbre ; les outils `web.*` le consomment, `ui.tree` et `ui.act` sont spécifiés.

```sh
cargo test -p sup
```
