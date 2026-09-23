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
  # programme a rendu la main, puis l'invité s'éteint, et le moniteur avec lui. En réserve,
  # « PROPHET_INVITE_ATTENTE » précède : l'invité attend le disque de sa tâche.
  init = writeScript "prophet-invite-init" ''
    #!/bin/sh
    export PATH=/bin
    mount -t proc proc /proc
    mount -t sysfs sys /sys
    mount -t devtmpfs dev /dev 2>/dev/null || true
    mount -t tmpfs tmp /tmp
    # La racine est en lecture seule ; l'espace de travail doit pourtant se monter au chemin
    # qu'il a sur l'hôte, quel qu'il soit (/tmp/…, /home/…, /var/…). Une surcouche en mémoire
    # sur la racine (overlay) rend ce chemin créable ; le programme s'exécute dans cette
    # surcouche. Sans overlay dans le noyau, la racine reste telle quelle et seuls les chemins
    # déjà présents (/tmp) sont possibles.
    racine=/
    mkdir -p /tmp/haut /tmp/ouvrage /tmp/racine
    if mount -t overlay overlay -o lowerdir=/,upperdir=/tmp/haut,workdir=/tmp/ouvrage /tmp/racine 2>/dev/null; then
      racine=/tmp/racine
      mount -t proc proc "$racine/proc"
      mount -t sysfs sys "$racine/sys"
      mount -t devtmpfs dev "$racine/dev" 2>/dev/null || true
      mount -t tmpfs tmp "$racine/tmp"
    fi
    programme=""
    travail=""
    reserve=0
    for mot in $(cat /proc/cmdline); do
      case "$mot" in
        prophet.program=*) programme="''${mot#prophet.program=}" ;;
        prophet.workdir=*) travail="''${mot#prophet.workdir=}" ;;
        prophet.pool=1) reserve=1 ;;
      esac
    done
    # En réserve (M5-T4, ADR 0045), l'invité démarre sans tâche : son second disque n'est qu'un
    # disque d'attente de 1 Mio. Il le dit, sandboxd le met en pause et en fait un instantané,
    # puis, quand une tâche arrive, remplace ce disque par celui de la tâche et le reprend. La
    # taille du disque change : c'est le signal. Le chemin de travail est écrit sur le disque,
    # la ligne de commande ayant été fixée avant qu'on sache pour qui l'invité travaillerait.
    if [ "$reserve" = 1 ]; then
      attente=$(cat /sys/block/vdb/size 2>/dev/null)
      echo "PROPHET_INVITE_ATTENTE"
      # L'attente ne court que machine en marche : en pause, rien ne tourne. Bornée à quelques
      # milliers de tours, elle ne retient pas un moniteur dont le disque ne viendrait jamais.
      tours=0
      while [ "$(cat /sys/block/vdb/size 2>/dev/null)" = "$attente" ]; do
        tours=$((tours + 1))
        if [ "$tours" -gt 4000 ]; then
          echo "PROPHET_INVITE_ERREUR le disque de la tâche n'est jamais venu"
          echo "PROPHET_INVITE_FIN code=125"
          reboot -f
        fi
        sleep 0.005 2>/dev/null || true
      done
      # Le noyau a lu le début du disque d'attente en le découvrant : on oublie ce qu'il en
      # garde avant de monter celui de la tâche.
      sync
      blockdev --flushbufs /dev/vdb 2>/dev/null || true
      echo 3 > /proc/sys/vm/drop_caches 2>/dev/null || true
      mkdir -p /tmp/arrivee
      if mount -t ext4 /dev/vdb /tmp/arrivee 2>/dev/null; then
        travail=$(cat /tmp/arrivee/.prophet/workdir 2>/dev/null)
        umount /tmp/arrivee
      fi
      [ -n "$travail" ] || echo "PROPHET_INVITE_ERREUR disque de travail sans chemin"
    fi
    echo "PROPHET_INVITE_PRET"
    code=0
    if [ -n "$travail" ] && [ -b /dev/vdb ]; then
      if ! mkdir -p "$racine$travail" || ! mount -t ext4 /dev/vdb "$racine$travail"; then
        echo "PROPHET_INVITE_ERREUR espace de travail non monté"
        code=125
      fi
    fi
    # Le programme et ses arguments viennent d'un fichier de l'espace de travail, écrit par
    # sandboxd ; la ligne de commande du noyau ne porte que le chemin du programme, sans ses
    # arguments ni son environnement.
    if [ "$code" = 0 ] && [ -n "$travail" ] && [ -f "$racine$travail/.prophet/exec.sh" ]; then
      if [ "$racine" = / ]; then
        cd "$travail" && sh "$travail/.prophet/exec.sh"
      else
        chroot "$racine" /bin/sh -c 'cd "$1" && exec sh "$1/.prophet/exec.sh"' sh "$travail"
      fi
      code=$?
    elif [ "$code" = 0 ] && [ -n "$programme" ] && [ -x "$programme" ]; then
      "$programme"
      code=$?
    fi
    echo "PROPHET_INVITE_FIN code=$code"
    sync
    [ -n "$travail" ] && umount "$racine$travail" 2>/dev/null
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
  for applet in sh mount umount cat echo ls mkdir rm cp mv sleep env grep sed reboot poweroff sync test chroot blockdev; do
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
