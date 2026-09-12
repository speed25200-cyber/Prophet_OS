# Compatibilité graphique du paquet officiel dans NixOS. Aucun compte connecté.
# La connexion automatique et le rendu logiciel sont propres à cette VM de test.
{ pkgs, chatgpt }:

pkgs.testers.runNixOSTest {
  name = "prophet-chatgpt-desktop";
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
        exec sway
      fi
    '';
    environment.variables = {
      SWAYSOCK = "/tmp/sway-ipc.sock";
      WLR_RENDERER = "pixman";
    };
    environment.systemPackages = [ chatgpt pkgs.jq pkgs.iptables ];
    fonts.packages = [ pkgs.dejavu_fonts ];
    virtualisation.memorySize = 4096;
    virtualisation.diskSize = 4096;
    virtualisation.qemu.options = [ "-vga none -device virtio-gpu-pci" ];
  };
  testScript = ''
    import shlex

    machine.start()
    machine.wait_for_file("/tmp/sway-ipc.sock")
    # Le test porte sur l'ouverture locale, pas sur les services cloud. Le canal du pilote
    # utilise le port série de la VM ; aucun accès Internet n'est nécessaire au client.
    machine.succeed("iptables -I OUTPUT ! -o lo -j REJECT")
    machine.succeed("ip6tables -I OUTPUT ! -o lo -j REJECT")

    with subtest("le client officiel ouvre une fenêtre sous XWayland sans désactiver sa sandbox"):
        launch = "${chatgpt}/bin/chatgpt --ozone-platform=x11 > /tmp/chatgpt-startup.log 2>&1"
        machine.succeed("su - tester -c " + shlex.quote("swaymsg exec " + shlex.quote(launch)))
        tree = "su - tester -c 'swaymsg -t get_tree'"
        selector = '.. | objects | select(.window_properties?.class? == "ChatGPT" or .window_properties?.class? == "chatgpt" or .app_id? == "chatgpt")'
        try:
            machine.wait_until_succeeds(tree + " | jq -e " + shlex.quote(selector), timeout=120)
            machine.sleep(3)
            machine.screenshot("chatgpt-xwayland")
        finally:
            print(machine.succeed("cat /tmp/chatgpt-startup.log"))

    with subtest("le processus appartient au compte ordinaire"):
        machine.succeed("pgrep -u 1000 -f '/chatgpt/ChatGPT'")
        machine.fail("pgrep -u 0 -f '/chatgpt/ChatGPT'")
        machine.succeed("su - tester -c " + shlex.quote("swaymsg '[class=\"(?i)chatgpt\"] kill'"))
  '';
}
