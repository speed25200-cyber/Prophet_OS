# Essai de l'ISO dans une machine virtuelle QEMU

Ce que la CI ne fait pas : installer l'ISO **avec son installeur**, redémarrer sur le disque
seul, ouvrir la session comme un humain. Fait la première fois le 15 septembre 2026 (voir
`docs/reports/installation-vm-2026-09-15.md`) ; trois fautes bloquantes y ont été trouvées.

Prérequis sur l'hôte Linux : KVM, QEMU (`qemu-system-x86_64`, `qemu-img`), `nix`, e2fsprogs
(`mkfs.ext4 -d`), ImageMagick et Tesseract pour lire l'écran. Les chemins par défaut sont
ceux de la machine de développement ; les variables `PROPHET_VM` (dossier de travail),
`PROPHET_QEMU_BIN`, `PROPHET_OVMF`, `PROPHET_MAGICK`, `PROPHET_TESSERACT`, `PROPHET_SRC`,
`PROPHET_REV` les remplacent.

1. `prophet.iso` dans `$PROPHET_VM` : l'artefact `prophet-os-iso` d'un run vert, empreinte
   vérifiée.
2. `build-cache.sh` : construit sur l'hôte la fermeture `prophet-ci` à la révision de l'ISO et
   la met dans `cache.img` (ext4, cache binaire de fichiers). Sans lui, la VM compile Prophet OS
   elle-même, des heures à 4 Gio.
3. `vm-install.py` : démarre l'ISO sous SeaBIOS, pilote l'installeur par la console série,
   ajuste au passage l'installeur d'une ISO antérieure aux correctifs (script sed envoyé par la
   console ; à retirer pour une ISO qui les porte), monte le cache en `/dev/vdb`.
4. `vm-boot.sh` : démarre le disque installé. Puis `vm-test.py` (phrase de passe, connexion,
   bureau), `tty2b.sh` (`prophet status`, services, journal sur tty2), `regard.sh` (une capture
   nommée), `eteindre.sh`. `vm-mon.py type|shot|cmd` parle au moniteur QEMU.

Sous WSL, la distribution s'éteint dès qu'aucune session `wsl.exe` n'est ouverte : lancer les
longues étapes dans une session tenue ouverte. Les captures sont des PPM (`.png` de nom) ;
`tesseract` les lit, un visionneur exige une conversion.
