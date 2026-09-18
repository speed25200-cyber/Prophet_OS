#!/usr/bin/env bash
# Fait tourner l'espace utilisateur de Prophet OS sur une machine qui n'est pas Prophet OS.
#
# ## Ce que ce script fait, et ce qu'il ne fait pas
#
# Il ne transforme pas la machine en Prophet OS. Le noyau reste celui de la distribution, la racine
# reste inscriptible, il n'y a ni emplacements A/B ni chiffrement posé par nous. Ce que l'ISO
# installe, lui, est un système entier ; ce script-ci installe **les sept daemons** et les fait
# tourner sous systemd, avec le même durcissement que sur l'image.
#
# C'est la différence entre « Prophet OS est installé » et « Prophet OS tourne ici ». Sur un
# serveur qu'on ne réinstalle pas, la seconde est ce qu'on peut avoir — et elle suffit pour que
# `prophet status`, `prophet task ls` et le chemin complet capd → agentd → ledger fonctionnent.
#
# ## Ce qu'il installe
#
#   /usr/local/lib/prophet/          les programmes
#   /usr/local/bin/prophet           la ligne de commande
#   /etc/systemd/system/prophet-*    sept unités
#   /etc/prophet/policies/           les politiques Cedar
#   /var/lib/prophet/<daemon>/       l'état de chacun, en 0700
#
# ## Configurer un service
#
# Chaque unité lit `/etc/prophet/<daemon>.env` s'il existe (une variable par ligne). C'est là
# que se règle ce que l'image règle par ses options NixOS. Pour le décideur Jev, par exemple :
#
#   echo 'PROPHET_JEV_SECRET=typesafe' | sudo tee /etc/prophet/agentd.env
#   echo 'PROPHET_EGRESS_QUERY_HOSTS=api.typesafe.ai' | sudo tee /etc/prophet/egress.env
#   sudo systemctl restart prophet-egress prophet-agentd
#   prophet secret put typesafe --host api.typesafe.ai < /chemin/vers/la/cle
#   prophet jev status
#
# La clé, elle, ne va jamais dans un fichier d'environnement : elle entre dans le coffre par
# `prophet secret put`, et seul le proxy de sortie peut l'en faire sortir.
#
# ## Comment tout défaire
#
#   sudo ./tools/lancer-sur-l-hote.sh --retirer
#
# Usage :
#   sudo ./tools/lancer-sur-l-hote.sh              compile, installe, démarre, montre l'état
#   sudo ./tools/lancer-sur-l-hote.sh --sans-compiler   si les binaires sont déjà dans target/release
#   sudo ./tools/lancer-sur-l-hote.sh --retirer    arrête, désinstalle, laisse l'état en place

set -uo pipefail

RACINE="$(cd "$(dirname "$0")/.." && pwd)"
CIBLE=/usr/local/lib/prophet
UNITES=/etc/systemd/system
COMPILER=1
ACTION=lancer

case "${1:-}" in
  --retirer)       ACTION=retirer ;;
  --sans-compiler) COMPILER=0 ;;
  "")              ;;
  *) echo "argument inconnu : $1 (attendus : --sans-compiler, --retirer)" >&2; exit 2 ;;
esac

ok()   { printf '  \033[32m✓\033[0m %s\n' "$1"; }
ko()   { printf '  \033[31m✗\033[0m %s\n' "$1"; }
info() { printf '  · %s\n' "$1"; }
titre(){ printf '\n\033[1m%s\033[0m\n' "$1"; }

[ "$(id -u)" = "0" ] || { echo "Ce script installe des services : relancez-le avec sudo." >&2; exit 1; }

DAEMONS="capd ledger vault egress sandboxd memoryd agentd"

# --- Retrait ---

if [ "$ACTION" = "retirer" ]; then
  titre "Retrait de Prophet OS"
  for d in $DAEMONS; do
    systemctl disable --now "prophet-$d.service" 2>/dev/null && ok "prophet-$d arrêté" || info "prophet-$d n'était pas là"
    rm -f "$UNITES/prophet-$d.service"
  done
  systemctl daemon-reload
  rm -rf "$CIBLE" /usr/local/bin/prophet
  ok "programmes et unités retirés"
  info "l'état est conservé dans /var/lib/prophet — à effacer à la main si vous le souhaitez"
  info "les comptes système et le groupe prophet-system sont conservés de même"
  exit 0
