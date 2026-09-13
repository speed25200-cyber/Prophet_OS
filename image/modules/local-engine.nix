# Un moteur CPU local et un premier contexte de mission explicitement partagé.
{ config, lib, pkgs, utils, ... }:
let
  cfg = config.prophet.localEngine;
  home = config.users.users.${config.prophet.user}.home;
  owner = config.prophet.user;
  engine = pkgs.callPackage ../packages/llama-cpp.nix { };
  endpoint = "http://127.0.0.1:${toString cfg.port}/v1";
  navigateur = config.prophet.navigateur != null;
  profiles = pkgs.writeText "prophet-mission-profiles.json" (builtins.toJSON [
    {
      id = "documents";
      name = "Documents Prophet";
      description = "Préparer des fichiers dans ~/Documents/Prophet. Les originaux restent à examiner avant application.";
      scopes = [ "~/Documents/Prophet" ];
      manifest = {
        agent = {
          id = "org.prophet.documents";
          version = "1.0.0";
          name = "Documents Prophet";
          publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        };
        model.preferred = [ "local:${cfg.model}" ];
        sandbox.min_level = 0;
        capabilities.max = {
          "fs.read" = [ "~/Documents/Prophet/**" ];
          "fs.write" = [ "~/Documents/Prophet/**" ];
          "tool.call" = [ "fs.read" "doc.read" "fs.write" ];
        };
        budget.default = { tokens = 20000; wall_time = "90s"; approvals = 3; };
      };
    }
    # Le web, par egress : chaque hôte ouvert est tranché par capd et inscrit au journal, les
    # lectures sont automatiques, un envoi de formulaire attend l'accord humain. Le navigateur
    # piloté n'apparaît que si le système en configure un (`prophet.navigateur`) ; sinon le
    # contexte se limite à `http.fetch`.
    {
      id = "web";
      name = "Recherche sur le web";
      description = "Consulter le web et déposer des notes dans ~/Documents/Prophet. Chaque sortie passe par egress et figure au journal ; un envoi de formulaire attend votre accord.";
      scopes = [ "~/Documents/Prophet" ];
      manifest = {
        agent = {
          id = "org.prophet.web";
          version = "1.0.0";
          name = "Recherche web Prophet";
          publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        };
        model.preferred = [ "local:${cfg.model}" ];
        sandbox.min_level = 0;
        capabilities.max = {
          "fs.read" = [ "~/Documents/Prophet/**" ];
          "fs.write" = [ "~/Documents/Prophet/**" ];
          "net.egress" = [ "*" ];
          # Le contexte web peut confier la rédaction au contexte documents : deux agents, deux
          # modèles au besoin, sous un jeton délégué par capd (ADR 0029).
          "task.spawn" = [ "documents" ];
          "tool.call" = [ "fs.read" "doc.read" "fs.write" "http.fetch" "task.delegate" ]
            ++ lib.optionals navigateur [ "web.open" "web.tree" "web.act" ];
        } // lib.optionalAttrs navigateur {
          "ui.read" = [ "browser" ];
          "ui.act" = [ "browser" ];
        };
        budget.default = { tokens = 30000; wall_time = "180s"; approvals = 3; };
      };
    }
    # Le bureau, par l'accessibilité : l'éditeur de texte de la session est lu et piloté par
    # son arbre AT-SPI, sans capture d'écran ; l'application est nommée, jamais l'écran.
    {
      id = "bureau";
      name = "Éditeur du bureau";
      description = "Travailler dans les applications ouvertes sur le bureau (éditeur, LibreOffice, GIMP, Inkscape, FreeCAD, lecteur PDF) et déposer des fichiers dans ~/Documents/Prophet. L'agent lit et manipule l'application par son arbre d'accessibilité, sans capture d'écran ; chaque action figure au journal.";
      scopes = [ "~/Documents/Prophet" ];
      manifest = {
        agent = {
          id = "org.prophet.bureau";
          version = "1.0.0";
          name = "Éditeur du bureau";
          publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        };
        model.preferred = [ "local:${cfg.model}" ];
        sandbox.min_level = 0;
        capabilities.max = {
          "fs.read" = [ "~/Documents/Prophet/**" ];
          "fs.write" = [ "~/Documents/Prophet/**" ];
          # L'éditeur, et la suite de l'humain quand elle publie une accessibilité : LibreOffice
          # (« soffice » sur le bus), GIMP, Inkscape, FreeCAD, le lecteur PDF ; jamais l'écran.
          "ui.read" = [ "mousepad" "soffice" "gimp" "inkscape" "freecad" "evince" ];
          "ui.act" = [ "mousepad" "soffice" "gimp" "inkscape" "freecad" "evince" ];
          "tool.call" = [ "fs.read" "doc.read" "fs.write" "ui.apps" "ui.tree" "ui.act" ];
        };
        budget.default = { tokens = 30000; wall_time = "180s"; approvals = 3; };
      };
    }
    # L'atelier : les clients officiels de l'humain comme rôles du relais (ADR 0034, 0035).
    # Claude Code réfléchit, Codex code, le modèle local exécute ; chacun n'est proposé que si
    # le lanceur de la session le dit connecté, sinon le rôle retombe sur le modèle local. Les
    # clients rejoignent leur sous-mission par une séance d'outils, sous un jeton délégué par
    # capd ; l'OS ne touche jamais à leurs identifiants.
    {
      id = "atelier";
      name = "Atelier des agents";
      description = "Faire avancer Claude Code, Codex et le modèle local ensemble sur un objectif dans ~/Documents/Prophet : la réflexion à Claude Code, le code à Codex, les étapes simples au modèle local, chacun dans sa propre mission contrôlée. Les clients doivent être connectés dans leur profil Prophet.";
      scopes = [ "~/Documents/Prophet" ];
      manifest = {
        agent = {
          id = "org.prophet.atelier";
          version = "1.0.0";
          name = "Atelier des agents";
          publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        };
        model = {
          preferred = [ "driver:claude-code" "driver:codex" "local:${cfg.model}" ];
          privacy = "local-preferred";
          roles = {
            reflect = [ "driver:claude-code" "local:${cfg.model}" ];
            code = [ "driver:codex" "driver:claude-code" "local:${cfg.model}" ];
            execute = [ "local:${cfg.model}" ];
          };
        };
        sandbox.min_level = 0;
        capabilities.max = {
          "fs.read" = [ "~/Documents/Prophet/**" ];
          "fs.write" = [ "~/Documents/Prophet/**" ];
          "task.spawn" = [ "atelier" "documents" ];
          "tool.call" = [ "fs.read" "doc.read" "fs.write" "task.delegate" ];
        };
        budget.default = { tokens = 60000; wall_time = "900s"; approvals = 3; };
      };
    }
  ]);
