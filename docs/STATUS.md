# Prophet OS — Avancement

Ce fichier est la source de vérité de l'avancement. L'agent constructeur prend la première tâche non cochée dont les dépendances sont cochées, et coche avec la date et le hash du commit.

Format : `- [ ] ID — titre` puis, une fois fini, `- [x] ID — titre (AAAA-MM-JJ, abc1234)`.

## Jalons

### M0 — Fondations du dépôt

- [x] M0-T1 — Flake Nix et dev shell (2026-09-11, b4c76f2) — flake Nix et dev shell (non exerçable ici : Nix absent)
- [x] M0-T2 — Workspace Cargo (2026-09-11, 3464efd) — workspace Cargo, édition 2024, lints du workspace
- [x] M0-T3 — justfile (2026-09-11, 3464efd) — justfile avec repli sans gitleaks
- [x] M0-T4 — CI GitHub Actions (2026-09-11, 3464efd) — CI : format, clippy, tests, secrets, job privilégié
- [x] M0-T5 — Documentation de base (2026-09-11, 3464efd) — ADR 0000 à 0004, STATUS, specs
- [x] M0-T6 — Hooks et hygiène (2026-09-11, b4c76f2) — recherche de secrets dans `just check`

### M1 — Spécifications gelées v0

- [x] M1-T1 — Manifeste d'agent (2026-09-11, 3464efd) — manifeste : types, parseur TOML, 11 tests de validation
- [x] M1-T2 — Jeton de capacité (2026-09-11, 3464efd) — jeton : signature, délégation, réflexivité et transitivité
- [x] M1-T3 — Événement du Ledger (2026-09-11, 3464efd) — événement : chaînage, altération/suppression/insertion détectées
- [x] M1-T4 — Contrat Agent Driver (2026-09-11, 3464efd) — types du contrat de pilote
- [x] M1-T5 — Convention IPC (2026-09-11, 3464efd) — prophet-ipc : 10 000 allers-retours en 0,4 s, SO_PEERCRED
- [x] M1-T6 — Nommage des outils MCP système (2026-09-11, b4c76f2) — liste normative des outils MCP, `requires` obligatoire

### M2 — capd : Capability Broker et Policy Engine

- [x] M2-T1 — Daemon et clé (2026-09-11, 3464efd) — clé ed25519, broker instanciable
- [x] M2-T2 — Émission (2026-09-11, 3464efd) — émission bornée par le plafond du manifeste
- [x] M2-T3 — Délégation (2026-09-11, 3464efd) — délégation ⊆, profondeur bornée, durée bornée
- [x] M2-T4 — Vérification (2026-09-11, 3464efd) — vérification : 11,6 µs par appel en binaire optimisé
- [x] M2-T5 — Politiques Cedar (2026-09-11, 3464efd) — politiques Cedar, interdits absolus, classes d'actions
- [x] M2-T6 — Approbations (2026-09-11, 3464efd) — approbations : portées once/task/agent, expiration, révocation
- [x] M2-T7 — CLI (2026-09-11, b4c76f2) — binaire `prophet`, sous-commandes cap
- [x] M2-T8 — Application noyau (2026-09-11, 3464efd) — règles Landlock, domaines, profil seccomp

### M3 — ledger : Event Bus et Ledger

- [x] M3-T1 — Stockage (2026-09-11, 3464efd) — stockage JSONL par jour, index, réouverture
- [x] M3-T2 — API (2026-09-11, 3464efd) — requêtes filtrées, bus de diffusion
- [x] M3-T3 — Scellement (2026-09-11, 3464efd) — scellement ed25519, vérification autonome
- [x] M3-T4 — CLI et rejeu (2026-09-11, b4c76f2) — `prophet log tail|replay|verify`, lisible sans daemon

### M4 — sfs : Semantic FS v0

- [x] M4-T1 — Disposition (2026-09-11, b4c76f2) — ADR-0004, détection de dorsale sans privilège
- [x] M4-T2 — Opérations (2026-09-11, b4c76f2) — 50 fichiers modifiés, validés, annulés à l'octet près
- [x] M4-T3 — Provenance (2026-09-11, b4c76f2) — provenance en attributs étendus, dégradation propre
- [x] M4-T4 — Transactions multi-fichiers (2026-09-11, b4c76f2) — transactions hors arbre de travail, balayage des restes
- [x] M4-T5 — Mode dégradé (2026-09-11, b4c76f2) — repli portable, limites annoncées

### M5 — sandboxd : Sandbox Manager

- [ ] M5-T1 — Niveau 0 (bwrap + Landlock + seccomp)
- [ ] M5-T2 — Niveau 1 (gVisor)
- [ ] M5-T3 — Niveau 2 (Firecracker)
- [ ] M5-T4 — Pool de snapshots
- [x] M5-T5 — Cycle de vie et quotas (2026-09-11, b4c76f2) — gel global de 8 sandboxes en 124 µs
- [x] M5-T6 — Sélection automatique (2026-09-11, b4c76f2) — sélection de niveau, microVM imposée pour tout code
- [x] M5-T7 — CLI (2026-09-11, b4c76f2) — sonde de capacités et rapport

### M6 — egress et vault

- [x] M6-T1 — Proxy (2026-09-11, b4c76f2) — politique par hôte, méthode, volume ; IP littérales refusées
- [x] M6-T2 — Détection d'exfiltration (2026-09-11, b4c76f2) — motifs de secrets bloquants, signaux faibles portés à l'humain
- [x] M6-T3 — Vault (2026-09-11, b4c76f2) — coffre chiffré, références jamais valeurs
- [x] M6-T4 — Injection dans le proxy (2026-09-11, b4c76f2) — substitution au dernier moment, hôte vérifié
- [x] M6-T5 — Sous-volumes d'identifiants des clients officiels (2026-09-11, b4c76f2) — répertoires privés par pilote et par utilisateur
- [ ] M6-T6 — Identité réseau d'agent

