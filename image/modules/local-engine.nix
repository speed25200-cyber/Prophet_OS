# Un moteur CPU local et un premier contexte de mission explicitement partagé.
{ config, lib, pkgs, utils, ... }:
let
  cfg = config.prophet.localEngine;
  home = config.users.users.${config.prophet.user}.home;
  owner = config.prophet.user;
  engine = pkgs.callPackage ../packages/llama-cpp.nix { };
  endpoint = "http://127.0.0.1:${toString cfg.port}/v1";
  profiles = pkgs.writeText "prophet-mission-profiles.json" (builtins.toJSON [{
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
        "tool.call" = [ "fs.read" "fs.write" ];
      };
      budget.default = { tokens = 20000; wall_time = "90s"; approvals = 3; };
    };
  }]);
in {
  options.prophet.localEngine = {
    enable = lib.mkEnableOption "le moteur local et le contexte Documents Prophet" // { default = true; };
    weights = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Fichier GGUF local sous /var/lib/prophet/models ou /nix/store. null laisse le moteur non configuré ; aucun poids n'est téléchargé automatiquement.";
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
