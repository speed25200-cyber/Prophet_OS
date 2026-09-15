#!/usr/bin/env python3
"""Installe Prophet OS dans une machine virtuelle QEMU (KVM, SeaBIOS), depuis l'ISO, en pilotant
l'installeur par la console série exactement comme un humain le ferait au clavier : connexion
root sur le support, `prophet-installer --disque /dev/vda`, recopie du nom du disque, phrase de
passe deux fois, mot de passe du compte deux fois, puis extinction. Tout ce que la console dit
est gardé dans /root/vm/serial-install.log ; les jalons vont sur la sortie standard.

Variables : PROPHET_VM (dossier de travail, /root/vm), PROPHET_QEMU_BIN (le bin de QEMU).

SeaBIOS, pas OVMF : l'OVMF du store Nix n'initialise ni l'écran ni la série dans ce QEMU (vu le
15 septembre, avec ou sans KVM) ; SeaBIOS démarre l'ISO en 50 s. Le système installé reçoit
donc GRUB, comme sur un PC sans UEFI (ADR 0032), le chemin que la CI teste aussi.

Un garde-fou coupe la machine si le disque C: de Windows (qui porte le fichier d'échange)
descend sous 2,5 Go : on a déjà vu ce disque se remplir et tronquer des fichiers.
"""
import os
import socket
import subprocess
import sys
import threading
import time

VM = os.environ.get("PROPHET_VM", "/root/vm")
QEMU = os.environ.get("PROPHET_QEMU_BIN", "/nix/store/gx222l0zm41h4zqrgpb07nc0brwpv3nk-qemu-11.1.0/bin")
ISO = f"{VM}/prophet.iso"
DISQUE = f"{VM}/prophet.qcow2"
SERIE = f"{VM}/serial.sock"
MONITEUR = f"{VM}/monitor.sock"
JOURNAL = f"{VM}/serial-install.log"
PHRASE = "phrasedepassedetest"
MOTDEPASSE = "motdepassedetest"


def jalon(texte):
    print(time.strftime("%H:%M:%S"), texte, flush=True)


def garde(proc):
    while proc.poll() is None:
        try:
            st = os.statvfs("/mnt/c")
            libre = st.f_bavail * st.f_frsize // (1024 * 1024)
            if libre < 2500:
                jalon(f"GARDE : C: n'a plus que {libre} Mo, arrêt de la machine")
                proc.kill()
                return
        except OSError:
            pass
        time.sleep(15)


class Console:
    def __init__(self, chemin, journal):
        self.tampon = b""
        self.journal = open(journal, "ab")
        for _ in range(60):
            try:
                self.s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                self.s.connect(chemin)
                break
            except OSError:
                time.sleep(1)
        else:
            raise RuntimeError("console série injoignable")
        self.s.settimeout(1.0)

    def attendre(self, motif, delai):
        cible = motif.encode()
        fin = time.time() + delai
        while time.time() < fin:
            try:
                bloc = self.s.recv(4096)
                if bloc:
                    self.journal.write(bloc)
                    self.journal.flush()
                    self.tampon += bloc
            except socket.timeout:
                pass
            if cible in self.tampon:
                idx = self.tampon.index(cible) + len(cible)
                self.tampon = self.tampon[idx:]
                return True
            if len(self.tampon) > 1_000_000:
                self.tampon = self.tampon[-200_000:]
        return False

    def envoyer(self, texte):
        self.s.sendall(texte.encode())


