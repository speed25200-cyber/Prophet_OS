# Les sept services démarrent-ils vraiment, sous systemd, avec leur durcissement ?
#
# Les tests Rust lancent chaque binaire à la main, dans un répertoire temporaire, sous le compte de
# celui qui teste. C'est utile, et ce n'est pas la même chose : sur la machine installée, chaque
# daemon tourne sous son propre utilisateur, avec `ProtectSystem = "strict"`, un
# `SystemCallFilter`, des capacités retirées, et un socket en 0660 dans un répertoire en 0750.
#
# Tout cela peut échouer sans qu'aucun test Rust ne s'en aperçoive. C'est d'ailleurs la forme
# exacte du défaut qui a occupé cette journée : sept unités déclarées vers des programmes
# inexistants, invisibles jusqu'au démarrage d'une vraie machine.
#
# Ce test démarre une vraie machine.
{ pkgs, module }:

let
  # De quoi parler aux daemons depuis la machine de test. La CLI ne sait pas encore créer une
  # tâche ; ce petit client fait l'appel que `agentd` attend, et rien de plus.
  essai = pkgs.writeShellScriptBin "prophet-essai-tache" ''
    exec ${pkgs.python3}/bin/python3 - "$@" <<'PYTHON'
    import json, socket, sys

    def appeler(chemin, methode, params):
        s = socket.socket(socket.AF_UNIX)
        s.connect(chemin)
        s.sendall((json.dumps({
            "jsonrpc": "2.0", "id": 1, "method": methode, "params": params
        }) + "\n").encode())
        reponse = json.loads(s.makefile().readline())
        if "error" in reponse:
            print(json.dumps(reponse["error"]), file=sys.stderr)
            raise SystemExit(1)
        return reponse["result"]

    manifeste = {
        "agent": {
            "id": "org.essai.vm",
            "version": "1.0.0",
            "name": "Essai en machine virtuelle",
            "publisher_key": "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        },
        "model": {"preferred": ["local:qwen3-8b"]},
        "capabilities": {"max": {"fs.read": ["~/essai/**"]}},
    }

    if len(sys.argv) > 1 and sys.argv[1] == "sandbox":
        caps = appeler("/run/prophet/sandboxd.sock", "sandbox.capabilities", {})
        print(json.dumps(caps, ensure_ascii=False, indent=2))
        niveau = caps["max_level"]
        if niveau < 0:
            raise SystemExit(0)
        # On demande le niveau que la machine dit tenir, et rien de plus : demander plus haut
        # testerait le refus, qui l'est déjà ailleurs.
        lancee = appeler("/run/prophet/sandboxd.sock", "sandbox.start", {
            "task": "task:essai-sandbox",
            "spec": {
                "level": niveau,
                "program": "/run/current-system/sw/bin/true",
                "args": [],
                "workdir": "/tmp",
                "env": [],
                "rules": {"paths": [], "egress": [], "exec": [], "min_sandbox_level": 0},
                "read_only_mounts": ["/nix/store", "/run/current-system/sw"],
            },
        })
        print(json.dumps(lancee, ensure_ascii=False))
        raise SystemExit(0)

    if len(sys.argv) > 1 and sys.argv[1] == "options":
        print(json.dumps(appeler("/run/prophet/agentd.sock", "task.options", {}), ensure_ascii=False))
        raise SystemExit(0)

    plan = appeler("/run/prophet/agentd.sock", "task.spawn", {
        "id": "task:essai-vm",
        "intent": "vérifier que la chaîne tourne sous systemd",
        "user": "prophet",
        "manifest": manifeste,
        "requested": [{"res": "fs", "act": "read", "match": "~/essai/**"}],
        "availability": {"local_models": ["qwen3-8b"]},
    })
    print(json.dumps(plan, ensure_ascii=False, indent=2))
    PYTHON
  '';
in
pkgs.testers.runNixOSTest {
  name = "prophet-services";

  nodes.machine = { ... }: {
    imports = [ module ];
    prophet.enable = true;
    # Le fichier de mot de passe est posé par l'installeur sur une machine réelle ; il n'existe
    # pas ici. On le débranche et on donne un mot de passe au compte, faute de quoi il serait
    # verrouillé et le test ne pourrait rien en dire.
    prophet.motDePasseHache = null;
    users.users.prophet.password = "essai-prophet";
    # Pas de surface : elle exige un écran et un adaptateur graphique, qui n'ont rien à faire ici.
    # Ce qu'on vérifie est ce qui tourne en dessous.
    environment.systemPackages = [ essai ];
    virtualisation.memorySize = 2048;
    virtualisation.diskSize = 4096;
  };

  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")

    services = ["capd", "ledger", "vault", "egress", "sandboxd", "memoryd", "agentd"]

    with subtest("chaque service démarre et reste debout"):
        for nom in services:
            machine.wait_for_unit(f"prophet-{nom}.service")
        # Attendre l'unité ne suffit pas : un service qui redémarre en boucle passe par « active »
        # à chaque tour. On regarde donc qu'il n'a pas redémarré.
        machine.succeed("sleep 3")
        for nom in services:
            tours = machine.succeed(
                f"systemctl show prophet-{nom}.service -p NRestarts --value"
            ).strip()
            assert tours == "0", f"prophet-{nom} a redémarré {tours} fois"

    with subtest("chaque socket est là, et fermé au reste du monde"):
        for nom in services:
            chemin = f"/run/prophet/{nom}.sock"
            machine.succeed(f"test -S {chemin}")
            mode = machine.succeed(f"stat -c %a {chemin}").strip()
            assert mode == "660", f"{chemin} est en {mode}, attendu 660"
        repertoire = machine.succeed("stat -c %a /run/prophet").strip()
        # 0770 : le groupe doit pouvoir *écrire*, sinon seul le premier service démarré arrive à
        # créer son socket et les six autres échouent sur un « Permission denied ». C'est
        # exactement ce que ce test a trouvé à sa première exécution.
        assert repertoire == "770", f"/run/prophet est en {repertoire}, attendu 770"
        groupe = machine.succeed("stat -c %G /run/prophet").strip()
        assert groupe == "prophet-system", f"/run/prophet appartient à {groupe}"

    with subtest("un utilisateur ordinaire n'atteint pas les sockets"):
        # La règle que `prophet-daemon` porte, vérifiée avec de vrais utilisateurs plutôt qu'avec
        # un `Pairs::explicite` de test. Les droits du répertoire l'arrêtent avant même le daemon,
        # et c'est bien ainsi : deux barrières valent mieux qu'une.
        machine.succeed("useradd -m intrus")
        machine.fail("su intrus -c 'test -r /run/prophet/capd.sock'")

    with subtest("un membre déclaré du groupe système est servi"):
        # L'autre moitié de la même règle, et celle qui manquait. `SO_PEERCRED` n'atteste que le
        # groupe **principal** du pair ; un compte que l'administrateur met dans `prophet-system`
        # par `extraGroups` garde le sien. Comparer ce seul `gid` refusait donc des membres
        # véritables — à commencer par la surface, dont tout le travail est d'afficher ce que les
        # daemons font. L'écran serait resté vide sur une machine parfaitement saine.
        machine.succeed("useradd -m -G prophet-system operateur")
        vu = machine.succeed("su operateur -c 'timeout 30 prophet task ls'")
        assert "n'appartient pas" not in vu, (
            f"un membre déclaré du groupe système doit être servi :\n{vu}"
        )

    with subtest("Codex et Claude Code sont réellement livrés et répondent"):
        import json
        for pilote, programme in [("codex", "codex"), ("claude-code", "claude")]:
            version = machine.succeed(f"timeout 15 {programme} --version").strip()
            assert version, f"{programme} doit annoncer sa version"
            diagnostic = json.loads(machine.succeed(
                f"su - prophet -c 'timeout 20 prophet --json provider doctor {pilote}'"
            ))
            assert diagnostic["executable"], diagnostic
            assert diagnostic["version"], diagnostic
            assert diagnostic["connection"] == "login_required", diagnostic
            assert diagnostic["agent_execution_ready"] is False, diagnostic
            instructions = machine.succeed(
                f"su - prophet -c 'prophet provider login {pilote}'"
            )
            assert "/home/prophet/.local/state/prophet/providers/" in instructions, instructions

    with subtest("ce que `provider ls` annonce est ce que la machine a"):
        annonce = machine.succeed("timeout 30 prophet provider ls")
        print(annonce)
        for pilote, programme in [
            ("claude-code", "claude"),
            ("codex", "codex"),
            ("gemini", "gemini"),
        ]:
            ligne = next(
                (l for l in annonce.splitlines() if l.startswith(pilote + " ")), None
            )
            assert ligne is not None, f"{pilote} devrait figurer dans la liste :\n{annonce}"
            annonce_present = "présent" in ligne
            reellement_present = machine.execute(f"command -v {programme}")[0] == 0
            assert annonce_present == reellement_present, (
                f"{pilote} : la liste dit "
                f"{'présent' if annonce_present else 'absent'}, la machine dit "
                f"{'présent' if reellement_present else 'absent'} — "
                f"annoncer un client qu'on n'a pas envoie l'utilisateur dans le vide"
            )

    with subtest("agentd peut écrire là où sa configuration le prétend"):
        # `ReadWritePaths = [ "/home/prophet" "/var/lib/prophet" ]` est une promesse, et
        # `ProtectHome = true` — hérité du modèle commun — rend `/home` inaccessible et vide dans
        # l'espace de montage du service. Les deux se contredisent en apparence ; c'est systemd qui
        # tranche, et le fichier ne dit pas dans quel sens.
        #
        # Le jour où la promesse serait fausse, une tâche qui ouvre son espace de travail
        # échouerait sur « Read-only file system » ou « No such file or directory », loin d'ici, et
        # personne ne remonterait jusqu'à cette ligne. On regarde donc depuis l'intérieur de
        # l'espace de montage du service, plutôt que depuis la machine.
        pid = machine.succeed(
            "systemctl show -p MainPID --value prophet-agentd.service"
        ).strip()
        for chemin in ["/home/prophet", "/var/lib/prophet"]:
            vu = machine.succeed(
                f"nsenter -t {pid} -m -- sh -c "
                f"'test -d {chemin} && test -w {chemin} && echo inscriptible || echo refusé'"
            ).strip()
            print(f"agentd voit {chemin} : {vu}")
            assert vu == "inscriptible", (
                f"agentd déclare {chemin} en écriture et ne l'a pas : {vu}. "
                "Une tâche qui ouvre son espace de travail échouerait loin d'ici."
            )

    with subtest("le compte humain existe, et peut se servir de la machine"):
        # Sans lui, le système installé n'a aucune session ouvrable : `root` est verrouillé par
        # `nixos-install --no-root-password`, et `systemd-boot` est configuré sans éditeur. Rien
        # ne le montrait, parce que seul le support d'amorçage avait jamais été démarré — et lui
        # ouvre une session automatiquement.
        machine.succeed("id prophet")
        machine.succeed("test -d /home/prophet")
        groupes = machine.succeed("id -nG prophet")
        for groupe in ["wheel", "prophet-system"]:
            assert groupe in groupes, f"prophet devrait être dans {groupe} : {groupes}"
        vu = machine.succeed("su - prophet -c 'timeout 30 prophet task ls'")
        assert "n'appartient pas" not in vu, (
            f"le propriétaire de la machine doit pouvoir voir ses tâches :\n{vu}"
        )
        # Et il doit pouvoir ouvrir une session : un compte sans mot de passe utilisable est un
        # compte qui n'existe pas, du point de vue de celui qui est devant l'écran.
        etat = machine.succeed("passwd -S prophet").strip()
        print(etat)
        assert etat.split()[1] == "P", f"le compte prophet doit avoir un mot de passe : {etat}"

    with subtest("root administre sa machine"):
        # Le refuser ne protégeait rien — `root` lit les clés de signature dans `/var/lib/prophet`
        # — et rendait `prophet status` inutilisable pour le propriétaire de la machine.
        vu = machine.succeed("timeout 30 prophet task ls")
        assert "n'appartient pas" not in vu, f"root doit être servi :\n{vu}"

    with subtest("redémarrer un service n'en coupe pas six autres"):
        # Les sept partagent `/run/prophet`. Sans `RuntimeDirectoryPreserve`, systemd supprime ce
        # répertoire quand l'un s'arrête et emporte les sockets des autres : un
        # `systemctl restart prophet-memoryd` couperait tout le reste. Aucun test de daemon pris
        # isolément ne peut voir cela.
        machine.succeed("systemctl restart prophet-memoryd.service")
        machine.wait_for_unit("prophet-memoryd.service")
        for nom in services:
            machine.succeed(f"test -S /run/prophet/{nom}.sock")
        statut = machine.succeed("timeout 30 prophet status")
        assert "muet" not in statut, (
            f"un redémarrage isolé ne doit rien couper :\n{statut}"
        )

    with subtest("prophet status voit ses services"):
        statut = machine.succeed("timeout 30 prophet status")
        print(statut)
        for nom in services:
            assert f"✓ {nom}" in statut, f"{nom} devrait répondre :\n{statut}"
        assert "muet" not in statut, f"aucun service ne devrait être muet :\n{statut}"

    with subtest("la chaîne complète planifie une tâche"):
        # Le vrai test : capd émet le jeton, agentd planifie, ledger écrit — chacun sous son
        # utilisateur, avec son durcissement, par ses sockets.
        plan = machine.succeed("prophet-essai-tache")
        print(plan)
        assert "task:essai-vm" in plan, plan
        assert "local:qwen3-8b" in plan, plan

        taches = machine.succeed("timeout 30 prophet task ls")
        print(taches)
        assert "task:essai-vm" in taches, taches
        # Les captures demeurent privées même lorsque le propriétaire consulte ses missions.
        machine.fail("su - prophet -c 'ls /home/prophet/.prophet/tasks'")
        taches = machine.succeed("su - prophet -c 'prophet task ls'")
        assert "task:essai-vm" in taches, taches
        detail = machine.succeed("su - prophet -c 'prophet task show task:essai-vm'")
        assert "task:essai-vm" in detail and "local:qwen3-8b" in detail, detail
        # Une tâche seulement planifiée n'a aucun diff à présenter : pas de faux diff vide.
        machine.fail("su - prophet -c 'prophet task diff task:essai-vm'")

    with subtest("le navigateur piloté répond sous le durcissement réel d'agentd"):
        # `agentd` sonde son navigateur au démarrage, sous ses propres contraintes systemd, et
        # `task.options` en rend le verdict. C'est ce qui sépare « un Chromium est dans l'image »
        # de « une mission peut ouvrir une page » : le durcissement commun tue un navigateur de
        # deux façons silencieuses (W^X, SIGSYS sur `setrlimit`), et seule une exécution sous la
        # vraie unité le voit.
        import time
        options = None
        for _ in range(60):
            options = json.loads(machine.succeed("prophet-essai-tache options"))
            if options.get("browser") and options["browser"]["detail"] != "sonde en cours":
                break
            time.sleep(2)
        print(json.dumps(options, ensure_ascii=False, indent=2))
        assert options and options.get("browser"), "agentd ne configure aucun navigateur piloté"
        navigateur = options["browser"]
        assert navigateur["ready"], f"navigateur piloté indisponible : {navigateur['detail']}"
        assert "Chrome" in navigateur["detail"], navigateur
        contextes = {p["id"]: p for p in options["profiles"]}
        assert "web" in contextes, list(contextes)
        assert contextes["web"]["web"] is True, contextes["web"]
        droits = " ".join(contextes["web"]["grants"])
        assert "net.egress sur *" in droits and "ui.act sur browser" in droits, droits
        assert contextes["documents"]["web"] is False, contextes["documents"]
        # Et l'humain le lit sans passer par le socket : `prophet status` porte le verdict.
        statut = machine.succeed("timeout 30 prophet status")
        print(statut)
        assert "Navigateur piloté" in statut and "✓ " in statut.split("Navigateur piloté")[1].splitlines()[1], statut

    with subtest("sandboxd peut réellement isoler, et pas seulement le dire"):
        # Le module donne à `sandboxd` les capacités CAP_SETUID et CAP_SYS_ADMIN, puis lui laisse
        # le filtre d'appels système hérité des autres daemons, qui retire `@privileged` et
        # `@resources`. Accorder une capacité qu'un filtre refuse ensuite est précisément le genre
        # de contradiction qui ne se voit qu'à l'exécution — et que `sandbox.capabilities` ne peut
        # pas signaler, puisqu'il sonde le noyau et non ses propres entraves.
        #
        # On lui demande donc de lancer vraiment quelque chose, au niveau qu'il dit tenir.
        #
        # Ce que la première exécution a rendu, en toutes lettres :
        #     confinement impossible : écriture de uid_map : Operation not permitted
        # Les entraves sont donc imprimées d'abord. Sans elles, un échec ne dit que « refusé »,
        # et il y a au moins quatre réglages capables de produire ce refus — chercher lequel
        # coûterait un aller-retour par hypothèse.
        print("--- ce que systemd impose réellement à sandboxd ---")
        print(machine.succeed(
            "systemctl show prophet-sandboxd.service "
            "-p User -p CapabilityBoundingSet -p AmbientCapabilities -p NoNewPrivileges "
            "-p SystemCallFilter -p RestrictSUIDSGID -p ProtectProc -p RestrictNamespaces"
        ))
        print("--- et ce que le noyau dit du processus ---")
        print(machine.succeed(
            "grep -E '^(Uid|Gid|CapEff|CapBnd|CapAmb|NoNewPrivs|Seccomp)' "
            "/proc/$(systemctl show -p MainPID --value prophet-sandboxd.service)/status"
        ))

        sortie = machine.succeed("prophet-essai-tache sandbox")
        print(sortie)
        assert "task:essai-sandbox" in sortie, sortie

        journal = machine.succeed("journalctl -u prophet-sandboxd -n 30 --no-pager")
        print(journal)
        assert "Bad system call" not in journal and "SIGSYS" not in journal, (
            f"le filtre d'appels système refuse ce que les capacités autorisent :\n{journal}"
        )

    with subtest("le journal a vu passer la tâche"):
        journal = machine.succeed("prophet log tail -n 20")
        print(journal)
        assert "task.created" in journal or "créée" in journal, journal
  '';
}
