#!/usr/bin/env bash
# Construit sur l'hôte WSL la fermeture du système `prophet-ci` à la révision de l'ISO (9abc822),
# puis l'exporte en cache binaire de fichiers et en image ext4 que la VM montera en /dev/vdb :
# l'installeur y puisera au lieu de compiler Prophet OS lui-même à 0,7 Mio/s dans 4 Gio.
# Garde-fou : C: sous 3 Go → arrêt.
set -u
VM=${PROPHET_VM:-/root/vm}
SRC=${PROPHET_SRC:-/root/prophet-src}
REV=${PROPHET_REV:-9abc822}
W=${PROPHET_WORKTREE:-/root/prophet-$REV}
jalon() { echo "$(date +%H:%M:%S) $*"; }
garde() {
  local pid=$1
  while kill -0 "$pid" 2>/dev/null; do
    libre=$(df -m /mnt/c 2>/dev/null | awk 'NR==2{print $4}')
    if [ "${libre:-99999}" -lt 3000 ]; then
      jalon "GARDE : C: n'a plus que ${libre} Mo, arrêt"
      pkill -f 'nix build' ; kill "$pid" 2>/dev/null
      return 1
    fi
    sleep 20
  done
}
cd "$SRC" || exit 1
if [ ! -d "$W" ]; then
  git worktree add -f "$W" "$REV" >/dev/null 2>&1 || { jalon "worktree impossible"; exit 1; }
fi
jalon "source : $(git -C "$W" rev-parse --short HEAD) dans $W"
jalon "construction de prophet-ci (max-jobs 2, cores 4)"
nix build "$W#nixosConfigurations.prophet-ci.config.system.build.toplevel" \
  --max-jobs 2 --cores 4 --no-link --print-out-paths > "$VM/toplevel.txt" 2> "$VM/build-cache.log" &
garde $! || exit 1
wait $!; code=$?
if [ "$code" -ne 0 ] || [ ! -s "$VM/toplevel.txt" ]; then
  jalon "ÉCHEC de la construction (code $code) ; fin du journal :"
  grep -E "error|failed" "$VM/build-cache.log" | tail -15 | cut -c1-220
  exit 1
fi
TOP=$(cat "$VM/toplevel.txt")
jalon "construit : $TOP"
jalon "fermeture : $(nix path-info -r "$TOP" | wc -l) chemins, $(nix path-info -S "$TOP" | awk '{printf "%.1f Gio", $2/1073741824}')"
rm -rf "$VM/cache"
jalon "export vers le cache de fichiers"
nix copy --to "file://$VM/cache?compression=zstd&parallel-compression=true" "$TOP" 2> "$VM/copy.log" || { jalon "ÉCHEC de l'export"; tail -5 "$VM/copy.log"; exit 1; }
taille=$(du -sm "$VM/cache" | cut -f1)
jalon "cache : ${taille} Mo, $(ls "$VM/cache" | wc -l) entrées"
rm -f "$VM/cache.img"
truncate -s "$((taille * 12 / 10 + 256))M" "$VM/cache.img"
mkfs.ext4 -q -F -L PROPHETCACHE -d "$VM/cache" "$VM/cache.img" || { jalon "ÉCHEC mkfs"; exit 1; }
jalon "image : $(du -m "$VM/cache.img" | cut -f1) Mo dans cache.img"
rm -rf "$VM/cache"
jalon "DONE_CACHE"
