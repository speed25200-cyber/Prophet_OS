# Recettes de développement de Prophet OS.
# Toute recette non encore implémentée sort avec le code 2 et nomme son jalon.

set shell := ["bash", "-uc"]

default:
    @just --list

# Format, lints, tests sans privilèges, recherche de secrets.
check:
    cargo fmt --all --check
    cargo clippy --all-targets --all-features -- -D warnings
    # Les tests interservices lancent aussi les binaires voisins, dont la CLI sans test d'intégration propre.
    cargo build --workspace --bins
    cargo test --workspace
    ./tools/verifier-les-services.sh
    ./tools/verifier-le-durcissement.sh
    ./tools/verifier-la-doc-des-travaux.sh
    @just secrets

# Recherche de secrets commités (gitleaks si présent, motifs de base sinon).
secrets:
    #!/usr/bin/env bash
    set -uo pipefail
    if command -v gitleaks >/dev/null 2>&1; then
        gitleaks detect --no-banner --redact
    else
        echo "gitleaks absent : repli sur une recherche de motifs"
        if git grep -nIE '(sk-ant-[A-Za-z0-9_-]{10,}|sk-[A-Za-z0-9]{32,}|AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY-----)' -- . ':!*.lock'; then
            echo "secret probable détecté" >&2
            exit 1
        fi
        echo "aucun motif de secret détecté"
    fi

# Tests nécessitant des privilèges ou des périphériques (root, KVM, btrfs).
test-privileged:
    cargo test --workspace -- --ignored

# Prépare un hôte Ubuntu : mémoire d'échange, Rust, just, gVisor.
setup-host:
    sudo ./tools/setup-ubuntu-host.sh

# Vérifie sur une machine complète ce que l'environnement de construction ne peut pas vérifier.
verify-host:
    ./tools/verify-on-host.sh

# Sonde seule : dit ce qui manque, sans rien exécuter.
probe-host:
    ./tools/verify-on-host.sh --probe

# Niveaux d'isolation seuls : ne compile que sandboxd. Pour une machine à mémoire courte, où la
# compilation de l'atelier entier risque d'être tuée avant d'avoir répondu à la question utile.
verify-levels:
    ./tools/verify-on-host.sh --niveaux

# Construit le support d'amorçage à graver sur une clé USB.
iso:
    nix build .#iso --print-build-logs
    @echo "image : $(readlink -f result)/iso/"
    @ls -lh $(readlink -f result)/iso/*.iso

# Tests NixOS en machine virtuelle : les sept services, sous systemd, avec leur durcissement.
# Exige Nix et KVM.
test-vm:
    nix build .#checks.x86_64-linux.services --print-build-logs

# Contrat du parseur et de la grammaire du paquet moteur local, sans télécharger de poids.
test-local-engine:
    nix build .#checks.x86_64-linux.llama-tool-grammar --print-build-logs

# KVM, 4 Go de RAM pour la VM et 1,83 Go de poids : services installés et mission réelle.
test-local-engine-vm:
    nix build .#local-engine-vm --print-build-logs

# KVM, 4 Go de RAM pour la VM : connexion et applications de la session humaine.
test-desktop:
    nix build .#checks.x86_64-linux.desktop-session --print-build-logs

# Démarre l'image dans QEMU.
vm:
    @echo "pas encore disponible (M9)" && exit 2

# Construit l'image disque A/B.
image:
    @echo "pas encore disponible (M9)" && exit 2

# Lance la suite de mesure.
bench:
    @echo "pas encore disponible (M13)" && exit 2

# Rejoue la démonstration d'un jalon.
demo MILESTONE:
    @echo "pas encore disponible ({{MILESTONE}})" && exit 2
