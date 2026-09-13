# ADR 0032 — Un PC ordinaire : micrologiciels, détection par machine, amorçage sans UEFI

Date : 2026-09-13. Statut : accepté, première livraison.

## Contexte

Le propriétaire du projet veut installer Prophet OS sur un PC de 2012 : processeur AMD FX-8350,
Radeon HD 7770, un micrologiciel qui n'est peut-être qu'un BIOS. L'audit du 13 septembre a
trouvé trois écarts entre « l'image démarre en CI » et « l'image s'installe sur cette machine » :

1. Le système installé n'avait **aucun micrologiciel redistribuable**. Le support d'amorçage en a
   (profil « tout matériel » de NixOS), donc il s'allumait sur la clé ; une fois posé sur le
   disque, une Radeon restait sans KMS et un Wi-Fi sans pilote : écran noir après l'installation,
   le pire moment pour le découvrir.
2. L'installeur lançait `nixos-generate-config`, puis **ignorait son résultat** :
   `nixos-install --flake` construit `nixosConfigurations.prophet` depuis la copie du dépôt, qui ne
   savait rien de la machine.
3. L'installeur **refusait toute machine sans UEFI**, alors que l'image, elle, est hybride et y
   démarre.

## Décision

1. **Micrologiciels et modules.** `hardware.enableRedistributableFirmware = true` dans le
   système installé ; l'initrd contient les pilotes SATA, PATA, USB, NVMe, cartes SD et virtio
   qu'un PC ordinaire peut avoir pour trouver sa racine. Les Radeon GCN 1 et 2 passent sous
   `amdgpu` (`radeon.si_support=0 amdgpu.si_support=1`, idem `cik`) : c'est le pilote qui leur
   donne Vulkan par RADV, dont la surface a besoin ; `radeon` n'offrait que GL.
2. **Deux fichiers par machine**, `image/machine/hardware-configuration.nix` et
   `image/machine/amorcage.nix`, importés par le flake et vides dans le dépôt. L'installeur les
   écrit dans la copie du dépôt qu'il pose sur le disque : le premier par
   `nixos-generate-config --show-hardware-config --no-filesystems` (les systèmes de fichiers
   restent ceux d'`immutable.nix`, par étiquettes), le second seulement sans UEFI. Le dépôt ne
   présume donc d'aucune machine, et la machine installée est exactement ce qui a été détecté.
3. **Amorçage sans UEFI.** `prophet.boot.firmware` vaut `uefi` (systemd-boot, comme avant) ou
   `bios` (GRUB, `efiSupport = false`, deux générations, posé sur `prophet.boot.disque`, un chemin
   stable de `/dev/disk/by-id`). L'installeur crée sur tout disque une sixième partition d'un
   mébioctet de type `ef02`, à la fin : GRUB y met son image ; sous UEFI elle est inerte. La
   disposition est donc la même quel que soit le micrologiciel. Une assertion refuse `bios`
   sans disque.
4. **Trois preuves en intégration continue** : l'ISO démarre aussi sous SeaBIOS (même bannière,
   sans OVMF) ; la configuration installée démarre sous SeaBIOS avec GRUB et ses sept services
   (`image/tests/installe-bios.nix`) ; les deux fichiers de machine, générés sur le coureur et
   posés en mode BIOS, s'évaluent avec la configuration.

## Conséquences

- Le système installé grossit de linux-firmware (quelques centaines de mégaoctets) ; c'est le
  prix d'un système qui s'allume sur du matériel qu'on n'a pas choisi.
- GRUB sans UEFI n'a pas l'équivalent de `editor = false` de systemd-boot : son menu s'édite au
  clavier. Un mot de passe GRUB (`boot.loader.grub.users`) est la suite ; Secure Boot ne
  s'applique pas à ce chemin.
- Une machine qui a l'UEFI mais a démarré la clé en mode « CSM » reçoit GRUB. C'est cohérent
  avec ce qu'elle a fait, et cela se change en réamorçant la clé en UEFI.
- Ce que la CI prouve reste une machine virtuelle : pilotes virtio, SeaBIOS et OVMF. La HD 7770
  sous `amdgpu`, le Wi-Fi d'un portable donné, un BIOS de 2012 avec ses caprices : c'est la
  matrice matérielle, à établir machine par machine, et le guide d'installation le dit.