fi

# --- 1. Compiler ---

if [ "$COMPILER" = "1" ]; then
  titre "Compilation"
  # `. cargo/env` : rustup installe dans le HOME de root, que sudo ne met pas toujours dans le PATH.
  # shellcheck disable=SC1090,SC1091
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  command -v cargo >/dev/null || { ko "cargo absent : lancez d'abord tools/setup-ubuntu-host.sh"; exit 1; }
  info "sept daemons, la ligne de commande et l'amorçage de sandbox — comptez dix minutes"
  ( cd "$RACINE" && cargo build --release \
      --bin prophet --bin prophet-sandbox-helper \
      $(for d in $DAEMONS; do printf -- '--bin prophet-%s ' "$d"; done) ) || {
    ko "la compilation a échoué ; rien n'est installé"
    exit 1
  }
  ok "compilé"
fi

for d in $DAEMONS prophet prophet-sandbox-helper; do
  case "$d" in
    prophet|prophet-sandbox-helper) binaire="$RACINE/target/release/$d" ;;
    *) binaire="$RACINE/target/release/prophet-$d" ;;
  esac
  [ -x "$binaire" ] || { ko "$binaire absent : la compilation n'a pas produit ce qu'elle devait"; exit 1; }
done

# --- 2. Comptes et répertoires ---

titre "Comptes et répertoires"
groupadd -f prophet-system
ok "groupe prophet-system"

# `sandboxd` tourne en root : projeter les identifiants d'un enfant l'exige. Les six autres ont
# chacun leur compte, pour qu'aucun ne puisse lire l'état d'un autre.
for d in capd ledger vault egress memoryd agentd; do
  id "$d" >/dev/null 2>&1 || useradd --system --no-create-home --shell /usr/sbin/nologin -g prophet-system "$d"
done
ok "six comptes de service, plus root pour sandboxd"

install -d -m 0750 -o root -g prophet-system /var/lib/prophet
for d in $DAEMONS; do
  proprio="$d"; [ "$d" = "sandboxd" ] && proprio=root
  install -d -m 0700 -o "$proprio" -g prophet-system "/var/lib/prophet/$d"
done
install -d -m 0755 /etc/prophet /etc/prophet/policies
install -m 0644 "$RACINE/policies/default.cedar" /etc/prophet/policies/default.cedar 2>/dev/null \
  && ok "politiques Cedar posées" || info "aucune politique à poser"
ok "état en 0700 par daemon, sous /var/lib/prophet en 0750"

# --- 3. Programmes ---

titre "Programmes"
install -d -m 0755 "$CIBLE"
for d in $DAEMONS; do install -m 0755 "$RACINE/target/release/prophet-$d" "$CIBLE/prophet-$d"; done
install -m 0755 "$RACINE/target/release/prophet-sandbox-helper" "$CIBLE/prophet-sandbox-helper"
install -m 0755 "$RACINE/target/release/prophet" /usr/local/bin/prophet
ok "installés dans $CIBLE, ligne de commande dans /usr/local/bin/prophet"

# --- 4. Unités systemd ---
#
# Le durcissement reprend celui de `image/modules/prophet.nix`, y compris ce que la journée du
# 12 septembre 2026 y a corrigé :
#
#   - `RuntimeDirectoryMode=0770` : en 0750, seul le premier service démarré peut créer son socket
#     et les six autres échouent sur « Permission denied » ;
#   - `RuntimeDirectoryPreserve=yes` : sans quoi l'arrêt d'un service emporte les six autres
#     sockets ;
#   - pour `sandboxd`, un filtre d'appels système qui lui laisse `@mount` et `@privileged` — lui
#     accorder CAP_SETUID et lui interdire `setuid` est une contradiction qui ne se voit qu'à
#     l'exécution — et `CAP_SETFCAP`, sans laquelle la projection de l'uid 0 est refusée depuis
#     Linux 5.12 ;
#   - pour `agentd`, `ProtectHome=no` : `ProtectHome=yes` l'emporte sur `ReadWritePaths` et lui
#     interdisait d'écrire dans le répertoire de l'utilisateur qu'il déclare pourtant inscriptible.
#
# Une chose n'est **pas** reprise de `prophet.nix` mais de la manière dont NixOS le rend : une
# négation par ligne. `SystemCallFilter=~@privileged ~@resources` ne fait pas ce qu'on lit — systemd
# ne prend le `~` qu'en tête de valeur, puis lit chaque mot comme un nom d'appel système. Le second
# `~@resources` n'est alors pas un groupe nié mais un nom invalide : il est écarté avec un simple
# avertissement dans le journal, et le filtre qu'on croyait poser est plus large que voulu. NixOS
# écrit une ligne par élément de liste, ce qui masque le piège ; ici il faut le faire à la main.

