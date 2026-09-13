{ llama-cpp }:

# Correctif limité au protocole d'appels : les optimisations et backends de nixpkgs
# sont conservés. Les poids, le template et les contrôles Prophet restent inchangés.
llama-cpp.overrideAttrs (old: {
  patches = (old.patches or [ ]) ++ [ ./llama-cpp-single-call.patch ];
})
