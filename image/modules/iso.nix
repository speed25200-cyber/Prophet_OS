# Le support d'amorçage : ce qu'on grave sur une clé USB pour installer Prophet OS.
#
# Ce n'est pas Prophet OS. C'est un système vivant, minimal, dont le seul métier est de poser
# Prophet OS sur un disque. La distinction compte : le système installé est immuable, chiffré et
# sans compte root utilisable ; le support, lui, doit pouvoir tout faire sur la machine qu'on lui
# confie, et n'existe que le temps de l'installation.
#
# On n'y met donc pas les daemons de Prophet OS. Ils n'y serviraient à rien, et les embarquer
# ferait grossir l'image de ce qu'elle n'utilise pas.
{ config, lib, pkgs, modulesPath, ... }:

{
  imports = [
    "${modulesPath}/installer/cd-dvd/installation-cd-minimal.nix"
    ./installateur.nix
  ];

  # Le nom du fichier produit, et ce qui s'affiche au démarrage.
  isoImage.isoName = lib.mkForce "prophet-os-installeur-${config.system.nixos.label}-x86_64.iso";
  isoImage.volumeID = lib.mkForce "PROPHET_OS";
  # Écrire l'image telle quelle sur une clé USB doit suffire : personne ne devrait avoir à
  # connaître `dd` pour essayer un système.
  isoImage.makeUsbBootable = true;
  isoImage.makeEfiBootable = true;

  # Ce que l'installeur appelle, et de quoi se dépanner quand une machine se tient mal.
  environment.systemPackages = with pkgs; [
    gptfdisk
    cryptsetup
    btrfs-progs
    dosfstools
    e2fsprogs
    pciutils
    usbutils
    efibootmgr
    os-prober
    tmux
    git
    vim
  ];

  # Une console série en plus de l'écran.
  #
  # Sur une vraie machine elle ne sert à rien — personne ne branche un câble série sur un portable.
  # Elle sert à ce qu'un démarrage puisse être *observé* : sans elle, vérifier que cette image
  # démarre exige quelqu'un devant un écran, et c'est précisément ce que personne n'a jamais fait
  # (M9-T6). Avec elle, une machine virtuelle sans écran raconte son démarrage sur sa sortie
  # standard, et l'intégration continue peut lire ce qu'elle raconte.
  #
  # `console=tty0` reste en dernier pour que ce soit l'écran qui reçoive la console principale sur
  # le matériel réel : le noyau retient la dernière déclarée.
  boot.kernelParams = [ "console=ttyS0,115200" "console=tty0" ];

  # Et un noyau bavard.
  #
  # NixOS filtre les messages du noyau au niveau 4 : « Linux version » et tout ce qui raconte le
  # démarrage n'apparaît jamais. Sur un système installé c'est un bon défaut — personne ne veut
  # lire cela tous les matins. Sur un *support d'installation*, c'est l'inverse : quand cette image
  # refuse de démarrer sur une machine inconnue, ces lignes sont le seul diagnostic que son
  # propriétaire aura sous les yeux.
  boot.consoleLogLevel = 7;

  # Le Wi-Fi est souvent la seule connexion disponible sur un portable qu'on vient de vider.
  networking.wireless.enable = lib.mkForce false;
  networking.networkmanager.enable = true;

  # Le message d'accueil. Un système d'installation qui laisse l'utilisateur devant une invite
  # nue sans lui dire quoi taper a déjà échoué à la moitié de son travail.
  services.getty.helpLine = lib.mkForce ''

    Prophet OS — support d'installation

      1. Connectez la machine au réseau (câble, ou « nmtui » pour le Wi-Fi).
      2. Repérez le disque cible :        lsblk
      3. Installez :                      sudo prophet-installer --disque /dev/VOTRE_DISQUE

    L'installeur montre ce qu'il va effacer et vous demande de recopier le nom du disque
    avant d'y toucher. « sudo prophet-installer --aide » détaille les options.

    Rien n'est écrit sur aucun disque tant que vous n'avez pas confirmé.
  '';

  # Une session graphique n'apporterait rien à une installation qui tient en une commande, et
  # ferait passer l'image de moins d'un gigaoctet à plusieurs.
  services.xserver.enable = false;

  # Le compte de ce système vivant n'a pas de mot de passe : il n'existe que le temps de
  # l'installation, sur une machine que son propriétaire a physiquement en main.
  users.users.root.initialHashedPassword = lib.mkForce "";

  system.stateVersion = "25.05";
}
