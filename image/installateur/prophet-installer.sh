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
#   partition 1  ESP            1 GiB   vfat      étiquette PROPHET-EFI
#   partition 2  racine A      24 GiB   ext4      étiquette prophet-a
#   partition 3  racine B      24 GiB   ext4      étiquette prophet-b
#   partition 4  état           32 GiB  LUKS2     étiquette prophet-state-luks → btrfs
#   partition 5  données       le reste LUKS2     étiquette prophet-home-luks  → btrfs
#   partition 6  amorçage BIOS  1 MiB   (ef02)    GRUB y met son image quand la machine n'a pas
#                                                 d'UEFI ; inerte sinon (ADR 0032)
#
# Deux racines parce qu'une mise à jour écrit dans celle qui ne tourne pas : un échec laisse la
# machine sur la précédente. Deux volumes chiffrés distincts parce que l'état des agents et les
# données de l'utilisateur n'ont pas la même durée de vie ni la même valeur. La partition BIOS
# est toujours créée : un mébioctet, et la même disposition quel que soit le micrologiciel.

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

# Le micrologiciel décide du chargeur : systemd-boot sur une machine démarrée en UEFI, GRUB sur
# une machine sans UEFI. Le support d'amorçage est hybride, donc ce qu'on constate ici est ce que
# la machine installée verra aussi (ADR 0032). Une machine qui a l'UEFI mais a démarré en mode
# « CSM » recevra GRUB : c'est cohérent, et cela se change en réamorçant la clé en UEFI.
if [ -d /sys/firmware/efi ]; then
  AMORCAGE=uefi
  vert "✓ démarrage UEFI : systemd-boot"
else
  AMORCAGE=bios
  vert "✓ démarrage sans UEFI : GRUB"
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
# Le modèle local par défaut (1,83 Go) vient de huggingface.co, à l'installation : sans lui, la
# machine installée n'aurait d'agent qu'avec un compte ou une clé (ADR 0033). On le vérifie
# avant d'effacer quoi que ce soit, comme le cache.
if [ "$JUSQU_AU_MONTAGE" = "0" ] && ! curl -sSI --max-time 15 https://huggingface.co/ >/dev/null 2>&1; then
  rouge "huggingface.co est injoignable."
  info "Le modèle local par défaut s'y télécharge pendant l'installation. Vérifiez la"
  info "connexion, ou un pare-feu qui bloquerait ce site, puis relancez."
  exit 1
fi
# Un « test && commande » en fin de ligne sortirait du script quand le test est faux, puisque la
# ligne rend alors 1 et que `set -e` veille. La forme longue dit la même chose sans ce piège.
if [ "$JUSQU_AU_MONTAGE" = "0" ]; then
  vert "✓ réseau, cache Nix et huggingface.co joignables"
fi

