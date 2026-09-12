#!/usr/bin/env bash
# Installe Prophet OS sur un disque, depuis le support d'amorçage.
#
# Ce script efface un disque entier. Il le dit, montre ce qu'il va détruire, et exige que la
# personne recopie le nom du disque avant d'agir. Aucune option ne permet de sauter cette
# confirmation : un installeur qui formate sans qu'on ait relu le nom du disque est un piège, pas
# une commodité.
#
# La disposition qu'il crée découle de `image/modules/immutable.nix` :
#
#   partition 1  ESP            1 GiB   vfat      étiquette prophet-boot
#   partition 2  racine A      24 GiB   ext4      étiquette prophet-a
#   partition 3  racine B      24 GiB   ext4      étiquette prophet-b
#   partition 4  état           32 GiB  LUKS2     étiquette prophet-state-luks → btrfs
#   partition 5  données       le reste LUKS2     étiquette prophet-home-luks  → btrfs
#
# Deux racines parce qu'une mise à jour écrit dans celle qui ne tourne pas : un échec laisse la
# machine sur la précédente. Deux volumes chiffrés distincts parce que l'état des agents et les
# données de l'utilisateur n'ont pas la même durée de vie ni la même valeur.

set -euo pipefail

DISQUE=""
CHIFFRER=1
JUSQU_AU_MONTAGE=0
DEPOT="${PROPHET_SOURCE:-/iso/prophet}"
CIBLE=/mnt

rouge()  { printf '\033[31m%s\033[0m\n' "$*"; }
vert()   { printf '\033[32m%s\033[0m\n' "$*"; }
titre()  { printf '\n\033[1m%s\033[0m\n' "$*"; }
info()   { printf '  %s\n' "$*"; }

mourir() { rouge "$*"; exit 1; }

usage() {
  cat <<'FIN'
Usage : prophet-installer --disque /dev/DISQUE [options]

  --disque CHEMIN      disque à effacer et sur lequel installer (obligatoire)
  --sans-chiffrement   ne pas chiffrer les volumes de données
                       (déconseillé : l'état des agents contient des traces de vos tâches)
  --source CHEMIN      dépôt Prophet OS à installer (défaut : /iso/prophet)
  --jusqu-au-montage   préparer le disque puis s'arrêter, sans installer le système
                       (pour inspecter la disposition avant de s'engager, et pour que
                       l'intégration continue puisse exercer la partie qui touche au disque)
  --aide               ce message

La confirmation n'est jamais contournable : le nom du disque doit être recopié, et la phrase de
passe saisie deux fois. Les réponses peuvent venir de l'entrée standard, ce qui rend le script
scriptable sans le rendre silencieux.

Exemple : prophet-installer --disque /dev/nvme0n1
FIN
}

while [ $# -gt 0 ]; do
  case "$1" in
    --disque) DISQUE="${2:-}"; shift 2 ;;
    --sans-chiffrement) CHIFFRER=0; shift ;;
    --source) DEPOT="${2:-}"; shift 2 ;;
    --jusqu-au-montage) JUSQU_AU_MONTAGE=1; shift ;;
    --aide|-h) usage; exit 0 ;;
    *) usage; mourir "argument inconnu : $1" ;;
  esac
done

# --- 1. Ce qu'il faut avant de toucher au disque ---

titre "Vérifications"

[ "$(id -u)" = "0" ] || mourir "l'installation doit être lancée en root."

# L'UEFI est nécessaire pour poser le chargeur d'amorçage, pas pour préparer un disque. Le
# contrôle ne porte donc que sur le chemin qui installe réellement. Ce n'est pas un contournement :
# une préparation sur une machine sans UEFI est parfaitement licite, c'est l'installation qui ne
# l'est pas.
if [ "$JUSQU_AU_MONTAGE" = "0" ]; then
  if [ ! -d /sys/firmware/efi ]; then
    rouge "cette machine n'a pas démarré en UEFI."
    info "Prophet OS démarre par systemd-boot, qui exige l'UEFI. Sur un PC livré avec Windows,"
    info "l'UEFI est presque toujours disponible : désactivez le « Legacy BIOS » ou le « CSM »"
    info "dans le menu du micrologiciel, puis réamorcez ce support."
    exit 1
  fi
  vert "✓ démarrage UEFI"
fi

[ -n "$DISQUE" ] || { usage; mourir "aucun disque indiqué."; }
[ -b "$DISQUE" ] || mourir "$DISQUE n'est pas un périphérique bloc."

if grep -q "^${DISQUE}" /proc/mounts; then
  mourir "$DISQUE porte un système de fichiers monté. Démontez-le d'abord."
fi

