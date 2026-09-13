# 0020 — Conserver un accès humain malgré les captures privées et les pannes graphiques

Date : 13 septembre 2026. Statut : accepté.

La CI de `d20ef74` échoue sur `prophet task ls` sous le compte humain. La réponse d'agentd
arrive, puis la CLI tente de lire `~/.prophet/tasks`, désormais privé au service. Cette seconde
lecture échoue et fait perdre la première. `show` et `diff` utilisaient également le disque
directement, contrairement à la surface qui consulte l'inspection conservée par agentd.

La liste reçue du service devient la réponse de référence. `show` et `diff` utilisent
`task.inspect`, avec contrôle de la référence de mission et formats humain/JSON. Un diff
indisponible reste distinct d'un diff vide. Les modes et propriétaires des captures ne changent
pas. La liste historique hors service et l'ancien undo de bibliothèque ne constituent pas un
parcours de récupération ou d'annulation des missions installées.

Le test installé échoue aussi en attendant `login:` : le service de secours écrit plusieurs
fois directement sur tty1 et fait défiler l'invite hors écran. Le diagnostic est désormais
publié dans `/run/issue.d/prophet-surface.issue` par remplacement atomique. `agetty --reload`
rafraîchit l'invite uniquement si la saisie n'a pas commencé. Aucun terminal ni processus de
connexion n'est repris par ce service. La configuration NixOS évaluée inclut `/run/issue.d`
dans `--issue-file`. Voir le [manuel d'agetty](https://man7.org/linux/man-pages/man8/agetty.8.html).

Le test court `surface-rescue` injecte un échec à la place du programme graphique et exerce le
vrai secours : avis après l'invite, nouvel avis pendant le mot de passe, puis après connexion.
Le test installé garde ces contrôles avec les vrais services et le chargeur d'amorçage. Ce
secours console ne remplace pas le bureau humain avec plusieurs applications demandé par
l'[ADR 0009](0009-clients-officiels-et-bureau.md).
