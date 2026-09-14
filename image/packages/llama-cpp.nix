{ llama-cpp, vulkan ? false }:

# Correctif limité au protocole d'appels : les optimisations et backends de nixpkgs sont
# conservés. Les poids, le template et les contrôles Prophet restent inchangés. `vulkan`
# demande le backend graphique de llama.cpp (ADR 0037) : même paquet, même correctif, et
# le moteur peut alors placer le modèle sur la carte (`prophet.localEngine.gpu`).
(llama-cpp.override { vulkanSupport = vulkan; }).overrideAttrs (old: {
  patches = (old.patches or [ ]) ++ [ ./llama-cpp-single-call.patch ];
})
