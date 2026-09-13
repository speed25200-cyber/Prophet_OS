# ADR 0029 — Délégation entre agents : des modèles qui avancent ensemble

Date : 2026-09-13. Statut : accepté, première livraison.

## Contexte

Une mission est menée par un agent et un modèle. Beaucoup de tâches gagnent à être partagées :
un modèle rapide lit et trie, un plus capable rédige ; un contexte web cherche, un contexte
documents écrit. Le plan prévoit `task.spawn_sub` (grants ⊆) et capd sait déjà déléguer un
jeton à une sous-tâche sans jamais l'élargir. Ce qui manquait : l'outil, la filiation dans le
runtime, le lancement et le retour du résultat.

## Décision

1. **Un outil, `task.delegate {intent, profile, model?}`.** Il n'existe que si le profil de la
   mission accorde `task.spawn` sur un contexte nommé du catalogue ; capd tranche ce droit sur
   le contexte visé avant tout. Sans contexte nommé, pas d'outil ; « tout contexte » n'existe pas.
2. **Le jeton de l'enfant est délégué par capd** (`cap.delegate`) : un sous-ensemble des droits
   du parent, jamais plus, jamais plus longtemps. Un contexte plus large que le parent est refusé,
   et le parent l'apprend comme une erreur d'outil.
3. **La sous-mission est une mission** : préparée avec le manifeste du contexte et le modèle
   demandé (ou celui du parent), planifiée, rattachée à son parent (filiation, profondeur bornée,
   budget **prélevé** à moitié sur celui du parent puis imputé à la fin), propriété du même
   humain, lancée dans le fil du parent, qui attend. Elle travaille dans son propre espace, ses
   outils sont ceux de son contexte (fichiers, formats, web, applications, et délégation à son
   tour), et ses changements s'examinent et s'appliquent comme ceux de toute mission.
4. **Le résultat revient au parent** comme celui d'un outil : identifiant, état, raison, texte.
   Le parent formule un objectif complet, l'enfant ne voit pas sa conversation ; ce qu'ils se
   disent tient dans l'intention et le résultat, tous deux au journal.

## Conséquences

- Des modèles différents coopèrent sur une tâche sous le contrôle de capd et sous les yeux de
  l'humain : chaque sous-mission est une ligne de la supervision, avec son parent.
- Aujourd'hui, l'enfant est un modèle local du même moteur ; un pilote officiel (Claude Code,
  Codex) ne peut être enfant que par une séance ouverte par l'humain (ADR 0026), pas de lui-même :
  les identifiants du client sont à l'humain, et le service ne les touche pas.
- Le parent est bloqué le temps de l'enfant, et une profondeur de trois est le maximum. Le
  passage de fichiers entre parent et enfant n'existe pas encore : ils se parlent par texte.
