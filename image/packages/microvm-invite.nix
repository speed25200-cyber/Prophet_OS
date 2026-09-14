# L'invité des microVM de sandboxd (niveau 2, ADR 0038) : un noyau et une racine.
#
# Le noyau est celui que le projet Firecracker publie pour ses propres essais : configuré pour
# lui (virtio par MMIO, console série, pas de PCI), il démarre en moins d'une seconde. Il est
# épinglé par son empreinte ; l'outil d'installation sur l'hôte (`tools/install-isolation.sh`)
# choisissait le même en listant le dépôt.
#
# La racine est construite ici, pas téléchargée : une image squashfs en lecture seule avec un
# busybox statique, Python 3 et sa fermeture Nix, et un `/init` qui lit la ligne de commande du
# noyau. C'est tout ce qu'un programme confié par un agent peut voir ; l'espace de travail de la
# tâche arrive par un second disque, monté à l'endroit que la ligne de commande nomme.
{ lib, runCommand, fetchurl, writeScript, squashfsTools, closureInfo, pkgsStatic, python3 }:

let
  noyau = fetchurl {
    url = "https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci/v1.12/x86_64/vmlinux-6.1.128";
    sha256 = "1j7c4xi88p2djyv92d4ffbjpvdyfnqj4a102xglifxbjk85k3a17";
  };

  # Ce que l'invité fait au démarrage. Tout est dit sur la console série, que sandboxd lit :
  # « PROPHET_INVITE_PRET » quand la racine est montée, « PROPHET_INVITE_FIN code=N » quand le
  # programme a rendu la main, puis l'invité s'éteint, et le moniteur avec lui.
  init = writeScript "prophet-invite-init" ''
    #!/bin/sh
    export PATH=/bin
    mount -t proc proc /proc
    mount -t sysfs sys /sys
    mount -t devtmpfs dev /dev 2>/dev/null || true
    mount -t tmpfs tmp /tmp
    echo "PROPHET_INVITE_PRET"
    programme=""
    travail=""
    for mot in $(cat /proc/cmdline); do
      case "$mot" in
        prophet.program=*) programme="''${mot#prophet.program=}" ;;
        prophet.workdir=*) travail="''${mot#prophet.workdir=}" ;;
      esac
    done
    code=0
    if [ -n "$travail" ] && [ -b /dev/vdb ]; then
      mkdir -p "$travail"
      if ! mount -t ext4 /dev/vdb "$travail"; then
        echo "PROPHET_INVITE_ERREUR espace de travail non monté"
        code=125
      fi
    fi
    # Le programme et ses arguments viennent d'un fichier de l'espace de travail, écrit par
    # sandboxd ; la ligne de commande du noyau ne porte que le chemin du programme, sans ses
    # arguments ni son environnement.
    if [ "$code" = 0 ] && [ -n "$travail" ] && [ -f "$travail/.prophet/exec.sh" ]; then
      cd "$travail" && sh "$travail/.prophet/exec.sh"
      code=$?
    elif [ "$code" = 0 ] && [ -n "$programme" ] && [ -x "$programme" ]; then
      "$programme"
      code=$?
    fi
    echo "PROPHET_INVITE_FIN code=$code"
    sync
    [ -n "$travail" ] && umount "$travail" 2>/dev/null
    # `poweroff` laisse ce noyau en « System halted », moniteur ouvert ; avec `reboot=k`, un
    # redémarrage passe par le contrôleur clavier, que Firecracker traduit en sortie du moniteur.
    reboot -f
  '';

  fermeture = closureInfo { rootPaths = [ python3 ]; };
in
runCommand "prophet-invite-microvm" {
  nativeBuildInputs = [ squashfsTools ];
  meta = {
    description = "Noyau et racine de l'invité des microVM de Prophet OS (niveau 2)";
    platforms = [ "x86_64-linux" ];
    license = lib.licenses.gpl2Only; # le noyau
  };
} ''
  mkdir -p racine/bin racine/sbin racine/proc racine/sys racine/dev racine/tmp racine/etc racine/nix/store
  cp ${pkgsStatic.busybox}/bin/busybox racine/bin/busybox
  for applet in sh mount umount cat echo ls mkdir rm cp mv sleep env grep sed reboot poweroff sync test; do
    ln -s busybox "racine/bin/$applet"
  done
  # Le noyau lance /sbin/init avant /init : que ce soit le même script, et non l'init de busybox,
  # qui chercherait /etc/init.d/rcS et attendrait une console.
  ln -s ../init racine/sbin/init
  # Python 3 et tout ce dont il dépend, aux mêmes chemins que dans le magasin.
  while read -r chemin; do
    cp -a "$chemin" racine/nix/store/
  done < ${fermeture}/store-paths
  ln -s ${python3}/bin/python3 racine/bin/python3
  cp ${init} racine/init
  chmod 0755 racine/init
  printf 'root:x:0:0:root:/:/bin/sh\n' > racine/etc/passwd
  printf 'root:x:0:\n' > racine/etc/group
  mkdir -p $out
  cp ${noyau} $out/vmlinux
  mksquashfs racine $out/rootfs.squashfs -comp zstd -noappend -all-root -quiet
''
