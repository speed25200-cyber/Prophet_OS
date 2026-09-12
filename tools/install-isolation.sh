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
#   sudo ./tools/install-isolation.sh userns    # lève la restriction AppArmor, si autorisé
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

# --- Restriction des espaces de noms par la distribution ---
#
# Ubuntu 24.04 et suivantes refusent, par AppArmor, qu'un programme absent de leurs profils exécute
# quoi que ce soit dans l'espace de noms qu'il vient de créer. La création réussit, l'exécution
# non : le système paraît capable et ne l'est pas, et l'échec arrive sous la forme d'un EACCES nu
# dans un composant qui n'y est pour rien. Les niveaux 0 et 1 en dépendent tous les deux.
RESTRICTION=/proc/sys/kernel/apparmor_restrict_unprivileged_userns

restriction_active() {
  [ -f "$RESTRICTION" ] && [ "$(cat "$RESTRICTION" 2>/dev/null)" = "1" ]
}

traiter_la_restriction() {
  if ! restriction_active; then
    return 0
  fi
  echo "Restriction des espaces de noms (AppArmor)"
  ko "cette distribution interdit d'exécuter dans un espace de noms non privilégié"
  info "les niveaux 0 et 1 échoueront sur EACCES tant qu'elle est active"
  if [ "${PROPHET_AUTORISER_USERNS:-}" != "1" ]; then
    info "rien n'est modifié : cette restriction protège la machine, et la lever est une"
    info "décision qui vous revient. Pour la lever, au choix :"
    info "  sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0        (jusqu'au redémarrage)"
    info "  PROPHET_AUTORISER_USERNS=1 sudo -E ./tools/install-isolation.sh all  (et de façon durable)"
    return 1
  fi
  sudo_si_besoin sysctl -w kernel.apparmor_restrict_unprivileged_userns=0 >/dev/null \
    || { ko "le réglage a été refusé"; return 1; }
  echo "kernel.apparmor_restrict_unprivileged_userns = 0" \
    | sudo_si_besoin tee /etc/sysctl.d/99-prophet-userns.conf >/dev/null
  ok "restriction levée, et rendue durable par /etc/sysctl.d/99-prophet-userns.conf"
  info "pour revenir en arrière : supprimez ce fichier et remettez le réglage à 1"
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
  # version qui bouge : on liste plutôt que de coder une URL en dur.
  local base="https://s3.amazonaws.com/spec.ccfc.min"

  # Les clés sont extraites du XML, pas grattées dans le texte. Un motif approximatif attrapait
  # « vmlinux-6.1.128. » — le début de « vmlinux-6.1.128.config » — et élisait une clé qui
  # n'existe pas, d'où un téléchargement en 404 et un niveau 2 déclaré indisponible pour une
  # raison fausse.
  lister_cles() {
    curl -fsSL "${base}/?list-type=2&prefix=$1&max-keys=1000" 2>/dev/null \
      | tr '<' '\n' | sed -n 's|^Key>||p'
  }

  # Une archive de 404 pèse quelques centaines d'octets. Un noyau, plusieurs mégaoctets.
  telecharger() {
    local url="$1" dest="$2" minimum="$3" nom="$4"
    curl -fsSL "$url" -o "$dest" || { ko "téléchargement de $nom impossible"; return 1; }
    local taille
    taille=$(stat -c %s "$dest" 2>/dev/null || echo 0)
    if [ "$taille" -lt "$minimum" ]; then
      ko "$nom fait $taille octets : ce n'est pas l'image attendue"
      rm -f "$dest"
      return 1
    fi
    return 0
  }

  local serie cle_noyau cle_racine
  for serie in v1.12 v1.11 v1.10; do
    local prefixe="firecracker-ci/${serie}/${ARCH}"
    cle_noyau=$(lister_cles "${prefixe}/vmlinux-" \
      | grep -E "/vmlinux-[0-9]+\.[0-9]+\.[0-9]+$" | sort -V | tail -1)
    cle_racine=$(lister_cles "${prefixe}/ubuntu-" \
      | grep -E "/ubuntu-[0-9]+\.[0-9]+\.(squashfs|ext4)$" | sort -V | tail -1)
    if [ -n "$cle_noyau" ] && [ -n "$cle_racine" ]; then
      break
    fi
  done

  if [ -z "$cle_noyau" ] || [ -z "$cle_racine" ]; then
    ko "aucune paire noyau + racine publiée n'a pu être listée"
    info "le niveau 2 restera non vérifiable : ce n'est pas un échec silencieux, c'est dit ici"
    return 1
  fi

  telecharger "${base}/${cle_noyau}" /tmp/vmlinux.prophet 1048576 "le noyau invité" || return 1
  telecharger "${base}/${cle_racine}" /tmp/rootfs.prophet 1048576 "la racine invitée" || {
    rm -f /tmp/vmlinux.prophet
    return 1
  }
  sudo_si_besoin install -m 0644 /tmp/vmlinux.prophet "$noyau"
  sudo_si_besoin install -m 0644 /tmp/rootfs.prophet "$racine"
  rm -f /tmp/vmlinux.prophet /tmp/rootfs.prophet
  ok "noyau invité : $(basename "$cle_noyau")"
  ok "racine invitée : $(basename "$cle_racine")"
}

