# Le paquet Prophet OS, construit depuis le dépôt.
{ lib, rustPlatform, pkg-config, sqlite, ... }:

rustPlatform.buildRustPackage {
  pname = "prophet-os";
  version = "0.1.0";
  src = ../..;
  cargoLock.lockFile = ../../Cargo.lock;

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ sqlite ];

  # Les tests qui exigent des espaces de noms, KVM ou un navigateur tournent par
  # `just test-privileged`, jamais pendant la construction : une construction doit être
  # reproductible et sans privilège.
  doCheck = false;

  meta = {
    description = "Système d'exploitation natif pour l'IA";
    license = lib.licenses.asl20;
    platforms = [ "x86_64-linux" "aarch64-linux" ];
  };
}
