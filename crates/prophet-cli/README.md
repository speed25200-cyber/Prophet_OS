# prophet-cli

`prophet` consulte et commande les services depuis le compte humain.

```sh
prophet task ls
prophet task show task:reference
prophet task diff task:reference
prophet --json task show task:reference
```

La liste, le détail et le diff utilisent agentd. Les missions terminées restent dans la
liste ; un diff indisponible est une erreur explicite. Les captures privées ne sont pas
ouvertes par la CLI. `--json task show` rend l'inspection complète ; `--json task diff`
rend son diff conservé. Aucune de ces commandes n'applique les changements.

Sans service, la liste peut encore signaler les anciens espaces de travail accessibles au
compte. L'ancien `task undo` agit sur ces espaces de bibliothèque ; il n'est pas raccordé
à la validation et à l'annulation des missions installées. Leur parcours sûr reste à livrer.

Validation : `nix develop --command cargo test -p prophet-cli`. Les tests de processus
`task_service` reproduisent une consultation avec captures inaccessibles. Les vrais comptes
et services sont exercés par `nix build .#checks.x86_64-linux.services`.

## Un client MCP dans une mission

`prophet task mcp-config <mission>` rend, pour une mission préparée et non lancée par le même
utilisateur, la configuration qui donne à Claude Code (`claude --mcp-config <fichier>`) ou à
Codex (`mcp_servers` de sa configuration) les outils de cette mission par le pont `prophet-mcp`.
Le client travaille alors dans le travail de la mission, sous le jeton tenu par `agentd` ; à sa
fermeture, la mission passe en `done` et `prophet task diff`, `apply`, `undo` s'appliquent.
