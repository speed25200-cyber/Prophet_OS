# ADR-0049 — Rappeler au modèle le fichier que l'objectif demande, quand il conclut sans lui

- **Statut** : accepté ; mesuré sur le banc M13 (ADR 0048, `6d01c38`)
- **Date** : 2026-09-23
- **Tâche liée** : M13-T1, M13-T4 ; M4 (boucle native d'agentd)

## Contexte

Le premier passage du banc M13 (`f4d13c6`, Qwen3 1.7B en Q8_0, sept tâches, une exécution
chacune) donne 0 réussite sur 7 par Prophet et 2 sur 7 par la boucle nue. Cinq des sept échecs
de Prophet ont la même forme : la mission se termine proprement, en deux étapes, et le fichier
que l'objectif nomme (`~/ventes/out/resume.md`, `~/documents/out/trouve.txt`…) n'existe pas.
Le modèle a lu, puis a répondu par du texte au lieu d'écrire. Côté boucle nue, les mêmes tâches
échouent de la même façon ; les deux réussites sont des exécutions où le modèle a, par hasard
d'échantillonnage, continué jusqu'à l'écriture (six tours).

Un humain qui confie « écris le total dans `~/ventes/out/total.txt` » attend ce fichier, pas un
message. Le service sait, sans interpréter l'objectif, ce qu'il nomme et ce qui existe.

## Décision

- **Un livrable est un chemin que l'objectif nomme** (`~/…`, ponctuation collée ôtée, sans
  `..`), **que la portée de la mission couvre** (l'espace de travail SFS sait le traduire) **et
  qui n'existe pas au départ** dans l'espace de travail. Un fichier qui existe déjà est une
  entrée ; un chemin hors de la portée n'est pas au service de le réclamer.
- **À chaque conclusion du modèle**, agentd (`agentd::livrables::Rappel`, autour du compteur
  commun) vérifie que chaque livrable existe dans l'espace de travail. S'il en manque, la
  conclusion et un message de l'utilisateur sont insérés à cette place dans l'historique que le
  modèle reçoit, et il est interrogé de nouveau. Le message dit un fait et laisse juger :
  « L'objectif nomme ~/…, qui n'existe pas encore. S'il vous revient de le produire,
  écrivez-le avec l'outil d'écriture de fichiers, puis concluez ; sinon, concluez en disant
  pourquoi. » — le service ne sait pas si l'objectif demandait ce chemin ou le nommait comme
  une entrée absente (« lis ~/notes/todo.txt »).
- **Borné** : un nouveau rappel n'a lieu que si le modèle a produit un livrable depuis le
  précédent (il en manque moins) ; un rappel décliné ne se répète donc pas. Au plus **deux
  rappels** par mission ; au-delà, sa conclusion est rendue telle quelle.
- **Chaque interrogation est une étape** : elle passe par le compteur commun (plafond d'étapes
  vérifié avant, tokens imputés au modèle qui a répondu), comme toute autre.
- **Le rappel se journalise** : événement `task.reminded` (`missing`, `nth`) avant l'envoi ; le
  résultat de la mission porte `reminded`, les chemins rappelés.
- **Le rappel ne donne aucun droit.** L'écriture passe par le même outil, le même jeton, la même
  politique Cedar et le même espace de travail ; un chemin que le jeton ne permet pas reste
  refusé.

## Alternatives écartées

- **Une consigne générale au départ** (« écrivez les fichiers demandés ») : elle coûte des
  tokens à chaque mission et le petit modèle l'oublie après la première lecture ; le rappel ne
  coûte que lorsqu'il manque quelque chose, au moment où le modèle conclut.
- **Faire échouer la mission** : l'humain ne gagne rien à un échec de plus ; le modèle, lui,
  sait souvent faire quand on lui dit ce qui manque.
- **Écrire la réponse du modèle dans le fichier à sa place** : le service inventerait le
  contenu d'un livrable ; il ne fait que dire ce qui manque.
- **Comprendre l'objectif** (un modèle qui en extrait les livrables) : un second modèle, des
  tokens, et une interprétation là où un chemin écrit suffit.

## Conséquences

- La boucle nue du banc n'a pas ce rappel : il fait partie de ce que Prophet ajoute, et le banc
  en mesure l'effet (réussites, tokens, exécutions rappelées).
- Un objectif qui nomme un chemin à ne pas créer (« sans créer ~/x ») ou une entrée absente
  reçoit, au pire, un rappel que le modèle peut décliner ; le banc n'en contient pas.
- Premier passage du banc avec le rappel (`6d01c38`) : message impératif (« Vous n'avez pas
  encore écrit … »), deux rappels sans condition. Les quinze exécutions rappelées (sur 45) ont
  toutes écrit le fichier demandé, et « ne-pas-toucher-au-reste » passe de 0 à 3 sur 3 ; le
  contenu, lui, reste celui du modèle (« 0 € » quand il n'a rien lu). Le message factuel et la
  condition de progrès viennent ensuite, pour ne pas pousser le modèle à créer une entrée
  absente.
- Un livrable produit hors de la portée n'est pas vu ; c'est voulu : la mission ne peut pas y
  écrire.
