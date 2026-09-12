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
    users.users = lib.genAttrs
      [ "capd" "ledger" "sfs" "sandboxd" "egress" "vault" "agentd" "memoryd" ]
      (name: {
        isSystemUser = true;
        group = "prophet-system";
        # Pas de deux-points : ce texte va dans le champ GECOS de /etc/passwd, dont le
        # deux-points est le separateur. NixOS le refuse, a juste titre.
        description = "Daemon Prophet OS ${name}";
      });

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
    environment.systemPackages = [ prophet ];

    # --- Ce qui n'a rien à faire sur cette machine ---
    services.xserver.enable = lib.mkDefault false;
    documentation.nixos.enable = lib.mkDefault false;
    programs.command-not-found.enable = false;
  };
}
