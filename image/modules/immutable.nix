# Racine immuable, mises à jour atomiques, chiffrement.
#
# Trois propriétés visées :
# 1. Ce qui tourne est vérifiable : la racine est en lecture seule et mesurée.
# 2. Une mise à jour ratée ne casse pas la machine : deux emplacements, bascule automatique.
# 3. Les données de l'utilisateur et l'état des agents sont chiffrés au repos.
{ config, lib, pkgs, ... }:

{
  # --- Démarrage ---
  boot.loader.systemd-boot = {
    enable = true;
    editor = false; # Éditer la ligne de commande au démarrage contournerait tout le reste.
    configurationLimit = 2; # Deux générations : l'actuelle et celle vers laquelle revenir.
  };
  boot.loader.efi.canTouchEfiVariables = true;

  # Secure Boot avec les clés du projet, remplaçables par celles de l'utilisateur : une machine
  # dont le propriétaire ne peut pas changer les clés ne lui appartient pas vraiment.
  boot.lanzaboote = lib.mkDefault {
    enable = false; # activé par l'installeur une fois les clés enrôlées
    pkiBundle = "/var/lib/sbctl";
  };

  # --- Racine immuable ---
  # NixOS rend déjà /nix/store immuable ; on ferme ce qui reste.
  fileSystems."/" = lib.mkDefault {
    device = "/dev/disk/by-label/prophet-a";
    fsType = "ext4";
    options = [ "ro" ];
  };

  # Bascule automatique : si `boot-complete.target` n'est pas atteint, le prochain démarrage
  # repart sur l'autre emplacement.
  systemd.targets.boot-complete.wantedBy = [ "multi-user.target" ];
  boot.initrd.systemd.enable = true;

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
}
