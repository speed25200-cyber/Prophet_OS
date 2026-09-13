# Le mode d'amorçage de la machine installée.
#
# Vide dans le dépôt : UEFI et systemd-boot par défaut. Sur une machine démarrée sans UEFI,
# l'installeur écrit ici `prophet.boot.firmware = "bios"` et le disque où poser GRUB, par son
# chemin stable sous /dev/disk/by-id (ADR 0032).
{ ... }: { }
