{
  description = "Prophet OS — un système d'exploitation natif pour l'IA";

  inputs = {
    # Épinglé à une révision, et non à la branche `nixos-unstable`.
    #
    # `flake.lock` fige aussi les entrées transitives. Sans épinglage, `nixos-unstable` est résolu au moment de
    # chaque construction. Deux gravures de la même ISO à quinze jours d'écart installaient donc
    # deux systèmes différents, et un travail d'intégration continue vert la veille pouvait être
    # rouge le lendemain sans qu'une seule ligne du dépôt ait changé. Pour un système qu'on
    # installe après avoir formaté son disque, « ce qu'on installe est ce qu'on a gravé » n'est
    # pas une formule : c'est la propriété qui permet de revenir en arrière.
    #
    # La révision retenue est celle du canal `nixos-unstable` du 12 septembre 2026 — un instantané
    # que Hydra a construit et testé, et celui-là même contre lequel l'ISO, le système installé et
    # les tests en machine virtuelle sont verts ce jour-là. La remonter est un geste explicite,
    # suivi d'une construction complète.
    nixpkgs.url = "github:NixOS/nixpkgs/8ce4ef6cb6f871616146b9fe26d2a5ae594e94fe";
    # La référence déclarée suit le dépôt ; sa résolution exacte est conservée dans flake.lock.
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    let
      # Les seuls paquets propriétaires que cette image accepte, nommés un par un.
      #
      # Claude Code est distribué sous les conditions de son éditeur ; nixpkgs le marque
      # « unfree » et refuse de l'évaluer sans autorisation. Celle-ci est nominative, et non
      # `allowUnfree = true` : la forme globale laisserait entrer n'importe quel paquet
      # propriétaire, aujourd'hui ou dans six mois, sans que personne ne s'en aperçoive.
      #
      # Elle est posée ici, et non dans `image/modules/prophet.nix`, parce que ce module est aussi
      # importé par les tests en machine virtuelle — lesquels reçoivent un `pkgs` déjà construit,
      # et NixOS refuse qu'un module touche à `nixpkgs.config` dans ce cas. Un seul endroit, qui
      # sert aux deux.
      clientsProprietaires = [ "claude-code" "gemini-cli" ];
      autoriserLesClients = paquet:
        builtins.elem (nixpkgs.lib.getName paquet) clientsProprietaires;

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
          ./image/modules/surface.nix
          {
            prophet.enable = true;
            nixpkgs.config.allowUnfreePredicate = autoriserLesClients;
          }
        ];
      };

      # Le support d'amorçage. Il porte sa propre source : ce qu'on installe est ce qu'on a
      # gravé, et non ce qui se trouvera sur GitHub au moment de l'installation.
      nixosConfigurations.prophet-iso = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          ./image/modules/iso.nix
          {
            prophet.installateur.source = self;
            nixpkgs.config.allowUnfreePredicate = autoriserLesClients;
          }
        ];
      };
    }
    // flake-utils.lib.eachDefaultSystem (system:
      let
        # Le même `pkgs` sert au shell de développement et aux tests en machine virtuelle ; les
        # seconds démarrent une machine qui embarque les clients officiels.
        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfreePredicate = autoriserLesClients;
        };
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

        # Ce que `nix flake check` exerce : une vraie machine, avec de vrais services.
        checks = pkgs.lib.optionalAttrs (system == "x86_64-linux") {
          services = import ./image/tests/services.nix {
            inherit pkgs;
            module = nixosModules.prophet;
          };
          # Et une vraie machine **installée** : racine en lecture seule, chargeur d'amorçage,
          # noyau verrouillé. Le support d'amorçage a démarré ; ce qu'il installe, jamais.
          installe = import ./image/tests/installe.nix {
            inherit pkgs;
            module = nixosModules.prophet;
          };
          # Une question, pas une garantie : `immutable.nix` demande une racine en lecture seule,
          # et personne n'a jamais démarré de machine où l'option soit réellement appliquée. Son
          # travail d'intégration continue ne barre pas la route (voir le fichier).
          racine-en-lecture-seule = import ./image/tests/racine-en-lecture-seule.nix {
            inherit pkgs;
            module = nixosModules.prophet;
          };
        };

        packages = {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "prophet-os";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            # Les tests d'intégration exigent des espaces de noms et un navigateur ; ils tournent
            # par `just test-privileged`, pas pendant la construction du paquet.
            doCheck = false;
          };
        }
        # `nix build .#iso` produit le fichier à graver. L'attribut n'existe que sur
        # x86_64-linux : construire une image amorçable pour une architecture depuis une autre
        # exige une émulation qu'on n'a pas mise en place, et annoncer une cible qu'on ne sait
        # pas produire serait la même faute que promettre une isolation qu'on ne sait pas mettre
        # en place. Absent vaut mieux que présent et cassé.
        // pkgs.lib.optionalAttrs (system == "x86_64-linux") {
          iso = self.nixosConfigurations.prophet-iso.config.system.build.isoImage;
        };
      });
}
