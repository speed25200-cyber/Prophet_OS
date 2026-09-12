# Matériel cible de la version 1.
#
# La liste est volontairement courte : moins de pilotes, moins de surface, et des machines dont on
# sait qu'elles fonctionnent. L'élargir est un choix explicite, pas un effet de bord.
{ config, lib, pkgs, ... }:

{
  # Processeurs.
  hardware.cpu.amd.updateMicrocode = true;
  hardware.cpu.intel.updateMicrocode = true;

  # Virtualisation : indispensable au niveau 2.
  boot.kernelModules = [ "kvm-amd" "kvm-intel" "vhost_vsock" ];
  virtualisation.libvirtd.enable = false; # Firecracker n'en a pas besoin.

  # Accélération : pilotes ouverts uniquement, pour rester compatibles avec un noyau verrouillé.
  hardware.graphics = {
    enable = true;
    extraPackages = with pkgs; [ mesa amdvlk intel-media-driver vulkan-loader ];
  };

  # Les modules NVIDIA propriétaires ne se chargent pas sous `lockdown=integrity`. Rien n'est
  # déclaré ici : `hardware.nvidia` est un ensemble d'options, et lui affecter un attributaire
  # enveloppé dans `lib.mkDefault` n'est pas une assignation valide — c'était la seconde raison
  # pour laquelle la configuration refusait d'évaluer. Une machine à NVIDIA ajoutera le module
  # ouvert dans sa propre configuration, en connaissance de cause.

  # Stockage : NVMe pour le cache de poids et les snapshots.
  boot.initrd.availableKernelModules = [ "nvme" "xhci_pci" "ahci" "usb_storage" "sd_mod" ];

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
