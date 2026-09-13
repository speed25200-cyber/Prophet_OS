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
      clientsProprietaires = [ "claude-code" "gemini-cli" "chatgpt-linux" "chatgpt-linux-payload" ];
      autoriserLesClients = paquet:
        builtins.elem (nixpkgs.lib.getName paquet) clientsProprietaires;

      # Modules réutilisables, indépendants du système hôte.
      nixosModules.prophet = import ./image/modules/prophet.nix;

      # Le modèle local par défaut : Qwen3-1.7B en Q8_0, 1,83 Go, celui qui réussit les missions
      # réelles du dépôt sur un processeur seul. La configuration de référence le télécharge à
      # l'installation, pour qu'une machine installée ait un agent sans clé d'API ni compte ;
      # la variante d'intégration continue et le support d'amorçage ne le portent pas (ADR 0033).
      modeleParDefaut = {
        url = "https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/resolve/90862c4b9d2787eaed51d12237eafdfe7c5f6077/Qwen3-1.7B-Q8_0.gguf";
        sha256 = "061b54daade076b5d3362dac252678d17da8c68f07560be70818cace6590cb1a";
      };
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
          ./image/modules/desktop.nix
          # Ce que l'installeur écrit pour la machine : son matériel détecté et son mode
          # d'amorçage. Vides dans le dépôt (ADR 0032).
          ./image/machine/hardware-configuration.nix
          ./image/machine/amorcage.nix
          ({ pkgs, ... }: {
            prophet.enable = true;
            prophet.localEngine.weights = pkgs.fetchurl modeleParDefaut;
            nixpkgs.config.allowUnfreePredicate = autoriserLesClients;
          })
        ];
      };

      # La même configuration sans la suite d'applications de l'humain (LibreOffice, GIMP,
      # Blender, FreeCAD…) ni le modèle local par défaut : ce que l'intégration continue construit
      # et démarre, parce qu'elle paie chaque gigaoctet et n'ouvre aucune application ; ce que
      # l'installeur pose reste `prophet`, suite et modèle compris.
      nixosConfigurations.prophet-ci = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          nixosModules.prophet
          ./image/modules/hardware.nix
          ./image/modules/immutable.nix
          ./image/modules/surface.nix
          ./image/modules/desktop.nix
          ./image/machine/hardware-configuration.nix
          ./image/machine/amorcage.nix
          {
            prophet.enable = true;
            prophet.desktop.suite.enable = false;
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
            python3
          ];
          env = {
            PROPHET_BROWSER = "${pkgs.chromium}/bin/chromium";
            RUST_BACKTRACE = "1";
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
              pkgs.vulkan-loader pkgs.wayland pkgs.libxkbcommon pkgs.libGL
            ];
          };
        };

        # Ce que `nix flake check` exerce : une vraie machine, avec de vrais services.
        checks = pkgs.lib.optionalAttrs (system == "x86_64-linux") {
          desktop-session = import ./image/tests/desktop-session.nix {
            inherit pkgs;
            module = nixosModules.prophet;
          };
          surface-rescue = import ./image/tests/surface-rescue.nix {
            inherit pkgs;
            module = nixosModules.prophet;
          };
          llama-tool-grammar = import ./image/tests/llama-tool-grammar.nix {
            inherit pkgs;
            engine = self.packages.${system}.llama-cpp;
          };
          services = import ./image/tests/services.nix {
            inherit pkgs;
            module = nixosModules.prophet;
          };
          chatgpt-desktop = import ./image/tests/chatgpt-desktop.nix {
            inherit pkgs;
            chatgpt = pkgs.callPackage ./image/packages/chatgpt-linux.nix { };
          };
          # Et une vraie machine **installée** : racine en lecture seule, chargeur d'amorçage,
          # noyau verrouillé. Le support d'amorçage a démarré ; ce qu'il installe, jamais.
          installe = import ./image/tests/installe.nix {
            inherit pkgs;
            module = nixosModules.prophet;
          };
          # La même configuration installée, démarrée sans UEFI : GRUB sous SeaBIOS, pour les PC
          # qui n'ont que cela (ADR 0032).
          installe-bios = import ./image/tests/installe-bios.nix {
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
          llama-cpp = pkgs.callPackage ./image/packages/llama-cpp.nix { };
          # Même paquet et mêmes bibliothèques graphiques dans l'atelier et dans l'image.
          default = pkgs.callPackage ./image/packages/prophet-os.nix { };
        }
        # `nix build .#iso` produit le fichier à graver. L'attribut n'existe que sur
        # x86_64-linux : construire une image amorçable pour une architecture depuis une autre
        # exige une émulation qu'on n'a pas mise en place, et annoncer une cible qu'on ne sait
        # pas produire serait la même faute que promettre une isolation qu'on ne sait pas mettre
        # en place. Absent vaut mieux que présent et cassé.
        // pkgs.lib.optionalAttrs (system == "x86_64-linux") {
          # Essai explicite : télécharge 1,83 Go de poids si absents du store. Hors checks sans poids.
          local-engine-vm = import ./image/tests/local-engine.nix {
            inherit pkgs;
            module = nixosModules.prophet;
            weights = pkgs.fetchurl modeleParDefaut;
          };
          chatgpt-linux = pkgs.callPackage ./image/packages/chatgpt-linux.nix { };
          iso = self.nixosConfigurations.prophet-iso.config.system.build.isoImage;
        };
      });
}
