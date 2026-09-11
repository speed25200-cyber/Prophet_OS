{
  description = "Prophet OS — un système d'exploitation natif pour l'IA";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    let
      # Modules réutilisables, indépendants du système hôte.
      nixosModules.prophet = import ./image/modules/prophet.nix;
    in
    {
      inherit nixosModules;

      # Configuration de référence : c'est elle que `just vm` démarre et que `just image`
      # construit.
      nixosConfigurations.prophet = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          nixosModules.prophet
          ./image/modules/hardware.nix
          ./image/modules/immutable.nix
          { prophet.enable = true; }
        ];
      };
    }
    // flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        # Environnement de développement : tout ce dont l'agent constructeur a besoin, et rien
        # d'autre, pour que deux machines produisent le même résultat.
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            rustc cargo rustfmt clippy cargo-nextest
            just
            btrfs-progs
            bubblewrap
            gvisor
            firecracker
            qemu
            chromium
            gitleaks
            sqlite
            pkg-config
          ];
          env = {
            PROPHET_BROWSER = "${pkgs.chromium}/bin/chromium";
            RUST_BACKTRACE = "1";
          };
        };

        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "prophet-os";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          # Les tests d'intégration exigent des espaces de noms et un navigateur ; ils tournent
          # par `just test-privileged`, pas pendant la construction du paquet.
          doCheck = false;
        };
      });
}
