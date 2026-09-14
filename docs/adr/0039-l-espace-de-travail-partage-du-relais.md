# ADR-0039 — Une sous-mission part de l'espace de travail de son parent et y rapporte son travail

- **Statut** : accepté
- **Date** : 2026-09-14
- **Tâche liée** : M8-T7, M4-T2

## Contexte

Le relais (ADR 0034) fait travailler plusieurs modèles sur une même mission : la réflexion
découpe, le code écrit, la relecture juge, chacun dans sa propre sous-mission sous un jeton
délégué par capd. Depuis l'ADR 0035 et son complément du 14 septembre, ces modèles sont
d'abord les clients officiels de l'humain — Claude Code réfléchit et relit, Codex code et
exécute — lancés dans la session par le lanceur de pilotes. En prouvant Codex menant une
mission et confiant la relecture à Claude Code, une limite est apparue : chaque mission ouvre
son espace de travail SFS par une capture des fichiers de l'humain (ADR 0018), et une
sous-mission ne voyait donc pas ce que son parent venait d'écrire sans l'avoir publié. Le
relecteur lisait un fichier absent ; le codeur repartait de zéro ; et le travail d'une
sous-mission ne rejoignait celui du parent qu'en le publiant à part, examiné à part. Le relais
n'était pas un travail commun, mais des travaux côte à côte.

## Décision

1. **L'enfant part de l'espace du parent.** À l'ouverture de son espace de travail, une
   sous-mission dont le parent a un espace ouvert capture ses périmètres depuis le répertoire
   de travail du parent, non depuis les fichiers de l'humain ; un périmètre que le parent n'a
   pas vient du répertoire personnel. Les droits de lecture se jugent toujours sur les chemins
   du répertoire personnel, par capd, comme pour toute capture. La capture garde ses bornes
   (taille, profondeur, liens refusés, ouvertures relatives sans symboles).
2. **Le travail de l'enfant revient au parent.** Quand une sous-mission finit sans erreur, le
   service rapporte son diff — ce qui a changé par rapport à l'état du parent qu'elle a reçu —
   dans l'espace du parent : fichiers créés ou modifiés copiés, supprimés retirés, dans les
   périmètres du parent seulement ; le reste demeure chez l'enfant. Aucun lien n'est suivi, la
   copie passe par un fichier provisoire renommé. Le résultat rendu au parent (`carried`) dit
   ce qui est revenu ; ce qui n'a pas pu l'être est dit sans faire échouer la délégation. Une
   sous-mission en échec ne rapporte rien.
3. **Le parent publie le tout.** Une sous-mission ne se publie pas seule (`can_apply`,
   `can_undo` faux pour une mission qui a un parent) : son travail atteint les fichiers de
   l'humain par la publication du parent, examinée d'un seul tenant, sous les droits du parent.
   L'espace de l'enfant reste consultable (son diff dit ce qu'il a fait).

## Conséquences

- Le relais devient un travail commun : Claude Code découpe, Codex écrit dans l'espace de la
  mission, Claude Code relit ce que Codex vient d'écrire, et l'humain examine et publie une
  seule fois. Preuves : dans sfs, une sous-tâche part de l'espace du parent (ses fichiers
  modifiés et ajoutés, un périmètre absent chez lui pris au répertoire personnel), rapporte
  ses changements (modifié, supprimé, ajouté) sans toucher au répertoire personnel, ce qui
  sort des périmètres du parent reste chez elle, et la publication du parent porte le tout ;
  dans agentd, avec les vrais services, le faux Claude Code lit le code que le faux Codex vient
  d'écrire dans la mission parente, y dépose son verdict, qui revient chez Codex ; et le code
  écrit par Codex pour un parent local est chez ce parent à la fin.
- Limites : le rapport au parent est une copie de fichiers, pas une fusion — deux sous-missions
  qui modifient le même fichier l'une après l'autre laissent le dernier mot à la dernière ; le
  parent qui écrit un fichier pendant qu'un enfant le modifie est écrasé au retour de l'enfant
  (le parent attend son enfant, ce cas ne se présente que par un client qui continuerait à
  écrire pendant sa délégation). Un périmètre de l'enfant hors de ceux du parent ne revient pas
  et ne se publie plus nulle part : le catalogue doit donner à un contexte confié des
  périmètres compris dans ceux du parent, ce que les contextes de l'image respectent.
