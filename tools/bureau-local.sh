#!/usr/bin/env bash
# Monte un bureau minimal (X virtuel, bus de session, bus d'accessibilité) et y lance les tests
# de l'adaptateur d'accessibilité contre un vrai éditeur GTK. Sans Nix : Xvfb, dbus-run-session,
# at-spi-bus-launcher et mousepad doivent être installés (Debian : xvfb dbus at-spi2-core mousepad).
#
#   tools/bureau-local.sh                 # cargo test -p supd, dans le banc
#   tools/bureau-local.sh cargo test -p supd -- --nocapture
set -euo pipefail
for outil in Xvfb dbus-run-session mousepad; do
  command -v "$outil" >/dev/null || { echo "outil absent : $outil" >&2; exit 2; }
done
LAUNCHER=""
for candidat in "$(command -v at-spi-bus-launcher 2>/dev/null || true)" /usr/libexec/at-spi-bus-launcher /usr/lib/at-spi2-core/at-spi-bus-launcher; do
  if [ -n "$candidat" ] && [ -x "$candidat" ]; then LAUNCHER=$candidat; break; fi
done
[ -n "$LAUNCHER" ] || { echo "at-spi-bus-launcher absent" >&2; exit 2; }
export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/tmp/prophet-bureau-$(id -u)}
mkdir -p "$XDG_RUNTIME_DIR"; chmod 700 "$XDG_RUNTIME_DIR"
ECRAN=${PROPHET_BUREAU_ECRAN:-:97}
Xvfb "$ECRAN" -screen 0 1280x800x24 >/dev/null 2>&1 &
XVFB=$!
trap 'kill $XVFB 2>/dev/null || true' EXIT
export DISPLAY=$ECRAN
export PROPHET_BUREAU_EDITEUR=$(command -v mousepad)
export PROPHET_EXIGER_BUREAU=1
exec dbus-run-session -- bash -c '
  "$0" --launch-immediately >/dev/null 2>&1 &
  timeout 2 tail -f /dev/null
  if [ $# -eq 0 ]; then set -- cargo test -p supd; fi
  "$@"
' "$LAUNCHER" "$@"
