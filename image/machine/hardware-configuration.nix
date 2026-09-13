# Le matériel de la machine installée.
#
# Dans le dépôt, ce fichier est vide : la configuration de référence ne présume d'aucune machine.
# L'installeur le remplace, dans la copie du dépôt qu'il pose sur le disque, par ce que
# `nixos-generate-config --show-hardware-config --no-filesystems` détecte sur la machine où il
# tourne : modules de l'initrd, microcode, plateforme. Les systèmes de fichiers, eux, viennent
# d'`immutable.nix`, par leurs étiquettes (ADR 0032).
{ ... }: { }
