# Session humaine : les clients interactifs gardent leur identité et leurs mécanismes officiels.
{ config, lib, pkgs, ... }:
let
  cfg = config.prophet.desktop;
  human = config.prophet.user;
  prophet = pkgs.callPackage ../packages/prophet-os.nix { };
  chatgpt = pkgs.callPackage ../packages/chatgpt-linux.nix { };
  # Le navigateur de la session humaine et des applications web installées par défaut (X).
  chromium = pkgs.chromium;
  compositor = config.programs.sway.package;
  # Le socket de l'adaptateur d'accessibilité : dans le répertoire d'exécution des services,
  # que le groupe système de Prophet peut écrire (l'humain en est membre) et joindre (agentd).
  supSocket = "/run/prophet/sup.sock";
  pilotSocket = "/run/prophet/pilot.sock";
  launcher = pkgs.writeShellApplication {
    name = "prophet-ouvrir";
    runtimeInputs = [ pkgs.coreutils pkgs.jq pkgs.fuzzel pkgs.foot compositor ];
    text = ''
      human=${lib.escapeShellArg human}
      app="''${1:-applications}"
      # La liste des applications, lisible sans session graphique : c'est ce que le test du
      # bureau vérifie, et ce que l'humain voit dans le lanceur.
      entrees='Supervision
ChatGPT
Claude Code
Claude Code · mission
Codex
Navigateur
X
Éditeur
Outils
${lib.optionalString cfg.suite.enable ''
LibreOffice
GIMP
Inkscape
Blender
FreeCAD
Lecteur PDF
Vidéo
Montage vidéo
Photo
''}Fichiers
Terminal
Verrouiller
Déconnexion'
      if [ "$app" = --liste ]; then
        printf '%s\n' "$entrees"
        exit 0
      fi
      # Les outils : les programmes que l'atelier logiciel a écrits à la demande et que l'humain
      # a publiés dans ~/Documents/Prophet/outils (ADR 0031, 0038) — un fichier Python ou shell,
      # ou un dossier qui porte un main.py / main.sh. Nommés par leur fichier, sans extension.
      outils="$HOME/Documents/Prophet/outils"
      outils_disponibles() {
        local f b
        [ -d "$outils" ] || return 0
        for f in "$outils"/*.py "$outils"/*.sh "$outils"/*/main.py "$outils"/*/main.sh; do
          [ -f "$f" ] || continue
          case "$f" in
            */main.py|*/main.sh) basename -- "$(dirname -- "$f")" ;;
            *) b=$(basename -- "$f"); printf '%s\n' "''${b%.*}" ;;
          esac
        done | sort -u
      }
      outil_programme() {
        local nom="$1" c
        for c in "$outils/$nom.py" "$outils/$nom.sh" "$outils/$nom/main.py" "$outils/$nom/main.sh"; do
          if [ -f "$c" ]; then printf '%s\n' "$c"; return 0; fi
        done
        return 1
      }
      if [ "$app" = outils ] && [ "''${2:-}" = --liste ]; then
        outils_disponibles
        exit 0
      fi
      if [ "$(id -un)" != "$human" ] || [ -z "''${WAYLAND_DISPLAY:-}" ]; then
        echo "Ouvrez une session graphique avec le compte $human." >&2
        exit 1
      fi
      umask 077
      if [ "$app" = applications ]; then
        selection=$(printf '%s\n' "$entrees" | fuzzel --dmenu --prompt='Ouvrir  ') || exit 0
        case "$selection" in
          Supervision) app=supervision ;; ChatGPT) app=chatgpt ;;
          'Claude Code') app=claude-code ;; Codex) app=codex ;;
          'Claude Code · mission') app=claude-code-mission ;;
          Fichiers) app=fichiers ;; Terminal) app=terminal ;;
          Navigateur) app=navigateur ;; X) app=x ;; Éditeur) app=editeur ;; Outils) app=outils ;;
          LibreOffice) app=libreoffice ;; GIMP) app=gimp ;; Inkscape) app=inkscape ;;
          Blender) app=blender ;; FreeCAD) app=freecad ;; 'Lecteur PDF') app=pdf ;; Vidéo) app=video ;;
          'Montage vidéo') app=montage ;; Photo) app=photo ;;
          Verrouiller) app=verrouiller ;; Déconnexion) app=deconnexion ;;
          *) exit 0 ;;
        esac
      fi
      focus() {
        if swaymsg -t get_tree | jq -e --arg app "$1" '[.. | objects | select(.app_id? == $app)] | length > 0' >/dev/null; then
          swaymsg "[app_id=\"$1\"] focus" >/dev/null
          return 0
        fi
        return 1
      }
      launch() {
        # Demander un contexte d'activation APRÈS le changement d'espace. Celui du
        # raccourci initial rattacherait la nouvelle fenêtre à l'espace précédent.
        local command
        printf -v command '%q ' "$@"
        swaymsg exec "$command" >/dev/null
      }
      case "$app" in
        supervision)
          swaymsg 'workspace "1: Supervision"' >/dev/null
          ${pkgs.systemd}/bin/systemctl --user start prophet-supervision.service
          ;;
        terminal)
          swaymsg 'workspace "2: Atelier"' >/dev/null
          focus org.prophet.Terminal || launch foot --working-directory="$HOME" --app-id=org.prophet.Terminal --title=Terminal
          ;;
        fichiers)
          swaymsg 'workspace "2: Atelier"' >/dev/null
          launch ${pkgs.thunar}/bin/thunar "$HOME/Documents/Prophet"
          ;;