# --- 1 bis. Ce que cette machine offre ---
#
# Dit avant d'effacer quoi que ce soit : un PC formaté dont l'écran, le réseau ou le micro ne
# répondent pas est une machine à réinstaller, et il vaut mieux le savoir tant que Windows est
# encore là. Tout vient de /sys et /proc, tels que le support d'amorçage les voit ; le système
# installé a les mêmes pilotes et verra la même chose. Une ligne qui commence par « ! » est un
# manque ; rien ici n'arrête l'installation, c'est la personne qui juge.
inventaire() {
  local modele coeurs memoire_kio memoire_gio machine c nom pilote genre etat reseau son
  machine=$(cat /sys/class/dmi/id/sys_vendor /sys/class/dmi/id/product_name 2>/dev/null | tr '\n' ' ' | sed 's/ *$//')
  [ -n "$machine" ] && printf '  machine : %s\n' "$machine"
  coeurs=$(nproc 2>/dev/null || echo "?")
  modele=$(sed -n 's/^model name[[:space:]]*: //p' /proc/cpuinfo 2>/dev/null | head -n 1)
  printf '  processeur : %s cœurs, %s\n' "$coeurs" "${modele:-inconnu}"
  memoire_kio=$(grep -E '^MemTotal:' /proc/meminfo 2>/dev/null | sed 's/[^0-9]//g')
  memoire_gio=$(( ${memoire_kio:-0} / 1024 / 1024 ))
  if [ "$memoire_gio" -lt 8 ]; then
    printf '! mémoire : %s Gio — il en faut 8 pour le bureau et un modèle local ; les modèles locaux resteront petits\n' "$memoire_gio"
  else
    printf '  mémoire : %s Gio\n' "$memoire_gio"
  fi
  if [ -e /dev/kvm ]; then
    printf '  virtualisation (KVM) : présente — le niveau 2 d'"'"'isolation (microVM) sera disponible\n'
  else
    printf '! virtualisation (KVM) : absente — activez VT-x ou AMD-V dans le micrologiciel, sinon le niveau 2 d'"'"'isolation (microVM) restera refusé\n'
  fi
  # La carte graphique : le pilote que le noyau lui a lié, et Vulkan s'il la voit (ADR 0037).
  local pilotes=""
  for c in /sys/class/drm/card*; do
    [ -e "$c/device/driver" ] || continue
    case "$(basename "$c")" in *-*) continue ;; esac
    pilote=$(basename "$(readlink -f "$c/device/driver")")
    pilotes="${pilotes:+$pilotes, }$pilote"
  done
  if [ -n "$CARTE" ]; then
    printf '  carte graphique : %s (pilote %s), Vulkan — le bureau et les modèles locaux l'"'"'utiliseront\n' "$CARTE" "${pilotes:-?}"
  elif [ -n "$pilotes" ]; then
    printf '  carte graphique : pilote %s, sans Vulkan — le bureau tournera en rendu logiciel, les modèles locaux sur processeur\n' "$pilotes"
  else
    printf '! carte graphique : aucun pilote d'"'"'affichage chargé — si un écran est branché, le bureau ne s'"'"'affichera peut-être pas (NVIDIA n'"'"'est pas pris en charge)\n'
  fi
  # Le réseau : les interfaces qui ont un périphérique (pas les virtuelles), filaires ou Wi-Fi.
  reseau=""
  for c in /sys/class/net/*; do
    nom=$(basename "$c")
    [ "$nom" = lo ] && continue
    [ -e "$c/device" ] || continue
    pilote=$(basename "$(readlink -f "$c/device/driver" 2>/dev/null || echo inconnu)")
    if [ -d "$c/wireless" ]; then genre="Wi-Fi"; else genre="filaire"; fi
    etat=$(cat "$c/operstate" 2>/dev/null || echo "?")
    reseau="${reseau:+$reseau ; }$nom ($genre, $pilote, $etat)"
  done
  if [ -n "$reseau" ]; then
    printf '  réseau : %s\n' "$reseau"
  else
    printf '! réseau : aucune interface avec pilote — sans réseau, ni installation ni clients\n'
  fi
  # Le son, et une entrée (micro) : la parole en dépend (ADR 0036).
  son=$(grep -E '^ *[0-9]+ \[' /proc/asound/cards 2>/dev/null | sed -E 's/^ *[0-9]+ \[[^]]*\]: [^ ]* - //' | paste -sd ';' -) || true
  if [ -n "$son" ]; then
    if ls -d /proc/asound/card*/pcm*c >/dev/null 2>&1; then
      printf '  son : %s — avec une entrée (micro) : la parole sera possible\n' "$son"
    else
      printf '! son : %s — sans entrée détectée : pas de micro, la parole attendra\n' "$son"
    fi
  else
    printf '! son : aucune carte détectée — ni voix ni parole sur cette machine\n'
  fi
  # Secure Boot : le chargeur n'est pas signé, la machine installée ne démarrerait pas.
  local sb=/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c
  if [ "$AMORCAGE" = uefi ] && [ -r "$sb" ]; then
    if [ "$(od -An -tu1 -j4 -N1 "$sb" 2>/dev/null | tr -d ' ')" = "1" ]; then
      printf '! Secure Boot : activé — le chargeur de Prophet OS n'"'"'est pas signé, la machine installée ne démarrera pas ; désactivez-le dans le micrologiciel avant de continuer\n'
    else
      printf '  Secure Boot : désactivé\n'
    fi
  fi
  if [ -e /sys/class/tpm/tpm0 ]; then
    printf '  TPM : présent — la phrase de passe pourra s'"'"'enrôler après l'"'"'installation\n'
  else
    printf '  TPM : absent — la phrase de passe sera tapée à chaque démarrage\n'
  fi
}

