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

`http.fetch` relaie maintenant par le socket d'egress, sous le jeton de la tâche, avec lecture
automatique et écriture soumise à décision ; les outils `web.open`, `web.tree` et `web.act`
pilotent un navigateur par son arbre sémantique quand le service en nomme un. Le registre
demande à chaque outil les effets de l'appel précis avant de faire trancher capd.

Voir l'[ADR 0012](../adr/0012-acces-fichiers-mcp.md), l'[ADR 0024](../adr/0024-navigateur-integre-et-applications-web.md)
et l'[essai avec Qwen3 réel](../reports/mcp-fichiers-2026-09-13.md).

## Approbations

Quand capd refuse un appel faute de décision humaine, le registre soumet la demande lui-même
(`approval.request`, avec la requête exacte jugée et un résumé), la consigne au journal et rend
`ApprovalRequired` avec l'identifiant ; `approval.wait {id, timeout_s}` attend la décision (45 s
au plus par appel) et rend `allowed`, `denied`, `expired` ou `pending` ; le modèle réessaie
alors le même appel, qu'une décision « une fois » laisse passer une fois, et qu'une décision
de tâche ou d'agent couvre durablement. Voir l'[ADR 0041](../adr/0041-les-approbations-de-bout-en-bout.md).
