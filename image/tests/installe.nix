# La configuration installée, son chargeur UEFI et le parcours graphique humain réel.
# Racine en lecture-écriture dans le cadre de test, déverrouillage LUKS exercé séparément.
{ pkgs, module }:
import ./desktop-session.nix { inherit pkgs module; installed = true; }
