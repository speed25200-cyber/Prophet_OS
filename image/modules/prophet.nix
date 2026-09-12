# Services système de Prophet OS.
#
# Chaque daemon a son utilisateur, son répertoire d'état et son socket. Les droits sont posés ici
# une fois pour toutes : un daemon ne peut pas lire ce qui appartient à un autre, et les fichiers
# d'identifiants des clients d'éditeurs ne sont lisibles que par le pilote qui les monte.
{ config, lib, pkgs, ... }:

let
  cfg = config.prophet;
  prophet = pkgs.callPackage ../packages/prophet-os.nix { };

  # Un service de daemon Prophet : durci par défaut, élargi seulement là où c'est nécessaire.
  daemon = { name, user, description, extra ? { } }: lib.mkMerge [
    {
      inherit description;
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];
      serviceConfig = {
        ExecStart = "${prophet}/bin/prophet-${name}";
        User = user;
        Group = "prophet-system";
        Restart = "on-failure";
        RestartSec = "2s";
        StateDirectory = "prophet/${name}";
        StateDirectoryMode = "0700";
        RuntimeDirectory = "prophet";
        # `0770`, et non `0750`.
        #
        # Les sept services partagent ce répertoire, et systemd le crée au nom du premier qui
        # démarre. En `0750`, le groupe n'a que lecture et traversée : seul ce premier service peut
        # y créer son socket, et les six autres échouent sur un `Permission denied` en s'ouvrant.
        # C'est ce qui s'est produit au premier démarrage sous systemd — `agentd` a gagné la
        # course, `capd` et `ledger` ont bouclé jusqu'à la limite de redémarrages.
        #
        # Le droit d'écriture est donné au groupe, pas au monde. Cela n'élargit rien : appartenir à
        # `prophet-system` donne déjà accès à toutes les méthodes système de tous les daemons, donc
        # pouvoir poser un fichier à côté de leurs sockets n'ajoute aucun pouvoir. Ce qui compte
        # est que les autres n'entrent pas, et `0770` le tient aussi bien que `0750`.
        RuntimeDirectoryMode = "0770";
        # Sans cette ligne, systemd supprime le répertoire quand l'un d'eux s'arrête — et emporte
        # les six autres sockets avec lui. Un `systemctl restart prophet-memoryd` couperait tout
        # le reste, ce qui est une façon remarquable de rendre un système fragile sans qu'aucun
        # test de daemon ne s'en aperçoive.
        RuntimeDirectoryPreserve = true;

        # Durcissement : ce qu'un daemon n'a pas besoin de faire, il ne peut pas le faire.
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectKernelLogs = true;
        ProtectControlGroups = true;
        ProtectClock = true;
        ProtectHostname = true;
        ProtectProc = "invisible";
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [ "@system-service" "~@privileged" "~@resources" ];
        CapabilityBoundingSet = "";
      };
    }
    extra
  ];
