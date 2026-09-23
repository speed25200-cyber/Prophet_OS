{ pkgs, engine }:
pkgs.runCommand "prophet-llama-tool-grammar" {
  nativeBuildInputs = [ pkgs.stdenv.cc ];
} ''
  c++ -std=c++17 \
    -I${engine.src}/common -I${engine.src}/include \
    -I${engine.src}/src -I${engine.src}/ggml/include \
    ${./llama-tool-grammar.cpp} \
    -L${engine}/lib -Wl,-rpath,${engine}/lib -lllama-common -lllama \
    -o regression
  mkdir -p "$out"
  for template in Qwen-Qwen3-0.6B.jinja Qwen-Qwen2.5-7B-Instruct.jinja; do
    echo "$template" >> "$out/result.txt"
    ./regression ${engine.src}/models/templates/"$template" >> "$out/result.txt"
  done
  # Le mode routeur du llama-server épinglé sait-il lire un dossier de modèles, et comment
  # nomme-t-il ce qu'il y trouve ? C'est ce qui permettrait de servir un poids téléchargé du
  # catalogue sans reconfigurer le moteur (ADR 0046). Relevé ici, dans la source même du paquet.
  {
    echo "--- routeur : dossier de modèles ---"
    grep -rn -- '--models-dir' ${engine.src}/common/arg.cpp || echo "absent : --models-dir"
    grep -rn -A12 'models_dir' ${engine.src}/tools/server/server-models.cpp | head -80 || true
  } >> "$out/result.txt"
  cat "$out/result.txt"
''
