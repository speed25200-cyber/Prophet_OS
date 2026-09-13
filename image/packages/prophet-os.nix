# Le paquet Prophet OS, construit depuis le dépôt.
{ lib, rustPlatform, pkg-config, sqlite, makeWrapper, vulkan-loader, wayland, libxkbcommon, libGL, ... }:

rustPlatform.buildRustPackage {
  pname = "prophet-os";
  version = "0.1.0";
  # Une capture ou un rapport ne doit pas reconstruire tous les binaires de la distribution.
  # Les ressources embarquées restent dans crates ; les exemples servent aux tests Rust.
  src = lib.fileset.toSource {
    root = ../..;
    fileset = lib.fileset.unions [
      ../../Cargo.toml ../../Cargo.lock ../../rust-toolchain.toml ../../rustfmt.toml
      ../../crates ../../examples ../../policies
    ];
  };
  cargoLock.lockFile = ../../Cargo.lock;

  nativeBuildInputs = [ pkg-config makeWrapper ];
  buildInputs = [ sqlite ];

  # Les tests qui exigent des espaces de noms, KVM ou un navigateur tournent par
  # `just test-privileged`, jamais pendant la construction : une construction doit être
  # reproductible et sans privilège.
  doCheck = false;

  # winit et wgpu chargent ces bibliothèques avec dlopen : elles ne sont pas déduites du
  # lien ELF. Sans ce wrapper, le binaire peut compiler et ne pas ouvrir de fenêtre sur NixOS.
  postFixup = ''
    wrapProgram $out/bin/prophet-surface \
      --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath [ vulkan-loader wayland libxkbcommon libGL ]}"
  '';

  meta = {
    description = "Système d'exploitation natif pour l'IA";
    license = lib.licenses.asl20;
    platforms = [ "x86_64-linux" "aarch64-linux" ];
  };
}
