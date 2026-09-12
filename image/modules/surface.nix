# La surface d'observation, à l'écran.
#
# Pas de gestionnaire de session, pas de bureau, pas de gestionnaire de fenêtres. Il n'y a rien à
# lancer, rien à disposer, rien à réduire : un seul programme occupe l'écran, et c'est celui qui
# montre ce que font les agents.
#
# `cage` est un compositeur Wayland qui ne sait faire qu'une chose — afficher un client en plein
# écran — et c'est exactement ce qu'on lui demande. Un compositeur complet apporterait des
# fonctions que personne n'utiliserait et une surface d'attaque que personne n'aurait choisie.
{ config, lib, pkgs, ... }:

let
  cfg = config.prophet.surface;
  # Le même paquet que celui des daemons : Nix reconnaît la dérivation et ne la construit qu'une
  # fois, ce qui évite d'inventer une option dont la seule utilité serait de le passer d'un module
  # à l'autre.
  prophet = pkgs.callPackage ../packages/prophet-os.nix { };
in
{
  options.prophet.surface.enable =
    lib.mkEnableOption "la surface d'observation à l'écran" // { default = true; };

  config = lib.mkIf cfg.enable {
    # Une police, et une seule famille, pour que le texte ait la même allure partout. Sans police,
    # la surface se dessinerait sans un mot et paraîtrait fonctionner : elle refuse plutôt de
    # démarrer, et ce refus se lit dans le journal.
    fonts = {
      packages = [ pkgs.inter pkgs.dejavu_fonts ];
      fontconfig.defaultFonts = {
        sansSerif = [ "Inter" "DejaVu Sans" ];
        monospace = [ "DejaVu Sans Mono" ];
      };
    };

    # Le compte qui tient l'écran. Il affiche, il ne décide de rien : ce qu'il transmet quand un
    # humain tranche une approbation passe par `capd`, qui vérifie.
    #
    # `prophet-system` lui est nécessaire pour atteindre les sockets : `/run/prophet` est en 0750
    # pour ce groupe, et les sockets en 0660. Sans lui, la surface ne joindrait aucun daemon et
    # afficherait un champ vide en permanence — ce qui ressemblerait à une machine au repos.
    #
    # Ce que cela donne, dit franchement : au niveau du socket, la surface a le même accès qu'un
    # daemon. Le restreindre demanderait une notion de méthode autorisée par pair que `prophet-ipc`
    # n'a pas encore ; c'est noté dans `docs/STATUS.md`. Le durcissement ci-dessous limite le reste
    # — pas de réseau, pas d'écriture ailleurs, pas d'acquisition de privilège.
    users.users.surface = {
      isSystemUser = true;
      group = "surface";
      description = "Surface d'observation Prophet OS";
      extraGroups = [ "video" "input" "render" "prophet-system" ];
    };
    users.groups.surface = { };

    systemd.services.prophet-surface = {
      description = "Prophet OS — surface d'observation";
      wantedBy = [ "graphical.target" ];
      after = [ "systemd-user-sessions.service" "prophet-agentd.service" ];
      serviceConfig = {
        User = "surface";
        Group = "surface";
        # Le premier terminal virtuel : la surface est ce que la machine montre en s'allumant,
        # pas une application qu'on va chercher.
        TTYPath = "/dev/tty1";
        TTYReset = true;
        TTYVHangup = true;
        StandardInput = "tty-force";
        StandardOutput = "journal";
        StandardError = "journal";
        ExecStart = "${pkgs.cage}/bin/cage -s -- ${prophet}/bin/prophet-surface";
        Restart = "always";
        RestartSec = 2;

        # Durcissement. Un afficheur n'a besoin ni du réseau, ni d'écrire ailleurs que dans son
        # propre état, ni d'acquérir des privilèges.
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictAddressFamilies = [ "AF_UNIX" ];
        RestrictNamespaces = true;
        SystemCallArchitectures = "native";
        StateDirectory = "prophet-surface";
        # `ProtectSystem = "strict"` rend toute la hiérarchie en lecture seule. Se connecter à un
        # socket n'est pas une écriture de système de fichiers, mais plutôt que de parier sur ce
        # détail — et de découvrir au premier démarrage sur une vraie machine que l'écran reste
        # vide — on le déclare.
        ReadWritePaths = [ "/run/prophet" ];
      };
    };

    # La cible graphique existe, mais sans gestionnaire d'affichage : personne ne se connecte,
    # il n'y a pas de session à ouvrir.
    systemd.targets.graphical.wantedBy = [ "multi-user.target" ];

    # Le compositeur a besoin du rendu accéléré ; la surface refuse de démarrer sans adaptateur
    # plutôt que d'afficher un écran noir.
    hardware.graphics.enable = lib.mkDefault true;

    # De quoi diagnostiquer un écran qui reste noir, sur une machine qu'on ne peut pas emporter.
    environment.systemPackages = with pkgs; [ vulkan-tools wayland-utils ];
  };
}
