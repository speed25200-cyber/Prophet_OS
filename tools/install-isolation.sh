#!/usr/bin/env bash
# Installe ce qui débloque un niveau d'isolation, et rien d'autre.
#
# Un niveau ne se débloque pas en installant « des paquets utiles » : chaque niveau a exactement
# ce qu'il lui faut, et installer le reste donne l'illusion d'avoir avancé. D'où un script par
# besoin, qui dit à la fin ce qui est réellement atteignable.
#
# Usage :
#   sudo ./tools/install-isolation.sh gvisor    # niveau 1
#   sudo ./tools/install-isolation.sh microvm   # niveau 2 — exige /dev/kvm
#   sudo ./tools/install-isolation.sh all
#
# Idempotent : ce qui est déjà là n'est pas réinstallé.

set -uo pipefail

CIBLE="${1:-all}"
MICROVM_DIR="${PROPHET_MICROVM_DIR:-/var/lib/prophet/microvm}"
ARCH="$(uname -m)"

ok()   { printf '  \033[32m✓\033[0m %s\n' "$1"; }
ko()   { printf '  \033[31m✗\033[0m %s\n' "$1"; }
info() { printf '  · %s\n' "$1"; }

sudo_si_besoin() {
  if [ "$(id -u)" = "0" ]; then "$@"; else sudo "$@"; fi
}

installer_gvisor() {
  echo "gVisor — débloque le niveau 1"
  if command -v runsc >/dev/null 2>&1; then
    ok "déjà présent : $(runsc --version 2>&1 | head -1)"
    return 0
  fi
  curl -fsSL https://gvisor.dev/archive.key \
    | sudo_si_besoin gpg --dearmor -o /usr/share/keyrings/gvisor-archive-keyring.gpg || {
    ko "clé de dépôt introuvable"; return 1; }
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/gvisor-archive-keyring.gpg] https://storage.googleapis.com/gvisor/releases release main" \
    | sudo_si_besoin tee /etc/apt/sources.list.d/gvisor.list >/dev/null
  sudo_si_besoin apt-get update -qq || { ko "apt-get update a échoué"; return 1; }
  sudo_si_besoin apt-get install -y -qq runsc >/dev/null || { ko "installation de runsc refusée"; return 1; }
  ok "$(runsc --version 2>&1 | head -1)"
}

installer_firecracker() {
  if command -v firecracker >/dev/null 2>&1; then
    ok "Firecracker déjà présent : $(firecracker --version 2>&1 | head -1)"
    return 0
  fi
  local tag url tmp
  tag=$(curl -fsSL https://api.github.com/repos/firecracker-microvm/firecracker/releases/latest \
        | grep -m1 '"tag_name"' | cut -d'"' -f4)
  [ -z "$tag" ] && { ko "version de Firecracker introuvable"; return 1; }
  url="https://github.com/firecracker-microvm/firecracker/releases/download/${tag}/firecracker-${tag}-${ARCH}.tgz"
  tmp=$(mktemp -d)
  curl -fsSL "$url" -o "$tmp/fc.tgz" || { ko "téléchargement de Firecracker ${tag} impossible"; rm -rf "$tmp"; return 1; }
  tar -xzf "$tmp/fc.tgz" -C "$tmp" || { ko "archive de Firecracker illisible"; rm -rf "$tmp"; return 1; }
  local bin
  bin=$(find "$tmp" -name "firecracker-${tag}-${ARCH}" -type f | head -1)
  [ -z "$bin" ] && { ko "binaire absent de l'archive"; rm -rf "$tmp"; return 1; }
  sudo_si_besoin install -m 0755 "$bin" /usr/local/bin/firecracker
  rm -rf "$tmp"
  ok "Firecracker ${tag}"
}