if [ "$JUSQU_AU_MONTAGE" = "0" ]; then
  [ -d "$DEPOT" ] || mourir "source Prophet OS introuvable : $DEPOT"
  [ -f "$DEPOT/flake.nix" ] || mourir "$DEPOT ne ressemble pas au dépôt Prophet OS (flake.nix absent)."
  vert "✓ source : $DEPOT"
fi

# 80 GiB est le minimum que la disposition ci-dessus rend utilisable : deux racines de 24 GiB,
# 32 GiB d'état, et de quoi mettre des données.
TAILLE_OCTETS=$(blockdev --getsize64 "$DISQUE")
TAILLE_GIO=$(( TAILLE_OCTETS / 1024 / 1024 / 1024 ))
[ "$TAILLE_GIO" -ge 80 ] || mourir "$DISQUE fait ${TAILLE_GIO} Gio ; il en faut au moins 80."
vert "✓ disque de ${TAILLE_GIO} Gio"

if [ "$JUSQU_AU_MONTAGE" = "0" ] && ! ping -c1 -W3 cache.nixos.org >/dev/null 2>&1; then
  rouge "cache.nixos.org est injoignable."
  info "L'installation télécharge le système depuis ce cache. Connectez la machine au réseau"
  info "(par câble, ou avec « nmtui » pour le Wi-Fi) puis relancez."
  exit 1
fi
# Un « test && commande » en fin de ligne sortirait du script quand le test est faux, puisque la
# ligne rend alors 1 et que `set -e` veille. La forme longue dit la même chose sans ce piège.
if [ "$JUSQU_AU_MONTAGE" = "0" ]; then
  vert "✓ réseau et cache Nix joignables"
fi

# --- 2. Ce qui va être détruit, et la confirmation ---

titre "Ce que cette installation va effacer"
echo
lsblk -o NAME,SIZE,FSTYPE,LABEL,MOUNTPOINT "$DISQUE" || true
echo
if command -v os-prober >/dev/null 2>&1; then
  AUTRES=$(os-prober 2>/dev/null | grep -i "$DISQUE" || true)
  if [ -n "$AUTRES" ]; then
    rouge "Systèmes d'exploitation détectés sur ce disque :"
    printf '%s\n' "$AUTRES" | sed 's/^/    /'
    info "ils seront effacés, avec tout ce qu'ils contiennent."
    echo
  fi
fi

rouge "TOUT le contenu de $DISQUE sera détruit. Cette opération est irréversible."
echo
printf 'Recopiez le nom du disque pour confirmer (%s), ou Ctrl-C pour renoncer : ' "$DISQUE"
read -r REPONSE
[ "$REPONSE" = "$DISQUE" ] || mourir "confirmation refusée : « $REPONSE » ne correspond pas. Rien n'a été touché."

# --- 3. La phrase de passe, avant tout écriture ---

