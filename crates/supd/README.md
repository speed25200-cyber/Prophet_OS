# `supd` — adaptateur d'accessibilité de la session

Les applications de bureau publient leur arbre d'accessibilité (AT-SPI 2) sur le bus de la
session de l'humain, que rien d'extérieur ne peut joindre. `prophet-supd` tourne donc dans la
session : il lit ces arbres, les rend en SUP (avec la confiance qu'une lecture d'accessibilité
mérite), et exécute les actions typées qu'`agentd` lui demande après avoir fait trancher capd —
`click`, `set_field`, `toggle`, par ce que l'application déclare (`Action`, `EditableText`),
jamais par une touche ou un clic simulés, jamais par un pixel.

Le socket (`/run/prophet/sup.sock`) porte le groupe système de Prophet et n'admet que le compte
`agentd`, attesté par `SO_PEERCRED`. Les outils `ui.apps`, `ui.tree` et `ui.act` de `mcp-system`
en sont les clients.

```sh
cargo test -p supd                 # se tait sans banc d'essai
tools/bureau-local.sh              # X virtuel, bus de session, bus d'accessibilité, Mousepad
tools/bureau-local.sh cargo test -p supd --test bureau dump_de_l_arbre_brut -- --nocapture
```

Voir l'[ADR 0027](../../docs/adr/0027-pilotage-des-applications-par-l-accessibilite.md) et
[docs/components/supd.md](../../docs/components/supd.md).
