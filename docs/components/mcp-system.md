# MCP système

`mcp-system` fournit les descriptions d'outils, leur registre avec contrôle de capacités,
la session MCP et un exécuteur pour la boucle native. Les appels et leurs résultats sont
journalisés avec une empreinte des arguments, sans leur contenu. La session est bornée et
exige la négociation d'initialisation avant l'accès aux outils.

La bibliothèque de fichiers utilise `openat2` et des descripteurs de répertoire, borne les
contenus et recontrôle les droits des descendants. Ses écritures restent dans le travail SFS.
Le [guide du crate](../../crates/mcp-system/README.md) détaille les garanties et commandes ;
la [spécification](../specs/mcp-system-tools.md) donne les limites de chaque opération.

Le binaire refuse encore de démarrer un service opérationnel. La provenance du contexte de
tâche, les connexions capd/ledger, les racines privées et le cycle de vie agentd restent à
raccorder. Plusieurs outils du registre complet sont aussi des fonctions indisponibles : le
lanceur doit n'exposer que ceux effectivement implémentés et autorisés. L'exécuteur natif
ne constitue pas, à lui seul, un transport MCP installé ni un mécanisme de confinement.

Voir l'[ADR 0012](../adr/0012-acces-fichiers-mcp.md) et l'[essai avec Qwen3 réel](../reports/mcp-fichiers-2026-09-13.md).
