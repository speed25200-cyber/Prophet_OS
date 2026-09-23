# Le routeur du llama-server épinglé, tel que l'image le lance en mode relais (ADR 0034, 0046) :
# un fichier de préréglages avec sa section globale `[*]`, et le dossier des poids téléchargés
# (`--models-dir`). Le routeur liste ses modèles sans en charger aucun ; on vérifie qu'il démarre
# avec ces options, qu'il connaît le poids posé dans le dossier et dit son chemin — c'est par
# ce chemin que `prophet model serve` et la page Modèles le reconnaissent.
#
# Aucun essai en machine virtuelle n'exerce le mode routeur : c'est ce contrôle qui prouve que
# l'image ne passe au moteur que des options qu'il connaît. Une option inconnue l'empêcherait de
# démarrer.
{ pkgs, engine }:
pkgs.runCommand "prophet-llama-router" {
  nativeBuildInputs = [ pkgs.python3 ];
} ''
  set -eu
  export HOME="$TMPDIR/home" LLAMA_CACHE="$TMPDIR/cache"
  mkdir -p "$HOME" "$LLAMA_CACHE" catalogue prereglage "$out"
  # Un en-tête GGUF minimal : lister ne charge rien, seuls les noms comptent.
  python3 - <<'PYTHON'
  import struct
  cle = b"general.architecture"
  entete = b"GGUF" + struct.pack("<IQQ", 3, 0, 1) + struct.pack("<Q", len(cle)) + cle
  entete += struct.pack("<IQ", 8, 5) + b"qwen3"
  for nom in ["catalogue/Tire-Q4_K_M.gguf", "prereglage/defaut.gguf"]:
      open(nom, "wb").write(entete)
  PYTHON
  cat > prereglages.ini <<INI
  [*]
  load-mode = none
  jinja = 1
  ctx-size = 4096
  threads = 2
  parallel = 1
  n-gpu-layers = 0

  [defaut]
  model = $PWD/prereglage/defaut.gguf
  reasoning = off
  INI
  ${engine}/bin/llama-server --host 127.0.0.1 --port 18097 \
    --models-preset prereglages.ini --models-max 2 --models-dir "$PWD/catalogue" \
    > routeur.log 2>&1 &
  routeur=$!
  if ! python3 - "$out" "$PWD/catalogue/Tire-Q4_K_M.gguf" <<'PYTHON'
  import json, sys, time, urllib.request
  sortie, tire = sys.argv[1], sys.argv[2]
  limite = time.monotonic() + 90
  while True:
      try:
          modeles = json.load(urllib.request.urlopen("http://127.0.0.1:18097/models", timeout=5))
          break
      except Exception as erreur:
          if time.monotonic() > limite:
              raise SystemExit(f"le routeur ne répond pas : {erreur}")
          time.sleep(0.5)
  json.dump(modeles, open(f"{sortie}/models.json", "w"), indent=2)
  print(json.dumps(modeles, indent=2))
  donnees = modeles["data"]
  ids = [m["id"] for m in donnees]
  assert "defaut" in ids, f"le préréglage manque : {ids}"
  def chemin(m):
      # Pas de `path` dans la réponse : le fichier est dans les arguments de l'instance.
      args = m.get("status", {}).get("args", [])
      return m.get("path") or next((args[i + 1] for i, a in enumerate(args[:-1]) if a in ("--model", "-m")), None)
  tires = [m for m in donnees if chemin(m) == tire]
  assert tires, f"le poids du dossier manque, ou sans son chemin : {ids}"
  args = tires[0]["status"]["args"]
  # La section globale [*] vaut pour lui : la fenêtre de l'image, pas celle d'entraînement.
  assert "--ctx-size" in args and args[args.index("--ctx-size") + 1] == "4096", args
  assert "--threads" in args and args[args.index("--threads") + 1] == "2", args
  # Les poids se lisent sans projection (ADR 0047) : l'option passe telle quelle à l'instance.
  assert "--load-mode" in args and args[args.index("--load-mode") + 1] == "none", args
  print(f"poids du dossier servi sous le nom {tires[0]['id']!r} ({tires[0].get('source')}), réglages {args}")
  PYTHON
  then
    echo "--- journal du routeur ---"
    cat routeur.log
    kill "$routeur" 2>/dev/null || true
    exit 1
  fi
  kill "$routeur"
  cp routeur.log "$out/"
''
