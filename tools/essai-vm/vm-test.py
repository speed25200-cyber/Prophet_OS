#!/usr/bin/env python3
"""Essaie le système installé dans la VM, comme un humain devant l'écran : lit l'écran par
capture et OCR, tape au clavier virtuel. Suppose QEMU lancé par vm-boot.sh (moniteur sur
/root/vm/monitor.sock). Étapes : phrase de passe des volumes chiffrés, écran de connexion,
session du bureau, puis une console texte (tty2) pour `prophet status` et l'état des services.
Captures dans /root/vm/shots et recopiées sur le scratchpad Windows ; jalons sur la sortie.
"""
import os
import re
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import importlib  # noqa: E402

vm_mon = importlib.import_module("vm-mon")

PHRASE = "phrasedepassedetest"
MOTDEPASSE = "motdepassedetest"


def jalon(texte):
    print(time.strftime("%H:%M:%S"), texte, flush=True)


class Ecran:
    def __init__(self):
        self.mon = vm_mon.Moniteur()
        self.n = 0

    def lire(self, nom=None):
        self.n += 1
        nom = nom or f"ecran-{self.n:03d}"
        return vm_mon.capture(self.mon, nom, silencieux=True)

    def attendre(self, motif, delai, nom=None):
        fin = time.time() + delai
        dernier = ""
        while time.time() < fin:
            dernier = self.lire(nom)
            if re.search(motif, dernier, re.IGNORECASE):
                return dernier
            time.sleep(4)
        jalon(f"ÉCHEC : « {motif} » n'est pas venu à l'écran en {delai} s ; dernière lecture :")
        print(dernier[-1200:], flush=True)
        sys.exit(1)

    def taper(self, texte):
        vm_mon.taper(self.mon, texte)

    def touche(self, touche):
        self.mon.cmd(f"sendkey {touche}")


def main():
    e = Ecran()
    jalon("attente de la demande de phrase de passe (volumes chiffrés)")
    e.attendre(r"passphrase|phrase de passe|prophet-(home|state)|enter pass", 240, "luks")
    e.taper(PHRASE + "\n")
    jalon("phrase de passe tapée")
    # Le second volume peut la redemander si la première n'est pas réutilisée.
    time.sleep(6)
    texte = e.lire("apres-luks")
    if re.search(r"passphrase|phrase de passe", texte, re.IGNORECASE) and "Prophet" not in texte:
        e.taper(PHRASE + "\n")
        jalon("phrase de passe tapée une seconde fois")
    jalon("attente de l'écran de connexion")
    e.attendre(r"Prophet\s*OS", 300, "connexion")
    e.touche("ret")
    e.attendre(r"[Pp]assword|[Mm]ot de passe", 60, "mot-de-passe")
    e.taper(MOTDEPASSE + "\n")
    jalon("session demandée")
    e.attendre(r"[Mm]issions|Supervision|Prophet", 240, "bureau")
    jalon("BUREAU AFFICHÉ (capture bureau.png)")
    e.lire("bureau")
    time.sleep(10)
    e.lire("bureau-10s")
    # Une console texte pour interroger le système sans toucher au bureau.
    jalon("passage sur tty2")
    e.touche("ctrl-alt-f2")
    e.attendre(r"login:", 60, "tty2")
    e.taper("prophet\n")
    e.attendre(r"[Pp]assword", 30, "tty2-mdp")
    e.taper(MOTDEPASSE + "\n")
    e.attendre(r"\$", 30, "tty2-shell")
    e.taper("clear; prophet status 2>&1 | head -30\n")
    time.sleep(12)
    jalon("prophet status :")
    print(e.lire("status")[-1800:], flush=True)
    e.taper("clear; systemctl --failed --no-legend; echo FAILED_FIN; systemctl is-active prophet-capd prophet-ledger prophet-vault prophet-egress prophet-sandboxd prophet-memoryd prophet-agentd | tr '\\n' ' '; echo; cat /etc/prophet/source/image/machine/inventaire.txt | head -12\n")
    time.sleep(8)
    jalon("services et relevé :")
    print(e.lire("services")[-1800:], flush=True)
    e.taper("clear; prophet task ls 2>&1 | head; uname -r; df -h / /home /var/lib/prophet | tail -3\n")
    time.sleep(8)
    print(e.lire("divers")[-1500:], flush=True)
    e.touche("ctrl-alt-f1")
    time.sleep(3)
    e.lire("bureau-retour")
    jalon("ESSAI TERMINÉ")
    return 0


if __name__ == "__main__":
    sys.exit(main())