${lib.optionalString cfg.suite.enable ''
        libreoffice|gimp|inkscape|blender|freecad|pdf|video|montage|photo)
          # La suite de l'humain, dans l'espace Atelier ; un chemin en second argument ouvre
          # ce fichier. LibreOffice et les applications GTK publient leur accessibilité, et
          # l'agent les lit et les pilote par elle (contexte « bureau ») ; Blender, non.
          swaymsg 'workspace "2: Atelier"' >/dev/null
          # Par le PATH de la session, où la suite est installée : les paquets nomment leurs
          # programmes à leur façon (FreeCAD en capitales selon la version).
          case "$app" in
            libreoffice) launch libreoffice "''${2:-}" ;;
            gimp) launch gimp "''${2:-}" ;;
            inkscape) launch inkscape "''${2:-}" ;;
            blender) launch blender "''${2:-}" ;;
            freecad) launch "$(command -v freecad || command -v FreeCAD)" "''${2:-}" ;;
            pdf) launch evince "''${2:-}" ;;
            video) launch mpv "''${2:-}" ;;
            montage) launch kdenlive "''${2:-}" ;;
            photo) launch darktable "''${2:-}" ;;
          esac
          ;;
''}        editeur)
          # L'éditeur de texte du bureau : une application GTK que l'agent peut lire et
          # piloter par son arbre d'accessibilité (contexte « bureau », ADR 0027). Un chemin
          # en second argument ouvre ce fichier ; sinon un document vide dans l'espace Prophet.
          swaymsg 'workspace "2: Atelier"' >/dev/null
          # Jamais `install -d -m 0700` ici : sur un répertoire qui existe déjà, c'est un chmod,
          # et un chmod réduit le masque de l'ACL que tmpfiles a posée pour agentd — le service
          # ne pourrait plus lire ce que l'humain lui confie (vu en CI le 13 septembre 2026).
          # tmpfiles crée l'espace Prophet à chaque démarrage ; ceci n'est qu'un secours.
          mkdir -p -- "$HOME/Documents/Prophet"
          launch ${pkgs.mousepad}/bin/mousepad "''${2:-}"
          ;;
        outils)
          # Un outil publié s'ouvre dans un terminal qui reste ouvert, dans son dossier, sous
          # l'identité de l'humain — comme un programme qu'il aurait écrit lui-même : c'est le
          # sien depuis qu'il l'a examiné et publié. L'agent, lui, ne l'exécute qu'en microVM.
          # Un nom en second argument ouvre cet outil ; sinon, le choix parmi ceux qui existent.
          swaymsg 'workspace "2: Atelier"' >/dev/null
          nom="''${2:-}"
          if [ -z "$nom" ]; then
            liste=$(outils_disponibles)
            if [ -z "$liste" ]; then
              printf '%s\n' "Aucun outil publié : demandez-en un à l'atelier logiciel." |
                fuzzel --dmenu --lines=1 --prompt='Outils  ' >/dev/null || true
              exit 0
            fi
            nom=$(printf '%s\n' "$liste" | fuzzel --dmenu --prompt='Outil  ') || exit 0
          fi
          if ! programme=$(outil_programme "$nom"); then
            echo "Outil inconnu : $nom (dans $outils)" >&2
            exit 2
          fi
          case "$programme" in
            *.py) interprete=${pkgs.python3}/bin/python3 ;;
            *) interprete=${pkgs.bash}/bin/sh ;;
          esac
          launch foot --hold --working-directory="$(dirname -- "$programme")" \
            --app-id=org.prophet.Outil --title="Outil · $nom" "$interprete" "$programme"
          ;;
        claude-code)
          swaymsg 'workspace "2: Atelier"' >/dev/null
          swaymsg 'layout tabbed' >/dev/null
          focus org.prophet.ClaudeCode && exit 0
          profile="$HOME/.local/state/prophet/providers/claude-code/$human"
          install -d -m 0700 -- "$profile"
          launch foot --hold --working-directory="$HOME/Documents/Prophet" --app-id=org.prophet.ClaudeCode --title='Claude Code' \
            env CLAUDE_CONFIG_DIR="$profile" ${pkgs.claude-code}/bin/claude
          ;;
        claude-code-mission)
          # Claude Code dans une mission Prophet : le service prépare la mission pour ce compte
          # (jeton capd, travail SFS, journal) sans exiger le moteur local, et le client reçoit
          # ses outils par le pont prophet-mcp ; à la fermeture du client, la mission passe à
          # l'examen dans la supervision, puis à la publication (ADR 0026). Le client garde ses
          # propres outils : la mission ajoute les outils contrôlés, elle ne le confine pas.
          swaymsg 'workspace "2: Atelier"' >/dev/null
          swaymsg 'layout tabbed' >/dev/null
          profile="$HOME/.local/state/prophet/providers/claude-code/$human"
          install -d -m 0700 -- "$profile"
          missions="''${XDG_RUNTIME_DIR:-/tmp}/prophet-missions"
          install -d -m 0700 -- "$missions"
          if ! mission=$(${prophet}/bin/prophet --json task prepare --client --profile documents \
              "Session Claude Code du $(date +%F)" | jq -r .task) || [ -z "$mission" ] || [ "$mission" = null ]; then
            echo "Aucune mission préparée : agentd ne répond pas ou n'a pas de contexte « documents »." >&2
            exit 1
          fi
          ${prophet}/bin/prophet task mcp-config "$mission" > "$missions/$mission.json"
          launch foot --hold --working-directory="$HOME/Documents/Prophet" --app-id=org.prophet.ClaudeCode --title="Claude Code · $mission" \
            env CLAUDE_CONFIG_DIR="$profile" ${pkgs.claude-code}/bin/claude --mcp-config="$missions/$mission.json"
          ;;
        codex)
          swaymsg 'workspace "2: Atelier"' >/dev/null
          swaymsg 'layout tabbed' >/dev/null
          focus org.prophet.Codex && exit 0
          profile="$HOME/.local/state/prophet/providers/codex/$human"
          install -d -m 0700 -- "$profile"
          launch foot --hold --working-directory="$HOME/Documents/Prophet" --app-id=org.prophet.Codex --title=Codex \
            env CODEX_HOME="$profile" ${pkgs.codex}/bin/codex
          ;;
        chatgpt)
          swaymsg 'workspace "3: Dialogue"' >/dev/null
          launch ${chatgpt}/bin/chatgpt --ozone-platform=x11
          ;;
        navigateur)
          # Le navigateur partagé : un profil Prophet distinct des profils des clients
          # officiels, ouvert dans l'espace Recherche. L'agent, lui, navigue par son propre
          # navigateur piloté (outils web) ; ce qu'il ouvre se lit dans la supervision.
          swaymsg 'workspace "4: Recherche"' >/dev/null
          focus org.prophet.Navigateur && exit 0
          profile="$HOME/.local/state/prophet/navigateur"
          install -d -m 0700 -- "$profile"
          launch ${chromium}/bin/chromium --ozone-platform=wayland \
            --user-data-dir="$profile" --class=org.prophet.Navigateur --no-first-run \
            --no-default-browser-check "''${2:-about:blank}"
          ;;
        x)
          # X, en fenêtre d'application dédiée : son propre profil, jamais celui du navigateur
          # partagé ni ceux des clients officiels ; la session appartient à l'humain.
          swaymsg 'workspace "3: Dialogue"' >/dev/null
          focus org.prophet.X && exit 0
          profile="$HOME/.local/state/prophet/apps/x"
          install -d -m 0700 -- "$profile"
          launch ${chromium}/bin/chromium --ozone-platform=wayland \
            --user-data-dir="$profile" --class=org.prophet.X --no-first-run \
            --no-default-browser-check --app=https://x.com/
          ;;
        verrouiller) exec ${pkgs.swaylock}/bin/swaylock --color 171b22 ;;
        deconnexion)
          decision=$(printf '%s\n' Annuler 'Se déconnecter' |
            fuzzel --dmenu --lines=2 --prompt='Fermer la session ?  ') || exit 0
          if [ "$decision" = 'Se déconnecter' ]; then
            swaymsg exit
          fi
          ;;
        *) echo "Application inconnue : $app" >&2; exit 2 ;;
      esac
    '';
  };
  session = pkgs.writeShellApplication {
    name = "prophet-session";
    runtimeInputs = [ compositor pkgs.systemd ];
    text = ''
      if [ "$(id -un)" != ${lib.escapeShellArg human} ]; then
        echo "Cette session appartient au propriétaire configuré de la machine." >&2
        exit 1
      fi
      umask 077
      # Le gestionnaire d'utilisateur survit à la session graphique. Sans ces deux lignes, la
      # cible `sway-session.target` restait active après une déconnexion, la supervision mourait
      # trois fois de suite faute de compositeur, atteignait sa limite de redémarrages, et la
      # session suivante s'ouvrait sans elle (constaté par le test du bureau le 13 septembre
      # 2026). Chaque session repart donc d'une cible arrêtée et d'un compteur remis à zéro.
      systemctl --user stop sway-session.target 2>/dev/null || true
      systemctl --user reset-failed prophet-supervision.service 2>/dev/null || true
      sway --config /etc/prophet/sway.conf
      # À la sortie du compositeur, ce qui en dépendait s'arrête au lieu de tourner à vide.
      systemctl --user stop sway-session.target 2>/dev/null || true
    '';
  };
  sessionEntry = pkgs.runCommand "prophet-desktop-session" {
    passthru.providedSessions = [ "prophet" ];
  } ''
    mkdir -p "$out/share/wayland-sessions"
    cat > "$out/share/wayland-sessions/prophet.desktop" <<EOF
    [Desktop Entry]
    Name=Prophet OS
    Comment=Supervision des agents et espace de travail humain
    Exec=${session}/bin/prophet-session
    Type=Application
    DesktopNames=sway
    EOF
  '';