installer_images() {
  local noyau="$MICROVM_DIR/vmlinux" racine="$MICROVM_DIR/rootfs.ext4"
  if [ -f "$noyau" ] && [ -f "$racine" ]; then
    ok "images d'invité déjà présentes dans $MICROVM_DIR"
    return 0
  fi
  sudo_si_besoin mkdir -p "$MICROVM_DIR"

  # Les artefacts publiés par le projet Firecracker pour ses propres essais. Le chemin porte une
  # version qui bouge : on liste plutôt que de coder une URL en dur, et on dit clairement quand la
  # liste ne donne rien, au lieu de laisser croire que le niveau 2 est prêt.
  local base="https://s3.amazonaws.com/spec.ccfc.min" prefixe trouve
  for serie in v1.12 v1.11 v1.10; do
    prefixe="firecracker-ci/${serie}/${ARCH}"
    trouve=$(curl -fsSL "${base}/?list-type=2&prefix=${prefixe}/vmlinux-" 2>/dev/null \
             | grep -o "${prefixe}/vmlinux-[0-9.]*" | sort -V | tail -1)
    [ -n "$trouve" ] && break
  done
  if [ -z "$trouve" ]; then
    ko "aucune image de noyau publiée n'a pu être listée"
    info "le niveau 2 restera non vérifiable : ce n'est pas un échec silencieux, c'est dit ici"
    return 1
  fi
  curl -fsSL "${base}/${trouve}" -o /tmp/vmlinux.prophet \
    || { ko "téléchargement du noyau invité impossible"; return 1; }
  sudo_si_besoin install -m 0644 /tmp/vmlinux.prophet "$noyau"
  rm -f /tmp/vmlinux.prophet
  ok "noyau invité : $(basename "$trouve")"

  local racine_distante
  racine_distante=$(curl -fsSL "${base}/?list-type=2&prefix=${prefixe}/ubuntu-" 2>/dev/null \
                    | grep -o "${prefixe}/ubuntu-[0-9.]*\.\(squashfs\|ext4\)" | sort -V | tail -1)
  if [ -z "$racine_distante" ]; then
    ko "aucune image de racine publiée n'a pu être listée"
    sudo_si_besoin rm -f "$noyau"
    info "le noyau seul ne démarre rien : il est retiré plutôt que de laisser un jeu incomplet"
    return 1
  fi
  curl -fsSL "${base}/${racine_distante}" -o /tmp/rootfs.prophet \
    || { ko "téléchargement de la racine invitée impossible"; sudo_si_besoin rm -f "$noyau"; return 1; }
  sudo_si_besoin install -m 0644 /tmp/rootfs.prophet "$racine"
  rm -f /tmp/rootfs.prophet
  ok "racine invitée : $(basename "$racine_distante")"
}

installer_microvm() {
  echo "Firecracker et images d'invité — débloquent le niveau 2"
  if [ ! -e /dev/kvm ]; then
    ko "/dev/kvm absent : rien de ce qui suit ne débloquerait quoi que ce soit"
    if grep -qE "hypervisor" /proc/cpuinfo 2>/dev/null; then
      info "cette machine est virtualisée ; le niveau 2 exige la virtualisation imbriquée, que"
      info "les hébergeurs de machines virtuelles n'offrent généralement pas"
    fi
    info "rien n'est installé : un moniteur sans KVM donnerait l'illusion d'avoir avancé"
    return 1
  fi
  ok "/dev/kvm présent"
  installer_firecracker || return 1
  installer_images || return 1
}

case "$CIBLE" in
  gvisor)  installer_gvisor ;;
  microvm) installer_microvm ;;
  all)     installer_gvisor; echo; installer_microvm ;;
  *) echo "cible inconnue : $CIBLE (attendues : gvisor, microvm, all)" >&2; exit 2 ;;
esac

echo
echo "Niveaux atteignables après cette installation :"
echo "  niveau 0 : toujours (espaces de noms)"
if command -v runsc >/dev/null 2>&1; then echo "  niveau 1 : oui"; else echo "  niveau 1 : non (gVisor absent)"; fi
if [ -e /dev/kvm ] && command -v firecracker >/dev/null 2>&1 \
   && [ -f "$MICROVM_DIR/vmlinux" ] && [ -f "$MICROVM_DIR/rootfs.ext4" ]; then
  echo "  niveau 2 : oui"
else
  echo "  niveau 2 : non"
fi