titre "Unités systemd"

unite() {
  local nom="$1" utilisateur="$2" description="$3" extra="$4"
  cat > "$UNITES/prophet-$nom.service" <<UNITE
[Unit]
Description=$description
After=network.target
$( [ "$nom" = "agentd" ] && echo "After=prophet-capd.service prophet-ledger.service" )
$( [ "$nom" = "agentd" ] && echo "Requires=prophet-capd.service prophet-ledger.service" )

[Service]
ExecStart=$CIBLE/prophet-$nom
User=$utilisateur
Group=prophet-system
Restart=on-failure
RestartSec=2s
StateDirectory=prophet/$nom
StateDirectoryMode=0700
RuntimeDirectory=prophet
RuntimeDirectoryMode=0770
RuntimeDirectoryPreserve=yes
Environment=PATH=$CIBLE:/usr/local/bin:/usr/bin:/bin
EnvironmentFile=-/etc/prophet/$nom.env

NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectKernelLogs=yes
ProtectControlGroups=yes
ProtectClock=yes
ProtectHostname=yes
ProtectProc=invisible
RestrictNamespaces=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
SystemCallArchitectures=native
SystemCallFilter=@system-service
SystemCallFilter=~@privileged
SystemCallFilter=~@resources
CapabilityBoundingSet=
$extra

[Install]
WantedBy=multi-user.target
UNITE
}

unite capd    capd    "Prophet OS — broker de capacités" ""
unite ledger  ledger  "Prophet OS — journal d'audit" "ReadWritePaths=/var/lib/prophet/ledger"
unite vault   vault   "Prophet OS — coffre à secrets" "DeviceAllow=/dev/tpmrm0 rw
PrivateDevices=no"
unite egress  egress  "Prophet OS — proxy de sortie" "PrivateNetwork=no
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6"
unite memoryd memoryd "Prophet OS — mémoire" ""
# `HOME` posé explicitement. Le compte est créé `--no-create-home` : sans cette ligne, `agentd`
# écrirait son espace de travail dans un répertoire qui n'existe pas, et échouerait loin d'ici avec
# un message qui ne nommerait pas la cause. Sur l'image, le compte humain existe et `ProtectHome`
# suffit ; ici il n'y a pas de compte humain, et l'état du daemon est le seul endroit qui soit à
# lui.
unite agentd  agentd  "Prophet OS — runtime d'agents" "ReadWritePaths=/var/lib/prophet/agentd
Environment=HOME=/var/lib/prophet/agentd
ProtectHome=no"
unite sandboxd root   "Prophet OS — gestionnaire de sandbox" "CapabilityBoundingSet=CAP_SETUID CAP_SETGID CAP_SETFCAP CAP_SYS_ADMIN
AmbientCapabilities=CAP_SETUID CAP_SETGID CAP_SETFCAP CAP_SYS_ADMIN
NoNewPrivileges=no
RestrictNamespaces=no
RestrictSUIDSGID=no
ProtectProc=default
SystemCallFilter=
SystemCallFilter=@system-service @mount @privileged
SystemCallFilter=~@resources
SystemCallFilter=~@module
SystemCallFilter=~@debug
SystemCallFilter=~@clock
SystemCallFilter=~@reboot
SystemCallFilter=~@swap
SystemCallFilter=~@obsolete
DeviceAllow=/dev/kvm rw
PrivateDevices=no"

