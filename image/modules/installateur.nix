# L'installeur, et ce dont il a besoin pour travailler.
#
# Le script lui-même est écrit en bash, dans `image/installateur/`, plutôt qu'en chaîne Nix. Ce
# n'est pas un détail de goût : un script dans un fichier se lit, se vérifie par `bash -n`, et
# s'exerce en intégration continue contre un disque en boucle. Enfermé dans une chaîne Nix, il ne
# se vérifierait qu'en construisant l'image.
#
# L'enveloppe fixe son PATH. Un installeur qui dépend du PATH de l'utilisateur peut trouver un
# `mkfs` différent de celui qu'on a testé, ce qui est la dernière chose qu'on veut d'un programme
# qui formate des disques.
{ config, lib, pkgs, ... }:

let
  # Exactement ce que le script appelle, et rien d'autre.
  outils = with pkgs; [
    gptfdisk # sgdisk
    util-linux # wipefs, lsblk, blockdev, losetup
    parted # partprobe
    systemd # udevadm
    cryptsetup
    dosfstools # mkfs.fat
    e2fsprogs # mkfs.ext4
    btrfs-progs # mkfs.btrfs
    coreutils
    gnugrep
    gnused
    iputils # ping
    nixos-install-tools # nixos-install, nixos-generate-config
  ];

  prophet-installer = pkgs.stdenv.mkDerivation {
    pname = "prophet-installer";
    version = "0.1.0";
    src = ../installateur/prophet-installer.sh;
    dontUnpack = true;
    nativeBuildInputs = [ pkgs.makeWrapper ];
    installPhase = ''
      install -Dm755 $src $out/bin/prophet-installer
      wrapProgram $out/bin/prophet-installer \
        --prefix PATH : ${lib.makeBinPath outils}
    '';
    meta = {
      description = "Installe Prophet OS sur un disque, depuis le support d'amorçage";
      mainProgram = "prophet-installer";
    };
  };
in
{
  options.prophet.installateur.source = lib.mkOption {
    type = lib.types.nullOr lib.types.path;
    default = null;
    description = ''
      Dépôt Prophet OS que l'installeur posera sur la machine. Le flake y met sa propre source,
      de sorte que le support d'amorçage porte le système qu'il installe : une installation ne
      dépend alors pas de ce qui se trouve sur GitHub au moment où on la lance.
    '';
  };

  config = {
    environment.systemPackages = [ prophet-installer ];

    environment.etc."prophet/source" = lib.mkIf (config.prophet.installateur.source != null) {
      source = config.prophet.installateur.source;
    };

    environment.sessionVariables.PROPHET_SOURCE =
      lib.mkIf (config.prophet.installateur.source != null) "/etc/prophet/source";
  };
}