in {
  options.prophet.desktop.enable = lib.mkEnableOption "la session humaine de Prophet OS" // { default = true; };
  # La suite d'applications de l'humain : bureautique, image, dessin vectoriel, 3D, CAO, PDF,
  # lecture vidéo, montage vidéo (Kdenlive) et développement photo (darktable). Ce sont les
  # logiciels libres qui tiennent les rôles de Word, Photoshop, Lightroom, Illustrator, Premiere,
  # Blender et AutoCAD ; ceux qui publient une accessibilité (GTK, Qt) se pilotent par l'agent
  # (ADR 0027). Désactivée dans les tests, qui n'en ont pas l'usage et paient chaque octet.
  options.prophet.desktop.suite.enable = lib.mkEnableOption "la suite d'applications du bureau" // { default = true; };

  config = lib.mkIf cfg.enable {
    assertions = [{
      assertion = config.prophet.enable;
      message = "Le bureau Prophet requiert les services Prophet et leur propriétaire configuré.";
    }];
    # L'ancien kiosque reste disponible en désactivant ce module. Ses droits ne sont pas élargis.
    prophet.surface.enable = lib.mkForce false;
    hardware.graphics.enable = true;
    fonts.packages = [ pkgs.inter pkgs.dejavu_fonts ];
    fonts.fontconfig.defaultFonts = {
      sansSerif = [ "Inter" "DejaVu Sans" ];
      monospace = [ "DejaVu Sans Mono" ];
    };
    programs.sway = {
      enable = true;
      package = pkgs.swayfx;
      wrapperFeatures.gtk = true;
      xwayland.enable = true;
      extraSessionCommands = ''
        export XDG_CURRENT_DESKTOP=sway
        export XDG_SESSION_DESKTOP=prophet
        export XDG_SESSION_TYPE=wayland
      '';
    };
    services.displayManager.sessionPackages = lib.mkForce [ sessionEntry ];
    services.displayManager.regreet = {
      enable = true;
      font = { package = pkgs.inter; name = "Inter"; size = 14; };
      settings = {
        appearance.greeting_msg = "Prophet OS";
        GTK.application_prefer_dark_theme = false;
        widget.clock.format = "%A %d %B · %H:%M";
      };
      extraCss = ''
        window { background: linear-gradient(135deg, #e5e9ed, #faf9f5); color: #20252d; }
        button { border-radius: 12px; padding: 10px 20px; }
        entry { border-radius: 12px; padding: 10px; }
        .suggested-action { background: #252c38; color: #fff; }
      '';
    };
    # greetd réserve tty1. La console de secours existe indépendamment sur tty2.
    systemd.targets.multi-user.wants = [ "getty@tty2.service" ];
    services.gnome.gnome-keyring.enable = true;
    services.gvfs.enable = true;
    xdg.portal = { enable = true; wlr.enable = true; extraPortals = [ pkgs.xdg-desktop-portal-gtk ]; };
    security.pam.services.swaylock = { };
    environment.systemPackages = [
      launcher session chatgpt chromium pkgs.foot pkgs.thunar pkgs.mousepad pkgs.wl-clipboard
    ] ++ lib.optionals cfg.suite.enable [
      pkgs.libreoffice pkgs.gimp pkgs.inkscape pkgs.blender pkgs.freecad pkgs.evince pkgs.mpv
      pkgs.kdePackages.kdenlive pkgs.darktable
      pkgs.fuzzel pkgs.waybar pkgs.swaylock pkgs.adwaita-icon-theme
    ];
    environment.sessionVariables = {
      BROWSER = "${launcher}/bin/prophet-ouvrir navigateur";
      TERMINAL = "${pkgs.foot}/bin/foot";
      GTK_USE_PORTAL = "1";
    };
    environment.etc."prophet/sway.conf".text = ''
      set $mod Mod4
      font pango:Inter 11
      output * bg #e9ecef solid_color
      gaps inner 14
      gaps outer 14
      default_border pixel 2
      default_floating_border pixel 2
      smart_borders on
      corner_radius 16
      shadows enable
      shadow_blur_radius 28
      shadow_color #11182735
      focus_follows_mouse no
      client.focused #697e9d #f9fafb #20252d #697e9d #697e9d
      client.unfocused #cdd4db #eff2f4 #69717d #cdd4db #cdd4db
      input type:touchpad { tap enabled; natural_scroll enabled; }
      bindsym $mod+space exec ${launcher}/bin/prophet-ouvrir
      bindsym $mod+Return exec ${launcher}/bin/prophet-ouvrir terminal
      bindsym $mod+e exec ${launcher}/bin/prophet-ouvrir fichiers
      bindsym $mod+n exec ${launcher}/bin/prophet-ouvrir navigateur
      bindsym $mod+x exec ${launcher}/bin/prophet-ouvrir x
      bindsym $mod+l exec ${launcher}/bin/prophet-ouvrir verrouiller
      bindsym $mod+1 workspace "1: Supervision"
      bindsym $mod+2 workspace "2: Atelier"
      bindsym $mod+3 workspace "3: Dialogue"
      bindsym $mod+4 workspace "4: Recherche"
      bindsym $mod+Shift+1 move container to workspace "1: Supervision"
      bindsym $mod+Shift+2 move container to workspace "2: Atelier"
      bindsym $mod+Shift+3 move container to workspace "3: Dialogue"
      bindsym $mod+Shift+4 move container to workspace "4: Recherche"
      bindsym $mod+Left focus left
      bindsym $mod+Right focus right
      bindsym $mod+Up focus up
      bindsym $mod+Down focus down
      bindsym $mod+Shift+Left move left
      bindsym $mod+Shift+Right move right
      bindsym $mod+Shift+space floating toggle
      bindsym $mod+f fullscreen toggle
      bindsym $mod+w layout tabbed
      bindsym $mod+b layout splith
      bindsym $mod+Shift+q kill
      bindsym $mod+Shift+e exec ${launcher}/bin/prophet-ouvrir deconnexion
      floating_modifier $mod normal
      for_window [app_id="^org.prophet.Supervision$"] move container to workspace "1: Supervision"
      workspace "1: Supervision"
      include /etc/sway/config.d/*
      exec ${pkgs.waybar}/bin/waybar --config /etc/prophet/waybar.json --style /etc/prophet/waybar.css
    '';
    # Le bus d'accessibilité de la session : c'est par lui que les applications GTK et Qt
    # publient leur arbre, et par lui que l'adaptateur les lit et les pilote (ADR 0027).
    services.gnome.at-spi2-core.enable = true;
    # Les applications Qt (FreeCAD) ne publient leur accessibilité que si on le leur demande ;
    # GTK le fait dès que le bus est là.
    environment.sessionVariables.QT_LINUX_ACCESSIBILITY_ALWAYS_ON = "1";
    # L'adaptateur d'accessibilité, dans la session : il joint le bus de la session, écoute sur
    # un socket du répertoire des services (groupe système de Prophet) et n'admet qu'agentd.
    systemd.user.services.prophet-supd = {
      description = "Prophet OS — adaptateur d'accessibilité de la session";
      wantedBy = [ "sway-session.target" ];
      partOf = [ "sway-session.target" ];
      after = [ "graphical-session-pre.target" ];
      environment = {
        PROPHET_SUP_SOCKET = supSocket;
        PROPHET_SUP_CLIENT = "agentd";
        PROPHET_SUP_GROUP = "prophet-system";
      };
      unitConfig = { StartLimitIntervalSec = 60; StartLimitBurst = 3; ConditionUser = human; };
      serviceConfig = {
        ExecStart = "${prophet}/bin/prophet-supd";
        Restart = "on-failure";
        RestartSec = "3s";
        UMask = "0007";
      };
    };
    systemd.services.prophet-agentd.environment.PROPHET_SUP_SOCKET = supSocket;
    # Le lanceur de pilotes, dans la session : il lance Claude Code, Codex ou Gemini, sans
    # modification, sous l'identité de l'humain et avec le profil privé de chaque client, dans
    # une mission préparée par agentd (séance MCP), quand un rôle du relais les désigne
    # (ADR 0035). Il n'admet qu'agentd ; l'OS ne lit jamais les identifiants des clients.
    systemd.user.services.prophet-pilotd = {
      description = "Prophet OS — lanceur de pilotes de la session";
      wantedBy = [ "sway-session.target" ];
      partOf = [ "sway-session.target" ];
      after = [ "graphical-session-pre.target" ];
      path = [ pkgs.claude-code pkgs.codex ];
      environment = {
        PROPHET_PILOT_SOCKET = pilotSocket;
        PROPHET_PILOT_CLIENT = "agentd";
        PROPHET_PILOT_GROUP = "prophet-system";
        PROPHET_MCP_BRIDGE = "${prophet}/bin/prophet-mcp";
      };
      unitConfig = { StartLimitIntervalSec = 60; StartLimitBurst = 3; ConditionUser = human; };
      serviceConfig = {
        ExecStart = "${prophet}/bin/prophet-pilotd";
        Restart = "on-failure";
        RestartSec = "3s";
        UMask = "0007";
      };
    };
    systemd.services.prophet-agentd.environment.PROPHET_PILOT_SOCKET = pilotSocket;
    systemd.user.services.prophet-supervision = {
      description = "Prophet OS — supervision humaine";
      wantedBy = [ "sway-session.target" ];
      partOf = [ "sway-session.target" ];
      after = [ "graphical-session-pre.target" ];
      environment = lib.optionalAttrs config.prophet.localEngine.enable {
        PROPHET_MODEL_ENDPOINT = "http://127.0.0.1:${toString config.prophet.localEngine.port}/v1";
      };
      unitConfig = { StartLimitIntervalSec = 60; StartLimitBurst = 3; ConditionUser = human; };
      serviceConfig = {
        ExecStart = "${prophet}/bin/prophet-surface --fenetree";
        Restart = "on-failure";
        RestartSec = "3s";
        UMask = "0077";
      };
    };
    environment.etc."prophet/waybar.json".text = builtins.toJSON {
      layer = "top";
      position = "top";
      height = 40;
      modules-left = [ "custom/prophet" "sway/workspaces" ];
      modules-center = [ "sway/window" ];
      modules-right = [ "clock" "custom/lock" ];
      "custom/prophet" = { format = "◈  Prophet"; tooltip-format = "Applications · Super + Espace"; on-click = "${launcher}/bin/prophet-ouvrir"; };
      "sway/workspaces" = { disable-scroll = true; format = "{name}"; };
      "sway/window" = { max-length = 36; separate-outputs = true; };
      clock = { format = "{:%a %d · %H:%M}"; tooltip-format = "{:%A %d %B %Y}"; };
      "custom/lock" = { format = "Verrouiller"; on-click = "${launcher}/bin/prophet-ouvrir verrouiller"; };
    };
    environment.etc."prophet/waybar.css".text = ''
      * { font-family: Inter, sans-serif; font-size: 12px; min-height: 0; }
      window#waybar { background: #202630; color: #e9edf3; border-bottom: 1px solid #353d48; }
      #custom-prophet { font-weight: 700; padding: 0 22px; color: #ffffff; }
      #workspaces button { border: 0; border-radius: 8px; margin: 6px 2px; padding: 0 10px; color: #aeb8c6; }
      #workspaces button.focused { background: #3d4757; color: #ffffff; }
      #workspaces button:hover { background: #323b49; box-shadow: none; }
      #window { color: #adb7c5; }
      #clock { padding: 0 16px; }
      #custom-lock { padding: 0 18px; color: #cad2dd; }
      tooltip { background: #252c38; border-radius: 10px; border: 1px solid #475163; }
    '';
    environment.etc."xdg/fuzzel/fuzzel.ini".text = ''
      [main]
      font=Inter:size=15
      width=36
      lines=8
      horizontal-pad=24
      vertical-pad=20
      inner-pad=14
      [colors]
      background=f8fafcff
      text=242c38ff
      match=566d93ff
      selection=dfe6efff
      selection-text=1c2635ff
      border=cbd4e0ff
      [border]
      width=1
      radius=20
    '';
    environment.etc."xdg/foot/foot.ini".text = ''
      [main]
      font=DejaVu Sans Mono:size=12
      pad=20x16
      [colors-dark]
      background=181e28
      foreground=e5ebf3
      regular4=90aed7
      regular2=a8c5b0
    '';
  };
}