in {
  options.prophet.localEngine = {
    enable = lib.mkEnableOption "le moteur local et le contexte Documents Prophet" // { default = true; };
    weights = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Fichier GGUF local sous /var/lib/prophet/models ou /nix/store. null laisse le moteur non configuré. Ce module ne télécharge rien ; la configuration de référence du flake pointe le modèle par défaut, téléchargé par l'installation (ADR 0033), et la variante d'intégration continue reste sans poids.";
    };
    model = lib.mkOption {
      type = lib.types.strMatching "[A-Za-z0-9][A-Za-z0-9._-]{0,127}";
      default = "qwen3-1.7b";
      description = "Identifiant exposé par le moteur et admis par le profil de mission.";
    };
    port = lib.mkOption { type = lib.types.port; default = 8080; description = "Port sur la boucle locale uniquement."; };
    threads = lib.mkOption { type = lib.types.ints.between 1 128; default = 4; description = "Nombre maximal de threads CPU d'inférence."; };
    contextSize = lib.mkOption { type = lib.types.ints.between 4096 131072; default = 4096; description = "Contexte par requête ; sa compatibilité et sa mémoire dépendent du modèle."; };
  };

  config = lib.mkIf (config.prophet.enable && cfg.enable) {
    assertions = [{
      assertion = cfg.weights == null || lib.hasPrefix "/var/lib/prophet/models/" (toString cfg.weights)
        || lib.hasPrefix "/nix/store/" (toString cfg.weights);
      message = "Les poids Prophet doivent être placés dans /var/lib/prophet/models ou /nix/store, hors des dossiers privés du propriétaire.";
    }];
    environment.systemPackages = [ engine ];
    environment.etc."prophet/mission-profiles.json".source = profiles;
    # Le dialogue et les missions doivent interroger le même serveur, même avec un port modifié.
    environment.sessionVariables.PROPHET_MODEL_ENDPOINT = endpoint;
    systemd.services.prophet-agentd.environment = {
      PROPHET_LOCAL_ENDPOINT = endpoint;
      PROPHET_MISSION_PROFILES = "/etc/prophet/mission-profiles.json";
    };
    users.groups.prophet-model = { };
    users.users.prophet-model = {
      isSystemUser = true;
      group = "prophet-model";
      description = "Moteur de modèles locaux Prophet";
    };

    # agentd lit seulement le contexte partagé et conserve son travail dans une racine privée.
    # Les ACL ne donnent pas accès aux autres contenus privés du home. Les fichiers dont le
    # propriétaire retire explicitement la lecture restent refusés au moment de la capture.
    systemd.tmpfiles.rules = [
      "a+ /var/lib/prophet - - - - u:prophet-model:--x"
      "a+ ${home} - - - - u:agentd:r-x"
      "d ${home}/Documents 0700 ${owner} users -"
      "a+ ${home}/Documents - - - - u:agentd:r-x"
      "d ${home}/Documents/Prophet 0700 ${owner} users -"
      "a+ ${home}/Documents/Prophet - - - - u:agentd:r-x,d:u:agentd:r-x"
      "d ${home}/.prophet 0700 ${owner} users -"
      "a+ ${home}/.prophet - - - - u:agentd:r-x"
      "d ${home}/.prophet/tasks 0700 agentd prophet-system -"
    ];

    systemd.services.prophet-local-engine = lib.mkIf (cfg.weights != null) {
      description = "Prophet OS — moteur local CPU";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-tmpfiles-setup.service" ];
      unitConfig.ConditionPathExists = toString cfg.weights;
      startLimitIntervalSec = 60;
      startLimitBurst = 3;
      serviceConfig = {
        ExecStart = utils.escapeSystemdExecArgs [
          "${engine}/bin/llama-server" "--model" (toString cfg.weights)
          "--host" "127.0.0.1" "--port" (toString cfg.port) "--alias" cfg.model
          "--jinja" "--reasoning" "off" "--ctx-size" (toString cfg.contextSize)
          "--threads" (toString cfg.threads) "--parallel" "1" "--gpu-layers" "0"
          "--temp" "0.7" "--top-p" "0.8" "--top-k" "20" "--min-p" "0"
          "--presence-penalty" "1.5"
        ];
        User = "prophet-model";
        Group = "prophet-model";
        Restart = "on-failure";
        RestartSec = "5s";
        TimeoutStopSec = "15s";
        UMask = "0077";
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectKernelLogs = true;
        ProtectControlGroups = true;
        RestrictSUIDSGID = true;
        RestrictNamespaces = true;
        LockPersonality = true;
        CapabilityBoundingSet = "";
        RestrictAddressFamilies = [ "AF_UNIX" "AF_INET" "AF_INET6" ];
        IPAddressDeny = "any";
        IPAddressAllow = "localhost";
      };
    };
  };
}