# La carte graphique, vue par Vulkan, se calcule ici et non dans `inventaire` : la fonction
# tourne dans un sous-shell (`$(…)`), et ce qu'elle y pose ne survit pas — l'étape de
# l'accélération, plus bas, lisait une variable absente et l'installeur mourait après la
# détection du matériel (vu dans une VM le 15 septembre 2026).
CARTE=""
if command -v vulkaninfo >/dev/null 2>&1; then
  CARTE=$(vulkaninfo --summary 2>/dev/null | grep -E '^[[:space:]]*deviceName' | grep -viE 'llvmpipe|lavapipe|swiftshader' | head -n 1 | sed 's/^[^=]*=[[:space:]]*//') || true
fi
titre "Ce que cette machine offre"
echo
INVENTAIRE=$(inventaire)
while IFS= read -r ligne; do
  case "$ligne" in
    "!"*) rouge "  ${ligne#! }" ;;
    *) printf '%s\n' "$ligne" ;;
  esac
done <<< "$INVENTAIRE"
echo

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

# --- 3 bis. Le mot de passe du compte, avant toute écriture ---
#
# Sans lui, la machine installée n'a aucune session ouvrable : `root` n'a pas de mot de passe et
# `systemd-boot` est configuré sans éditeur, donc il n'y a pas non plus de démarrage de secours.
# Une installation sans mot de passe produirait une machine à réinstaller. On le demande donc ici,
# avant que quoi que ce soit ne soit écrit sur le disque, et on refuse de continuer sans.

titre "Mot de passe de votre compte"
info "Il ouvre votre session et sert à « sudo ». Le compte root reste verrouillé : c'est celui-ci"
info "qui administre la machine."
info "Il n'est pas écrit dans le dépôt — seul son haché est posé sur le disque installé."
echo
MOTDEPASSE=""
while :; do
  printf '  mot de passe : '; read -rs MOTDEPASSE; echo
  printf '  répétez      : '; read -rs MOTDEPASSE2; echo
  [ "$MOTDEPASSE" = "$MOTDEPASSE2" ] || { rouge "  ils diffèrent."; continue; }
  [ ${#MOTDEPASSE} -ge 8 ] || { rouge "  huit caractères au minimum."; continue; }
  break
done
unset MOTDEPASSE2
command -v mkpasswd >/dev/null || mourir "mkpasswd est absent ; impossible de hacher le mot de passe."
# `yescrypt` est le défaut des distributions récentes ; `sha512crypt` existe partout. On essaie le
# meilleur, et on retombe sur l'autre plutôt que d'échouer — mais jamais sur rien.
HACHE="$(printf '%s' "$MOTDEPASSE" | mkpasswd -m yescrypt -s 2>/dev/null || true)"
if [ -z "$HACHE" ]; then
  HACHE="$(printf '%s' "$MOTDEPASSE" | mkpasswd -m sha-512 -s 2>/dev/null || true)"
fi
unset MOTDEPASSE
case "$HACHE" in
  '$'*) : ;;
  *) mourir "le hachage du mot de passe a échoué ; rien n'a été écrit." ;;
esac
vert "✓ mot de passe retenu"

# --- 4. Partitionnement ---

titre "Partitionnement"

