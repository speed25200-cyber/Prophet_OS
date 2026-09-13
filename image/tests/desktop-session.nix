# Parcours humain réel : PAM, session Wayland, fenêtres, clients et reprise après verrouillage.
# Les comptes et données sont propres à cette VM ; aucune connexion cloud n'est utilisée.
{ pkgs, module, installed ? false }:
let
  chatgpt = pkgs.callPackage ../packages/chatgpt-linux.nix { };
in
pkgs.testers.runNixOSTest {
  name = if installed then "prophet-installe" else "prophet-desktop-session";
  enableOCR = true;
  nodes.machine = { lib, ... }: {
    imports = [ module ../modules/surface.nix ../modules/desktop.nix ]
      ++ lib.optionals installed [ ../modules/hardware.nix ../modules/immutable.nix ];
    prophet.enable = true;
    prophet.user = "pilot";
    prophet.motDePasseHache = null;
    users.users.pilot = { uid = 1000; password = "essai-bureau"; };
    environment.variables = {
      SWAYSOCK = "/run/user/1000/prophet-sway.sock";
      WLR_RENDERER = "pixman";
    };
    environment.systemPackages = [ pkgs.jq pkgs.iptables ];
    virtualisation.memorySize = 4096;
    virtualisation.cores = 2;
    virtualisation.diskSize = 8192;
    virtualisation.qemu.options = [ "-vga none -device virtio-gpu-pci" ];
    virtualisation.useBootLoader = installed;
    virtualisation.useEFIBoot = installed;
    # Le déverrouillage des volumes reste exercé séparément par le test de l'installeur.
    boot.initrd.luks.devices = lib.mkIf installed (lib.mkForce { });
    networking.networkmanager.enable = lib.mkIf installed (lib.mkForce false);
  };
  testScript = ''
    import json
    import os
    import re
    import shlex
    from datetime import timedelta

    q = shlex.quote
    # Les variantes OCR sont déjà orchestrées par le pilote ; borner leurs threads évite
    # de saturer l'hôte de la VM. Le moteur et les textes attendus restent identiques.
    os.environ["OMP_THREAD_LIMIT"] = "1"
    tree = "su - pilot -c 'swaymsg -t get_tree'"

    def wait_text(pattern, timeout=timedelta(seconds=120), variants=False):
        # Lire d'abord la capture brute. La supervision nécessite parfois les variantes
        # OCR pour ses glyphes ; le formulaire de connexion est lisible sans ce coût.
        def matches(last_try):
            visible_text = machine.get_screen_text()
            if re.search(pattern, visible_text, re.IGNORECASE) is not None:
                return True
            if variants:
                visible_text = "\n".join(machine.get_screen_text_variants())
            if last_try:
                machine.log("Dernière lecture de l'écran : " + visible_text)
            return re.search(pattern, visible_text, re.IGNORECASE) is not None
        with machine.nested("lecture de l'écran : " + pattern):
            retry(matches, timeout)

    def window(app_id, focused=False):
        selector = ".. | objects | select(.app_id? == " + json.dumps(app_id) + " and .visible? == true)"
        if focused:
            selector += " | select(.focused == true)"
        command = tree + " | jq -c " + q(selector)
        machine.wait_until_succeeds(command + " | grep . >/dev/null", timeout=timedelta(seconds=120))
        result = json.loads(machine.succeed(command))
        assert machine.succeed(f"stat -c %u /proc/{int(result['pid'])}").strip() == "1000"
        return result

    def open_app(name):
        command = "swaymsg exec " + q("prophet-ouvrir " + q(name))
        machine.succeed("su - pilot -c " + q(command))

    def processes(package, binary):
        # Les wrappers Nix changent le nom comm ; identifier le véritable exécutable.
        executables = [package + "/bin/" + binary, package + "/bin/." + binary + "-wrapped"]
        command = "for pid in $(pgrep -u pilot); do exe=$(readlink /proc/$pid/exe || true); case $exe in "
        command += "|".join(q(path) for path in executables) + ") echo $pid;; esac; done"
        return command

    def client_process(terminal, package, binary):
        # Lire le processus et sa filiation, jamais le contenu d'un profil de connexion.
        command = processes(package, binary)
        machine.wait_until_succeeds(command + " | grep . >/dev/null", timeout=60)
        parents = {}
        for line in machine.succeed("ps -u pilot -o pid=,ppid=").splitlines():
            child, parent = line.split()
            parents[int(child)] = int(parent)
        for line in machine.succeed(command).splitlines():
            pid = int(line)
            ancestor = pid
            visited = set()
            while ancestor in parents and ancestor not in visited:
                visited.add(ancestor)
                ancestor = parents[ancestor]
                if ancestor == terminal["pid"]:
                    assert machine.succeed(f"stat -c %u /proc/{pid}").strip() == "1000"
                    assert machine.succeed(f"readlink /proc/{pid}/cwd").strip() == "/home/pilot/Documents/Prophet"
                    return pid
        raise AssertionError(f"{binary} doit être un processus du terminal humain")

    def window_ids():
        return set(json.loads(machine.succeed(tree + " | jq '[.. | objects | select(.pid?) | .id]'")))

    machine.start()
    machine.wait_for_unit("graphical.target")
    ${pkgs.lib.optionalString installed ''
    with subtest("le bureau installé démarre par systemd-boot"):
        boot = machine.succeed("bootctl status")
        assert "systemd-boot" in boot, boot
        cmdline = machine.succeed("cat /proc/cmdline")
        for parameter in ["lockdown=integrity", "module.sig_enforce=1"]:
            assert parameter in cmdline, cmdline
        print("Montage réel de / : " + machine.succeed("findmnt -no OPTIONS /"))
        # La racine reste en lecture-écriture ; ce test ne prouve ni immuabilité ni chiffrement.
    with subtest("le magasin de l'image conserve le contenu enregistré"):
        store_mount = machine.succeed("findmnt -no SOURCE,FSTYPE -T /nix/store")
        assert "ext4" in store_mount and "9p" not in store_mount, store_mount
        # Contrôle des empreintes après la copie sur disque, sans réparation des fichiers.
        machine.succeed("nix-store --verify --check-contents", timeout=timedelta(seconds=600))
    ''}
    try:
        with subtest("la session demande le mot de passe du propriétaire"):
            machine.wait_for_unit("greetd.service")
            machine.wait_for_unit("getty@tty2.service")
            machine.fail("pgrep -u pilot -x sway")
            wait_text("Prophet OS")
            machine.screenshot("bureau-connexion")
            machine.send_key("ret")
            wait_text("[Pp]assword", timeout=timedelta(seconds=30))
            machine.send_chars("essai-bureau")
            machine.send_key("ret")
            machine.wait_until_succeeds("su - pilot -c 'swaymsg -t get_outputs' | jq -e '[.[] | select(.active)] | length > 0'", timeout=timedelta(seconds=120))

        # Les essais portent sur les applications locales ; aucun fournisseur ne reçoit de requête.
        machine.succeed("iptables -I OUTPUT ! -o lo -j REJECT")
        machine.succeed("ip6tables -I OUTPUT ! -o lo -j REJECT")

        with subtest("la supervision tourne sous le compte humain"):
            for service in ["capd", "ledger", "vault", "egress", "sandboxd", "memoryd", "agentd"]:
                machine.wait_for_unit(f"prophet-{service}.service")
            surface = window("org.prophet.Supervision")
            assert surface["shell"] == "xdg_shell"
            wait_text("Vos missions", timeout=timedelta(seconds=90), variants=True)
            machine.succeed("su - pilot -c 'prophet task ls'")
            machine.fail("su - pilot -c 'ls /home/pilot/.prophet/tasks'")
            status = machine.succeed("su - pilot -c 'timeout 30 prophet status'")
            assert "muet" not in status, status
            for service in ["capd", "ledger", "vault", "egress", "sandboxd", "memoryd", "agentd"]:
                assert service in status, status
            assert not machine.succeed("systemctl --failed --no-legend --plain").strip()
            machine.screenshot("bureau-supervision")

        with subtest("terminal, fichiers et supervision coexistent"):
            machine.succeed("foot --check-config --config /etc/xdg/foot/foot.ini")
            machine.send_key("meta_l-ret")
            window("org.prophet.Terminal", focused=True)
            machine.send_chars("printf 'Fichier de la session humaine\\n' > Documents/Prophet/session.txt\n")
            machine.wait_until_succeeds("test -s /home/pilot/Documents/Prophet/session.txt", timeout=30)
            open_app("fichiers")
            window("thunar")
            machine.succeed(tree + " | jq -e " + q('.. | objects | select(.app_id? == "org.prophet.Supervision")'))
            # Une donnée factice transite par le vrai presse-papiers Wayland.
            open_app("terminal")
            window("org.prophet.Terminal", focused=True)
            machine.send_chars("printf 'presse-papiers-prophet' | wl-copy\n")
            machine.send_chars("wl-paste --no-newline > /home/pilot/paste-result\n")
            machine.wait_until_succeeds("test -s /home/pilot/paste-result", timeout=30)
            assert machine.succeed("cat /home/pilot/paste-result").strip() == "presse-papiers-prophet"
            machine.screenshot("bureau-travail")

        with subtest("les clients officiels sont des applications humaines"):
            open_app("claude-code")
            claude = window("org.prophet.ClaudeCode")
            claude_pid = client_process(claude, "${pkgs.claude-code}", "claude")
            machine.screenshot("bureau-claude-demarrage")
            open_app("codex")
            codex = window("org.prophet.Codex")
            codex_pid = client_process(codex, "${pkgs.codex}", "codex")
            open_app("chatgpt")
            selector = '.. | objects | select(.window_properties?.class? == "Chatgpt" and .name? == "ChatGPT")'
            machine.wait_until_succeeds(tree + " | jq -e " + q(selector), timeout=timedelta(seconds=120))
            app = json.loads(machine.succeed(tree + " | jq -c " + q(selector)))
            assert machine.succeed(f"stat -c %u /proc/{int(app['pid'])}").strip() == "1000"
            assert app["visible"] and app["shell"] == "xwayland", app
            assert machine.succeed(f"readlink /proc/{int(app['pid'])}/exe").strip() == "${chatgpt.payload}/usr/lib/chatgpt/ChatGPT"
            wait_text("Sign in to ChatGPT", timeout=timedelta(seconds=90))
            machine.screenshot("bureau-clients")
            open_app("claude-code")
            window("org.prophet.ClaudeCode", focused=True)
            # Le réseau est bloqué : le vrai client quitte après ce diagnostic précis.
            # Le terminal doit le conserver, pas faire croire à une session connectée.
            wait_text(r"Unable\s+to\s+connect\s+to\s+Anthropic\s+services", timeout=timedelta(seconds=30))
            wait_text("ENOTFOUND", timeout=timedelta(seconds=30))
            machine.wait_until_fails(f"test -d /proc/{claude_pid}", timeout=30)
            machine.screenshot("bureau-claude")
            open_app("codex")
            window("org.prophet.Codex", focused=True)
            wait_text("Sign in with ChatGPT", timeout=timedelta(seconds=30))
            machine.screenshot("bureau-codex")
            machine.succeed(f"test -d /proc/{codex_pid}")

        with subtest("portails et trousseau sont disponibles dans la session"):
            # su ouvre une commande de contrôle, pas la session graphique ; cibler son bus réel.
            bus = "XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus busctl --user --timeout=15 "
            machine.succeed("su - pilot -c " + q(bus + "introspect org.freedesktop.portal.Desktop /org/freedesktop/portal/desktop org.freedesktop.portal.FileChooser"))
            machine.succeed("su - pilot -c " + q(bus + "introspect org.freedesktop.secrets /org/freedesktop/secrets org.freedesktop.Secret.Service"))

        with subtest("verrouiller puis reprendre conserve les fenêtres"):
            before = window_ids()
            # Consommer toute la liste : grep -q ferme trop tôt le tube quand PAM a deux PID.
            lock = processes("${pkgs.swaylock}", "swaylock") + " | grep . >/dev/null"
            open_app("verrouiller")
            machine.wait_until_succeeds(lock, timeout=30)
            machine.sleep(1)
            machine.send_chars("mot-de-passe-incorrect")
            machine.send_key("ret")
            machine.sleep(2)
            machine.succeed(lock)
            machine.screenshot("bureau-verrouille")
            machine.send_chars("essai-bureau")
            machine.send_key("ret")
            machine.wait_until_fails(lock, timeout=30)
            after = window_ids()
            assert surface["id"] in before and before == after, (before, after)
            machine.screenshot("bureau-reprise")

        with subtest("le lanceur clavier et la déconnexion demandent une action explicite"):
            machine.send_key("meta_l-spc")
            wait_text("Ouvrir", timeout=timedelta(seconds=30))
            machine.screenshot("bureau-lanceur")
            machine.send_key("esc")
            machine.wait_until_fails("pgrep -u pilot -x fuzzel", timeout=30)
            machine.send_key("meta_l-shift-e")
            wait_text("Annuler", timeout=timedelta(seconds=30))
            machine.send_key("ret")
            machine.wait_until_fails("pgrep -u pilot -x fuzzel", timeout=30)
            assert window_ids() == after
            machine.send_key("meta_l-shift-e")
            wait_text("Annuler", timeout=timedelta(seconds=30))
            machine.send_key("down")
            machine.send_key("ret")
            machine.wait_until_fails("su - pilot -c 'swaymsg -t get_outputs'", timeout=30)
            wait_text("Prophet OS", timeout=timedelta(seconds=60))
            # Une nouvelle connexion doit relancer la supervision sous la même identité.
            machine.send_key("ret")
            wait_text("[Pp]assword", timeout=timedelta(seconds=30))
            machine.send_chars("essai-bureau")
            machine.send_key("ret")
            reopened = window("org.prophet.Supervision")
            assert reopened["pid"] != surface["pid"]
            wait_text("Vos missions", timeout=timedelta(seconds=60), variants=True)
            machine.screenshot("bureau-nouvelle-session")
    finally:
        machine.screenshot("bureau-diagnostic")
        if machine.execute(tree + " > /tmp/bureau-tree.json")[0] == 0:
            machine.copy_from_machine("/tmp/bureau-tree.json")
        print(machine.execute("journalctl -u greetd --no-pager -n 45"))
        print(machine.execute("journalctl _UID=1000 --no-pager -n 70"))
        print(machine.execute(tree + " | jq '[.. | objects | select(.pid?) | {name, app_id, pid, visible, shell}]'"))
  '';
}
