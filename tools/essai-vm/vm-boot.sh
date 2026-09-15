#!/usr/bin/env bash
# Démarre le système installé (sans l'ISO) dans QEMU, au premier plan, sous SeaBIOS (le système
# a été installé sans UEFI : GRUB sur la partition d'amorçage BIOS). À lancer dans une session
# WSL qui reste ouverte (un moniteur persistant). L'écran se lit par `vm-mon.py shot`, le
# clavier par `vm-mon.py type` ; la console série va dans un fichier.
set -euo pipefail
VM=${PROPHET_VM:-/root/vm}
QEMU=${PROPHET_QEMU_BIN:-/nix/store/gx222l0zm41h4zqrgpb07nc0brwpv3nk-qemu-11.1.0/bin}
rm -f "$VM/monitor.sock"
exec "$QEMU/qemu-system-x86_64" -enable-kvm -cpu host -m 4096 -smp 4 \
  -drive "file=$VM/prophet.qcow2,if=virtio,format=qcow2" \
  -boot c -nic user,model=virtio-net-pci \
  -display none -vga std \
  -serial "file:$VM/serial-boot.log" \
  -monitor "unix:$VM/monitor.sock,server=on,wait=off" \
  > "$VM/qemu-boot.out" 2>&1 < /dev/null
