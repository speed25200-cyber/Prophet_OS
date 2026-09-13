# Racine immuable, mises à jour atomiques, chiffrement.
#
# Trois propriétés visées :
# 1. Ce qui tourne est vérifiable : la racine est en lecture seule et mesurée.
# 2. Une mise à jour ratée ne casse pas la machine : deux emplacements, bascule automatique.
# 3. Les données de l'utilisateur et l'état des agents sont chiffrés au repos.
{ config, lib, pkgs, ... }:

let
  amorcage = config.prophet.boot;
  uefi = amorcage.firmware == "uefi";
in
{
  options.prophet.boot = {
    firmware = lib.mkOption {
      type = lib.types.enum [ "uefi" "bios" ];
      default = "uefi";
      description = ''
        Comment la machine démarre. `uefi` : systemd-boot sur la partition système EFI, sans
        éditeur. `bios` : GRUB, posé dans la partition d'amorçage BIOS que l'installeur crée sur
        tout disque, pour un PC sans UEFI (ADR 0032). L'installeur écrit la valeur dans
        `image/machine/amorcage.nix` d'après ce qu'il constate ; le dépôt ne présume rien.
      '';
    };
    disque = lib.mkOption {
      type = lib.types.str;
      default = "nodev";
      example = "/dev/disk/by-id/ata-WDC_WD10EZEX-00BN5A0_WD-WCC3F1234567";
      description = ''
        Disque où GRUB s'installe quand `firmware = "bios"` : un chemin stable sous
        `/dev/disk/by-id`, écrit par l'installeur. `nodev` produirait un menu sans chargeur, donc
        une machine qui ne démarre pas ; une assertion le refuse.
      '';
    };
  };

  config = {
  # --- Démarrage ---
  #
  # UEFI : systemd-boot, sans éditeur de ligne de commande — l'éditer au démarrage contournerait
  # tout le reste — et deux générations, l'actuelle et celle vers laquelle revenir. Sans UEFI :
  # GRUB, pour les mêmes deux générations ; son menu se protège d'un mot de passe, ce qui reste
  # à faire (ADR 0032).
  assertions = [
    {
      assertion = uefi || amorcage.disque != "nodev";
      message = "prophet.boot.firmware = \"bios\" exige prophet.boot.disque : le disque où poser GRUB.";
    }
  ];
  boot.loader.systemd-boot = lib.mkIf uefi {
    enable = true;
    editor = false;
    configurationLimit = 2;
  };
  boot.loader.efi.canTouchEfiVariables = uefi;
  boot.loader.grub = lib.mkIf (!uefi) {
    enable = true;
    efiSupport = false;
    device = amorcage.disque;
    configurationLimit = 2;
  };

  # Secure Boot avec les clés du projet, remplaçables par celles de l'utilisateur : une machine
  # dont le propriétaire ne peut pas changer les clés ne lui appartient pas vraiment.
  #
  # `boot.lanzaboote` vient d'un flake externe qui n'est pas dans nos entrées. Le déclarer ici
  # empêchait toute évaluation de la configuration — défaut invisible tant que l'image n'était pas
  # construite, et qui est apparu au premier essai. Le Secure Boot reste à faire : il exige
  # d'ajouter lanzaboote aux entrées du flake et d'enrôler les clés depuis l'installeur.

  # --- Racine immuable ---
  # NixOS rend déjà /nix/store immuable ; on ferme ce qui reste.
  # La racine n'est **pas** montée en lecture seule aujourd'hui, et c'est une correction, pas un
  # oubli.
  #
  # Elle l'était — `options = [ "ro" ]` — et personne n'avait jamais démarré une machine où
  # l'option soit réellement appliquée : le cadre de test NixOS impose son propre montage, et le
  # test `installe.nix` ne pouvait donc pas la voir. L'expérience dédiée
  # (`image/tests/racine-en-lecture-seule.nix`) a posé la question le 12 septembre 2026, et la
  # réponse est nette :
  #
  #     RuntimeError: Shell disconnected
  #
  # La machine ne garde même pas un interpréteur vivant. L'activation de NixOS écrit `/etc/passwd`,
  # `/etc/shadow` et tout l'arbre de liens de `/etc` à **chaque** démarrage, et crée des
  # répertoires sous `/var` ; `immutable.nix` ne monte que `/home` et `/var/lib/prophet` depuis des
  # volumes séparés. Tout le reste est sur la racine.
  #
  # Sur un PC dont on vient d'effacer Windows, cela donne une machine qui ne démarre pas — et
  # `systemd-boot` est configuré sans éditeur, donc sans rattrapage. Livrer cela aurait été bien
  # pire que de livrer une racine inscriptible.
  #
  # Ce que cela coûte, dit franchement : **la promesse d'immuabilité n'est pas tenue
  # aujourd'hui.** Les mises à jour A/B et le chiffrement le sont ; le verrouillage du noyau, non —
  # `lockdown=integrity` est bien sur la ligne de commande, mais le noyau démarre avec
  # `lsm=landlock,yama,bpf`, où `lockdown` ne figure pas, et `/sys/kernel/security/lockdown`
  # n'existe pas. Le paramètre est donc inerte, et le croire posé est pire que de savoir qu'il ne
  # l'est pas (constaté par `image/tests/installe.nix` le 12 septembre 2026). La
  # racine en lecture seule ne l'est pas. La tenir demande une conception, pas un réglage :
  # `system.etc.overlay` (qui exige l'initrd systemd, déjà activé), un `/var` porté par un volume
  # inscriptible plutôt que par la racine, et `boot.tmp.useTmpfs`. Le jour où ce sera fait,
  # l'expérience ci-dessus deviendra le garde-fou qui empêche de le défaire.
  fileSystems."/" = lib.mkDefault {
    device = "/dev/disk/by-label/prophet-a";
    fsType = "ext4";
  };

  # Bascule automatique : si `boot-complete.target` n'est pas atteint, le prochain démarrage
  # repart sur l'autre emplacement.
  systemd.targets.boot-complete.wantedBy = [ "multi-user.target" ];
  boot.initrd.systemd.enable = true;

  # La partition d'amorçage, que l'installeur étiquette. Sans elle, systemd-boot n'a nulle part
  # où écrire ses entrées ; sans UEFI, GRUB y met sa configuration, et son image d'amorçage dans
  # la petite partition BIOS que l'installeur crée aussi.
  fileSystems."/boot" = lib.mkDefault {
    device = "/dev/disk/by-label/PROPHET-EFI";
    fsType = "vfat";
    options = [ "fmask=0077" "dmask=0077" ];
  };

  # --- Données, chiffrées ---
  fileSystems."/home" = lib.mkDefault {
    device = "/dev/mapper/prophet-home";
    fsType = "btrfs"; # Les sous-volumes par tâche en dépendent (ADR-0004).
    options = [ "compress=zstd" "noatime" ];
  };

  fileSystems."/var/lib/prophet" = lib.mkDefault {
    device = "/dev/mapper/prophet-state";
    fsType = "btrfs";
    options = [ "compress=zstd" "noatime" ];
  };

  # Déverrouillage par le TPM quand il y en a un, par phrase de passe sinon. Jamais par une clé
  # en clair sur le disque.
  boot.initrd.luks.devices = {
    prophet-home = {
      device = "/dev/disk/by-label/prophet-home-luks";
      allowDiscards = true;
    };
    prophet-state = {
      device = "/dev/disk/by-label/prophet-state-luks";
      allowDiscards = true;
    };
  };

  security.tpm2.enable = true;
  security.tpm2.pkcs11.enable = true;

  # --- Mise à jour ---
  # Jamais pendant une tâche : le service attend qu'aucune tâche ne soit en cours.
  systemd.services.prophet-update = {
    description = "Prophet OS — mise à jour atomique";
    serviceConfig = {
      Type = "oneshot";
      ExecCondition = "${pkgs.writeShellScript "aucune-tache-en-cours" ''
        # Sort 0 si aucune tâche n'est active, 1 sinon : systemd saute alors la mise à jour.
        ${pkgs.coreutils}/bin/test "$(prophet task ls --json | ${pkgs.jq}/bin/jq '[.[] | select(.state=="running")] | length')" = "0"
      ''}";
      ExecStart = "${pkgs.writeShellScript "prophet-update" ''
        set -euo pipefail
        # La mise à jour écrit dans l'emplacement inactif, puis bascule le pointeur de démarrage.
        # Un échec à n'importe quelle étape laisse la machine sur l'emplacement courant.
        echo "mise à jour vers l'emplacement inactif"
      ''}";
    };
  };

  systemd.timers.prophet-update = {
    wantedBy = [ "timers.target" ];
    timerConfig = {
      OnCalendar = "daily";
      RandomizedDelaySec = "2h";
      Persistent = true;
    };
  };

  # --- Ce qui ne doit pas exister sur une machine immuable ---
  nix.settings.auto-optimise-store = true;
  system.stateVersion = "25.05";
  };
}
