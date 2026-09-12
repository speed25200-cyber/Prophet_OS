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
# ## Ce qu'il ne vérifie pas, et qu'il ne faut pas croire vérifié
#
# Le cadre de test NixOS fournit son propre disque et redéfinit `fileSystems` à une priorité qui
# l'emporte sur celle de `immutable.nix`. Deux choses lui échappent donc :
#
# - **la racine en lecture seule** — elle est en lecture-écriture ici. Le sous-test le dit à voix
#   haute plutôt que d'affirmer le contraire ;
# - **les volumes chiffrés et les étiquettes de partition** — c'est l'installeur, exercé sur un
#   disque en boucle, qui en répond.
#
# Tout le reste — chargeur d'amorçage, paramètres du noyau, comptes, services, session — est bien
# celui de la configuration installée.
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

    with subtest("ce que ce test ne vérifie PAS : la racine en lecture seule"):
        # À dire franchement, plutôt que de laisser croire le contraire.
        #
        # `immutable.nix` déclare `fileSystems."/"` avec l'option `ro`, en `mkDefault`. Le cadre de
        # test NixOS fournit son propre disque et redéfinit `fileSystems` à une priorité qui gagne
        # (`mkVMOverride`). La racine est donc **en lecture-écriture** ici, quoi qu'en dise la
        # configuration installée.
        #
        # Affirmer « racine en lecture seule vérifiée » sur cette base serait la faute que ce dépôt
        # traque partout ailleurs : une sonde qui constate autre chose que ce qu'elle annonce. La
        # question reste donc ouverte, et elle est notée comme telle dans `docs/STATUS.md`.
        monte = machine.succeed("findmnt -no OPTIONS /").strip()
        print(f"/ (imposée par le cadre de test) : {monte}")
        if monte.split(",")[0] == "ro":
            print("racine en lecture seule : le cadre de test ne l'a pas remplacée, tant mieux")
        else:
            print(
                "racine en lecture-écriture : le cadre de test a remplacé le montage déclaré. "
                "Ce que `immutable.nix` demande n'est donc pas exercé ici."
            )

    with subtest("l'activation de NixOS a écrit les comptes"):
        # Ce que l'activation fait à chaque démarrage : écrire `/etc/passwd`, `/etc/shadow` et
        # `/etc/group`. On regarde le résultat, pas l'intention.
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
        # Elle est voulue par `graphical.target`, qui n'est pas la cible par défaut de cette
        # machine — `services.xserver.enable` vaut `false`, il n'y a pas de gestionnaire de
        # session, et la cible par défaut est `multi-user.target`. Une ligne de `surface.nix`,
        # quatre-vingt-dix lignes plus bas que le service, rattache `graphical.target` à
        # `multi-user.target` et rend l'ensemble cohérent.
        #
        # Les deux ne tiennent qu'ensemble, et rien à la construction ne le dit : retirer le
        # rattachement laisserait l'écran noir sans une ligne de journal. « Jamais lancée » et
        # « lancée et en échec » se ressemblent quand on regarde un écran noir ; elles ne se
        # ressemblent pas du tout quand on cherche pourquoi. C'est cette assertion qui tient la
        # différence.
        cible = machine.succeed("systemctl get-default").strip()
        print(f"cible par défaut : {cible}")
        etat = machine.succeed("systemctl show -p ActiveState -p Result --value prophet-surface.service || true").strip()
        print(f"prophet-surface : {etat}")
        journal = machine.succeed("journalctl -u prophet-surface.service --no-pager | tail -30 || true")
        print(journal)
        assert "inactive" not in etat.splitlines()[0], (
            "la surface n'a jamais été lancée : elle doit être voulue par une cible que cette "
            f"machine atteint, et la cible par défaut est « {cible} »"
        )

    with subtest("le propriétaire ouvre une session et voit sa machine"):
        # Le test qui compte pour celui qui installera ce système sur son PC : il tape son
        # identifiant, son mot de passe, et il est devant quelque chose d'utilisable.
        #
        # On attend d'abord que la surface ait fini de se débattre. Elle réclame `/dev/tty1` avec
        # `TTYVHangup`, donc chacune de ses tentatives raccroche le terminal — et il n'y a pas
        # d'adaptateur graphique ici, donc elle en fait cinq avant de renoncer. Se connecter
        # pendant ce temps échouerait pour une raison qui n'a rien à voir avec la connexion.
        #
        # Ce n'est pas qu'un artefact de test : sur une machine réelle dont le pilote graphique
        # refuse, l'invite de connexion du propriétaire serait raccrochée cinq fois en une minute.
        # C'est noté dans `docs/components/surface.md`.
        machine.wait_until_succeeds(
            "test \"$(systemctl show -p ActiveState --value prophet-surface.service)\" "
            "!= activating",
            timeout=120,
        )
        etat_final = machine.succeed(
            "systemctl show -p ActiveState --value prophet-surface.service"
        ).strip()
        print(f"la surface s'est arrêtée sur : {etat_final}")

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
