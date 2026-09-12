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
      # Un écran qui ne peut pas s'allumer ne doit pas réessayer toutes les deux secondes jusqu'à
      # la fin des temps : cinq tentatives en une minute, puis l'unité s'arrête en échec et le
      # repli ci-dessous écrit pourquoi, à l'écran. Marteler remplirait le journal d'une seule
      # erreur répétée, ce qui la rend plus difficile à trouver, pas plus facile.
      startLimitIntervalSec = 60;
      startLimitBurst = 5;
      # `graphical.target` — qui n'est atteinte que grâce à la ligne
      # `systemd.targets.graphical.wantedBy` en bas de ce fichier.
      #
      # Sans elle, cette cible ne serait jamais atteinte : cette machine n'a ni serveur X ni
      # gestionnaire de session, `services.xserver.enable` vaut `false`, et la cible par défaut de
      # systemd est alors `multi-user.target`. Une unité voulue par une cible qu'on n'atteint pas
      # n'est pas lancée — et « pas lancée » ne veut pas dire « en échec » : pas de journal, pas de
      # service de repli, rien à chercher. L'écran d'un PC fraîchement installé montrerait une
      # invite de connexion, et le diagnostic ne commencerait nulle part.
      #
      # Les deux lignes sont à quatre-vingt-dix lignes l'une de l'autre et ne tiennent que
      # ensemble. `image/tests/installe.nix` vérifie sur une vraie machine que la surface est bien
      # lancée : c'est ce qui empêche que l'une parte sans l'autre.
      wantedBy = [ "graphical.target" ];
      after = [ "systemd-user-sessions.service" "prophet-agentd.service" ];
      onFailure = [ "prophet-surface-repli.service" ];
      serviceConfig = {
        User = "surface";
        Group = "surface";

        # Le **septième** terminal virtuel, et non le premier.
        #
        # Elle était sur `/dev/tty1`, avec `TTYVHangup` et `Restart = "always"`. Sur une machine
        # dont le pilote graphique refuse, cela fait cinq tentatives en une minute, et **chaque
        # tentative raccroche le terminal où le propriétaire tape son mot de passe**. Le test du
        # système installé l'a montré en toutes lettres, à vingt-deux millisecondes près :
        #
        #     16:42:00.600  machine: sending keys 'essai-prophet\n'
        #     16:42:00.686  systemd: prophet-surface.service: Scheduled restart job, counter 4
        #     16:42:00.708  unix_chkpwd: password check failed for user (prophet)
        #
        # puis `getty@tty1` redémarré trois fois de suite. Sur un vrai PC, le propriétaire aurait
        # tapé le bon mot de passe et lu « Login incorrect », sans écran graphique pour lui dire
        # pourquoi, sur une machine dont il vient d'effacer le disque. Le pire moment possible.
        #
        # Le septième terminal est celui que les serveurs graphiques occupent depuis toujours, et
        # pour cette raison exacte : NixOS ne fait naître de `getty` que sur tty1 à tty6, donc
        # personne ne se connecte sur tty7. Une surface qui s'y débat ne prend plus en otage la
        # seule porte d'entrée de la machine.
        #
        # Ce que cela change à l'écran : quand la surface démarre, le compositeur bascule sur son
        # terminal et l'occupe — c'est ce que fait un serveur graphique. Quand elle échoue, le
        # propriétaire garde sous les yeux une invite de connexion utilisable, avec au-dessus le
        # message du service de repli qui dit pourquoi l'écran n'est pas ce qu'il devrait être.
        # Une invite de connexion et une explication valent mieux qu'un écran noir, et infiniment
        # mieux qu'une invite qui refuse le bon mot de passe.
        TTYPath = "/dev/tty7";
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
        # Conversation avec le moteur de confiance de la machine uniquement. Les endpoints
        # distants et les redirections sont aussi refusés par le client HTTP.
        RestrictAddressFamilies = [ "AF_UNIX" "AF_INET" "AF_INET6" ];
        IPAddressDeny = "any";
        IPAddressAllow = "localhost";
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

    # Quand la surface ne peut pas démarrer, quelque chose doit apparaître.
    #
    # Sans cela, une machine sans pilote graphique montre un écran noir et rien d'autre : le
    # diagnostic part au journal, que personne ne peut lire puisqu'il n'y a pas d'écran. C'est le
    # pire résultat possible pour un système conçu pour qu'on ait à s'en occuper le moins possible.
    #
    # Ce service ne se déclenche que sur l'échec du premier, et écrit sur le terminal lui-même.
    systemd.services.prophet-surface-repli = {
      description = "Prophet OS — dire pourquoi l'écran est resté noir";
      after = [ "prophet-surface.service" ];
      # Sur `tty1`, et non sur le terminal de la surface. La surface a déménagé sur tty7 ; ce
      # message-ci doit rester là où un humain regarde, c'est-à-dire devant l'invite de connexion.
      # L'écrire sur tty7 reviendrait à expliquer un écran noir sur cet écran noir.
      unitConfig.ConditionPathExists = "/dev/tty1";
      serviceConfig = {
        Type = "oneshot";
        StandardOutput = "tty";
        TTYPath = "/dev/tty1";
        # Un heredoc cité : le texte passe tel quel, sans qu'une apostrophe française ne devienne
        # un problème de guillemets.
        ExecStart = pkgs.writeShellScript "prophet-surface-repli" ''
          cat <<'MESSAGE'

  Prophet OS — la surface graphique n'a pas pu démarrer.

  Le système fonctionne. C'est l'affichage qui manque, pas le reste.

  Pour savoir pourquoi :
      journalctl -u prophet-surface -n 40

  Les deux causes les plus fréquentes :
      aucun pilote Vulkan utilisable    vulkaninfo --summary
      aucune police installée           fc-list | head

  En attendant, tout se fait en ligne de commande :
      prophet status      les services, l'isolation, les limites de la machine
      prophet task ls     ce qui travaille en ce moment

MESSAGE
        '';
      };
    };

    # La cible graphique existe, mais sans gestionnaire d'affichage : personne ne se connecte,
    # il n'y a pas de session à ouvrir.
    #
    # **Cette ligne est ce qui fait démarrer la surface.** `graphical.target` n'est la cible par
    # défaut que sur une machine à gestionnaire de session ; ici la cible par défaut est
    # `multi-user.target`, et sans ce rattachement la surface — voulue par `graphical.target` —
    # ne serait jamais lancée. La retirer ne casserait rien de visible à la construction : l'écran
    # resterait simplement noir, sans une ligne de journal pour le dire.
    systemd.targets.graphical.wantedBy = [ "multi-user.target" ];

    # Le compositeur a besoin du rendu accéléré ; la surface refuse de démarrer sans adaptateur
    # plutôt que d'afficher un écran noir.
    hardware.graphics.enable = lib.mkDefault true;

    # De quoi diagnostiquer un écran qui reste noir, sur une machine qu'on ne peut pas emporter.
    environment.systemPackages = with pkgs; [ vulkan-tools wayland-utils ];
  };
}
