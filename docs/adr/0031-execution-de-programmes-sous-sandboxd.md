# ADR 0031 — Exécuter des programmes : `proc.exec` sous sandboxd

Date : 2026-09-13. Statut : accepté, première livraison.

## Contexte

« Compatible avec tout programme » commence par pouvoir en lancer un. `proc.exec` existait dans
le registre comme un talon qui refusait tout ; sandboxd savait lancer un programme confiné mais
n'en rendait pas la sortie. Un agent qui veut compter des lignes, comparer deux fichiers ou lire
la sortie d'un convertisseur n'avait rien.

## Décision

1. **`sandbox.run`** : sandboxd exécute un programme sous la même sandbox que `sandbox.start`,
   attend jusqu'à un délai, tue au-delà, et rend code de retour, sortie et erreur bornées.
2. **`proc.exec {program, args, level?, timeout_s?}`** : l'outil résout le programme par le
   chemin du service (ou un chemin absolu, jamais relatif ni remontant), le lance par sandboxd
   dans l'espace de travail de la tâche, avec les règles du jeton **en lecture seule** sur le
   home et l'espace de travail comme seul lieu d'écriture : ce qu'une commande produit s'examine
   et s'applique comme toute écriture d'agent.
3. **Deux niveaux.** Une courte liste d'utilitaires qui ne modifient rien (`cat`, `ls`, `wc`,
   `head`, `tail`, `sort`, `uniq`, `grep`, `cut`, `tr`, `diff`, `file`), désignés par leur
   nom, tourne confinée sur place (niveau 0 : espaces de noms, Landlock, seccomp) et sans
   décision humaine ; tout autre programme exige la microVM (niveau 2) et est tenu pour
   irréversible, donc soumis à décision. Un niveau demandé ne s'abaisse jamais.
4. **Un droit par programme.** Le profil accorde `proc.exec` sur des noms de programmes, jamais
   « tout » ; capd tranche sur le nom à chaque appel. Le fait « utilitaire confiné » est établi
   par capd, jamais déclaré par l'appelant.
5. **Un chemin n'est jamais un utilitaire confiné.** `cat` désigne ce que le PATH du service
   résout ; `/tmp/x/cat` est un binaire choisi par l'appelant, donc du code arbitraire : microVM,
   et la cible jugée par capd reste le chemin, jamais son nom de base. Un motif `wc` du profil
   n'accepte que le nom nu ; un motif `/usr/bin/**` accepte des chemins.
6. **Une sandbox est un groupe de processus.** sandboxd lance l'amorçage chef de son groupe et
   gèle, dégèle ou tue le groupe entier : une commande interrompue au délai n'abandonne pas ses
   enfants (le `sleep` d'un `sh -c`) qui, sinon, gardaient les tubes ouverts et la sortie
   suspendue. Au niveau 0, `setsid` et `setpgid` sont refusés par seccomp pour qu'un programme
   ne quitte pas son groupe.

## Conséquences

Correction après audit local du 13 septembre : `rg` n'est pas un simple lecteur, car
`rg --pre /bin/sh fichier` exécute le contenu du fichier. Il exige désormais le niveau 2
et une décision humaine. Les commandes `sandbox.run` sont suivies pendant leur attente :
liste, gel global, dégel et arrêt utilisent la même poignée que l'exécution. Une seconde
commande sur la même tâche est refusée. Le test réel vérifie aussi l'état arrêté dans `/proc`.

- Les agents lisent, comptent, comparent et convertissent avec les outils du système, sous
  contrôle, et le journal garde chaque commande.
- La microVM exige KVM, Firecracker et des images d'invité : sans eux, un programme hors liste
  est refusé avec la raison, pas exécuté avec moins d'isolation. La liste blanche vit dans le
  code ; l'élargir est un choix à justifier.
- Le PATH vu par la commande est celui du service : ce que l'image y met (poppler, ffmpeg,
  tesseract, coreutils) est atteignable, le reste non. Un test le prouve sans sandboxd (plan) ;
  l'exécution réelle est prouvée par le daemon sur une machine à espaces de noms (`sandbox.run`
  rend code 3, sortie et erreur d'un `sh -c`, et tue en une seconde un `sleep 30`).
- Le niveau 0 n'a pas d'espace de noms PID : le groupe de processus, avec `setsid`/`setpgid`
  refusés, tient lieu d'arbre. Un espace de noms PID (l'amorçage devenant l'init de la sandbox)
  serait la forme plus forte, à faire quand `proc.exec` servira au-delà des utilitaires.