# `sgdisk --zap-all` efface les deux tables, GPT et MBR : une table MBR résiduelle ferait
# démarrer le micrologiciel sur un fantôme.
sgdisk --zap-all "$DISQUE" >/dev/null
wipefs -a "$DISQUE" >/dev/null 2>&1 || true

# La partition BIOS prend le dernier mébioctet : GRUB ne se soucie pas de sa place, et la mettre
# à la fin laisse les cinq autres numérotées et alignées comme avant.
sgdisk \
  -n 1:0:+1G     -t 1:ef00 -c 1:prophet-boot \
  -n 2:0:+24G    -t 2:8300 -c 2:prophet-a \
  -n 3:0:+24G    -t 3:8300 -c 3:prophet-b \
  -n 4:0:+32G    -t 4:8309 -c 4:prophet-state \
  -n 5:0:-1M     -t 5:8309 -c 5:prophet-home \
  -n 6:0:0       -t 6:ef02 -c 6:prophet-bios \
  "$DISQUE" >/dev/null

partprobe "$DISQUE" 2>/dev/null || true
udevadm settle

# Les disques NVMe et mmc numérotent leurs partitions avec un « p » ; les disques SATA non.
if [[ "$DISQUE" =~ (nvme|mmcblk|loop) ]]; then P="${DISQUE}p"; else P="$DISQUE"; fi
ESP="${P}1"; RACINE_A="${P}2"; RACINE_B="${P}3"; ETAT="${P}4"; DONNEES="${P}5"; BIOS="${P}6"

for partition in "$ESP" "$RACINE_A" "$RACINE_B" "$ETAT" "$DONNEES" "$BIOS"; do
  [ -b "$partition" ] || mourir "la partition $partition n'est pas apparue. Le partitionnement a échoué."
done
vert "✓ six partitions créées"

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
# Onze caracteres au maximum : c'est la limite d'une etiquette FAT, et « PROPHET-BOOT » en
# faisait douze. La valeur doit rester identique a celle qu'immutable.nix cherche au
# demarrage, sans quoi la machine ne trouverait pas sa partition d'amorcage.
mkfs.fat -F32 -n PROPHET-EFI "$ESP" >/dev/null
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
info "Le système est téléchargé depuis cache.nixos.org et assemblé, avec le modèle local par"
info "défaut (Qwen3-1.7B, 1,83 Go). Comptez vingt minutes à une heure selon la connexion et"
info "la machine."
echo

mkdir -p "$CIBLE/etc/prophet"
# `-L` : sur le support, /etc/prophet/source est un lien vers le magasin Nix ; sans lui, `cp`
# copiait le lien et non le dépôt, et le `chmod` qui suit échouait sur le magasin en lecture
# seule — l'installation s'interrompait après le formatage (vu dans une VM le 15 septembre
# 2026, jamais en CI, dont l'essai de l'installeur s'arrête au montage). `--no-preserve=mode` :
# les fichiers du magasin sont en lecture seule, la copie doit accepter les deux fichiers de la
# machine qu'on écrit ci-dessous.
cp -rL --no-preserve=mode "$DEPOT" "$CIBLE/etc/prophet/source"
chmod -R u+w "$CIBLE/etc/prophet/source"

# Le haché, et lui seul. `install -m 0600` pose le mode à la création : l'écrire puis le corriger
# laisserait une fenêtre où le fichier est lisible.
printf '%s\n' "$HACHE" | install -m 0600 /dev/stdin "$CIBLE/etc/prophet/motdepasse"
unset HACHE
[ -s "$CIBLE/etc/prophet/motdepasse" ] || mourir "le fichier de mot de passe est vide ; un compte sans mot de passe se connecterait sans en taper."

