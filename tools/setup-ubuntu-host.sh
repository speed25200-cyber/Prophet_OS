#!/usr/bin/env bash
# Prépare un serveur Ubuntu pour vérifier Prophet OS.
#
# Installe ce qui manque et rien d'autre : Rust, just, gVisor, btrfs. Le script dit ce qu'il va
# faire avant de le faire, et n'installe jamais deux fois la même chose.
#
# Ce qu'il n'installe pas, et pourquoi :
# - Firecracker et les images de microVM : sans /dev/kvm ils ne servent à rien, et une machine
#   virtuelle d'hébergeur n'offre pas la virtualisation imbriquée.
# - Nix : utile seulement pour construire l'image, ce qui est une étape séparée.
#
# Usage : sudo ./tools/setup-ubuntu-host.sh

set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
  echo "Ce script installe des paquets : relancez-le avec sudo." >&2
  exit 1
fi

echo "Préparation d'un hôte Ubuntu pour Prophet OS"
echo

# --- Mémoire ---
MEM_MIB=$(awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo)
SWAP_MIB=$(awk '/SwapTotal/ {print int($2/1024)}' /proc/meminfo)
if [ "$((MEM_MIB + SWAP_MIB))" -lt 4096 ]; then
  echo "→ ${MEM_MIB} Mio de mémoire, ${SWAP_MIB} Mio d'échange : ajout de 4 Gio d'échange"
  echo "  (la compilation est le poste gourmand ; les tests, eux, tiennent dans peu de mémoire)"
  if [ ! -f /swapfile ]; then
    fallocate -l 4G /swapfile
    chmod 600 /swapfile
    mkswap /swapfile >/dev/null
    swapon /swapfile
    grep -q '^/swapfile' /etc/fstab || echo '/swapfile none swap sw 0 0' >> /etc/fstab
    echo "  fichier d'échange ajouté et rendu permanent"
  else
    swapon /swapfile 2>/dev/null || true
    echo "  /swapfile existait déjà"
  fi
else
  echo "→ mémoire suffisante : ${MEM_MIB} Mio + ${SWAP_MIB} Mio d'échange"
fi

# --- Paquets de base ---
echo "→ paquets de base"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq build-essential pkg-config libssl-dev curl git btrfs-progs jq >/dev/null
echo "  build-essential, git, btrfs-progs installés"

# --- Rust ---
if command -v cargo >/dev/null 2>&1; then
  echo "→ Rust déjà présent : $(cargo --version)"
else
  echo "→ installation de Rust"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal >/dev/null
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
  echo "  $(cargo --version)"
fi

# --- just ---
if command -v just >/dev/null 2>&1; then
  echo "→ just déjà présent"
else
  echo "→ installation de just"
  # shellcheck disable=SC1091
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  cargo install just --quiet 2>/dev/null || apt-get install -y -qq just >/dev/null
  echo "  just installé"
fi

# --- gVisor : c'est lui qui débloque le niveau 1 ---
# Délégué au script partagé, pour que cette machine et l'intégration continue débloquent le
# niveau par exactement le même chemin. Sans cela, ce que la CI prouve ne dirait rien d'ici.
"$(dirname "$0")/install-isolation.sh" gvisor

# --- Constat sur le niveau 2 ---
echo
if [ -e /dev/kvm ]; then
  echo "→ /dev/kvm présent : le niveau 2 est envisageable."
  echo "  Il reste à fournir les images d'invité dans /var/lib/prophet/microvm."
else
  echo "→ /dev/kvm absent."
  if grep -qE "hypervisor" /proc/cpuinfo 2>/dev/null; then
    echo "  Cette machine est elle-même virtualisée. Le niveau 2 exige la virtualisation"
    echo "  imbriquée, que les hébergeurs de machines virtuelles n'offrent généralement pas."
    echo "  Le niveau 2 restera non vérifié ici ; il faut un serveur dédié ou une machine physique."
  fi
fi

echo
echo "Prêt. Étapes suivantes :"
echo "  just probe-host     # ce que la machine offre désormais"
echo "  just verify-host    # suite complète, et rapport"