PHRASE=""
if [ "$CHIFFRER" = "1" ]; then
  titre "Phrase de passe du chiffrement"
  info "Elle protège vos données et l'état de vos agents. Elle vous sera demandée à chaque"
  info "démarrage, jusqu'à ce que vous l'enrôliez dans le TPM."
  info "Elle n'est écrite nulle part : si vous la perdez, les données sont perdues."
  echo
  while :; do
    printf '  phrase de passe : '; read -rs PHRASE; echo
    printf '  répétez         : '; read -rs PHRASE2; echo
    [ "$PHRASE" = "$PHRASE2" ] || { rouge "  elles diffèrent."; continue; }
    [ ${#PHRASE} -ge 8 ] || { rouge "  huit caractères au minimum."; continue; }
    break
  done
  unset PHRASE2
  vert "✓ phrase retenue"
fi

# --- 4. Partitionnement ---

titre "Partitionnement"

# `sgdisk --zap-all` efface les deux tables, GPT et MBR : une table MBR résiduelle ferait
# démarrer le micrologiciel sur un fantôme.
sgdisk --zap-all "$DISQUE" >/dev/null
wipefs -a "$DISQUE" >/dev/null 2>&1 || true

sgdisk \
  -n 1:0:+1G     -t 1:ef00 -c 1:prophet-boot \
  -n 2:0:+24G    -t 2:8300 -c 2:prophet-a \
  -n 3:0:+24G    -t 3:8300 -c 3:prophet-b \
  -n 4:0:+32G    -t 4:8309 -c 4:prophet-state \
  -n 5:0:0       -t 5:8309 -c 5:prophet-home \
  "$DISQUE" >/dev/null

partprobe "$DISQUE" 2>/dev/null || true
udevadm settle

# Les disques NVMe et mmc numérotent leurs partitions avec un « p » ; les disques SATA non.
if [[ "$DISQUE" =~ (nvme|mmcblk|loop) ]]; then P="${DISQUE}p"; else P="$DISQUE"; fi
ESP="${P}1"; RACINE_A="${P}2"; RACINE_B="${P}3"; ETAT="${P}4"; DONNEES="${P}5"

for partition in "$ESP" "$RACINE_A" "$RACINE_B" "$ETAT" "$DONNEES"; do
  [ -b "$partition" ] || mourir "la partition $partition n'est pas apparue. Le partitionnement a échoué."
done
vert "✓ cinq partitions créées"

# --- 5. Chiffrement ---

if [ "$CHIFFRER" = "1" ]; then
  titre "Chiffrement"
  # LUKS2 porte une étiquette, que udev expose sous /dev/disk/by-label : c'est ainsi que
  # `immutable.nix` désigne ces volumes sans dépendre d'un numéro de partition.
  for couple in "$ETAT:prophet-state" "$DONNEES:prophet-home"; do
    partition="${couple%%:*}"; nom="${couple##*:}"
    printf '%s' "$PHRASE" | cryptsetup luksFormat \
      --type luks2 --label "${nom}-luks" --batch-mode --key-file - "$partition"
    printf '%s' "$PHRASE" | cryptsetup open --key-file - "$partition" "$nom"
    vert "✓ $nom chiffré et ouvert"
  done
  VOL_ETAT=/dev/mapper/prophet-state
  VOL_DONNEES=/dev/mapper/prophet-home
else
  rouge "installation sans chiffrement, à votre demande."
  VOL_ETAT="$ETAT"
  VOL_DONNEES="$DONNEES"
fi
unset PHRASE

# --- 6. Systèmes de fichiers ---

titre "Systèmes de fichiers"
mkfs.fat -F32 -n PROPHET-BOOT "$ESP" >/dev/null
mkfs.ext4 -q -L prophet-a "$RACINE_A"
mkfs.ext4 -q -L prophet-b "$RACINE_B"
# btrfs pour l'état et les données : les sous-volumes par tâche en dépendent (ADR-0004).
mkfs.btrfs -q -f -L prophet-state "$VOL_ETAT"
mkfs.btrfs -q -f -L prophet-home "$VOL_DONNEES"
vert "✓ formatés"

# --- 7. Montage ---

titre "Montage"
mount "$RACINE_A" "$CIBLE"
mkdir -p "$CIBLE/boot" "$CIBLE/home" "$CIBLE/var/lib/prophet"
mount "$ESP" "$CIBLE/boot"
mount -o compress=zstd,noatime "$VOL_DONNEES" "$CIBLE/home"
mount -o compress=zstd,noatime "$VOL_ETAT" "$CIBLE/var/lib/prophet"
vert "✓ montés sous $CIBLE"

# --- 8. Installation ---

if [ "$JUSQU_AU_MONTAGE" = "1" ]; then
  titre "Disque préparé, arrêt demandé"
  echo
  lsblk -o NAME,SIZE,FSTYPE,LABEL,MOUNTPOINT "$DISQUE"
  echo
  info "Rien n'a été installé. Relancez sans --jusqu-au-montage pour poser le système,"
  info "ou démontez $CIBLE si vous renoncez."
  exit 0
fi

titre "Installation du système"
info "Le système est téléchargé depuis cache.nixos.org et assemblé. Comptez vingt minutes à"
info "une heure selon la connexion et la machine."
echo

mkdir -p "$CIBLE/etc/prophet"
cp -r "$DEPOT" "$CIBLE/etc/prophet/source"

# Le matériel de cette machine, détecté ici : c'est le seul fichier qui lui soit propre.
nixos-generate-config --root "$CIBLE" --no-filesystems

# La racine est montée en lecture seule par `immutable.nix` une fois installée ; pendant
# l'installation elle ne l'est pas, sinon rien ne pourrait y être écrit.
nixos-install \
  --root "$CIBLE" \
  --flake "$CIBLE/etc/prophet/source#prophet" \
  --no-root-password

vert "✓ système installé"

# --- 9. Fin ---

titre "Installation terminée"
echo
info "Retirez le support, puis redémarrez."
echo
info "Au premier démarrage :"
info "  • la phrase de passe vous sera demandée pour ouvrir les volumes chiffrés ;"
info "  • « prophet status » dira ce que cette machine sait isoler ;"
info "  • « prophet provider login claude-code » connectera votre abonnement."
echo
if [ "$CHIFFRER" = "1" ]; then
  info "Pour ne plus taper la phrase à chaque démarrage, enrôlez-la dans le TPM :"
  info "  systemd-cryptenroll --tpm2-device=auto /dev/disk/by-label/prophet-home-luks"
  info "Gardez la phrase : elle reste le seul recours si la carte mère change."
  echo
fi