in
{
  options.prophet = {
    enable = lib.mkEnableOption "les services de Prophet OS";

    user = lib.mkOption {
      type = lib.types.str;
      default = "prophet";
      description = "Utilisateur humain pour le compte duquel les agents travaillent.";
    };

    motDePasseHache = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = "/etc/prophet/motdepasse";
      description = ''
        Fichier contenant le mot de passe **haché** du compte humain, posé par l'installeur.

        Jamais le mot de passe lui-même, et jamais dans le dépôt : ce dépôt est public, et un
        mot de passe écrit dans une configuration versionnée est un mot de passe connu. Le
        fichier est écrit à l'installation, en `0600`, et n'est lu que par l'activation de
        NixOS.

        `null` laisse le compte sans mot de passe défini par ce module ; il faut alors en
        fournir un autrement, sans quoi personne ne peut ouvrir de session. Les tests s'en
        servent.
      '';
    };

    maxSandboxLevel = lib.mkOption {
      type = lib.types.ints.between 0 2;
      default = 2;
      description = ''
        Niveau d'isolation maximal autorisé. Le réduire n'assouplit rien : il empêche seulement
        de démarrer des tâches qui exigeraient plus que ce que la machine peut garantir.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    # --- Utilisateurs et groupes ---
    users.groups.prophet-system = { };

    # Le compte humain est déclaré plus bas, avec les daemons. Sans lui, la machine installée n'a
    # **aucune** session ouvrable : `root` n'a pas de mot de passe (`nixos-install
    # --no-root-password`), et aucun autre compte n'existait. On installait donc un système sur
    # lequel personne ne pouvait se connecter. Aucun test ne pouvait le voir : seul le support
    # d'amorçage avait jamais été démarré, et lui ouvre une session automatiquement.

    # Les comptes viennent de la configuration, pas d'un `useradd` local : sur une racine en
    # lecture seule, une modification faite à la main serait perdue au prochain démarrage sans
    # que rien ne le dise.
    users.mutableUsers = lib.mkDefault false;

    users.users = lib.genAttrs
      [ "capd" "ledger" "sfs" "sandboxd" "egress" "vault" "agentd" "memoryd" ]
      (name: {
        isSystemUser = true;
        group = "prophet-system";
        # Pas de deux-points : ce texte va dans le champ GECOS de /etc/passwd, dont le
        # deux-points est le separateur. NixOS le refuse, a juste titre.
        description = "Daemon Prophet OS ${name}";
      })
    // {
      # Le compte humain, dans la même définition que les daemons : deux affectations de
      # `users.users` dans le même ensemble seraient une redéfinition d'attribut, que Nix refuse.
      #
      # `wheel` lui donne `sudo` avec son propre mot de passe ; `root` reste verrouillé. C'est
      # l'arrangement habituel, et le bon : un compte administrateur sans mot de passe ne se
      # connecte pas, il ne s'ouvre pas non plus par accident.
      ${cfg.user} = {
        isNormalUser = true;
        description = "Propriétaire de cette machine";
        home = "/home/${cfg.user}";
        # `networkmanager` n'existe que si NetworkManager est activé — il l'est par
        # `hardware.nix`, pas par ce module. L'écrire en dur ferait échouer toute configuration
        # qui n'importe que celui-ci, à commencer par le test des services.
        extraGroups = [ "wheel" "prophet-system" "video" "input" "render" ]
          ++ lib.optional config.networking.networkmanager.enable "networkmanager";
        hashedPasswordFile = lib.mkIf (cfg.motDePasseHache != null) cfg.motDePasseHache;
      };
    };

    # --- Services ---
    systemd.services = {
      # `capd` d'abord : rien n'obtient de droit avant lui.
      prophet-capd = daemon {
        name = "capd";
        user = "capd";
        description = "Prophet OS — broker de capacités";
        extra.before = [ "prophet-agentd.service" "prophet-sandboxd.service" ];
      };

      prophet-ledger = daemon {
        name = "ledger";
        user = "ledger";
        description = "Prophet OS — journal d'audit";
        # Le journal est en ajout seul : le daemon n'a pas besoin de supprimer.
        extra.serviceConfig.ReadWritePaths = [ "/var/lib/prophet/ledger" ];
      };

      prophet-vault = daemon {
        name = "vault";
        user = "vault";
        description = "Prophet OS — coffre à secrets";
        extra.serviceConfig = {
          # Le coffre est le seul service autorisé à parler au TPM.
          DeviceAllow = [ "/dev/tpmrm0 rw" ];
          PrivateDevices = lib.mkForce false;
        };
      };

      prophet-egress = daemon {
        name = "egress";
        user = "egress";
        description = "Prophet OS — proxy de sortie";
        extra.serviceConfig = {
          # Seul service à avoir une vraie pile réseau : c'est tout l'intérêt.
          PrivateNetwork = false;
          RestrictAddressFamilies = [ "AF_UNIX" "AF_INET" "AF_INET6" ];
        };
      };

      prophet-sandboxd = daemon {
        name = "sandboxd";
        user = "root";
        description = "Prophet OS — gestionnaire de sandbox";
        extra.serviceConfig = {
          # Projeter les identifiants d'un enfant exige CAP_SETUID dans l'espace parent
          # (voir ADR-0005) ; créer des microVM exige l'accès à KVM.
          CapabilityBoundingSet = lib.mkForce [ "CAP_SETUID" "CAP_SETGID" "CAP_SYS_ADMIN" ];
          AmbientCapabilities = [ "CAP_SETUID" "CAP_SETGID" "CAP_SYS_ADMIN" ];
          NoNewPrivileges = lib.mkForce false;
          RestrictNamespaces = lib.mkForce false;

          # Le filtre d'appels système des six autres daemons ne convient pas à celui-ci.
          #
          # `@system-service` ne contient pas `@mount`, et `~@privileged` retire `setuid`,
          # `setgid`, `setgroups` et `pivot_root`. Or c'est exactement le travail de ce
          # service : créer des espaces de noms, y monter une racine minimale, y projeter des
          # identifiants. Lui accorder `CAP_SETUID` d'une main et lui interdire `setuid` de
          # l'autre est une contradiction qui ne se voit qu'à l'exécution — et c'est ce que le
          # test des services a fini par montrer, en toutes lettres :
          #
          #     confinement impossible : écriture de uid_map : Operation not permitted
          #
          # Ce que cela n'élargit **pas** : la sandbox elle-même. Le filtre qu'une tâche subit
          # est posé par `sandboxd` dans son enfant (`crates/sandboxd/src/confine.rs`), après
          # le confinement, et il est bien plus étroit que celui-ci. Le filtre du service borne
          # le gestionnaire ; celui de la sandbox borne la tâche. Les confondre revenait à
          # borner le gardien avec les règles du prisonnier.
          #
          # Ce qui est ajouté, et pourquoi chaque morceau :
          #
          # - `@mount` : monter la racine minimale de la sandbox, puis `pivot_root`. Absent de
          #   `@system-service`, donc à demander explicitement.
          # - `@privileged` : `setuid`, `setgid`, `setgroups` pour projeter les identifiants, et
          #   `capset` pour retirer à l'enfant ce qu'il ne doit pas garder.
          #
          # Ce qui en est aussitôt retiré est plus long que ce qui est ajouté, et c'est voulu.
          # `@privileged` est un fourre-tout : il contient de quoi charger un module, changer
          # l'heure, arrêter la machine ou lire la mémoire d'un autre processus. Rien de cela
          # n'est le travail de ce service, et un service qui peut charger un module noyau rend
          # tout le reste décoratif.
          SystemCallFilter = lib.mkForce [
            "@system-service"
            "@mount"
            "@privileged"
            "~@resources" # ni priorités, ni limites, ni cgroups du système
            "~@module" # charger un module noyau rendrait tout le reste décoratif
            "~@debug" # ptrace et lecture de la mémoire d'autrui
            "~@clock" # `ProtectClock` le dit déjà ; le filtre le redit
            "~@reboot"
            "~@swap"
            "~@obsolete"
          ];
          # `RestrictSUIDSGID` implique `NoNewPrivileges`, que ce service désactive juste
          # au-dessus. Les laisser tous deux revient à demander une chose et son contraire.
          RestrictSUIDSGID = lib.mkForce false;
          # Le gestionnaire lit et écrit `/proc/<pid>/uid_map` de ses propres enfants. Ils lui
          # appartiennent, donc `invisible` devrait suffire ; mais ce service est le seul dont
          # le travail passe par `/proc` d'un autre processus, et une hypothèse de moins vaut
          # mieux ici qu'un durcissement qu'on ne sait pas expliquer.
          ProtectProc = lib.mkForce "default";

          DeviceAllow = [ "/dev/kvm rw" ];
          PrivateDevices = lib.mkForce false;
        };
      };

      prophet-memoryd = daemon {
        name = "memoryd";
        user = "memoryd";
        description = "Prophet OS — mémoire";
      };

      prophet-agentd = daemon {
        name = "agentd";
        user = "agentd";
        description = "Prophet OS — runtime d'agents";
        extra = {
          after = [ "prophet-capd.service" "prophet-ledger.service" "prophet-sandboxd.service" ];
          requires = [ "prophet-capd.service" "prophet-ledger.service" ];
          serviceConfig.ReadWritePaths = [ "/home/${cfg.user}" "/var/lib/prophet" ];
        };
      };
    };

    # --- Répertoires d'état ---
    systemd.tmpfiles.rules = [
      "d /var/lib/prophet 0750 root prophet-system -"
      "d /var/lib/prophet/capd 0700 capd prophet-system -"
      "d /var/lib/prophet/ledger 0700 ledger prophet-system -"
      "d /var/lib/prophet/vault 0700 vault prophet-system -"
      "d /var/lib/prophet/models 0755 root prophet-system -"
      # Les sessions d'abonnement des clients d'éditeurs : lisibles par le seul runtime qui les
      # monte dans la sandbox du client, jamais par les outils ni par un agent.
      "d /var/lib/prophet/providers 0700 agentd prophet-system -"
      "d /etc/prophet 0755 root root -"
      "d /etc/prophet/policies 0755 root root -"
      "d /etc/prophet/mcp 0755 root root -"
    ];

    environment.etc."prophet/policies/default.cedar".source = ../../policies/default.cedar;

    # --- Noyau ---
    boot.kernel.sysctl = {
      # Les espaces de noms utilisateur non privilégiés sont la base du niveau 0.
      "user.max_user_namespaces" = 15000;
      # Rien n'a besoin de lire la mémoire du noyau.
      "kernel.kptr_restrict" = 2;
      "kernel.dmesg_restrict" = 1;
      # Pas de ptrace entre processus non apparentés.
      "kernel.yama.ptrace_scope" = 2;
      # Le réseau des tâches passe par le proxy, pas par des redirections.
      "net.ipv4.conf.all.send_redirects" = 0;
      "net.ipv4.conf.all.accept_redirects" = 0;
    };

    boot.kernelParams = [
      # Verrouillage du noyau : ni chargement de module non signé, ni écriture en mémoire noyau.
      "lockdown=integrity"
      "module.sig_enforce=1"
      # Atténuations activées : une machine qui exécute du code d'origine inconnue ne peut pas se
      # permettre de les désactiver pour gagner quelques pourcents.
      "mitigations=auto"
      "init_on_alloc=1"
      "init_on_free=1"
    ];

    # Les cgroups v2 sont le seul mode que systemd accepte désormais, et `sandboxd` en dépend
    # pour ses quotas. L'option qui les demandait a disparu parce que ce qu'elle demandait est
    # devenu le comportement par défaut ; la déclarer fait maintenant échouer l'évaluation.

    # --- Paquets ---
    # `prophet`, et les clients officiels qu'il pilote.
    #
    # Sans eux, `prophet provider login claude-code` dit « lancez `claude login` » sur une machine
    # où `claude` n'existe pas. C'est le genre de découverte qu'on fait après avoir formaté son
    # disque, et le seul moment où il est trop tard.
    #
    # Ils sont pris **tels quels** dans nixpkgs : l'invariant est qu'un client officiel tourne sans
    # modification, et qu'aucun de ses fichiers d'identifiants n'est lu, copié ni réutilisé par
    # l'OS. Prophet OS se contente de monter le répertoire de session dans la sandbox du client.
    #
    # `lib.optional (pkgs ? …)` plutôt qu'une référence directe : le jour où l'un d'eux change de
    # nom ou disparaît de nixpkgs, l'image se construit quand même, sans ce client. Une image qui
    # refuse de se construire parce qu'un client a été renommé en amont serait une dépendance plus
    # dure que ce que ce système veut assumer — et `prophet provider ls` dit, sur la machine, ce
    # qui est réellement là.
    # Codex CLI n'y est pas, et c'est délibéré. `pkgs.codex` est un nom générique : rien ne
    # garantit, depuis ici, qu'il désigne le client d'OpenAI plutôt qu'un homonyme. `lib.optional
    # (pkgs ? …)` protège d'un attribut **absent**, pas d'un attribut **qui n'est pas le bon** —
    # et livrer un binaire étranger sous un nom auquel l'OS fait confiance serait pire que de ne
    # rien livrer. `claude-code` et `gemini-cli` sont assez spécifiques pour qu'une collision soit
    # invraisemblable. Le jour où quelqu'un peut vérifier le nom de l'attribut de Codex sur une
    # machine avec Nix, il l'ajoutera ici en une ligne.
    environment.systemPackages = [ prophet ]
      ++ lib.optional (pkgs ? claude-code) pkgs.claude-code
      ++ lib.optional (pkgs ? gemini-cli) pkgs.gemini-cli;

    # --- Ce qui n'a rien à faire sur cette machine ---
    services.xserver.enable = lib.mkDefault false;
    documentation.nixos.enable = lib.mkDefault false;
    programs.command-not-found.enable = false;
  };
}
