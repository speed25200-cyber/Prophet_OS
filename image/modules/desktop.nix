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
Codex
Navigateur
X
Fichiers
Terminal
Verrouiller
Déconnexion'
      if [ "$app" = --liste ]; then
        printf '%s\n' "$entrees"
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
          Fichiers) app=fichiers ;; Terminal) app=terminal ;;
          Navigateur) app=navigateur ;; X) app=x ;;
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
        claude-code)
          swaymsg 'workspace "2: Atelier"' >/dev/null
          swaymsg 'layout tabbed' >/dev/null
          focus org.prophet.ClaudeCode && exit 0
          profile="$HOME/.local/state/prophet/providers/claude-code/$human"
          install -d -m 0700 -- "$profile"
          launch foot --hold --working-directory="$HOME/Documents/Prophet" --app-id=org.prophet.ClaudeCode --title='Claude Code' \
            env CLAUDE_CONFIG_DIR="$profile" ${pkgs.claude-code}/bin/claude
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
    runtimeInputs = [ compositor ];
    text = ''
      if [ "$(id -un)" != ${lib.escapeShellArg human} ]; then
        echo "Cette session appartient au propriétaire configuré de la machine." >&2
        exit 1
      fi
      umask 077
      exec sway --config /etc/prophet/sway.conf
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
      launcher session chatgpt chromium pkgs.foot pkgs.thunar pkgs.wl-clipboard
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
