# Compatibilité graphique du paquet officiel dans NixOS. Aucun compte connecté.
# La connexion automatique et le rendu logiciel sont propres à cette VM de test.
{ pkgs, chatgpt }:

pkgs.testers.runNixOSTest {
  name = "prophet-chatgpt-desktop";
  enableOCR = true;
  nodes.machine = { ... }: {
    users.users.tester = {
      isNormalUser = true;
      uid = 1000;
      password = "compte-vm-uniquement";
      extraGroups = [ "video" ];
    };
    services.getty.autologinUser = "tester";
    programs.sway.enable = true;
    programs.sway.xwayland.enable = true;
    programs.bash.loginShellInit = ''
      if [ "$(tty)" = /dev/tty1 ]; then
        # Une tentative de démarrage avortée peut laisser ce socket. Sway choisit alors
        # un autre chemin ; le pilote doit retrouver celui de la session relancée.
        rm -f -- /run/user/1000/prophet-sway.sock
        exec sway --debug >> /tmp/sway-startup.log 2>&1
      fi
    '';
    environment.variables = {
      SWAYSOCK = "/run/user/1000/prophet-sway.sock";
      WLR_RENDERER = "pixman";
    };
    environment.systemPackages = [ chatgpt pkgs.jq pkgs.iptables ];
    fonts.packages = [ pkgs.dejavu_fonts ];
    virtualisation.memorySize = 4096;
    virtualisation.cores = 2;
    virtualisation.diskSize = 4096;
    virtualisation.qemu.options = [ "-vga none -device virtio-gpu-pci" ];
  };
  testScript = ''
    import json
    import shlex
    from datetime import timedelta

    machine.start()
    machine.wait_for_unit("multi-user.target")
    try:
        machine.wait_until_succeeds("su - tester -c 'swaymsg -t get_outputs' | jq -e '[.[] | select(.active)] | length > 0'", timeout=timedelta(seconds=120))
    except Exception:
        print(machine.execute("tail -100 /tmp/sway-startup.log"))
        print(machine.execute("id tester; ls -l /dev/dri"))
        machine.screenshot("sway-echec")
        raise
    # Le test porte sur l'ouverture locale, pas sur les services cloud. Le canal du pilote
    # utilise le port série de la VM ; aucun accès Internet n'est nécessaire au client.
    machine.succeed("iptables -I OUTPUT ! -o lo -j REJECT")
    machine.succeed("ip6tables -I OUTPUT ! -o lo -j REJECT")

    with subtest("le client officiel ouvre une fenêtre sous XWayland sans désactiver sa sandbox"):
        launch = "${chatgpt}/bin/chatgpt --ozone-platform=x11 > /tmp/chatgpt-startup.log 2>&1"
        tree = "su - tester -c 'swaymsg -t get_tree'"
        # Classe observée avec le paquet officiel 26.908.40834, et non son nom commercial.
        selector = '.. | objects | select(.window_properties?.class? == "Chatgpt" and .name? == "ChatGPT" and .visible? == true and .shell? == "xwayland")'
        try:
            machine.succeed("su - tester -c " + shlex.quote("swaymsg exec " + shlex.quote(launch)))
            machine.wait_until_succeeds(tree + " | jq -e " + shlex.quote(selector), timeout=timedelta(seconds=120))
            window = json.loads(machine.succeed(tree + " | jq -c " + shlex.quote(selector)))
            client_pid = int(window["pid"])
            assert machine.succeed(f"stat -c %u /proc/{client_pid}").strip() == "1000"
            assert machine.succeed(f"readlink /proc/{client_pid}/exe").strip() == "${chatgpt.payload}/usr/lib/chatgpt/ChatGPT"
            machine.wait_for_text("Sign in to ChatGPT", timeout=timedelta(seconds=90))
            machine.wait_until_succeeds("grep -q 'bundled_plugins_reconcile_completed.*reason=startup' /tmp/chatgpt-startup.log", timeout=timedelta(seconds=60))
            machine.succeed(tree + " | jq -e " + shlex.quote(selector))
            machine.fail("grep -E 'plugin_marketplace_.*failed|bundled_plugins_marketplace_resolve_failed' /tmp/chatgpt-startup.log")
            machine.screenshot("chatgpt-xwayland")
        except Exception:
            machine.screenshot("chatgpt-echec")
            raise
        finally:
            print(machine.execute(tree + " | jq " + shlex.quote('.. | objects | select(.pid?) | {name, pid, visible, shell, window_properties}')))
            print(machine.execute("tail -100 /tmp/chatgpt-startup.log"))
            print(machine.execute("tail -100 /tmp/sway-startup.log"))

    with subtest("le processus appartient au compte ordinaire"):
        machine.succeed("pgrep -u 1000 -f '/chatgpt/ChatGPT'")
        machine.fail("pgrep -u 0 -f '/chatgpt/ChatGPT'")
        machine.succeed("su - tester -c " + shlex.quote(f"swaymsg '[con_id={int(window['id'])}] kill'"))
        closed = f"[.. | objects | select(.id? == {int(window['id'])})] | length == 0"
        machine.wait_until_succeeds(tree + " | jq -e " + shlex.quote(closed), timeout=timedelta(seconds=30))

    # Le défaut connu du renderer secondaire reste bloquant. Ce contrôle vient après
    # l'écran lisible, les plugins et la fermeture afin de conserver leurs preuves,
    # même lorsque cette dernière condition de livraison échoue.
    with subtest("aucune erreur de configuration des polices au démarrage"):
        machine.fail("grep -q 'Fontconfig error' /tmp/chatgpt-startup.log")
  '';
}
