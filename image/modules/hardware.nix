# Matériel cible : un PC ordinaire.
#
# La version 1 visait une liste courte de machines connues. Un système qu'on installe en effaçant
# le disque d'un PC de 2012 comme d'un portable de 2025 doit d'abord s'allumer sur l'un et sur
# l'autre : micrologiciels redistribuables, pilotes de disque et d'USB dans l'initrd, et le bon
# pilote graphique pour les Radeon d'ancienne génération (ADR 0032). Ce que la machine a
# réellement est détecté par l'installeur et écrit dans `image/machine/hardware-configuration.nix`.
{ config, lib, pkgs, ... }:

{
  # Processeurs.
  hardware.cpu.amd.updateMicrocode = true;
  hardware.cpu.intel.updateMicrocode = true;

  # Micrologiciels redistribuables (linux-firmware) : Radeon et amdgpu, Wi-Fi et Bluetooth Intel,
  # Realtek, Atheros, Broadcom… Sans eux, une Radeon HD 7770 n'a pas de KMS, donc pas de session
  # graphique, et un portable n'a pas de Wi-Fi. Le support d'installation les a par son profil
  # « tout matériel » ; le système installé ne les avait pas : il s'allumait sur la clé et
  # restait noir une fois posé sur le disque.
  hardware.enableRedistributableFirmware = true;

  # Virtualisation : indispensable au niveau 2.
  boot.kernelModules = [ "kvm-amd" "kvm-intel" "vhost_vsock" ];
  virtualisation.libvirtd.enable = false; # Firecracker n'en a pas besoin.

  # Accélération : pilotes ouverts uniquement, pour rester compatibles avec un noyau verrouillé.
  #
  # La liste ne contient que ce que Mesa n'apporte pas déjà. `amdvlk` a été retiré de nixpkgs —
  # AMD l'a abandonné au profit de RADV, qui vient avec Mesa et est actif par défaut —, et
  # redéclarer `mesa` ou `vulkan-loader` ici ne fait que répéter ce qui est acquis. La surface
  # graphique de Prophet OS s'appuie sur Vulkan par Mesa ; c'est donc ce chemin-là qui compte.
  hardware.graphics = {
    enable = true;
    extraPackages = with pkgs; [ intel-media-driver ];
  };

  # Radeon GCN 1 et 2 (HD 7000 et 8000, R7 et R9 200 : Southern Islands et Sea Islands) :
  # `amdgpu` plutôt que `radeon`. Le noyau laisse ces puces à `radeon` par défaut, qui n'offre
  # que GL ; `amdgpu` leur donne Vulkan par RADV, dont la surface a besoin. Ces paramètres ne
  # touchent que ces puces : une carte plus récente, ou d'une autre marque, ne les voit pas.
  boot.kernelParams = [
    "radeon.si_support=0"
    "amdgpu.si_support=1"
    "radeon.cik_support=0"
    "amdgpu.cik_support=1"
  ];

  # Les modules NVIDIA propriétaires ne se chargent pas sous `lockdown=integrity`. Rien n'est
  # déclaré ici : `hardware.nvidia` est un ensemble d'options, et lui affecter un attributaire
  # enveloppé dans `lib.mkDefault` n'est pas une assignation valide — c'était la seconde raison
  # pour laquelle la configuration refusait d'évaluer. Une machine à NVIDIA ajoutera le module
  # ouvert dans sa propre configuration, en connaissance de cause.

  # Stockage et périphériques d'amorçage dans l'initrd : ce dont un PC ordinaire a besoin pour
  # trouver sa racine — SATA d'un chipset AMD ou Intel de 2010 comme NVMe, clé USB sur un
  # contrôleur EHCI comme xHCI, lecteur de cartes, et les disques virtuels des essais en machine
  # virtuelle. Ce que `nixos-generate-config` détecte sur la machine s'y ajoute à l'installation.
  boot.initrd.availableKernelModules = [
    "nvme"
    "ahci"
    "ata_piix"
    "sata_nv"
    "sata_sil"
    "sata_sil24"
    "sata_via"
    "pata_amd"
    "pata_atiixp"
    "pata_via"
    "xhci_pci"
    "ehci_pci"
    "ohci_pci"
    "uhci_hcd"
    "usbhid"
    "usb_storage"
    "uas"
    "sd_mod"
    "sr_mod"
    "sdhci_pci"
    "rtsx_pci_sdmmc"
    "virtio_pci"
    "virtio_blk"
    "virtio_scsi"
  ];

  # Réseau : le minimum, puisque les tâches ne l'atteignent que par le proxy.
  networking.networkmanager.enable = true;
  networking.firewall = {
    enable = true;
    # Aucun port en écoute par défaut : une machine d'agents n'est pas un serveur.
    allowedTCPPorts = [ ];
    allowedUDPPorts = [ ];
  };

  # Audio : nécessaire à la parole locale, sans plus.
  services.pipewire = {
    enable = true;
    pulse.enable = true;
  };

  # Confiance matérielle.
  security.tpm2.tctiEnvironment.enable = true;
}