systemctl daemon-reload
ok "sept unités écrites dans $UNITES"

# Ce que systemd pense de ce qu'on vient d'écrire, avant de le lui faire exécuter.
#
# `systemd-analyze verify` relit chaque directive comme le ferait le gestionnaire au démarrage, et
# dit ce qu'il écarte. Un mot qu'il ne reconnaît pas — `~@resources` en deuxième position d'une
# ligne `SystemCallFilter=`, par exemple — n'est pas une erreur pour lui : il l'ignore avec un
# avertissement et démarre le service avec un filtre plus large que celui qu'on croyait poser.
# Personne ne le verrait jamais. Ici, on le lit et on refuse de continuer.
#
# **Ne juger que ce qui nous concerne.** `verify` ne relit pas une unité isolée : il charge tout le
# graphe de dépendances, et rapporte au passage les reproches qu'il a à faire aux unités de la
# distribution. Sur le serveur, ce furent celles-ci :
#
#     /usr/lib/systemd/system/xfs_scrub_all.service:26: Support for option CPUAccounting= has been
#     removed and it is ignored
#
# Rien à voir avec Prophet OS, et pourtant ce contrôle a refusé de démarrer les sept services. Il a
# fait exactement la faute qu'il existe pour empêcher : conclure sur autre chose que ce qu'il
# prétendait mesurer. On ne retient donc que les lignes qui **nomment l'unité examinée** ; les
# autres sont comptées et affichées, parce qu'un avertissement qu'on écarte sans le montrer est un
# avertissement qu'on a caché.
if command -v systemd-analyze >/dev/null 2>&1; then
  PLAINTES=0
  AILLEURS=0
  for d in $DAEMONS; do
    brut=$(systemd-analyze verify "$UNITES/prophet-$d.service" 2>&1 || true)
    # `Unit … not found` : les dépendances entre unités ne sont pas résolues hors du gestionnaire.
    nous=$(printf '%s\n' "$brut" | grep -F "prophet-$d.service" | grep -v 'not found' || true)
    autres=$(printf '%s\n' "$brut" | grep -vF "prophet-$d.service" | grep -v '^$' || true)
    [ -n "$autres" ] && AILLEURS=$((AILLEURS + 1))
    if [ -n "$nous" ]; then
      ko "prophet-$d — systemd a des réserves sur cette unité"
      printf '%s\n' "$nous" | sed 's/^/      /'
      PLAINTES=$((PLAINTES + 1))
    fi
  done
  if [ "$PLAINTES" -gt 0 ]; then
    ko "$PLAINTES unité(s) que systemd relit autrement qu'écrites — rien n'est démarré"
    exit 1
  fi
  ok "systemd relit les sept unités telles qu'elles sont écrites"
  if [ "$AILLEURS" -gt 0 ]; then
    info "systemd a par ailleurs des reproches à faire à des unités de cette distribution,"
    info "qui ne nous concernent pas : « systemd-analyze verify » les montre."
  fi
else
  info "systemd-analyze absent : les unités ne sont pas relues avant d'être démarrées"
fi

# --- 5. Démarrage ---

titre "Démarrage"
for d in $DAEMONS; do
  systemctl enable --now "prophet-$d.service" >/dev/null 2>&1
done
sleep 2

DEBOUT=0
for d in $DAEMONS; do
  if systemctl is-active --quiet "prophet-$d.service"; then
    ok "prophet-$d"
    DEBOUT=$((DEBOUT + 1))
  else
    ko "prophet-$d — $(systemctl show -p Result --value "prophet-$d.service")"
    journalctl -u "prophet-$d.service" -n 5 --no-pager 2>/dev/null | sed 's/^/      /'
  fi
done

# --- 6. Ce que la machine répond ---

titre "Ce que la machine répond"
echo
/usr/local/bin/prophet status 2>&1 | sed 's/^/  /'

echo
if [ "$DEBOUT" -eq 7 ]; then
  ok "les sept services tournent"
  info "essayez : prophet status, prophet task ls, prophet provider ls"
  info "pour tout défaire : sudo $0 --retirer"
  exit 0
fi
ko "$((7 - DEBOUT)) service(s) ne tournent pas — voir les journaux ci-dessus"
exit 1
