# Outils MCP système

La bibliothèque fournit le registre, le contrôle des capacités, les outils et une session MCP
sur des flux délimités par des sauts de ligne. Le binaire `prophet-mcp` n'est pas encore raccordé
aux daemons : il refuse explicitement de servir. La présence d'un outil dans la bibliothèque
ne prouve pas que son service est intégré ; plusieurs outils renvoient encore une indisponibilité.

Chaque appel du registre vérifie le droit `tool.call`, puis la capacité sur la ressource concrète.
Une exigence inconnue, une cible de ressource absente, un niveau de sandbox inférieur au minimum
ou une tâche différente du sujet du jeton provoquent un refus avant l'exécution de l'outil.

La session exige `initialize`, puis `notifications/initialized`, avant `tools/list` et
`tools/call`. La version prise en charge est `2025-06-18`. Le transport limite chaque message
entrant à 1 Mio, refuse les lignes tronquées et ne traite jamais une notification comme un
appel d'outil. Les réponses sont exclusivement du JSON-RPC ; les diagnostics vont sur stderr.

```sh
nix develop --command cargo test -p mcp-system
```

Les tests couvrent les droits, les écritures en espace de travail, le refus hors périmètre,
le protocole, les entrées invalides et les limites du transport. Ils ne prouvent pas encore
un accès sûr en présence de liens symboliques ou de modifications concurrentes du système de
fichiers. Les outils utilisent encore des chemins ordinaires, et la recherche récursive ne
refait pas le contrôle de capacité pour chaque descendant. Corriger ces accès, raccorder capd
et le journal en service, puis exercer le binaire avec les vrais daemons sont les prochaines
étapes avant de le relier à agentd et aux modèles locaux.

La négociation suit le [cycle de vie MCP 2025-06-18](https://modelcontextprotocol.io/specification/2025-06-18/basic/lifecycle).
