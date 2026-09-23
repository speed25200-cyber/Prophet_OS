#!/usr/bin/env bash
# Vérifie Prophet OS sur une machine complète.
#
# Ce script existe parce que l'environnement de construction ne peut pas tout vérifier : il n'a ni
# KVM, ni gVisor, ni Landlock, ni cgroups v2. Neuf tâches du plan en dépendent. Lancé sur une
# machine équipée, il répond à une seule question : **ces neuf-là tiennent-elles ?**
#
# Usage :
#   ./tools/verify-on-host.sh            # sonde, niveaux d'isolation, puis suite complète
#   ./tools/verify-on-host.sh --probe    # sonde seule, sans rien exécuter
#   ./tools/verify-on-host.sh --niveaux  # sonde + niveaux d'isolation seulement
#
# Le mode `--niveaux` ne compile que `sandboxd`. Sur une petite machine, c'est la différence entre
# obtenir la réponse qui compte et se faire tuer par le gestionnaire de mémoire avant de l'obtenir.
#
# Il ne modifie rien en dehors de son répertoire de rapport, et n'installe rien.

set -uo pipefail

RAPPORT="${RAPPORT:-verification-$(date +%Y-%m-%d-%H%M%S).md}"
MODE=complet
case "${1:-}" in
  --probe)   MODE=sonde ;;
  --niveaux) MODE=niveaux ;;
  "")        ;;
  *) echo "argument inconnu : $1 (attendus : --probe, --niveaux)" >&2; exit 2 ;;
esac

ok()    { printf '  \033[32m✓\033[0m %s\n' "$1"; }
ko()    { printf '  \033[31m✗\033[0m %s\n' "$1"; }
hors()  { printf '  \033[33m–\033[0m %s\n' "$1"; }
info()  { printf '  · %s\n' "$1"; }

echo "Prophet OS — vérification sur machine complète"
echo

# --- 1. Ce que la machine offre ---
echo "Sonde du matériel et du noyau"
MANQUES=()

# Présence et accès sont deux choses. Le nœud appartient au groupe kvm, et le constater présent
# pendant qu'il est refusé fait lancer des tests qui échoueront pour une raison qu'on croira
# ailleurs.
if [ -r /dev/kvm ] && [ -w /dev/kvm ]; then ok "/dev/kvm accessible"; KVM=1
elif [ -e /dev/kvm ]; then
  ko "/dev/kvm présent mais refusé à $(id -un) (groupe kvm)"; KVM=0
  MANQUES+=("acces-kvm")
else
  ko "/dev/kvm absent"; KVM=0
  MANQUES+=("kvm")
  # Sur une machine virtuelle d'hébergeur, l'absence de KVM n'est pas une configuration
  # manquante : c'est que la virtualisation imbriquée n'est pas offerte. Le dire évite de
  # chercher longtemps une option qui n'existe pas.
  if [ -d /sys/hypervisor ] || grep -qE "hypervisor" /proc/cpuinfo 2>/dev/null; then
    info "cette machine est elle-même virtualisée : le niveau 2 exige la virtualisation imbriquée,"
    info "que la plupart des hébergeurs de VM n'offrent pas. Un serveur dédié ou une machine"
    info "physique est nécessaire pour vérifier le niveau 2."
  fi
fi

if command -v runsc >/dev/null 2>&1; then ok "gVisor : $(command -v runsc)"; GVISOR=1
else ko "gVisor absent"; GVISOR=0; MANQUES+=("gvisor"); fi

if command -v firecracker >/dev/null 2>&1; then ok "Firecracker : $(command -v firecracker)"; FC=1
else ko "Firecracker absent"; FC=0; MANQUES+=("firecracker"); fi

KERNEL_IMG="${PROPHET_MICROVM_KERNEL:-/var/lib/prophet/microvm/vmlinux}"
ROOTFS_IMG="${PROPHET_MICROVM_ROOTFS:-/var/lib/prophet/microvm/rootfs.ext4}"
if [ -f "$KERNEL_IMG" ] && [ -f "$ROOTFS_IMG" ]; then ok "images de microVM présentes"; IMAGES=1
else ko "images de microVM absentes ($KERNEL_IMG, $ROOTFS_IMG)"; IMAGES=0; MANQUES+=("images-microvm"); fi