# Le matériel de cette machine, détecté ici, et son mode d'amorçage : les deux seuls fichiers qui
# lui soient propres, écrits dans la copie du dépôt que le flake importe (ADR 0032). Les systèmes
# de fichiers n'y sont pas : `immutable.nix` les désigne par leurs étiquettes.
MACHINE="$CIBLE/etc/prophet/source/image/machine"
nixos-generate-config --show-hardware-config --no-filesystems > "$MACHINE/hardware-configuration.nix"
grep -q "boot.initrd.availableKernelModules" "$MACHINE/hardware-configuration.nix" \
  || mourir "la détection du matériel n'a rien produit ; rien n'est installé."
vert "✓ matériel détecté : $(grep -c '"' "$MACHINE/hardware-configuration.nix") lignes de modules et de réglages"

# Ce que l'installeur a vu de la machine, gardé avec elle : la supervision et un humain qui
# cherche pourquoi le micro ou l'écran ne répond pas le reliront.
printf '%s\n' "$INVENTAIRE" > "$MACHINE/inventaire.txt"

# La carte graphique, si elle sert à quelque chose : un périphérique Vulkan qui n'est pas le
# rastériseur logiciel (vu par l'inventaire, ci-dessus). Alors les modèles locaux tourneront
# dessus (ADR 0037) ; sinon, sur processeur, et le fichier reste celui du dépôt, vide.
if [ -n "$CARTE" ]; then
  {
    printf '# Écrit par l'"'"'installeur : cette machine a une carte graphique utilisable par Vulkan (ADR 0037) :
'
    printf '# %s
' "$CARTE"
    printf '{ ... }: { prophet.localEngine.gpu.enable = true; }
'
  } > "$MACHINE/acceleration.nix"
  vert "✓ carte graphique : $CARTE — les modèles locaux tourneront dessus (Vulkan)"
else
  info "aucune carte graphique utilisable par Vulkan : les modèles locaux tourneront sur processeur"
fi

if [ "$AMORCAGE" = "bios" ]; then
  # GRUB s'installe sur le disque, désigné par un chemin qui ne change pas d'un démarrage à
  # l'autre : les liens de /dev/disk/by-id (ata-…, nvme-…, wwn-…). Un disque en boucle n'en a
  # pas ; on retombe alors sur son chemin direct.
  DISQUE_STABLE="$DISQUE"
  for lien in /dev/disk/by-id/*; do
    [ -e "$lien" ] || continue
    case "$lien" in *-part[0-9]*) continue ;; esac
    if [ "$(readlink -f "$lien")" = "$(readlink -f "$DISQUE")" ]; then
      DISQUE_STABLE="$lien"
      case "$lien" in /dev/disk/by-id/wwn-*) break ;; esac
    fi
  done
  cat > "$MACHINE/amorcage.nix" <<FIN
# Écrit par l'installeur : cette machine a démarré sans UEFI (ADR 0032).
{ ... }: {
  prophet.boot.firmware = "bios";
  prophet.boot.disque = "$DISQUE_STABLE";
}
FIN
  vert "✓ GRUB sera posé sur $DISQUE_STABLE"
fi

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
if [ "$AMORCAGE" = "bios" ]; then
  info "Cette machine démarre par GRUB, sans UEFI : laissez le disque en premier dans l'ordre"
  info "d'amorçage du BIOS."
  echo
fi
info "Au premier démarrage :"
info "  • la phrase de passe vous sera demandée pour ouvrir les volumes chiffrés ;"
info "  • ouvrez une session avec l'identifiant « prophet » et le mot de passe choisi ;"
info "  • « prophet status » dira ce que cette machine sait isoler ;"
info "  • le modèle local Qwen3-1.7B est prêt : les missions tournent sans compte ni clé ;"
info "  • « prophet provider login claude-code » connectera votre abonnement, si vous en avez un."
echo
if [ "$CHIFFRER" = "1" ]; then
  info "Pour ne plus taper la phrase à chaque démarrage, enrôlez-la dans le TPM :"
  info "  systemd-cryptenroll --tpm2-device=auto /dev/disk/by-label/prophet-home-luks"
  info "Gardez la phrase : elle reste le seul recours si la carte mère change."
  echo
fi
