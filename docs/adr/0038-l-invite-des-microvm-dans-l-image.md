# ADR-0038 — L'invité des microVM est construit par le dépôt et embarqué dans l'image

- **Statut** : accepté
- **Date** : 2026-09-14
- **Tâche liée** : M5-T3, M8-T7

## Contexte

Le niveau 2 de sandboxd — une microVM Firecracker par programme hors liste blanche (ADR 0031)
— exigeait trois choses qu'aucune machine installée n'avait : le moniteur, un noyau d'invité,
une racine d'invité. `tools/install-isolation.sh microvm` les procurait à un hôte de
développement en téléchargeant les artefacts d'essai publiés par le projet Firecracker (un
noyau, une racine Ubuntu de plusieurs centaines de mégaoctets), à des chemins qui bougent. Sur
la machine installée, le niveau 2 était donc « inatteignable, il manque le binaire firecracker
et les images d'invité » — et avec lui tout programme qu'un agent voudrait exécuter : un script
Python, un outil qu'il vient d'écrire. C'est la marche qui manque à l'atelier logiciel que le
plan demande.

## Décision

Le dépôt construit l'invité : `invite-microvm`, un paquet du flake qui réunit le noyau publié
par Firecracker pour ses essais (épinglé par son empreinte, configuré pour lui : virtio par
MMIO, console série, pas de PCI) et une racine squashfs construite par Nix — un busybox
statique, Python 3 et sa fermeture, et un `/init` qui lit la ligne de commande du noyau. La
machine installée sert ce paquet à sandboxd par `PROPHET_MICROVM_KERNEL` et
`PROPHET_MICROVM_ROOTFS`, et met `firecracker` sur le chemin du service ; l'unité voyait déjà
`/dev/kvm`. L'invité dit tout sur la console série, que sandboxd lit : « PROPHET_INVITE_PRET »
quand la racine est montée, « PROPHET_INVITE_FIN code=N » quand le programme a rendu la main,
puis il s'éteint, et le moniteur avec lui. L'espace de travail de la tâche lui arrive par un
second disque (`/dev/vdb`), monté à l'endroit que la ligne de commande nomme, avec un fichier
`.prophet/exec.sh` qui porte le programme, ses arguments et son environnement. Côté hôte,
sandboxd remplit ce contrat sans privilège (`crates/sandboxd/src/invite.rs`) : le disque est
une image ext4 faite du répertoire de travail par `mkfs.ext4 -d`, le script y est écrit par
`debugfs`, le moniteur reçoit le disque en second lecteur, la console est lue entre les deux
marques (les lignes du noyau écartées), le code du programme en est tiré, et le disque est
rapatrié par `debugfs rdump` dans le répertoire de travail — ce que le programme a écrit ou
modifié revient, ce qu'il a effacé reste, le répertoire étant annulable par ailleurs. Le
programme est cherché dans l'invité par son nom (`python3` de l'hôte est `/bin/python3`
là-bas) ; la racine ne s'image jamais, et un répertoire de plus de 2 Gio est refusé.

L'hôte de l'intégration continue, qui a KVM, construit l'invité et l'emploie pour les essais
de niveau 2, à la place de la racine Ubuntu téléchargée.

## Alternatives écartées

- Télécharger les images à l'installation, comme l'outil de développement : des chemins qui
  bougent, une racine Ubuntu qu'on ne construit pas et qu'on ne peut pas lire, et une machine
  installée qui dépendrait d'un dépôt tiers au moment de s'installer.
- Construire le noyau avec Nix : le noyau générique de NixOS charge virtio par modules, donc
  exige un initrd ; un noyau taillé pour Firecracker demanderait une compilation d'une heure à
  chaque poussée. Le noyau publié fait ce travail, et son empreinte le fige.
- Une racine Ubuntu : plusieurs centaines de mégaoctets pour un shell et un interpréteur ; la
  racine construite ici pèse ce que pèse Python.

## Preuve

Sur la machine de cette session (WSL2, KVM imbriqué), Firecracker 1.16.1 démarre l'invité :
« PROPHET_INVITE_PRET », « PROPHET_INVITE_FIN code=0 », « reboot: Restarting system », et le
moniteur sort avec le code 0 en 1,1 s, trois fois sur trois. La racine pèse 88 Mo, le noyau
41 Mo. Puis le contrat entier, par le test `le_niveau_deux_execute_un_programme_et_rapatrie_
ses_fichiers` (`needs_kvm`) : un programme Python lit `entree.txt` dans l'espace de travail,
dit « somme 7 » sur la console, écrit `resultat.txt`, sort avec le code 7 ; l'hôte lit
« somme 7 », le code 7, et retrouve `resultat.txt` à côté d'`entree.txt` intact — 1,8 s de
bout en bout, microVM comprise ; le démarrage seul prend 56 ms. Un premier essai laissait le noyau lancer l'init de busybox (`/sbin/init`), qui
cherchait `/etc/init.d/rcS` et attendait une console : `/sbin/init` est désormais le script
lui-même ; un second laissait l'invité en « System halted » sur `poweroff` : c'est `reboot -f`
qui, sous `reboot=k`, fait sortir le moniteur.

## Conséquences

- Le niveau 2 devient possible sur toute machine installée qui a KVM : Firecracker et l'invité
  sont là. Sans KVM, rien ne change : sandboxd le dit, et le niveau 2 reste refusé.
- Le niveau 2 exécute pour de vrai : `proc.exec` d'un programme hors liste blanche passe par
  ce contrat, et sa sortie comme son code reviennent à l'agent comme au niveau 0. La sortie
  d'erreur du programme se mêle à sa sortie standard, la console n'ayant qu'un canal ; un
  programme qui écrirait lui-même une ligne horodatée entre crochets la verrait écartée.
- L'hôte a besoin d'e2fsprogs (`mkfs.ext4`, `debugfs`) : l'image le met sur le chemin de
  sandboxd ; sans lui, le lancement le dit.
- Un contexte « Atelier logiciel » du catalogue s'appuie dessus : écrire un outil dans
  `~/Documents/Prophet/outils` et l'exécuter en microVM. En machine virtuelle sans KVM
  imbriqué — les essais d'image de la CI — il est proposé mais son exécution est refusée avec
  ce qui manque ; l'hôte de la CI, lui, exerce le contrat.
- La racine embarque Python 3 : c'est le langage des outils qu'un agent écrit à la demande.
  D'autres interpréteurs s'ajoutent au même endroit, au prix de leur fermeture.