# Landlock : la version d'ABI se lit par l'appel système, pas par un fichier.
if command -v python3 >/dev/null 2>&1 && python3 - <<'PY' 2>/dev/null
import ctypes, sys
libc = ctypes.CDLL("libc.so.6", use_errno=True)
sys.exit(0 if libc.syscall(444, None, 0, 1) > 0 else 1)
PY
then ok "Landlock disponible"; LANDLOCK=1; else ko "Landlock absent"; LANDLOCK=0; MANQUES+=("landlock"); fi

# Espaces de noms utilisateur : `unshare` figure dans les profils AppArmor d'Ubuntu et passe même
# sous restriction ; c'est le réglage qui dit si un programme quelconque — l'amorçage de
# sandboxd, un binaire d'essai — pourra s'en servir.
restriction_userns=$(cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns 2>/dev/null || echo 0)
if unshare --user true 2>/dev/null && [ "$restriction_userns" != "1" ]; then
  ok "espaces de noms utilisateur utilisables"; USERNS=1
else
  ko "espaces de noms utilisateur inutilisables par un programme quelconque"; USERNS=0
  MANQUES+=("userns")
fi

if [ -f /sys/fs/cgroup/cgroup.controllers ]; then ok "cgroups v2 montés"
else ko "cgroups v2 absents"; MANQUES+=("cgroups-v2"); fi

if command -v btrfs >/dev/null 2>&1; then ok "btrfs-progs présent"
else info "btrfs-progs absent : le repli portable sera employé"; fi

if command -v nix >/dev/null 2>&1; then ok "Nix : $(nix --version 2>/dev/null | head -1)"
else ko "Nix absent : l'image ne peut pas être construite"; MANQUES+=("nix"); fi

if command -v cargo >/dev/null 2>&1; then ok "Rust : $(cargo --version)"
else ko "cargo absent : rien ne peut être compilé"; exit 2; fi

# --- Mémoire : la compilation est le poste le plus gourmand, pas les tests ---
MEM_MIB=$(awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo 2>/dev/null || echo 0)
SWAP_MIB=$(awk '/SwapTotal/ {print int($2/1024)}' /proc/meminfo 2>/dev/null || echo 0)
JOBS=""
PETITE_MACHINE=0
if [ "$MEM_MIB" -gt 0 ]; then
  info "mémoire : ${MEM_MIB} Mio, dont ${SWAP_MIB} Mio d'échange"
  if [ "$((MEM_MIB + SWAP_MIB))" -lt 4096 ]; then
    ko "moins de 4 Gio au total : la compilation risque de se faire tuer"
    echo "     La dépendance de politique est volumineuse. Deux remèdes, au choix :"
    echo "       sudo fallocate -l 4G /swapfile && sudo chmod 600 /swapfile \\"
    echo "         && sudo mkswap /swapfile && sudo swapon /swapfile"
    echo "       ou laisser ce script compiler en série (il le fait automatiquement)."
    JOBS="-j 1"
    export CARGO_BUILD_JOBS=1
    PETITE_MACHINE=1
  else
    ok "mémoire suffisante pour compiler"
  fi
fi

