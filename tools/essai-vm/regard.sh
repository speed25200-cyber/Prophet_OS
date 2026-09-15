#!/usr/bin/env bash
# Une capture nommée de l'écran de la VM, plus la fin de la console série du système installé.
nom=${1:-regard}
for i in $(seq 1 20); do [ -S $VM/monitor.sock ] && break; sleep 2; done
python3 $VM/vm-mon.py shot "$nom" 2>&1 | head -40
echo "--- série ---"
tail -c 1500 $VM/serial-boot.log 2>/dev/null | tr -d '\033' | tr '\r' '\n' | grep -v '^\s*$' | tail -10 | cut -c1-160
