#!/usr/bin/env bash
# Un service peut-il faire ce que ses capacités lui accordent ?
#
# `sandboxd` recevait `CAP_SETUID`, `CAP_SETGID` et `CAP_SYS_ADMIN`, et gardait le filtre d'appels
# système des six autres daemons : `@system-service` moins `@privileged`. Or `@system-service` ne
# contient pas `@mount`, et `~@privileged` retire `setuid`, `setgid`, `setgroups` et `pivot_root`.
# Accorder une capacité d'une main et interdire de l'autre l'appel qui s'en sert ne se voit ni à
# l'évaluation, ni à la construction, ni au démarrage du service — seulement au premier essai réel.
# Il a fallu un test en machine virtuelle de sept minutes pour l'apprendre, sous la forme :
#
#     confinement impossible : écriture de uid_map : Operation not permitted
#
# Ce contrôle-ci le dit en une seconde. Il ne remplace pas le test — lui seul exerce le
# durcissement réel — mais il évite d'y aller pour une faute qui se lit dans le fichier.
#
# Deux contradictions sont cherchées, et rien d'autre :
#
# 1. Un service à qui l'on accorde `CAP_SETUID`, `CAP_SETGID` ou `CAP_SYS_ADMIN`, et dont le
#    filtre retire `@privileged` ou n'ajoute pas `@mount`.
# 2. `RestrictSUIDSGID = true` en même temps que `NoNewPrivileges = false`. Le premier implique le
#    second à `true` : les déclarer ensemble, c'est demander une chose et son contraire.
#
# Codes de sortie : 0 tout va bien, 1 contradiction trouvée, 3 fichier illisible — auquel cas on
# ne conclut pas. Une sonde qui n'a rien pu lire n'a rien vérifié.

set -uo pipefail

MODULE="${1:-image/modules/prophet.nix}"

ok()   { printf '  \033[32m✓\033[0m %s\n' "$1"; }
ko()   { printf '  \033[31m✗\033[0m %s\n' "$1"; }
info() { printf '    %s\n' "$1"; }

if [ ! -r "$MODULE" ]; then
  echo "MODULE_ILLISIBLE : $MODULE" >&2
  echo "Rien n'est conclu : une sonde qui n'a pas pu lire n'a rien vérifié." >&2
  exit 3
fi

echo "Ce que chaque service reçoit, et ce que son filtre lui laisse faire."
echo

# Découpe le module en blocs `prophet-<nom> = daemon { ... };`, un par ligne, champs séparés par
# des tabulations, pour qu'un seul passage d'awk suffise.
DEFAUTS=0
TROUVES=0

# shellcheck disable=SC2016
BLOCS=$(awk '
  /^ *prophet-[a-z]+ = daemon \{/ {
    nom = $1; sub(/^prophet-/, "", nom)
    dans = 1; profondeur = 0; bloc = ""
  }
  dans {
    bloc = bloc "\n" $0
    profondeur += gsub(/\{/, "{")
    profondeur -= gsub(/\}/, "}")
    if (profondeur <= 0) {
      print "===" nom
      print bloc
      dans = 0
    }
  }
' "$MODULE")

if [ -z "$BLOCS" ]; then
  echo "AUCUN_SERVICE_RECONNU dans $MODULE" >&2
  echo "Le modèle de bloc a dû changer ; on ne conclut pas sur une lecture qui n'a rien trouvé." >&2
  exit 3
fi

nom=""
bloc=""
analyser() {
  [ -n "$nom" ] || return 0
  TROUVES=$((TROUVES + 1))

  local capacites filtre restrict_suid nnp manques
  # Les capacités accordées, quelle que soit la ligne qui les accorde.
  capacites=$(printf '%s' "$bloc" | grep -oE 'CAP_(SETUID|SETGID|SYS_ADMIN)' | sort -u | tr '\n' ' ')
  # Le filtre : celui du bloc s'il en pose un, celui du modèle commun sinon.
  if printf '%s' "$bloc" | grep -q 'SystemCallFilter'; then
    filtre=$(printf '%s' "$bloc" | sed -n '/SystemCallFilter/,/\]/p')
  else
    filtre=$(sed -n '/SystemCallFilter = \[/,/\]/p' "$MODULE" | head -20)
  fi
  restrict_suid=$(printf '%s' "$bloc" | grep -c 'RestrictSUIDSGID = lib.mkForce false' || true)
  nnp=$(printf '%s' "$bloc" | grep -c 'NoNewPrivileges = lib.mkForce false' || true)

  if [ -z "$capacites" ]; then
    ok "$nom — aucune capacité privilégiée demandée"
  else
    manques=""
    printf '%s' "$filtre" | grep -q '"~@privileged"' && manques="$manques ~@privileged"
    printf '%s' "$filtre" | grep -q '"@mount"' || manques="$manques (pas de @mount)"
    if [ -n "$manques" ]; then
      ko "$nom — reçoit ${capacites}mais son filtre le lui refuse :$manques"
      info "un service à qui l'on accorde CAP_SETUID et qui ne peut pas appeler setuid ne fera"
      info "rien de cette capacité, et ne le dira qu'au premier essai réel"
      DEFAUTS=$((DEFAUTS + 1))
    else
      ok "$nom — reçoit ${capacites}et son filtre le lui permet"
    fi
  fi

  # `RestrictSUIDSGID = true` implique `NoNewPrivileges = true`. Le modèle commun pose le premier ;
  # un service qui désactive le second sans désactiver le premier demande une chose et son
  # contraire, et c'est systemd qui tranche — pas celui qui a écrit le fichier.
  if [ "$nnp" -gt 0 ] && [ "$restrict_suid" -eq 0 ]; then
    ko "$nom — désactive NoNewPrivileges mais garde RestrictSUIDSGID, qui l'implique"
    info "systemd tranchera en faveur de RestrictSUIDSGID, et NoNewPrivileges restera actif"
    DEFAUTS=$((DEFAUTS + 1))
  fi
}

while IFS= read -r ligne; do
  case "$ligne" in
    ===*) analyser; nom="${ligne#===}"; bloc="" ;;
    *) bloc="$bloc
$ligne" ;;
  esac
done <<< "$BLOCS"
analyser

echo
if [ "$DEFAUTS" -gt 0 ]; then
  echo "$DEFAUTS contradiction(s) entre ce qui est accordé et ce qui est permis." >&2
  exit 1
fi
echo "$TROUVES service(s) : aucun ne reçoit une capacité que son filtre lui refuse."
