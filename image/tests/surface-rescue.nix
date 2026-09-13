# Panne graphique injectée ; le secours et getty sont ceux des modules de production.
# Ce test court ne construit ni le client graphique ni les daemons.
{ pkgs, module }:
pkgs.testers.runNixOSTest {
  name = "prophet-surface-rescue";
  nodes.machine = { lib, ... }: {
    imports = [ module ../modules/surface.nix ];
    prophet.enable = false;
    prophet.localEngine.enable = false;
    users.groups.prophet-system = { };
    users.users.tester = { isNormalUser = true; password = "essai-secours"; };
    systemd.services.prophet-surface.serviceConfig = {
      ExecStart = lib.mkForce "${pkgs.coreutils}/bin/false";
      Restart = lib.mkForce "no";
    };
    virtualisation.memorySize = 1024;
  };
  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")
    machine.wait_until_succeeds("systemctl is-failed prophet-surface.service", timeout=30)
    machine.wait_for_unit("getty@tty1.service")

    with subtest("le diagnostic tardif laisse une invite utilisable"):
        machine.succeed("systemctl start prophet-surface-repli.service")
        machine.wait_until_tty_matches("1", "journalctl -u prophet-surface", timeout=30)
        machine.wait_until_tty_matches("1", "login:", timeout=30)
        machine.screenshot("secours-et-connexion")

    with subtest("une nouvelle panne ne coupe pas la saisie du mot de passe"):
        pid = machine.succeed("systemctl show -p MainPID --value getty@tty1.service").strip()
        machine.send_chars("tester\n")
        machine.wait_until_tty_matches("1", "[Pp]assword:", timeout=30)
        machine.succeed("systemctl start prophet-surface-repli.service")
        assert machine.succeed("systemctl show -p MainPID --value getty@tty1.service").strip() == pid
        machine.send_chars("essai-secours\n")
        machine.wait_until_tty_matches("1", r"\$|tester@", timeout=30)
        machine.send_chars("printf 'SESSION-HUMAINE-OK\\n'\n")
        machine.wait_until_tty_matches("1", "SESSION-HUMAINE-OK", timeout=30)

    with subtest("le secours ne touche pas une session déjà ouverte"):
        machine.succeed("systemctl start prophet-surface-repli.service")
        machine.send_chars("id -un > /home/tester/session-owner\n")
        machine.wait_until_succeeds("test -s /home/tester/session-owner", timeout=30)
        assert machine.succeed("cat /home/tester/session-owner").strip() == "tester"
        machine.screenshot("session-apres-secours")
  '';
}
