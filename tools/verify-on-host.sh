#!/usr/bin/env bash
# Vérifie Prophet OS sur une machine complète.
#
# Ce script existe parce que l'environnement de construction ne peut pas tout vérifier : il n'a ni
# KVM, ni gVisor, ni Landlock, ni cgroups v2. Neuf tâches du plan en dépendent. Lancé sur une
# machine équipée, il répond à une seule question : **ces neuf-là tiennent-elles ?**
#
# Usage :
#   ./tools/verify-on-host.sh            # sonde, tests complets, rapport
#   ./tools/verify-on-host.sh --probe    # sonde seule, sans rien exécuter
#
# Il ne modifie rien en dehors de son répertoire de rapport, et n'installe rien.

set -uo pipefail

RAPPORT="${RAPPORT:-verification-$(date +%Y-%m-%d-%H%M%S).md}"
PROBE_ONLY=0
[ "${1:-}" = "--probe" ] && PROBE_ONLY=1

ok()    { printf '  \033[32m✓\033[0m %s\n' "$1"; }
ko()    { printf '  \033[31m✗\033[0m %s\n' "$1"; }
info()  { printf '  · %s\n' "$1"; }

echo "Prophet OS — vérification sur machine complète"
echo

# --- 1. Ce que la machine offre ---
echo "Sonde du matériel et du noyau"
MANQUES=()

if [ -e /dev/kvm ]; then ok "/dev/kvm présent"; else ko "/dev/kvm absent"; MANQUES+=("kvm"); fi

if command -v runsc >/dev/null 2>&1; then ok "gVisor : $(command -v runsc)"
else ko "gVisor absent"; MANQUES+=("gvisor"); fi

if command -v firecracker >/dev/null 2>&1; then ok "Firecracker : $(command -v firecracker)"
else ko "Firecracker absent"; MANQUES+=("firecracker"); fi

KERNEL_IMG="${PROPHET_MICROVM_KERNEL:-/var/lib/prophet/microvm/vmlinux}"
ROOTFS_IMG="${PROPHET_MICROVM_ROOTFS:-/var/lib/prophet/microvm/rootfs.ext4}"
if [ -f "$KERNEL_IMG" ] && [ -f "$ROOTFS_IMG" ]; then ok "images de microVM présentes"
else ko "images de microVM absentes ($KERNEL_IMG, $ROOTFS_IMG)"; MANQUES+=("images-microvm"); fi

# Landlock : la version d'ABI se lit par l'appel système, pas par un fichier.
if command -v python3 >/dev/null 2>&1 && python3 - <<'PY' 2>/dev/null
import ctypes, sys
libc = ctypes.CDLL("libc.so.6", use_errno=True)
sys.exit(0 if libc.syscall(444, None, 0, 1) > 0 else 1)
PY
then ok "Landlock disponible"; else ko "Landlock absent"; MANQUES+=("landlock"); fi

if [ -f /sys/fs/cgroup/cgroup.controllers ]; then ok "cgroups v2 montés"
else ko "cgroups v2 absents"; MANQUES+=("cgroups-v2"); fi

if command -v btrfs >/dev/null 2>&1; then ok "btrfs-progs présent"
else info "btrfs-progs absent : le repli portable sera employé"; fi

if command -v nix >/dev/null 2>&1; then ok "Nix : $(nix --version 2>/dev/null | head -1)"
else ko "Nix absent : l'image ne peut pas être construite"; MANQUES+=("nix"); fi

if command -v cargo >/dev/null 2>&1; then ok "Rust : $(cargo --version)"
else ko "cargo absent : rien ne peut être compilé"; exit 2; fi

echo
if [ ${#MANQUES[@]} -eq 0 ]; then
  echo "Machine complète : les neuf tâches bloquées sont vérifiables ici."
else
  echo "Manquent : ${MANQUES[*]}"
  echo "Les tests correspondants seront ignorés et signalés comme tels, jamais comptés réussis."
fi
echo

[ "$PROBE_ONLY" = "1" ] && exit 0

# --- 2. Ce que la machine peut vérifier ---
{
  echo "# Vérification de Prophet OS sur machine complète"
  echo
  echo "- Date : $(date -Is)"
  echo "- Machine : $(uname -srm)"
  echo "- Manques : ${MANQUES[*]:-aucun}"
  echo
} > "$RAPPORT"

lancer() {
  local titre="$1"; shift
  echo "→ $titre"
  echo "## $titre" >> "$RAPPORT"
  echo '```' >> "$RAPPORT"
  if "$@" >> "$RAPPORT" 2>&1; then
    ok "$titre"
    echo '```' >> "$RAPPORT"
    echo >> "$RAPPORT"
    return 0
  fi
  ko "$titre"
  echo '```' >> "$RAPPORT"
  echo >> "$RAPPORT"
  return 1
}

ECHECS=0
lancer "Suite sans privilèges" cargo test --workspace || ECHECS=$((ECHECS+1))
lancer "Tests exigeant du matériel (--ignored)" cargo test --workspace -- --ignored --test-threads=1 || ECHECS=$((ECHECS+1))
lancer "Latences en binaire optimisé" cargo test --release -p capd performance -- --nocapture || ECHECS=$((ECHECS+1))
lancer "Suite adversariale" cargo test -p bench --test adversarial -- --nocapture || ECHECS=$((ECHECS+1))
lancer "Démonstration M8" cargo test -p agentd --test demo_m8 -- --nocapture || ECHECS=$((ECHECS+1))
lancer "Sonde de capacités" cargo run --quiet --release -p prophet-cli -- status || ECHECS=$((ECHECS+1))

if command -v nix >/dev/null 2>&1; then
  lancer "Construction de l'image NixOS" nix build .#nixosConfigurations.prophet.config.system.build.toplevel || ECHECS=$((ECHECS+1))
else
  echo "→ Construction de l'image : ignorée, Nix absent"
  echo "## Construction de l'image" >> "$RAPPORT"
  echo "Ignorée : Nix absent de la machine." >> "$RAPPORT"
  echo >> "$RAPPORT"
fi

echo
echo "Rapport écrit dans $RAPPORT"
if [ "$ECHECS" -eq 0 ]; then
  echo "Tout ce que cette machine peut vérifier est vert."
  exit 0
fi
echo "$ECHECS étape(s) en échec : voir le rapport."
exit 1
