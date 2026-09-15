#!/usr/bin/env bash
M="python3 $VM/vm-mon.py"
$M cmd 'sendkey ctrl-u' >/dev/null
$M type 'clear; prophet status 2>&1 | head -32\n'; sleep 15
echo "=== prophet status ==="; $M shot tty2-status
$M type 'clear; systemctl --failed --no-legend; echo FIN_FAILED; for s in capd ledger vault egress sandboxd memoryd agentd; do printf "%s=%s " $s $(systemctl is-active prophet-$s); done; echo; sed -n 1,12p /etc/prophet/source/image/machine/inventaire.txt\n'; sleep 8
echo "=== services et releve ==="; $M shot tty2-services
$M type 'clear; journalctl -b --no-pager -o cat | grep -i -E "greetd|sway|prophet-session|wlr|renderer|EGL|drm" | grep -v -E "Started|Starting|Reached" | tail -22 | cut -c1-118\n'; sleep 8
echo "=== journal session ==="; $M shot tty2-journal
$M type 'clear; ls -l /dev/dri/ 2>&1; lspci | grep -i -E "vga|display|3d"; uname -r; free -m | head -2; df -h / /home /var/lib/prophet | tail -3\n'; sleep 6
echo "=== dri et divers ==="; $M shot tty2-dri
