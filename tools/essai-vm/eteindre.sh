#!/usr/bin/env bash
# Éteindre proprement le système installé : une console texte (tty3), session, sudo poweroff.
M="python3 $VM/vm-mon.py"
$M cmd 'sendkey ctrl-alt-f3' >/dev/null; sleep 4
$M type 'prophet\n'; sleep 3
$M type 'motdepassedetest\n'; sleep 5
$M type 'sudo poweroff\n'; sleep 3
$M type 'motdepassedetest\n'; sleep 20
if pgrep -f '[q]emu-system-x86_64' >/dev/null; then echo "QEMU tourne encore"; sleep 20; pgrep -f '[q]emu-system-x86_64' >/dev/null && echo "toujours vivant" || echo "éteint"; else echo "éteint"; fi
