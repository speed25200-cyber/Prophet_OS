#!/usr/bin/env bash
# Chaque service déclaré nomme-t-il un programme qui existe ?
#
# Pourquoi ce script existe. `image/modules/prophet.nix` déclare sept services systemd dont
# l'`ExecStart` pointe vers `${prophet}/bin/prophet-<nom>`. Rien, jusqu'ici, ne vérifiait que
# l'atelier produise ces programmes. Nix n'y voit rien : il substitue un chemin, il ne regarde pas
# ce qu'il y a au bout. systemd ne s'en aperçoit qu'au démarrage de la machine, trop tard.
#
# C'est le même défaut que celui d'ADR-0006, sous un autre costume : constater une déclaration
# n'est pas vérifier une présence. Un système qui déclare sept daemons et n'en installe aucun
# paraît complet partout sauf là où il compte.
#
# Une remarque sur ce script lui-même. Sa première version cherchait le texte
# `${prophet}/bin/...` et trouvait `prophet-`, parce que le nom du daemon est interpolé par Nix.
# Elle rapportait donc un seul manque là où il y en a sept — en paraissant marcher. Le modèle est
# désormais résolu, et un modèle non résolu est une erreur, pas un résultat.

set -uo pipefail
cd "$(dirname "$0")/.."

produits=$(cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c '
import json, sys
for p in json.load(sys.stdin)["packages"]:
    for t in p["targets"]:
        if "bin" in t["kind"]:
            print(t["name"])
' | sort -u)

if [ -z "$produits" ]; then
  echo "impossible de lire les binaires de l'atelier (cargo metadata a échoué)" >&2
  exit 1
fi

declares=$(python3 - <<'PY'
import re, sys, pathlib

modules = list(pathlib.Path("image/modules").glob("*.nix"))
texte = "\n".join(p.read_text() for p in modules)

# Les noms passés au constructeur `daemon`, qui interpole `${name}` dans son ExecStart.
noms = re.findall(r'=\s*daemon\s*\{[^}]*?name\s*=\s*"([^"]+)"', texte, re.S)

attendus = set()
for chemin in re.findall(r'\$\{prophet\}/bin/([A-Za-z0-9_${}-]+)', texte):
    if "${name}" in chemin:
        if not noms:
            print("MODELE_NON_RESOLU", chemin, file=sys.stderr)
            sys.exit(3)
        for n in noms:
            attendus.add(chemin.replace("${name}", n))
    elif "$" in chemin or "{" in chemin:
        # Une autre interpolation, que ce script ne sait pas résoudre. Se taire serait mentir.
        print("MODELE_NON_RESOLU", chemin, file=sys.stderr)
        sys.exit(3)
    else:
        attendus.add(chemin)

for nom in sorted(attendus):
    print(nom)
PY
)
etat=$?

if [ "$etat" -eq 3 ]; then
  echo "un ExecStart contient une interpolation que ce script ne sait pas résoudre (voir ci-dessus)." >&2
  echo "Il refuse de conclure plutôt que de rapporter un résultat qu'il n'a pas vérifié." >&2
  exit 1
fi

if [ -z "$declares" ]; then
  echo "aucun ExecStart ne référence le paquet Prophet OS — le script ne vérifie donc rien," >&2
  echo "ce qui est plus suspect qu'un manque. Vérifiez le motif de recherche." >&2
  exit 1
fi

echo "Programmes déclarés par les modules NixOS, et produits — ou non — par l'atelier."
echo

manquants=0
for nom in $declares; do
  if grep -qx "$nom" <<<"$produits"; then
    printf '  ✓ %-28s produit\n' "$nom"
  else
    printf '  ✗ %-28s DÉCLARÉ MAIS JAMAIS CONSTRUIT\n' "$nom"
    manquants=$((manquants + 1))
  fi
done

echo
if [ "$manquants" -gt 0 ]; then
  cat >&2 <<FIN
$manquants programme(s) déclaré(s) par un service systemd n'existe(nt) pas.

Une machine installée démarrerait avec autant d'unités en échec, pendant que le reste du système
paraîtrait fonctionner. Soit l'atelier produit ces programmes, soit le module cesse de les
déclarer — mais pas les deux états à la fois.
FIN
  exit 1
fi

echo "Tous les programmes déclarés sont produits."
