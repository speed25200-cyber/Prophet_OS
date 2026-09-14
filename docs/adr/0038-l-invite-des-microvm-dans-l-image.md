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
`.prophet/exec.sh` qui porte le programme, ses arguments et son environnement : c'est le
contrat que sandboxd doit remplir côté hôte, et cela reste à faire.

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
41 Mo. Un premier essai laissait le noyau lancer l'init de busybox (`/sbin/init`), qui
cherchait `/etc/init.d/rcS` et attendait une console : `/sbin/init` est désormais le script
lui-même ; un second laissait l'invité en « System halted » sur `poweroff` : c'est `reboot -f`
qui, sous `reboot=k`, fait sortir le moniteur.

## Conséquences

- Le niveau 2 devient possible sur toute machine installée qui a KVM : Firecracker et l'invité
  sont là. Sans KVM, rien ne change : sandboxd le dit, et le niveau 2 reste refusé.
- Le contrat d'exécution côté hôte — écrire l'espace de travail sur un disque, le donner à
  l'invité, lire la console jusqu'à « PROPHET_INVITE_FIN », rapatrier les fichiers — n'est pas
  encore dans sandboxd : aujourd'hui le niveau 2 démarre l'invité et lit sa console, sans lui
  confier d'espace de travail. C'est la prochaine marche, et l'essai sur l'hôte de la CI la
  jugera.
- La racine embarque Python 3 : c'est le langage des outils qu'un agent écrit à la demande.
  D'autres interpréteurs s'ajoutent au même endroit, au prix de leur fermeture.
