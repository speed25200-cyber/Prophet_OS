# Un moteur local (processeur, ou carte graphique par Vulkan sur demande, ADR 0037) et un
# premier contexte de mission explicitement partagé.
{ config, lib, pkgs, utils, ... }:
let
  cfg = config.prophet.localEngine;
  home = config.users.users.${config.prophet.user}.home;
  owner = config.prophet.user;
  engine = pkgs.callPackage ../packages/llama-cpp.nix { vulkan = cfg.gpu.enable; };
  # Couches du modèle placées sur la carte : toutes si l'accélération est demandée, aucune sinon.
  couches = if cfg.gpu.enable then toString cfg.gpu.layers else "0";
  endpoint = "http://127.0.0.1:${toString cfg.port}/v1";
  navigateur = config.prophet.navigateur != null;
  # Le relais local (ADR 0034) : avec un second poids, le moteur sert deux modèles en mode
  # routeur, le grand réfléchit, le petit exécute ; sans lui, un seul modèle fait tout.
  relais = cfg.executeWeights != null;
  modelesLocaux = [ "local:${cfg.model}" ] ++ lib.optional relais "local:${cfg.executeModel}";
  # Les clients officiels de l'humain — Claude Code (Anthropic) et Codex (OpenAI) — sont les
  # modèles principaux de tout contexte (ADR 0035, complément du 14 septembre) : le service les
  # propose d'abord quand le lanceur de la session les dit connectés, et une mission préparée sur
  # l'un d'eux est lancée par ce lanceur, sans le moteur local. Le modèle local n'est qu'un
  # secours, hors ligne ou sans compte connecté ; il ferme chaque liste. Les rôles suivent :
  # Claude Code réfléchit et relit, Codex code et exécute, chacun n'étant proposé que connecté.
  clients = [ "driver:claude-code" "driver:codex" ];
  # Les paliers de modèles (ADR 0040) : le meilleur modèle de Claude Code pour réfléchir et
  # coder, un palier moins coûteux pour relire, le moins cher pour exécuter — l'économie de
  # tokens du relais, par l'option `--model` que le lanceur passe au client. Codex garde son
  # modèle par défaut. Les paliers se changent par `prophet.localEngine.paliers`.
  paliers = cfg.paliers;
  modele = {
    preferred = clients ++ modelesLocaux;
    privacy = "local-preferred";
    roles = {
      reflect = [ "driver:claude-code@${paliers.reflect}" "driver:codex" "local:${cfg.model}" ];
      code = [ "driver:claude-code@${paliers.code}" "driver:codex" "local:${cfg.model}" ];
      review = [ "driver:codex" "driver:claude-code@${paliers.review}" "local:${cfg.model}" ];
      execute = [ "driver:claude-code@${paliers.execute}" "driver:codex" ] ++ lib.optional relais "local:${cfg.executeModel}" ++ [ "local:${cfg.model}" ];
    };
  };
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
        model = modele;
        sandbox.min_level = 0;
        capabilities.max = {
          "fs.read" = [ "~/Documents/Prophet/**" ];
          "fs.write" = [ "~/Documents/Prophet/**" ];
          "tool.call" = [ "task.status" "task.diff" "fs.read" "doc.read" "fs.write" ];
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
        model = modele;
        sandbox.min_level = 0;
        capabilities.max = {
          "fs.read" = [ "~/Documents/Prophet/**" ];
          "fs.write" = [ "~/Documents/Prophet/**" ];
          "net.egress" = [ "*" ];
          # Le contexte web peut confier la rédaction au contexte documents : deux agents, deux
          # modèles au besoin, sous un jeton délégué par capd (ADR 0029).
          "task.spawn" = [ "documents" ];
          "tool.call" = [ "task.status" "task.diff" "fs.read" "doc.read" "fs.write" "http.fetch" "task.delegate" ]
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
      description = "Travailler dans les applications ouvertes sur le bureau (éditeur, LibreOffice, GIMP, Inkscape, FreeCAD, Kdenlive, darktable, lecteur PDF) et déposer des fichiers dans ~/Documents/Prophet. L'agent lit et manipule l'application par son arbre d'accessibilité, sans capture d'écran ; chaque action figure au journal.";
      scopes = [ "~/Documents/Prophet" ];
      manifest = {
        agent = {
          id = "org.prophet.bureau";
          version = "1.0.0";
          name = "Éditeur du bureau";
          publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        };
        model = modele;
        sandbox.min_level = 0;
        capabilities.max = {
          "fs.read" = [ "~/Documents/Prophet/**" ];
          "fs.write" = [ "~/Documents/Prophet/**" ];
          # L'éditeur, et la suite de l'humain quand elle publie une accessibilité : LibreOffice
          # (« soffice » sur le bus), GIMP, Inkscape, FreeCAD, le lecteur PDF ; jamais l'écran.
          "ui.read" = [ "mousepad" "soffice" "gimp" "inkscape" "freecad" "evince" "kdenlive" "darktable" ];
          "ui.act" = [ "mousepad" "soffice" "gimp" "inkscape" "freecad" "evince" "kdenlive" "darktable" ];
          "tool.call" = [ "task.status" "task.diff" "fs.read" "doc.read" "fs.write" "ui.apps" "ui.tree" "ui.act" ];
        };
        budget.default = { tokens = 30000; wall_time = "180s"; approvals = 3; };
      };
    }
    # L'atelier logiciel (ADR 0031, 0038) : écrire un outil à la demande et l'exécuter en
    # microVM. `proc.exec` élève tout programme hors liste blanche au niveau 2, que sandboxd
    # n'accorde que si KVM, Firecracker et l'invité sont là ; sinon, il le dit et ne lance rien.
    # Ce que l'agent écrit va dans `outils`, à examiner avant publication comme le reste.
    {
      id = "logiciel";
      name = "Atelier logiciel";
      description = "Écrire un petit programme à la demande dans ~/Documents/Prophet/outils et l'exécuter dans une microVM pour le vérifier (Python 3, shell). La machine doit offrir la virtualisation (KVM) ; sinon l'exécution est refusée en le disant.";
      scopes = [ "~/Documents/Prophet" ];
      manifest = {
        agent = {
          id = "org.prophet.logiciel";
          version = "1.0.0";
          name = "Atelier logiciel";
          publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        };
        model = modele;
        sandbox = { min_level = 0; code_execution = "microvm"; };
        capabilities.max = {
          "fs.read" = [ "~/Documents/Prophet/**" ];
          "fs.write" = [ "~/Documents/Prophet/outils/**" ];
          "proc.exec" = [ "python3" "sh" ];
          "tool.call" = [ "task.status" "task.diff" "fs.read" "doc.read" "fs.write" "proc.exec" "proc.kill" ];
        };
        budget.default = { tokens = 40000; wall_time = "300s"; approvals = 3; };
      };
    }
    # L'atelier : les clients officiels de l'humain travaillent ensemble par le relais (ADR
    # 0034, 0035, 0040). Claude Code réfléchit et code avec son meilleur palier, Codex relit
    # (un autre regard que l'auteur) puis Claude Code à un palier moindre, le palier le moins
    # cher exécute les étapes simples ; chacun n'est proposé que si le lanceur de la session
    # le dit connecté, sinon le rôle retombe sur le modèle local. Les clients rejoignent leur sous-mission par une séance d'outils, sous un jeton
    # délégué par capd ; l'OS ne touche jamais à leurs identifiants.
    {
      id = "atelier";
      name = "Atelier des agents";
      description = "Faire avancer Claude Code et Codex ensemble sur un objectif dans ~/Documents/Prophet : la réflexion et le code au meilleur palier de Claude Code, la relecture à Codex, les étapes simples au palier le moins cher, chacun dans sa propre mission contrôlée ; le modèle local ne sert que de secours. Les clients doivent être connectés dans leur profil Prophet.";
      scopes = [ "~/Documents/Prophet" ];
      manifest = {
        agent = {
          id = "org.prophet.atelier";
          version = "1.0.0";
          name = "Atelier des agents";
          publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        };
        model = modele;
        sandbox.min_level = 0;
        capabilities.max = {
          "fs.read" = [ "~/Documents/Prophet/**" ];
          "fs.write" = [ "~/Documents/Prophet/**" ];
          "task.spawn" = [ "atelier" "documents" ];
          "tool.call" = [ "task.status" "task.diff" "fs.read" "doc.read" "fs.write" "task.delegate" ];
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
    executeWeights = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Second fichier GGUF, le modèle d'exécution du relais (ADR 0034) : le moteur passe alors en mode routeur et sert les deux modèles, chargés à la demande ; les profils gagnent les rôles reflect (modèle principal) et execute (celui-ci). null : un seul modèle.";
    };
    executeModel = lib.mkOption {
      type = lib.types.strMatching "[A-Za-z0-9][A-Za-z0-9._-]{0,127}";
      default = "qwen3-0.6b";
      description = "Identifiant du modèle d'exécution exposé par le moteur.";
    };
    gpu = {
      enable = lib.mkEnableOption "l'accélération des modèles locaux par la carte graphique, avec le backend Vulkan de llama.cpp (ADR 0037) ; le moteur est alors servi par la variante Vulkan du paquet et l'unité voit les périphériques DRM. Non mesuré sur une vraie carte : le processeur reste le défaut";
      layers = lib.mkOption {
        type = lib.types.ints.between 1 999;
        default = 999;
        description = "Couches du modèle placées sur la carte quand l'accélération est active ; 999 les place toutes, une valeur plus basse partage avec le processeur quand la mémoire vidéo manque.";
      };
    };
    paliers = {
      reflect = lib.mkOption { type = lib.types.str; default = "opus"; description = "Palier de modèle demandé à Claude Code pour la réflexion (ADR 0040) : un alias ou un identifiant que `claude --model` accepte."; };
      code = lib.mkOption { type = lib.types.str; default = "opus"; description = "Palier de modèle demandé à Claude Code pour le code."; };
      review = lib.mkOption { type = lib.types.str; default = "sonnet"; description = "Palier de modèle demandé à Claude Code pour la relecture, après Codex."; };
      execute = lib.mkOption { type = lib.types.str; default = "haiku"; description = "Palier de modèle demandé à Claude Code pour les étapes simples : le moins cher."; };
    };
    port = lib.mkOption { type = lib.types.port; default = 8080; description = "Port sur la boucle locale uniquement."; };
    threads = lib.mkOption { type = lib.types.ints.between 1 128; default = 4; description = "Nombre maximal de threads CPU d'inférence."; };
    contextSize = lib.mkOption { type = lib.types.ints.between 4096 131072; default = 4096; description = "Contexte par requête ; sa compatibilité et sa mémoire dépendent du modèle."; };
  };

  config = lib.mkIf (config.prophet.enable && cfg.enable) {
    assertions = [
      {
        assertion = cfg.weights == null || lib.hasPrefix "/var/lib/prophet/models/" (toString cfg.weights)
          || lib.hasPrefix "/nix/store/" (toString cfg.weights);
        message = "Les poids Prophet doivent être placés dans /var/lib/prophet/models ou /nix/store, hors des dossiers privés du propriétaire.";
      }
      {
        assertion = cfg.executeWeights == null || (cfg.weights != null && cfg.model != cfg.executeModel
          && (lib.hasPrefix "/var/lib/prophet/models/" (toString cfg.executeWeights)
            || lib.hasPrefix "/nix/store/" (toString cfg.executeWeights)));
        message = "Le modèle d'exécution exige le modèle principal, un identifiant distinct, et des poids sous /var/lib/prophet/models ou /nix/store.";
      }
    ];
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
    #
    # Le masque est écrit à chaque fois (`m::r-x`), et ce n'est pas un détail. Un répertoire
    # 0700 qui reçoit une entrée `u:agentd:r-x` se lit ensuite 0750 : le masque tient lieu de
    # bits de groupe. Au démarrage suivant, l'activation de NixOS (`homeMode`) et les lignes
    # `d … 0700` ci-dessous voient 0750, remettent 0700, et ce chmod ramène le masque à `---` ;
    # `a+` ne recalcule pas un masque qui existe déjà, et agentd perd sa traversée — sur le
    # système installé, qui démarre au moins deux fois, `user:agentd:r-x #effective:---` sur
    # les trois niveaux, lu en CI le 14 septembre 2026. Un masque explicite survit à ce cycle.
    systemd.tmpfiles.rules = [
      "a+ /var/lib/prophet - - - - u:prophet-model:--x"
      "a+ ${home} - - - - u:agentd:r-x,m::r-x"
      "d ${home}/Documents 0700 ${owner} users -"
      "a+ ${home}/Documents - - - - u:agentd:r-x,m::r-x"
      "d ${home}/Documents/Prophet 0700 ${owner} users -"
      "a+ ${home}/Documents/Prophet - - - - u:agentd:r-x,m::r-x,d:u:agentd:r-x,d:m::r-x"
      "d ${home}/.prophet 0700 ${owner} users -"
      "a+ ${home}/.prophet - - - - u:agentd:r-x,m::r-x"
      "d ${home}/.prophet/tasks 0700 agentd prophet-system -"
    ];

    systemd.services.prophet-local-engine = let
      # Les réglages d'un modèle, identiques en mode simple et en mode routeur.
      reglages = [
        "--jinja" "--reasoning" "off" "--ctx-size" (toString cfg.contextSize)
        "--threads" (toString cfg.threads) "--parallel" "1" "--gpu-layers" couches
        "--temp" "0.7" "--top-p" "0.8" "--top-k" "20" "--min-p" "0"
        "--presence-penalty" "1.5"
      ];
      # En mode routeur, chaque modèle est une section du fichier de préréglages, nommée par
      # l'identifiant qu'il expose ; le routeur charge les modèles à la demande sur le même port.
      section = nom: poids: ''
        [${nom}]
        model = ${toString poids}
        jinja = 1
        reasoning = off
        ctx-size = ${toString cfg.contextSize}
        threads = ${toString cfg.threads}
        parallel = 1
        n-gpu-layers = ${couches}
        temp = 0.7
        top-p = 0.8
        top-k = 20
        min-p = 0
        presence-penalty = 1.5
      '';
      prereglages = pkgs.writeText "prophet-modeles.ini"
        (section cfg.model cfg.weights + "\n" + section cfg.executeModel cfg.executeWeights);
    in lib.mkIf (cfg.weights != null) {
      description = "Prophet OS — moteur local ${if cfg.gpu.enable then "sur carte graphique (Vulkan)" else "CPU"}";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-tmpfiles-setup.service" ];
      unitConfig.ConditionPathExists = [ (toString cfg.weights) ]
        ++ lib.optional relais (toString cfg.executeWeights);
      startLimitIntervalSec = 60;
      startLimitBurst = 3;
      serviceConfig = {
        ExecStart = utils.escapeSystemdExecArgs (
          [ "${engine}/bin/llama-server" "--host" "127.0.0.1" "--port" (toString cfg.port) ]
          ++ (if relais
            then [ "--models-preset" (toString prereglages) "--models-max" "2" ]
            else [ "--model" (toString cfg.weights) "--alias" cfg.model ] ++ reglages)
        );
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
        # Sans carte, aucun périphérique ; avec, les seuls nœuds DRM, par le contrôle de
        # périphériques du cgroup, et les groupes qui y donnent accès (ADR 0037).
        PrivateDevices = !cfg.gpu.enable;
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
      } // lib.optionalAttrs cfg.gpu.enable {
        DeviceAllow = [ "char-drm rw" ];
        SupplementaryGroups = [ "video" "render" ];
      };
    };
  };
}
