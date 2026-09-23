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

`prophet model ls` rend le catalogue des poids de la machine : le dossier des poids
(`PROPHET_MODELS_DIR`, sinon `/var/lib/prophet/models`) et les fichiers que `PROPHET_WEIGHTS`
nomme, comme le modèle par défaut de l'image dans `/nix/store`. Pour chacun, ce que son en-tête
GGUF dit de lui : architecture, taille annoncée, quantification, fenêtre de contexte et poids
du fichier, sans charger les poids ; un fichier illisible est dit refusé avec sa raison.
`--dir` lit un dossier seul, `--json` rend `weights` et `refused`. Le moteur local
(`--endpoint`, sinon `PROPHET_MODEL_ENDPOINT`) est interrogé sur ce qu'il sert (`/props`) :
le fichier chargé est marqué avec la fenêtre accordée à chaque requête — 4 096 tokens sur
l'image, là où le fichier en annonce souvent dix fois plus —, `served` en JSON ; injoignable,
le catalogue se lit quand même et le dit.

Validation : `nix develop --command cargo test -p prophet-cli`. Les tests de processus
`task_service` reproduisent une consultation avec captures inaccessibles. Les vrais comptes
et services sont exercés par `nix build .#checks.x86_64-linux.services`.

## Un client MCP dans une mission

`prophet task options` liste les contextes du service, leurs modèles disponibles et l'état du
navigateur piloté ; `prophet task prepare --profile <contexte> --model <modèle> "<objectif>"`
prépare une mission sans manifeste ni droits fournis par la CLI, comme « Nouvel objectif » ;
`--client` la destine à un client MCP, sans exiger le moteur local (le modèle du contexte
suffit, `--model` devient facultatif). `prophet task retry <mission> [--id <référence>]`
prépare à nouveau une mission échouée ou arrêtée, par le même contexte, avec le même modèle et
la même intention, sans la lancer — comme « Relancer » dans l'inspecteur ; une mission
préparée hors du catalogue se prépare avec `prophet task prepare`.
`prophet task attach <mission>` ouvre une séance d'outils depuis le terminal, `prophet task
call <mission> <outil> '<json>'` y appelle un outil (`ui.tree`, `ui.act`, `fs.write`…) et
`prophet task detach <mission>` la ferme, la mission passant à l'examen : c'est ainsi que le
test du bureau fait écrire un agent dans l'éditeur par son arbre d'accessibilité.
`prophet task mcp-config <mission>` rend, pour une mission préparée et non lancée par le même
utilisateur, la configuration qui donne à Claude Code (`claude --mcp-config <fichier>`) ou à
Codex (`mcp_servers` de sa configuration) les outils de cette mission par le pont `prophet-mcp`.
Le client travaille alors dans le travail de la mission, sous le jeton tenu par `agentd` ; à sa
fermeture, la mission passe en `done` et `prophet task diff`, `apply`, `undo` s'appliquent.
