# Recettes de développement de Prophet OS.
# Toute recette non encore implémentée sort avec le code 2 et nomme son jalon.

set shell := ["bash", "-uc"]

default:
    @just --list

# Format, lints, tests sans privilèges, recherche de secrets.
check:
    cargo fmt --all --check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test --workspace
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

# Tests NixOS en machine virtuelle.
test-vm:
    @echo "pas encore disponible (M9)" && exit 2

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
