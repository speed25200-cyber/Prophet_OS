#!/usr/bin/env bash
# Le guide d'installation décrit-il tous les travaux dont il dit de vérifier qu'ils sont verts ?
#
# `docs/installation.md` demande, avant de graver une image, d'ouvrir l'exécution qui l'a produite
# et de vérifier ses travaux. Ce tableau a pris du retard une première fois : il annonçait « cinq
# travaux » alors que le workflow en comptait sept. Quelqu'un qui suit la consigne aurait donc
# déclaré bonne une image dont deux contrôles — dont celui qui démarre le système installé —
# avaient échoué, sans jamais les regarder.
#
# Une consigne qui a pris du retard est pire qu'une consigne absente : elle donne la confiance
# sans la vérification. Ce contrôle la maintient à jour, en une seconde.
#
# Codes de sortie : 0 tout est décrit, 1 un travail manque au tableau, 3 lecture impossible.

set -uo pipefail

WORKFLOW="${1:-.github/workflows/iso.yml}"
GUIDE="${2:-docs/installation.md}"

ok() { printf '  \033[32m✓\033[0m %s\n' "$1"; }
ko() { printf '  \033[31m✗\033[0m %s\n' "$1"; }

for fichier in "$WORKFLOW" "$GUIDE"; do
  if [ ! -r "$fichier" ]; then
    echo "ILLISIBLE : $fichier" >&2
    echo "Rien n'est conclu : une sonde qui n'a pas pu lire n'a rien vérifié." >&2
    exit 3
  fi
done

# Les noms affichés par GitHub, c'est-à-dire `name:` à l'intérieur d'un travail — et non la clé du
# travail, que personne ne voit dans l'interface.
NOMS=$(awk '
  /^  [a-z-]+:$/ { dans = 1; next }
  dans && /^    name: / { sub(/^    name: /, ""); gsub(/^"|"$|^'"'"'|'"'"'$/, ""); print; dans = 0 }
' "$WORKFLOW")

if [ -z "$NOMS" ]; then
  echo "AUCUN_TRAVAIL_RECONNU dans $WORKFLOW" >&2
  echo "Le modèle a dû changer ; on ne conclut pas sur une lecture qui n'a rien trouvé." >&2
  exit 3
fi

echo "Les travaux du workflow, et leur description dans le guide d'installation."
echo

MANQUE=0
TOTAL=0
while IFS= read -r nom; do
  [ -n "$nom" ] || continue
  TOTAL=$((TOTAL + 1))
  if grep -qF "$nom" "$GUIDE"; then
    ok "$nom"
  else
    ko "$nom — absent du tableau de $GUIDE"
    MANQUE=$((MANQUE + 1))
  fi
done <<< "$NOMS"

echo
if [ "$MANQUE" -gt 0 ]; then
  echo "$MANQUE travail(x) non décrit(s) : le guide dit de vérifier une liste incomplète." >&2
  exit 1
fi
echo "$TOTAL travaux, tous décrits."