# Le droit d'ouvrir /dev/kvm, distinct de sa présence.
#
# Le fichier appartient au groupe kvm. Un utilisateur qui n'en fait pas partie obtient EACCES au
# moment de démarrer la machine virtuelle — trop tard, et sous la forme d'une erreur du moniteur
# qui n'en est pas la cause.
kvm_accessible() {
  [ -r /dev/kvm ] && [ -w /dev/kvm ]
}

traiter_l_acces_kvm() {
  if kvm_accessible; then
    ok "/dev/kvm accessible"
    return 0
  fi
  ko "/dev/kvm existe mais n'est pas ouvrable par $(id -un)"
  if [ "${PROPHET_AUTORISER_KVM:-}" != "1" ]; then
    info "rien n'est modifié : élargir l'accès à l'hyperviseur est une décision qui vous revient."
    info "Pour l'accorder, au choix :"
    info "  sudo usermod -aG kvm $(id -un)    puis rouvrir une session (propre et durable)"
    info "  PROPHET_AUTORISER_KVM=1 sudo -E ./tools/install-isolation.sh microvm   (immédiat)"
    return 1
  fi
  sudo_si_besoin usermod -aG kvm "$(id -un)" 2>/dev/null || true
  # L'appartenance à un groupe ne prend effet qu'à la session suivante ; sur une machine jetable
  # on ouvre le nœud directement, faute de quoi l'autorisation ne servirait à rien aujourd'hui.
  sudo_si_besoin chmod 0666 /dev/kvm || { ko "l'accès a été refusé"; return 1; }
  if kvm_accessible; then
    ok "/dev/kvm rendu accessible"
    info "sur une machine durable, préférez l'appartenance au groupe kvm à ce mode d'accès"
    return 0
  fi
  ko "/dev/kvm reste inaccessible"
  return 1
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
  traiter_l_acces_kvm || return 1
  installer_firecracker || return 1
  installer_images || return 1
}

# Le résultat est retenu : sans cela, le récapitulatif qui suit deviendrait le dernier code de
# sortie du script, et une installation ratée se déclarerait réussie — exactement le silence que
# ce projet passe son temps à traquer ailleurs.
RESULTAT=0
case "$CIBLE" in
  gvisor)  installer_gvisor || RESULTAT=1; echo; traiter_la_restriction || RESULTAT=1 ;;
  microvm) installer_microvm || RESULTAT=1 ;;
  userns)  traiter_la_restriction || RESULTAT=1 ;;
  all)     installer_gvisor || RESULTAT=1; echo; installer_microvm || RESULTAT=1
           echo; traiter_la_restriction || RESULTAT=1 ;;
  *) echo "cible inconnue : $CIBLE (attendues : gvisor, microvm, userns, all)" >&2; exit 2 ;;
esac

echo
if restriction_active; then
  echo
  echo "Aucun niveau ne fonctionnera tant que la restriction AppArmor est active :"
  echo "  l'espace de noms se crée, mais rien ne s'exécute dedans (EACCES)."
fi

echo "Niveaux atteignables après cette installation :"
if restriction_active; then
  echo "  niveau 0 : non (restriction AppArmor)"
else
  echo "  niveau 0 : oui (espaces de noms)"
fi
if command -v runsc >/dev/null 2>&1; then echo "  niveau 1 : oui"; else echo "  niveau 1 : non (gVisor absent)"; fi
if kvm_accessible && command -v firecracker >/dev/null 2>&1 \
   && [ -f "$MICROVM_DIR/vmlinux" ] && [ -f "$MICROVM_DIR/rootfs.ext4" ]; then
  echo "  niveau 2 : oui"
else
  echo "  niveau 2 : non"
fi

exit "$RESULTAT"