echo
if [ ${#MANQUES[@]} -eq 0 ]; then
  echo "Machine complète : les neuf tâches bloquées sont vérifiables ici."
else
  echo "Manquent : ${MANQUES[*]}"
  echo "Les tests correspondants ne seront pas lancés, et seront signalés comme non vérifiables,"
  echo "jamais comptés réussis ni comptés en échec."
fi
if [ "$PETITE_MACHINE" = "1" ] && [ "$MODE" = "complet" ]; then
  echo
  info "machine à mémoire courte : les niveaux d'isolation sont vérifiés en premier, avant la"
  info "compilation longue, pour que la réponse qui compte arrive même si la suite n'aboutit pas."
fi
echo

[ "$MODE" = "sonde" ] && exit 0

# --- 2. Ce que la machine peut vérifier ---
{
  echo "# Vérification de Prophet OS sur machine complète"
  echo
  echo "- Date : $(date -Is)"
  echo "- Machine : $(uname -srm)"
  echo "- Mémoire : ${MEM_MIB} Mio + ${SWAP_MIB} Mio d'échange"
  echo "- Manques : ${MANQUES[*]:-aucun}"
  echo
} > "$RAPPORT"

ECHECS=0
NON_VERIFIES=()

lancer() {
  local titre="$1"; shift
  local sortie
  sortie=$(mktemp)
  echo "→ $titre"
  echo "## $titre" >> "$RAPPORT"
  echo '```' >> "$RAPPORT"
  if "$@" > "$sortie" 2>&1; then
    cat "$sortie" >> "$RAPPORT"
    ok "$titre"
    # Ce qu'un essai réussi mesure (« mesure : … ») atteint le journal : sans cela, la preuve
    # d'un objectif chiffré ne se lirait que dans un rapport qu'on n'ouvre pas.
    # Avec --nocapture, libtest écrit le nom de l'essai sur la ligne où arrive la première
    # sortie : la mesure est cherchée partout dans la ligne, pas seulement au début.
    grep -ho 'mesure : .*' "$sortie" | sed 's/^/    /' || true
    rm -f "$sortie"
    echo '```' >> "$RAPPORT"
    echo >> "$RAPPORT"
    return 0
  fi
  cat "$sortie" >> "$RAPPORT"
  rm -f "$sortie"
  ko "$titre"
  echo '```' >> "$RAPPORT"
  echo >> "$RAPPORT"
  # Le diagnostic doit atteindre celui qui lit, et il ne lit pas toujours le fichier : sur une
  # machine distante, dans un journal d'intégration, le rapport est parfois hors d'atteinte. Ce
  # qui a échoué est donc répété ici, borné pour rester lisible.
  echo "    ↓ ce que l'étape a dit (30 dernières lignes) :"
  tail -30 "$RAPPORT" | grep -v '^```$' | sed 's/^/    | /'
  return 1
}

# --- 3. Les niveaux d'isolation, marqueur par marqueur ---
#
# Chaque test qui exige du matériel porte son marqueur : `#[ignore = "needs_gvisor"]`. On lit ces
# marqueurs dans les sources plutôt que d'en tenir une liste ici, pour qu'un test ajouté demain
# soit pris en compte sans que ce script soit modifié.
#
# Le défaut que cette section corrige : lancer `cargo test -- --ignored` en bloc. Sur une machine
# qui a gVisor mais pas KVM — le cas d'un serveur d'hébergeur, qui n'offre pas la virtualisation
# imbriquée — les tests de niveau 2 échouent, et l'étape entière passe au rouge. On ne voit plus
# que le niveau 1, le seul que cette machine ait débloqué, fonctionne.

tests_marques() {
  find crates -name '*.rs' \( -path '*/tests/*' -o -path '*/src/*' \) 2>/dev/null | sort | while read -r fichier; do
    crate=$(printf '%s' "$fichier" | cut -d/ -f2)
    awk -v crate="$crate" '
      /#\[ignore/ {
        if (match($0, /"[^"]+"/)) { marqueur = substr($0, RSTART + 1, RLENGTH - 2) }
        else { marqueur = "sans-marqueur" }
        attente = 1
        next
      }
      attente && /^[[:space:]]*(pub[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+/ {
        nom = $0
        sub(/^[[:space:]]*(pub[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+/, "", nom)
        sub(/[(<].*$/, "", nom)
        print marqueur "\t" crate "\t" nom
        attente = 0
      }
    ' "$fichier"
  done
}

# Répond : ce marqueur a-t-il son matériel ici ? 0 = oui, 1 = non (avec la raison sur stdout).
# Un marqueur peut être suivi d'une description (`needs_codex_login : PROPHET_TEST_CLIENT…`) :
# seul son premier mot le classe.
materiel_pour() {
  local marqueur="${1%%[ :]*}"
  case "$marqueur" in
    needs_gvisor)
      [ "$GVISOR" = "1" ] && return 0
      echo "gVisor absent"; return 1 ;;
    needs_kvm)
      # Le niveau 2 exige les trois : le module, le moniteur, et les images d'invité.
      manquants=""
      [ "$KVM" = "1" ]    || manquants="$manquants acces-kvm"
      [ "$FC" = "1" ]     || manquants="$manquants firecracker"
      [ "$IMAGES" = "1" ] || manquants="$manquants images-microvm"
      [ -z "$manquants" ] && return 0
      echo "il manque${manquants}"; return 1 ;;
    needs_userns)
      [ "$USERNS" = "1" ] && return 0
      echo "espaces de noms utilisateur inutilisables"; return 1 ;;
    needs_gpu)
      # Un nœud /dev/dri existe sur des machines qui n'ont aucun pilote Vulkan chargé — c'est le
      # cas des coureurs d'intégration. Le constater revenait à annoncer une capacité qu'on n'a
      # pas essayée, et le premier passage de cette sonde a effectivement lancé six tests
      # graphiques sur une machine incapable d'en dessiner un seul.
      #
      # On interroge donc le chargeur Vulkan, qui est le chemin qu'emprunte le rendu lui-même. Un
      # rastériseur logiciel compte : il suffit à exercer la surface, et exiger un GPU matériel
      # écarterait une machine parfaitement capable de la vérifier.
      if command -v vulkaninfo >/dev/null 2>&1 \
         && vulkaninfo --summary 2>/dev/null | grep -qiE "deviceName|llvmpipe|lavapipe"; then
        return 0
      fi
      echo "aucun périphérique Vulkan utilisable (un nœud /dev/dri ne suffit pas)"; return 1 ;;
    needs_network)
      # Le dépôt de poids du catalogue du système (ADR 0046) : joignable, ou l'essai n'a pas
      # de sens. On le demande à son API, qui répond vite et sans rien télécharger.
      if command -v curl >/dev/null 2>&1 \
         && [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 10 \
               https://huggingface.co/api/models/Qwen/Qwen3-0.6B-GGUF)" = "200" ]; then
        return 0
      fi
      echo "Hugging Face injoignable d'ici"; return 1 ;;
    needs_claude_login|needs_codex_login|needs_gemini_login)
      # Une session de compte ne se sonde pas : l'affirmer serait deviner.
      echo "exige un compte connecté, ce qu'aucune sonde ne peut établir"; return 1 ;;
    *)
      # Un marqueur inconnu n'est jamais supposé satisfait : le silence prudent vaut mieux
      # qu'un vert obtenu par défaut.
      echo "marqueur inconnu de ce script"; return 1 ;;
  esac
}

etape_niveaux() {
  local marques
  marques=$(tests_marques)
  if [ -z "$marques" ]; then
    info "aucun test marqué trouvé"
    return 0
  fi

  local crates_concernes
  crates_concernes=$(printf '%s\n' "$marques" | cut -f2 | sort -u)
  local args_paquets=()
  local c
  for c in $crates_concernes; do args_paquets+=(-p "$c"); done

  lancer "Compilation des tests matériels" \
    cargo build "${args_paquets[@]}" --tests $JOBS || {
    ko "les tests matériels ne compilent pas ; rien ne peut être conclu des niveaux"
    ECHECS=$((ECHECS + 1))
    return 1
  }
  # Les tests d'agentd et de la CLI lancent les binaires voisins (capd, ledger, sandboxd, le
  # pont, le lanceur de pilotes) : ils doivent exister, et à jour.
  if printf '%s\n' "$crates_concernes" | grep -qxE "agentd|prophet-cli"; then
    lancer "Construction des binaires voisins" \
      cargo build --workspace --bins $JOBS || {
      ko "les binaires voisins ne se construisent pas ; les tests interservices ne peuvent pas tourner"
      ECHECS=$((ECHECS + 1))
      return 1
    }
  fi

  # Le niveau 0 lui-même : ses essais ne portent pas de marqueur et se taisent là où les
  # espaces de noms manquent — ce qui fait verdir « check » sans rien confiner. Ici, où ils
  # existent, on les exige, et Landlock avec eux s'il est dans le noyau.
  if [ "$USERNS" = "1" ]; then
    lancer "Niveau 0 réellement confiné (espaces de noms$([ "$LANDLOCK" = 1 ] && echo ', Landlock'), seccomp)" \
      env PROPHET_EXIGER_ESPACES_DE_NOMS=1 PROPHET_EXIGER_LANDLOCK="$LANDLOCK" \
      cargo test -p sandboxd --test enforcement $JOBS -- --nocapture \
      || ECHECS=$((ECHECS + 1))
  else
    hors "Niveau 0 réellement confiné — non vérifiable : espaces de noms utilisateur inutilisables"
    NON_VERIFIES+=("niveau 0 réellement confiné — espaces de noms utilisateur inutilisables")
  fi

  local marqueur crate nom raison
  while IFS=$'\t' read -r marqueur crate nom; do
    [ -z "$nom" ] && continue
    if raison=$(materiel_pour "$marqueur"); then
      lancer "$nom ($marqueur)" \
        cargo test -p "$crate" $JOBS -- --ignored --test-threads=1 --nocapture "$nom" \
        || ECHECS=$((ECHECS + 1))
    else
      hors "$nom ($marqueur) — non vérifiable : $raison"
      NON_VERIFIES+=("$nom ($marqueur) — $raison")
    fi
  done <<< "$marques"
}

echo "Niveaux d'isolation"
etape_niveaux
echo

if [ "$MODE" = "niveaux" ]; then
  {
    echo "## Non vérifiable sur cette machine"
    echo
    if [ ${#NON_VERIFIES[@]} -eq 0 ]; then
      echo "Rien : tous les tests matériels ont pu être lancés."
    else
      printf -- "- %s\n" "${NON_VERIFIES[@]}"
      echo
      echo "Ces tests n'ont pas été lancés. Ils ne comptent ni comme réussis ni comme échoués."
    fi
    echo
  } >> "$RAPPORT"
  echo "Rapport écrit dans $RAPPORT"
  [ "$ECHECS" -eq 0 ] && { echo "Niveaux : tout ce que cette machine peut vérifier est vert."; exit 0; }
  echo "$ECHECS étape(s) en échec : voir le rapport."
  exit 1
fi

# --- 4. La suite complète ---
#
# La compilation d'abord, séparément : si elle échoue faute de mémoire, autant le savoir avant
# d'attribuer l'échec aux tests.
echo "Suite complète"
lancer "Compilation" cargo build --workspace --tests $JOBS || {
  ko "compilation impossible, le reste est sans objet"
  echo "     Les niveaux d'isolation ci-dessus restent valables : ils ont été vérifiés avant."
  exit 2
}
lancer "Suite sans privilèges" cargo test --workspace $JOBS || ECHECS=$((ECHECS+1))
lancer "Latences en binaire optimisé" cargo test --release -p capd performance $JOBS -- --nocapture || ECHECS=$((ECHECS+1))
lancer "Suite adversariale" cargo test -p bench --test adversarial -- --nocapture || ECHECS=$((ECHECS+1))
lancer "Démonstration M8" cargo test -p agentd --test demo_m8 -- --nocapture || ECHECS=$((ECHECS+1))
lancer "Sonde de capacités" cargo run --quiet --release -p prophet-cli -- status || ECHECS=$((ECHECS+1))

if command -v nix >/dev/null 2>&1; then
  lancer "Construction de l'image NixOS" nix build .#nixosConfigurations.prophet.config.system.build.toplevel || ECHECS=$((ECHECS+1))
else
  echo "→ Construction de l'image : ignorée, Nix absent"
  hors "image NixOS — non vérifiable : Nix absent"
  NON_VERIFIES+=("image NixOS — Nix absent de la machine")
fi

{
  echo "## Non vérifiable sur cette machine"
  echo
  if [ ${#NON_VERIFIES[@]} -eq 0 ]; then
    echo "Rien : tout a pu être lancé."
  else
    printf -- "- %s\n" "${NON_VERIFIES[@]}"
    echo
    echo "Ces vérifications n'ont pas été lancées. Elles ne comptent ni comme réussies ni comme"
    echo "échouées : ce qu'elles établissent reste inconnu sur cette machine."
  fi
  echo
} >> "$RAPPORT"

echo
echo "Rapport écrit dans $RAPPORT"
if [ ${#NON_VERIFIES[@]} -gt 0 ]; then
  echo "Non vérifié ici : ${#NON_VERIFIES[@]} élément(s), détaillés dans le rapport."
fi
if [ "$ECHECS" -eq 0 ]; then
  echo "Tout ce que cette machine peut vérifier est vert."
  exit 0
fi
echo "$ECHECS étape(s) en échec : voir le rapport."
exit 1
