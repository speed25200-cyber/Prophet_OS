# ADR-0006 — La restriction des espaces de noms par la distribution est une dépendance déclarée

- Statut : accepté
- Date : 2026-09-12
- Contexte du plan : M5 (niveaux d'isolation)

## Contexte

Ubuntu 24.04 et suivantes activent par défaut
`kernel.apparmor_restrict_unprivileged_userns`. Sous ce réglage, un programme absent des profils
AppArmor livrés peut **créer** un espace de noms utilisateur mais ne peut rien **exécuter**
dedans. L'échec arrive sous la forme d'un `EACCES` nu, dans un composant qui n'en est pas la
cause.

Le défaut a été trouvé au premier passage des tests sur une machine réelle. Il frappe les deux
niveaux inférieurs pour la même raison :

- niveau 0 : `prophet-sandbox-helper` est refusé après `unshare` ;
- niveau 1 : `runsc --rootless` échoue sur
  `re-executing self: fork/exec /proc/self/exe: permission denied`.

Trois sondes concluaient pourtant que tout allait bien. `unshare --user true` réussit, parce que
l'outil `unshare` figure justement dans les profils livrés par Ubuntu. Notre propre sonde
réussissait aussi, parce qu'elle se contentait de créer l'espace de noms sans rien exécuter
dedans. Une capacité vérifiée à moitié est une capacité non vérifiée.

## Décision

1. La restriction est **détectée et nommée** : `Capabilities` porte
   `userns_restreint_par_politique`, et `prophet status` avertit en donnant le remède. Un système
   qui ne peut pas s'isoler doit le dire avant qu'on le lui demande, pas au moment de l'échec.
2. Prophet OS **ne lève pas** cette restriction de sa propre initiative. C'est une protection de
   la machine hôte ; la lever est une décision de son propriétaire.
   `tools/install-isolation.sh userns` ne l'applique que si `PROPHET_AUTORISER_USERNS=1` est
   posé, et dit comment revenir en arrière.
3. L'intégration continue la lève explicitement, parce que le coureur est jetable et qu'on y
   cherche à exercer l'isolation, pas à protéger la machine.

## Conséquences

- Sur une Ubuntu 24.04 récente installée telle quelle — le cas de la plupart des serveurs loués —
  Prophet OS annonce que les niveaux 0 et 1 sont indisponibles jusqu'à décision de l'exploitant.
  C'est un aveu, pas une régression : avant, il l'ignorait et échouait plus tard, plus mal.
- La bonne réponse à terme n'est pas de désactiver la protection mais de **livrer un profil
  AppArmor** pour `prophet-sandbox-helper` et `runsc`, qui rende à ces deux binaires le droit
  qu'Ubuntu accorde déjà à `unshare` et `bwrap`. Cette tâche reste à faire, et ce document est là
  pour qu'on ne l'oublie pas.
- Les images Prophet OS construites par Nix ne sont pas concernées : elles n'embarquent pas cette
  politique. Le problème n'existe que lorsque Prophet OS tourne **au-dessus** d'une distribution
  hôte.