### M7 — mcp-system : serveurs MCP système

- [x] M7-T10 — Registre (2026-09-11, b4c76f2) — registre, visibilité selon le jeton

### M8 — agentd et providers

- [x] M8-T1 — Cycle de vie de tâche (2026-09-11, b4c76f2) — machine à états, table de transitions testée en entier
- [x] M8-T2 — Budgets et quotas (2026-09-11, b4c76f2) — budgets multidimensionnels, quotas d'abonnement
- [x] M8-T3 — Hiérarchie (2026-09-11, b4c76f2) — hiérarchie bornée, budget prélevé sur le parent
- [x] M8-T4 — Pilote `claude-code` (2026-09-11, b4c76f2) — pilote Claude Code : ligne de commande, environnement, session
- [x] M8-T5 — Pilote `codex` (2026-09-11, b4c76f2) — pilote Codex CLI
- [x] M8-T6 — Pilote `gemini` (2026-09-11, b4c76f2) — pilote Gemini CLI
- [ ] M8-T7 — Moteurs locaux
- [x] M8-T8 — Pilote `prophet-agent` (2026-09-11, b4c76f2) — boucle native : points de reprise, fork, rejeu
- [x] M8-T9 — Sélection de pilote (2026-09-11, b4c76f2) — sélection expliquée, confidentialité locale respectée
- [x] M8-T10 — CLI (2026-09-11, b4c76f2) — `prophet provider ls|login`
- [x] M8-T11 — Démo M8 (2026-09-11, b4c76f2) — démonstration sur trois pilotes

### M9 — image bootable

- [x] M9-T1 — Modules NixOS (2026-09-11, b4c76f2) — modules NixOS, un service durci par daemon
- [x] M9-T2 — Noyau (2026-09-11, b4c76f2) — exigences noyau documentées et conséquences d'une absence
- [x] M9-T3 — Immuabilité et A/B (2026-09-11, b4c76f2) — racine A/B, bascule automatique
- [x] M9-T4 — Chiffrement (2026-09-11, b4c76f2) — LUKS2, TPM avec repli par phrase de passe
- [ ] M9-T5 — Installeur
- [ ] M9-T6 — Démo M9

### M10 — browser-bridge et SUP v0

- [x] M10-T1 — Spécification SUP v0 (2026-09-11, b4c76f2) — arbre, actions typées, niveaux de détail
- [x] M10-T2 — Registre SUP (2026-09-11, b4c76f2) — registre cloisonné, différentiels
- [x] M10-T3 — Pont navigateur (2026-09-11, b4c76f2) — réservation sans capture d'écran, 1 929 octets
- [ ] M10-T4 — Adaptateur AT-SPI
- [ ] M10-T5 — Application native de référence
- [x] M10-T6 — Repli vision (2026-09-11, b4c76f2) — capture d'écran réservée, hors défaut

### M11 — memoryd

- [x] M11-T1 — Stockage (2026-09-11, b4c76f2) — espaces cloisonnés, provenance
- [x] M11-T2 — API MCP (2026-09-11, b4c76f2) — recherche hybride, rappel vérifié
- [ ] M11-T3 — Mémoire épisodique
- [x] M11-T4 — Édition humaine (2026-09-11, b4c76f2) — `prophet memory ls|search|forget`

### M12 — shell-tui

- [ ] M12-T1 — Barre d'intentions
- [x] M12-T2 — Timeline (2026-09-11, b4c76f2) — timeline groupée par étape
- [x] M12-T3 — Centre d'approbations (2026-09-11, b4c76f2) — centre d'approbations lisible en cinq secondes
- [x] M12-T4 — Undo (2026-09-11, b4c76f2) — `prophet task undo`, sans daemon
- [x] M12-T5 — Gel d'urgence (2026-09-11, b4c76f2) — `prophet freeze`

### M13 — bench et adversarial

- [ ] M13-T1 — Suite de tâches
- [ ] M13-T2 — Ligne de base « pixels »
- [x] M13-T3 — Suite adversariale (2026-09-11, b4c76f2) — 20 scénarios, 20 sans conséquence
- [x] M13-T4 — Rapport de phase 0 (2026-09-11, b4c76f2) — docs/reports/phase0.md

## Blocages

Environnement de construction sans KVM, sans Nix, sans Landlock et sans cgroups v2. Les tâches
suivantes sont **écrites et relues mais non exerçables ici** ; elles sont détaillées dans
`docs/reports/phase0.md` section 5.

- M5-T2, M5-T3, M5-T4 : sandbox de niveau 1 et 2, pool de microVM. Exigent gVisor, KVM,
  Firecracker.
- M8-T7 : moteurs de modèles locaux. Exigent un GPU et un modèle du catalogue.
- M9-T5, M9-T6 : installeur et démonstration en machine virtuelle. Exigent Nix et la
  virtualisation.
- M13-T1, M13-T2 : suite de tâches et ligne de base par captures d'écran. Exigent une machine
  complète et un agent de référence.

Les pilotes de clients officiels (M8-T4, M8-T5, M8-T6) sont testés jusqu'à la limite de ce qui est
vérifiable sans compte : construction de la ligne de commande, environnement transmis, détection
de session, messages d'erreur. L'exécution de bout en bout exige une connexion réelle.

## Backlog (hors tâche courante, à ne pas faire maintenant)

_Vide._

## Incidents (demandes de violation des invariants, refusées)

_Aucun._
