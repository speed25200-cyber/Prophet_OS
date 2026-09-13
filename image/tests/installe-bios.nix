# La configuration installée démarre-t-elle sans UEFI ?
#
# Un PC de 2012 n'a parfois qu'un BIOS. L'installeur y pose GRUB plutôt que systemd-boot
# (ADR 0032) ; ce test démarre cette configuration sous SeaBIOS, avec la partition d'amorçage
# séparée que le cadre de test sait créer, et vérifie que les services y tournent comme sous UEFI.
# Le déverrouillage LUKS et le bureau sont exercés par les autres tests ; ici, seule la voie
# d'amorçage change, et c'est elle qu'on regarde.
{ pkgs, module }:
pkgs.testers.runNixOSTest {
  name = "prophet-installe-bios";
  nodes.machine = { lib, ... }: {
    imports = [ module ../modules/hardware.nix ../modules/immutable.nix ];
    prophet.enable = true;
    prophet.motDePasseHache = null;
    users.users.prophet.password = "essai-prophet";
    prophet.boot.firmware = "bios";
    # Le cadre de test impose son propre disque à GRUB ; la valeur ne sert qu'à l'assertion.
    prophet.boot.disque = "/dev/vda";
    virtualisation.useBootLoader = true;
    virtualisation.useEFIBoot = false;
    virtualisation.useBIOSBoot = true;
    virtualisation.memorySize = 2048;
    virtualisation.diskSize = 8192;
    boot.initrd.luks.devices = lib.mkForce { };
    networking.networkmanager.enable = lib.mkForce false;
  };
  testScript = ''
    machine.wait_for_unit("multi-user.target")
    # Pas d'UEFI : le micrologiciel n'a rien exposé, et c'est GRUB qui a démarré.
    machine.fail("test -d /sys/firmware/efi")
    machine.succeed("test -s /boot/grub/grub.cfg")
    machine.fail("test -d /boot/loader/entries")
    # Le durcissement du noyau passe par GRUB comme par systemd-boot.
    machine.succeed("grep -q lockdown=integrity /proc/cmdline")
    machine.succeed("grep -q amdgpu.si_support=1 /proc/cmdline")
    for nom in ["capd", "ledger", "vault", "egress", "sandboxd", "memoryd", "agentd"]:
        machine.wait_for_unit(f"prophet-{nom}.service")
    print(machine.succeed("cat /proc/cmdline"))
  '';
}
