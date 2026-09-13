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
  cat "$out/result.txt"
''
