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
        assert repertoire == "750", f"/run/prophet est en {repertoire}, attendu 750"

    with subtest("un utilisateur ordinaire n'atteint pas les sockets"):
        # La règle que `prophet-daemon` porte, vérifiée avec de vrais utilisateurs plutôt qu'avec
        # un `Pairs::explicite` de test. Les droits du répertoire l'arrêtent avant même le daemon,
        # et c'est bien ainsi : deux barrières valent mieux qu'une.
        machine.succeed("useradd -m intrus")
        machine.fail("su intrus -c 'test -r /run/prophet/capd.sock'")

    with subtest("redémarrer un service n'en coupe pas six autres"):
        # Les sept partagent `/run/prophet`. Sans `RuntimeDirectoryPreserve`, systemd supprime ce
        # répertoire quand l'un s'arrête et emporte les sockets des autres : un
        # `systemctl restart prophet-memoryd` couperait tout le reste. Aucun test de daemon pris
        # isolément ne peut voir cela.
        machine.succeed("systemctl restart prophet-memoryd.service")
        machine.wait_for_unit("prophet-memoryd.service")
        for nom in services:
            machine.succeed(f"test -S /run/prophet/{nom}.sock")
        statut = machine.succeed("prophet status")
        assert "muet" not in statut, (
            f"un redémarrage isolé ne doit rien couper :\n{statut}"
        )

    with subtest("prophet status voit ses services"):
        statut = machine.succeed("prophet status")
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

        taches = machine.succeed("prophet task ls")
        print(taches)
        assert "task:essai-vm" in taches, taches

    with subtest("le journal a vu passer la tâche"):
        journal = machine.succeed("prophet log tail -n 20")
        print(journal)
        assert "task.created" in journal or "créée" in journal, journal
  '';
}
