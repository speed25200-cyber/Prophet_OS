# Le système **installé** démarre-t-il ?
#
# Ce n'est pas la même question que celle du support d'amorçage, qui a démarré (M9-T6). Le support
# est une configuration à part : racine en lecture-écriture sur un disque virtuel, session ouverte
# automatiquement, ni chiffrement ni chargeur d'amorçage posé sur disque. Ce que l'on installe est
# autre chose, et n'avait jamais été démarré — seulement *construit*.
#
# Entre les deux, `immutable.nix` ajoute tout ce qui peut empêcher une machine de démarrer :
#
# - une racine montée en **lecture seule**, alors que l'activation de NixOS écrit `/etc/passwd`,
#   `/etc/shadow` et `/etc/group` à chaque démarrage ;
# - `systemd-boot` **sans éditeur**, donc sans ligne de commande de secours ;
# - `lockdown=integrity` et `module.sig_enforce=1`, qui peuvent refuser de charger des modules ;
# - un initrd systemd, et `boot-complete.target` comme condition de bascule.
#
# Chacun de ces points peut produire une machine qui ne démarre pas, et aucun ne se voit en
# construisant. Ce test démarre donc la configuration installée **par son chargeur d'amorçage**,
# depuis un vrai disque, en UEFI.
#
# Ce qu'il ne reproduit pas : les volumes chiffrés et les étiquettes de partition, que le cadre de
# test remplace par son propre disque. C'est l'installeur, exercé ailleurs sur un disque en boucle,
# qui répond de la disposition.
{ pkgs, module }:

