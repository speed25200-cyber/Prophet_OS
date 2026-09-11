# Prophet OS — Avancement

Ce fichier est la source de vérité de l'avancement. L'agent constructeur prend la première tâche non cochée dont les dépendances sont cochées, et coche avec la date et le hash du commit.

Format : `- [ ] ID — titre` puis, une fois fini, `- [x] ID — titre (AAAA-MM-JJ, abc1234)`.

## Jalons

### M0 — Fondations du dépôt

- [ ] M0-T1 — Flake Nix et dev shell
- [x] M0-T2 — Workspace Cargo (2026-09-11, 3464efd) — workspace Cargo, édition 2024, lints du workspace
- [x] M0-T3 — justfile (2026-09-11, 3464efd) — justfile avec repli sans gitleaks
- [x] M0-T4 — CI GitHub Actions (2026-09-11, 3464efd) — CI : format, clippy, tests, secrets, job privilégié
- [x] M0-T5 — Documentation de base (2026-09-11, 3464efd) — ADR 0000 à 0004, STATUS, specs
- [ ] M0-T6 — Hooks et hygiène

### M1 — Spécifications gelées v0

- [x] M1-T1 — Manifeste d'agent (2026-09-11, 3464efd) — manifeste : types, parseur TOML, 11 tests de validation
- [x] M1-T2 — Jeton de capacité (2026-09-11, 3464efd) — jeton : signature, délégation, réflexivité et transitivité
- [x] M1-T3 — Événement du Ledger (2026-09-11, 3464efd) — événement : chaînage, altération/suppression/insertion détectées
- [x] M1-T4 — Contrat Agent Driver (2026-09-11, 3464efd) — types du contrat de pilote
- [x] M1-T5 — Convention IPC (2026-09-11, 3464efd) — prophet-ipc : 10 000 allers-retours en 0,4 s, SO_PEERCRED
- [ ] M1-T6 — Nommage des outils MCP système

### M2 — capd : Capability Broker et Policy Engine

- [x] M2-T1 — Daemon et clé (2026-09-11, 3464efd) — clé ed25519, broker instanciable
- [x] M2-T2 — Émission (2026-09-11, 3464efd) — émission bornée par le plafond du manifeste
- [x] M2-T3 — Délégation (2026-09-11, 3464efd) — délégation ⊆, profondeur bornée, durée bornée
- [x] M2-T4 — Vérification (2026-09-11, 3464efd) — vérification : 11,6 µs par appel en binaire optimisé
- [x] M2-T5 — Politiques Cedar (2026-09-11, 3464efd) — politiques Cedar, interdits absolus, classes d'actions
- [x] M2-T6 — Approbations (2026-09-11, 3464efd) — approbations : portées once/task/agent, expiration, révocation
- [ ] M2-T7 — CLI
- [x] M2-T8 — Application noyau (2026-09-11, 3464efd) — règles Landlock, domaines, profil seccomp

### M3 — ledger : Event Bus et Ledger

- [x] M3-T1 — Stockage (2026-09-11, 3464efd) — stockage JSONL par jour, index, réouverture
- [x] M3-T2 — API (2026-09-11, 3464efd) — requêtes filtrées, bus de diffusion
- [x] M3-T3 — Scellement (2026-09-11, 3464efd) — scellement ed25519, vérification autonome
- [ ] M3-T4 — CLI et rejeu

### M4 — sfs : Semantic FS v0

- [ ] M4-T1 — Disposition
- [ ] M4-T2 — Opérations
- [ ] M4-T3 — Provenance
- [ ] M4-T4 — Transactions multi-fichiers
- [ ] M4-T5 — Mode dégradé

### M5 — sandboxd : Sandbox Manager

- [ ] M5-T1 — Niveau 0 (bwrap + Landlock + seccomp)
- [ ] M5-T2 — Niveau 1 (gVisor)
- [ ] M5-T3 — Niveau 2 (Firecracker)
- [ ] M5-T4 — Pool de snapshots
- [ ] M5-T5 — Cycle de vie et quotas
- [ ] M5-T6 — Sélection automatique
- [ ] M5-T7 — CLI

### M6 — egress et vault

- [ ] M6-T1 — Proxy
- [ ] M6-T2 — Détection d'exfiltration
- [ ] M6-T3 — Vault
- [ ] M6-T4 — Injection dans le proxy
- [ ] M6-T5 — Sous-volumes d'identifiants des clients officiels
- [ ] M6-T6 — Identité réseau d'agent

### M7 — mcp-system : serveurs MCP système

- [ ] M7-T10 — Registre

### M8 — agentd et providers

- [ ] M8-T1 — Cycle de vie de tâche
- [ ] M8-T2 — Budgets et quotas
- [ ] M8-T3 — Hiérarchie
- [ ] M8-T4 — Pilote `claude-code`
- [ ] M8-T5 — Pilote `codex`
- [ ] M8-T6 — Pilote `gemini`
- [ ] M8-T7 — Moteurs locaux
- [ ] M8-T8 — Pilote `prophet-agent`
- [ ] M8-T9 — Sélection de pilote
- [ ] M8-T10 — CLI
- [ ] M8-T11 — Démo M8

### M9 — image bootable

- [ ] M9-T1 — Modules NixOS
- [ ] M9-T2 — Noyau
- [ ] M9-T3 — Immuabilité et A/B
- [ ] M9-T4 — Chiffrement
- [ ] M9-T5 — Installeur
- [ ] M9-T6 — Démo M9

### M10 — browser-bridge et SUP v0

- [ ] M10-T1 — Spécification SUP v0
- [ ] M10-T2 — Registre SUP
- [ ] M10-T3 — Pont navigateur
- [ ] M10-T4 — Adaptateur AT-SPI
- [ ] M10-T5 — Application native de référence
- [ ] M10-T6 — Repli vision

### M11 — memoryd

- [ ] M11-T1 — Stockage
- [ ] M11-T2 — API MCP
- [ ] M11-T3 — Mémoire épisodique
- [ ] M11-T4 — Édition humaine

### M12 — shell-tui

- [ ] M12-T1 — Barre d'intentions
- [ ] M12-T2 — Timeline
- [ ] M12-T3 — Centre d'approbations
- [ ] M12-T4 — Undo
- [ ] M12-T5 — Gel d'urgence

### M13 — bench et adversarial

- [ ] M13-T1 — Suite de tâches
- [ ] M13-T2 — Ligne de base « pixels »
- [ ] M13-T3 — Suite adversariale
- [ ] M13-T4 — Rapport de phase 0

## Blocages

_Aucun pour le moment._

## Backlog (hors tâche courante, à ne pas faire maintenant)

_Vide._

## Incidents (demandes de violation des invariants, refusées)

_Aucun._
