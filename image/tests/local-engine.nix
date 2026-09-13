# Poids réels, unités installées et appel effectué sous le compte humain personnalisé.
# Le modèle reste un argument explicite : aucun téléchargement au démarrage du système.
{ pkgs, module, weights }:
let
  probe = pkgs.writeText "prophet-installed-mission.py" ''
    import json, socket, sys, time, uuid

    def call(method, params, denied=False):
        with socket.socket(socket.AF_UNIX) as connection:
            connection.settimeout(10)
            connection.connect('/run/prophet/agentd.sock')
            connection.sendall((json.dumps({'jsonrpc':'2.0','id':1,'method':method,'params':params})+'\n').encode())
            reply = json.loads(connection.makefile().readline())
        if denied:
            assert 'result' not in reply and reply.get('error', {}).get('code') == -32002, reply
            return None
        assert 'error' not in reply, reply
        return reply['result']

    def versions(ident):
        return call('task.change', {'id':ident,'path':'Documents/Prophet/note.txt'})

    if len(sys.argv) == 3 and sys.argv[1] == 'deny':
        call('task.change', {'id':sys.argv[2],'path':'Documents/Prophet/note.txt'}, denied=True)
        raise SystemExit(0)

    if len(sys.argv) == 2:
        print(json.dumps({'result':call('task.result', {'id':sys.argv[1]}), 'review':versions(sys.argv[1])}))
        raise SystemExit(0)

    options = call('task.options', {})
    assert options['model_error'] is None, options
    assert options['profiles'][0]['models'] == ['qwen3-1.7b'], options
    ident = 'installed-' + uuid.uuid4().hex
    content = 'mission-' + str(uuid.uuid4().int % 4294967296)
    call('task.prepare', {'id':ident,'profile':'documents','model':'qwen3-1.7b',
        'intent':f'Use fs.write to save exactly {content} into ~/Documents/Prophet/note.txt. After staged=true, answer Done. /no_think'})
    assert call('task.status', {'id':ident})['state'] == 'planned'
    call('task.start', {'id':ident})
    deadline = time.monotonic() + 120
    while True:
        state = call('task.status', {'id':ident})
        if state['state'] in ['done','failed','cancelled']:
            assert state['state'] == 'done', state
            break
        assert time.monotonic() < deadline, state
        time.sleep(.2)
    result = call('task.result', {'id':ident})
    review = versions(ident)
    assert review['task'] == ident, review
    assert review['file']['before'] is None, review
    assert review['file']['after']['content'] == {'kind':'text','text':content}, review
    print(json.dumps({'id':ident,'content':content,'result':result,'review':review}), flush=True)
  '';
in pkgs.testers.runNixOSTest {
  name = "prophet-local-engine";
  nodes.machine = { ... }: {
    imports = [ module ];
    prophet.enable = true;
    prophet.user = "pilot";
    prophet.motDePasseHache = null;
    prophet.localEngine.weights = "/var/lib/prophet/models/active.gguf";
    environment.etc."test-model.gguf".source = weights;
    environment.etc."test-mission.py".source = probe;
    environment.systemPackages = [ pkgs.python3 pkgs.curl ];
    virtualisation.memorySize = 4096;
    virtualisation.cores = 4;
    virtualisation.diskSize = 8192;
  };
  testScript = ''
    import json
    machine.start()
    machine.wait_for_unit("prophet-agentd.service")
    with subtest("les poids absents ne provoquent pas une boucle de redémarrage"):
        machine.succeed("test $(systemctl show prophet-local-engine -p NRestarts --value) = 0")
        machine.fail("curl -fsS http://127.0.0.1:8080/health")
    with subtest("le moteur installé charge les poids sous un compte distinct"):
        machine.succeed("install -m 0644 /etc/test-model.gguf /var/lib/prophet/models/active.gguf")
        machine.succeed("systemctl start prophet-local-engine")
        machine.wait_until_succeeds("curl -fsS http://127.0.0.1:8080/health", timeout=120)
        machine.succeed("test $(systemctl show prophet-local-engine -p User --value) = prophet-model")
        machine.fail("runuser -u prophet-model -- cat /var/lib/prophet/agentd/taches.json")
    with subtest("le propriétaire personnalisé prépare et lance une vraie mission"):
        machine.succeed("install -m 0600 -o pilot -g users /etc/hostname /home/pilot/private-note")
        machine.fail("runuser -u agentd -- cat /home/pilot/private-note")
        try:
            proof = json.loads(machine.succeed("runuser -u pilot -- python3 /etc/test-mission.py", timeout=180))
        except Exception:
            print(machine.succeed("journalctl -u prophet-agentd -u prophet-local-engine --no-pager -n 100"))
            raise
        ident, expected = proof["id"], proof["content"]
        machine.succeed(f"python3 /etc/test-mission.py deny {ident}")
        actual = machine.succeed(f"cat /home/pilot/.prophet/tasks/{ident}/work/Documents/Prophet/note.txt")
        assert actual == expected, (actual, expected)
        machine.fail("test -e /home/pilot/Documents/Prophet/note.txt")
        machine.succeed("test ! -d /home/prophet")
        machine.succeed("systemctl restart prophet-agentd")
        machine.wait_for_unit("prophet-agentd.service")
        persisted = json.loads(machine.succeed(f"runuser -u pilot -- python3 /etc/test-mission.py {ident}"))
        assert persisted == {key:proof[key] for key in ['result','review']}, persisted
        machine.succeed(f"python3 /etc/test-mission.py deny {ident}")
    with subtest("l'arrêt du moteur est effectif"):
        machine.succeed("systemctl stop prophet-local-engine")
        machine.fail("curl -fsS http://127.0.0.1:8080/health")
  '';
}