pkgs.testers.runNixOSTest {
  name = "prophet-installe";

  nodes.machine = { lib, ... }: {
    imports = [
      module
      ../modules/hardware.nix
      ../modules/immutable.nix
      # La surface fait partie de ce qu'on installe : `nixosConfigurations.prophet` l'inclut.
      # L'omettre ici reviendrait à tester une machine que personne ne recevra.
      ../modules/surface.nix
    ];
    prophet.enable = true;

    # Le fichier de haché est posé par l'installeur sur une machine réelle. Ici, un mot de passe
    # en clair : ce qui est vérifié est qu'une session s'ouvre, pas la qualité du secret.
    prophet.motDePasseHache = null;
    users.users.prophet.password = "essai-prophet";

    # Le chargeur d'amorçage, pour de vrai. Sans cela, le cadre de test démarre le noyau
    # directement et ne prouve rien de `systemd-boot` ni des paramètres du noyau.
    virtualisation.useBootLoader = true;
    virtualisation.useEFIBoot = true;
    virtualisation.memorySize = 3072;
    virtualisation.diskSize = 8192;

    # Les volumes chiffrés de `immutable.nix` désignent des étiquettes qui n'existent pas sur le
    # disque du test : l'initrd les attendrait indéfiniment. Ce qu'on vérifie ici est ce qui vient
    # après le déverrouillage, et le déverrouillage lui-même relève de l'installeur.
    boot.initrd.luks.devices = lib.mkForce { };

    # NetworkManager prend la main sur les interfaces du cadre de test, qui les configure
    # lui-même. Les deux ensemble font attendre `network-online.target` sans rien apprendre.
    networking.networkmanager.enable = lib.mkForce false;
  };

  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")

    with subtest("la machine a démarré par son chargeur d'amorçage"):
        # `systemd-boot` pose cette variable EFI. Sa présence distingue « le noyau tourne » de
        # « le chargeur d'amorçage installé l'a lancé », qui sont deux choses différentes et dont
        # seule la seconde est en question ici.
        info = machine.succeed("bootctl status || true")
        print(info)
        assert "systemd-boot" in info, f"le chargeur attendu n'a pas démarré :\n{info}"

    with subtest("la racine est bien en lecture seule"):
        # Le cœur du sujet. Si elle ne l'est pas, ce test ne vérifie pas ce qu'il prétend, et il
        # vaut mieux le savoir que de croire l'avoir vérifié.
        monte = machine.succeed("findmnt -no OPTIONS /").strip()
        print(f"/ : {monte}")
        assert monte.split(",")[0] == "ro", f"/ devrait être montée en lecture seule : {monte}"
        machine.fail("touch /essai-ecriture-racine")

    with subtest("l'activation de NixOS a pu écrire les comptes malgré cela"):
        # NixOS écrit `/etc/passwd`, `/etc/shadow` et `/etc/group` à chaque démarrage. Sur une
        # racine en lecture seule, cette écriture échoue — et le service qui la porte échoue avec
        # elle. On regarde donc le résultat, pas l'intention.
        machine.succeed("id prophet")
        machine.succeed("getent shadow prophet")
        etat = machine.succeed("passwd -S prophet").strip()
        print(etat)
        assert etat.split()[1] == "P", f"le compte doit avoir un mot de passe utilisable : {etat}"

    with subtest("aucune unité n'a échoué au démarrage"):
        # La question que personne n'a encore posée à cette configuration.
        echecs = machine.succeed("systemctl --failed --no-legend --plain || true").strip()
        print(echecs if echecs else "(aucune)")
        # La surface n'est pas de la partie : elle exige un adaptateur graphique.
        restantes = [
            ligne for ligne in echecs.splitlines()
            if ligne.strip() and "prophet-surface" not in ligne
        ]
        assert not restantes, "des unités ont échoué :\n" + "\n".join(restantes)

    with subtest("les sept services tournent sur la machine installée"):
        for nom in ["capd", "ledger", "vault", "egress", "sandboxd", "memoryd", "agentd"]:
            machine.wait_for_unit(f"prophet-{nom}.service")
            machine.succeed(f"test -S /run/prophet/{nom}.sock")

    with subtest("la surface a au moins été tentée"):
        # Sans écran ni adaptateur graphique, elle échouera : c'est attendu, et le service de repli
        # est là pour ça. Ce qui est vérifié ici est autre chose, et personne ne l'a jamais vérifié
        # — qu'elle soit **lancée**.
        #
        # `wantedBy = [ "graphical.target" ]` suppose que cette cible est atteinte. Sur cette
        # machine, `services.xserver.enable` vaut `false` et il n'y a pas de gestionnaire de
        # session : la cible par défaut est `multi-user.target`. Si c'est bien le cas, la surface
        # n'est jamais démarrée du tout, et l'écran d'un PC fraîchement installé montre une invite
        # de connexion au lieu de ce que la machine existe pour montrer.
        #
        # « jamais tentée » et « tentée et échouée » se ressemblent quand on regarde l'écran. Elles
        # ne se ressemblent pas du tout quand on cherche pourquoi.
        cible = machine.succeed("systemctl get-default").strip()
        print(f"cible par défaut : {cible}")
        etat = machine.succeed("systemctl show -p ActiveState -p Result --value prophet-surface.service || true").strip()
        print(f"prophet-surface : {etat}")
        journal = machine.succeed("journalctl -u prophet-surface.service --no-pager | tail -30 || true")
        print(journal)
        assert "inactive" not in etat.splitlines()[0], (
            "la surface n'a jamais été lancée — elle est voulue par « graphical.target », "
            f"et la cible atteinte est « {cible} »"
        )

    with subtest("le propriétaire ouvre une session et voit sa machine"):
        # Le test qui compte pour celui qui installera ce système sur son PC : il tape son
        # identifiant, son mot de passe, et il est devant quelque chose d'utilisable.
        machine.wait_for_unit("getty@tty1.service")
        machine.wait_until_tty_matches("1", "login:")
        machine.send_chars("prophet\n")
        machine.wait_until_tty_matches("1", "[Pp]assword:")
        machine.send_chars("essai-prophet\n")
        machine.wait_until_tty_matches("1", r"\$|prophet@")

        statut = machine.succeed("su - prophet -c 'timeout 60 prophet status'")
        print(statut)
        assert "muet" not in statut, f"aucun service ne devrait être muet :\n{statut}"

    with subtest("le noyau tourne avec les paramètres demandés"):
        # `lockdown=integrity` et `module.sig_enforce=1` sont posés par `immutable.nix`. Un
        # paramètre que le noyau ignore ne protège rien, et le croire posé est pire que de savoir
        # qu'il ne l'est pas.
        ligne = machine.succeed("cat /proc/cmdline").strip()
        print(ligne)
        for parametre in ["lockdown=integrity", "module.sig_enforce=1"]:
            assert parametre in ligne, f"{parametre} absent de la ligne de commande :\n{ligne}"
        verrou = machine.succeed("cat /sys/kernel/security/lockdown 2>/dev/null || echo absent").strip()
        print(f"lockdown : {verrou}")
  '';
}