def main():
    os.makedirs(VM, exist_ok=True)
    for f in (SERIE, MONITEUR):
        if os.path.exists(f):
            os.remove(f)
    if os.path.exists(JOURNAL):
        os.replace(JOURNAL, JOURNAL + ".precedent")
    if not os.path.exists(f"{VM}/cache.img"):
        jalon("cache.img absent : lancer build-cache.sh d'abord")
        return 2
    if not os.path.exists(ISO):
        jalon(f"ISO absente : {ISO}")
        return 2
    if os.path.exists(DISQUE):
        os.remove(DISQUE)
    subprocess.run([f"{QEMU}/qemu-img", "create", "-f", "qcow2", DISQUE, "100G"], check=True)
    commande = [
        # 3 Gio : la mémoire engagée pour la VM fait grossir le fichier d'échange de Windows
        # sur C:, qui a déjà déclenché le garde-fou à 4 Gio.
        f"{QEMU}/qemu-system-x86_64", "-enable-kvm", "-cpu", "host", "-m", "4096", "-smp", "4",
        "-drive", f"file={DISQUE},if=virtio,format=qcow2",
        # /dev/vdb : la fermeture prophet-ci construite sur l'hôte (build-cache.sh), en cache
        # binaire ext4 en lecture seule ; l'installeur y puise au lieu de tout compiler.
        "-drive", f"file={VM}/cache.img,if=virtio,format=raw,readonly=on",
        "-cdrom", ISO, "-boot", "d",
        "-nic", "user,model=virtio-net-pci",
        "-display", "none", "-vga", "std",
        "-serial", f"unix:{SERIE},server=on,wait=off",
        "-monitor", f"unix:{MONITEUR},server=on,wait=off",
        "-no-reboot",
    ]
    jalon("démarrage de QEMU sur l'ISO (SeaBIOS)")
    proc = subprocess.Popen(commande, stdout=open(f"{VM}/qemu-install.out", "wb"), stderr=subprocess.STDOUT)
    threading.Thread(target=garde, args=(proc,), daemon=True).start()
    con = Console(SERIE, JOURNAL)

    def etape(motif, delai, reponse=None, nom=None):
        if not con.attendre(motif, delai):
            jalon(f"ÉCHEC : « {motif} » n'est pas venu en {delai} s")
            proc.kill()
            sys.exit(1)
        jalon(nom or f"vu : {motif}")
        if reponse is not None:
            time.sleep(0.5)
            con.envoyer(reponse)

    etape("support d'installation", 600, nom="le support d'installation a démarré (SeaBIOS, ISOLINUX)")
    # Le support ouvre de lui-même une session « nixos » sur la console série (invite « ~]$ ») ;
    # l'installeur se lance par sudo, sans mot de passe, comme l'accueil le dit.
    etape("automatic login", 120, nom="session ouverte d'elle-même sur la console")
    etape("~]$", 60, "lsblk -o NAME,SIZE,TYPE\n", "invite du shell")
    # Les ajustements de l'installeur de cette ISO (9abc822), posés sur la clé par un script
    # sed envoyé par la console : (1) cp -rL — le lien vers le magasin était copié au lieu du
    # dépôt ; (2) CARTE — posée dans un sous-shell ; (3) la configuration prophet-ci (sans
    # suite ni modèle : ce que la CI démarre) ; (4) le cache de l'hôte sur /dev/vdb, donné à
    # nixos-install comme substituteur sans signature ; (5) 4 Gio d'échange sur la racine A
    # pendant l'évaluation (le noyau a tué nix à 3 Gio), retirés avant la fin.
    PATCH = r"""s/cp -r "$DEPOT"/cp -rL --no-preserve=mode "$DEPOT"/
s/^INVENTAIRE=$(inventaire)$/CARTE=""; INVENTAIRE=$(inventaire)/
s|source#prophet"|source#prophet-ci"|
/^vert "✓ montés sous \$CIBLE"/a mkdir -p /cache && mount -o ro /dev/vdb /cache && echo "cache local : $(ls /cache | wc -l) entrées" ; fallocate -l 4G /mnt/prophet-swap && chmod 600 /mnt/prophet-swap && mkswap -q /mnt/prophet-swap && swapon /mnt/prophet-swap
s|^nixos-install \\$|nixos-install --option substituters "file:///cache https://cache.nixos.org" --option require-sigs false --max-jobs 2 --cores 4 \\|
/^titre "Installation terminée"/i swapoff /mnt/prophet-swap ; rm -f /mnt/prophet-swap
"""
    # Les marqueurs attendus sont coupés en deux à la frappe (SYNT''AXE_OK) : la console
    # renvoie ce qu'on tape, et le texte tapé ne doit pas passer pour le résultat.
    etape("~]$", 30,
          "W=$(command -v prophet-installer); S=$(dirname \"$(readlink -f \"$W\")\")/.prophet-installer-wrapped; "
          "cat > /tmp/patch.sed <<'EOF_SED'\n" + PATCH + "EOF_SED\n",
          "disques listés, script d'ajustement envoyé")
    etape("~]$", 30,
          "sed -f /tmp/patch.sed \"$S\" > /tmp/inst.sh; echo AJUST''EMENTS=$(( $(grep -c 'cp -rL' /tmp/inst.sh) + $(grep -c '^CARTE=\"\"; INVENTAIRE' /tmp/inst.sh) + $(grep -c 'source#prophet-ci' /tmp/inst.sh) + $(grep -c 'mount -o ro /dev/vdb' /tmp/inst.sh) + $(grep -c 'require-sigs false' /tmp/inst.sh) + $(grep -c '^swapoff' /tmp/inst.sh) ))\n",
          "installeur ajusté")
    etape("AJUSTEMENTS=6", 30, nom="les six ajustements vérifiés (cp -rL, CARTE, prophet-ci, cache /dev/vdb, substituteur, échange)")
    etape("~]$", 30, "bash -n /tmp/inst.sh && echo SYNT''AXE_OK\n", "vérification de la syntaxe")
    etape("SYNTAXE_OK", 30, nom="syntaxe du script ajusté correcte")
    etape("~]$", 30,
          "sudo bash -c 'eval \"$(grep -E \"^(export )?PATH\" \"$W\")\"; bash /tmp/inst.sh --disque /dev/vda'\n",
          "lancement de l'installeur ajusté")
    etape("Recopiez le nom du disque", 300, "/dev/vda\n", "vérifications et relevé de la machine faits, confirmation demandée")
    etape("phrase de passe :", 60, PHRASE + "\n", "phrase de passe demandée")
    etape("répétez", 60, PHRASE + "\n", "phrase répétée")
    etape("mot de passe :", 60, MOTDEPASSE + "\n", "mot de passe du compte demandé")
    etape("répétez", 60, MOTDEPASSE + "\n", "mot de passe répété")
    etape("Installation du système", 900, nom="disque partitionné, chiffré, monté ; installation lancée")
    etape("Installation terminée", 14400, "poweroff\n", "INSTALLATION TERMINÉE")
    for _ in range(180):
        if proc.poll() is not None:
            break
        time.sleep(1)
    else:
        jalon("la machine ne s'est pas éteinte seule ; arrêt forcé")
        proc.kill()
    jalon(f"QEMU terminé (code {proc.returncode})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
