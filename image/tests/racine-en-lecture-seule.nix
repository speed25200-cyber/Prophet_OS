# Une question ouverte, posée à la machine plutôt que raisonnée.
#
# `immutable.nix` déclare `fileSystems."/"` avec l'option `ro`. Personne n'a jamais démarré un
# système Prophet OS avec cette option effectivement appliquée : le test `installe.nix` ne le peut
# pas, parce que le cadre de test NixOS redéfinit `fileSystems` à une priorité qui l'emporte.
#
# Ce fichier force la racine en lecture seule là où le cadre la pose — `virtualisation.fileSystems`
# — et regarde ce qui arrive. Ce qui est en jeu tient en une phrase : l'activation de NixOS écrit
# `/etc/passwd`, `/etc/shadow`, `/etc/group` et tout l'arbre de liens de `/etc` à **chaque**
# démarrage, et crée des répertoires sous `/var`. `immutable.nix` monte `/home` et
# `/var/lib/prophet` depuis des volumes séparés ; `/etc`, `/var/log`, `/var/lib` et `/tmp` restent
# sur la racine.
#
# Si l'activation échoue, la machine part en mode de secours — et `systemd-boot` est configuré sans
# éditeur, donc il n'y a pas de mode de secours utilisable. Sur un PC dont on vient d'effacer le
# disque, cela ne se rattrape pas.
#
# ## Pourquoi ce test peut échouer sans que rien ne soit cassé
#
# Il est une **question**, pas une garantie. Son travail d'intégration continue est en
# `continue-on-error` et ne barre pas la route : un échec ici ne veut pas dire qu'une régression
# vient d'apparaître, il veut dire que la réponse est « non » et qu'on la connaît enfin.
#
# La correction, si la réponse est « non », n'est pas un réglage mais une décision de conception.
# La piste que NixOS documente pour ce cas précis : `system.etc.overlay` (qui exige l'initrd
# systemd, déjà activé ici), un `/var` porté par un volume inscriptible plutôt que par la racine,
# et `boot.tmp.useTmpfs`. Elle se prendra en la prenant, pas en l'essayant sur la machine de
# quelqu'un.
{ pkgs, module }:

pkgs.testers.runNixOSTest {
  name = "prophet-racine-en-lecture-seule";

  nodes.machine = { lib, ... }: {
    imports = [
      module
      ../modules/immutable.nix
    ];
    prophet.enable = true;
    prophet.motDePasseHache = null;
    users.users.prophet.password = "essai-prophet";

    # Le cœur de l'expérience. `virtualisation.fileSystems` est l'endroit où le cadre de test pose
    # la racine ; c'est donc là qu'il faut écrire pour que l'option survive.
    virtualisation.fileSystems."/".options = lib.mkForce [ "ro" ];
    virtualisation.memorySize = 2048;
    virtualisation.diskSize = 4096;

    boot.initrd.luks.devices = lib.mkForce { };
    networking.networkmanager.enable = lib.mkForce false;
  };

  testScript = ''
    # `wait_for_unit` attendrait le délai entier sans rien dire de ce qui bloque. On regarde
    # d'abord si la machine arrive quelque part, puis on raconte où.
    machine.start()
    try:
        machine.wait_for_unit("multi-user.target", timeout=180)
        atteint = True
    except Exception as erreur:
        atteint = False
        print(f"multi-user.target n'a pas été atteint : {erreur}")

    print("--- ce que la machine dit d'elle-même ---")
    for commande in [
        "findmnt -no OPTIONS /",
        "systemctl --failed --no-legend --plain",
        "systemctl status systemd-tmpfiles-setup.service --no-pager -l",
        "journalctl -b --no-pager -p err | tail -40",
        "getent passwd prophet",
    ]:
        sortie = machine.execute(f"{commande} 2>&1 | head -40")[1]
        print(f"$ {commande}\n{sortie}")

    assert atteint, (
        "la racine en lecture seule empêche le démarrage. La réponse à la question posée en "
        "tête de ce fichier est « non » : il faut un /etc et un /var inscriptibles avant que "
        "`immutable.nix` puisse tenir sa promesse."
    )

    with subtest("et si elle démarre, elle est utilisable"):
        monte = machine.succeed("findmnt -no OPTIONS /").strip()
        assert monte.split(",")[0] == "ro", (
            f"l'expérience n'a pas eu lieu : la racine est en {monte}"
        )
        machine.succeed("getent shadow prophet")
        for nom in ["capd", "ledger", "agentd"]:
            machine.wait_for_unit(f"prophet-{nom}.service")
  '';
}
