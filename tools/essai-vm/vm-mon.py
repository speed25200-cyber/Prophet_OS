#!/usr/bin/env python3
"""Parle au moniteur QEMU de la machine ($PROPHET_VM/monitor.sock, /root/vm par défaut).

  vm-mon.py cmd <commande>        une commande du moniteur, sa réponse sur la sortie
  vm-mon.py type <texte>          tape le texte au clavier virtuel (lettres, chiffres, - / . : _ espace), puis Entrée si le texte finit par \\n
  vm-mon.py shot <nom>            capture l'écran dans /root/vm/shots/<nom>.png (+ .txt par OCR ; copie dans $PROPHET_SHOTS_COPIE si posé)
"""
import os
import socket
import subprocess
import sys
import time

VM = os.environ.get("PROPHET_VM", "/root/vm")
MONITEUR = f"{VM}/monitor.sock"
SHOTS = f"{VM}/shots"
MAGICK = os.environ.get("PROPHET_MAGICK", "/nix/store/5rryq2jvixa60ppj58v9pwyhl6aj2ydf-imagemagick-7.1.2-29/bin/magick")
TESSERACT = os.environ.get("PROPHET_TESSERACT", "/nix/store/2xxxr4m7gp0h7qr9z8wi980f1h1g0gqd-tesseract-4.1.3/bin/tesseract")
COPIE = os.environ.get("PROPHET_SHOTS_COPIE", "")

TOUCHES = {"-": "minus", "/": "slash", ".": "dot", " ": "spc", ":": "shift-semicolon", "_": "shift-minus",
           "=": "equal", ",": "comma", "@": "shift-2", "!": "shift-1", "?": "shift-slash", ";": "semicolon",
           "|": "shift-backslash", "\\": "backslash", "'": "apostrophe", '"': "shift-apostrophe",
           "(": "shift-9", ")": "shift-0", "$": "shift-4", "%": "shift-5", "&": "shift-7", "*": "shift-8",
           "<": "shift-comma", ">": "shift-dot", "[": "bracket_left", "]": "bracket_right",
           "{": "shift-bracket_left", "}": "shift-bracket_right", "~": "shift-grave_accent", "`": "grave_accent",
           "#": "shift-3", "^": "shift-6", "+": "shift-equal", "	": "tab"}


class Moniteur:
    def __init__(self):
        self.s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.s.connect(MONITEUR)
        self.s.settimeout(2.0)
        self._lire()

    def _lire(self):
        sortie = b""
        while True:
            try:
                bloc = self.s.recv(65536)
                if not bloc:
                    break
                sortie += bloc
                if sortie.rstrip().endswith(b"(qemu)"):
                    break
            except socket.timeout:
                break
        return sortie.decode(errors="replace")

    def cmd(self, commande):
        self.s.sendall((commande + "\n").encode())
        reponse = self._lire()
        lignes = [l for l in reponse.splitlines() if l.strip() and not l.strip().startswith("(qemu)") and l.strip() != commande]
        return "\n".join(lignes)


def taper(mon, texte):
    for c in texte:
        if c == "\n":
            touche = "ret"
        elif c in TOUCHES:
            touche = TOUCHES[c]
        elif c.isalpha() and c.isupper():
            touche = "shift-" + c.lower()
        elif c.isalnum():
            touche = c
        else:
            raise SystemExit(f"caractère non tapable : {c!r}")
        mon.cmd(f"sendkey {touche}")
        time.sleep(0.08)


def capture(mon, nom, silencieux=False):
    os.makedirs(SHOTS, exist_ok=True)
    ppm = f"{SHOTS}/{nom}.ppm"
    png = f"{SHOTS}/{nom}.png"
    mon.cmd(f"screendump {ppm}")
    for _ in range(20):
        if os.path.exists(ppm) and os.path.getsize(ppm) > 0:
            break
        time.sleep(0.2)
    subprocess.run([MAGICK, ppm, png], check=True)
    texte = subprocess.run([TESSERACT, png, "-", "--psm", "6"], capture_output=True, text=True).stdout
    with open(f"{SHOTS}/{nom}.txt", "w") as f:
        f.write(texte)
    if COPIE:
            subprocess.run(["cp", png, f"{COPIE}/{nom}.png"], check=False)
    if not silencieux:
        print(texte.strip())
    return texte


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    mon = Moniteur()
    action, arg = sys.argv[1], " ".join(sys.argv[2:])
    if action == "cmd":
        print(mon.cmd(arg))
    elif action == "type":
        taper(mon, arg.replace("\\n", "\n"))
    elif action == "shot":
        capture(mon, arg)
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
