# Prophet OS — Avancement

Ce fichier est la source de vérité de l'avancement. L'agent constructeur prend la première tâche non cochée dont les dépendances sont cochées, et coche avec la date et le hash du commit.

Une tâche marquée ⛔ est écrite et relue, mais **non exerçable dans l'environnement de construction** ; le détail est dans `docs/reports/phase0.md` section 5.

## Jalons

### M0 — Fondations du dépôt

- [x] M0-T1 — Flake Nix et dev shell (2026-09-12, 24b8338) — flake Nix et dev shell (non exerçable ici : Nix absent)
- [x] M0-T2 — Workspace Cargo (2026-09-12, 24b8338) — workspace Cargo, édition 2024, lints du workspace
- [x] M0-T3 — justfile (2026-09-12, 24b8338) — justfile, repli sans gitleaks
- [x] M0-T4 — CI GitHub Actions (2026-09-12, 24b8338) — CI : format, clippy, tests, secrets, job privilégié
- [x] M0-T5 — Documentation de base (2026-09-12, 24b8338) — ADR 0000 à 0005, STATUS, specs
- [x] M0-T6 — Hooks et hygiène (2026-09-12, 24b8338) — recherche de secrets dans `just check`

### M1 — Spécifications gelées v0

- [x] M1-T1 — Manifeste d'agent (2026-09-12, 24b8338) — manifeste : types, parseur TOML, 11 tests de validation
- [x] M1-T2 — Jeton de capacité (2026-09-12, 24b8338) — jeton : signature, délégation, réflexivité et transitivité
- [x] M1-T3 — Événement du Ledger (2026-09-12, 24b8338) — événement : chaînage, altération, suppression, insertion détectées
- [x] M1-T4 — Contrat Agent Driver (2026-09-12, 24b8338) — types du contrat de pilote
- [x] M1-T5 — Convention IPC (2026-09-12, 24b8338) — prophet-ipc : 10 000 allers-retours en 386 ms, SO_PEERCRED
- [x] M1-T6 — Nommage des outils MCP système (2026-09-12, 24b8338) — liste normative des outils, `requires` obligatoire

### M2 — capd : Capability Broker et Policy Engine

- [x] M2-T1 — Daemon et clé (2026-09-12, 24b8338) — clé ed25519, broker instanciable
- [x] M2-T2 — Émission (2026-09-12, 24b8338) — émission bornée par le plafond du manifeste
- [x] M2-T3 — Délégation (2026-09-12, 24b8338) — délégation ⊆, profondeur bornée, durée bornée
- [x] M2-T4 — Vérification (2026-09-12, 24b8338) — 11,6 µs par contrôle en binaire optimisé
- [x] M2-T5 — Politiques Cedar (2026-09-12, 24b8338) — politiques Cedar, interdits absolus, classes d'actions
- [x] M2-T6 — Approbations (2026-09-12, 24b8338) — approbations : portées once, task, agent ; expiration ; révocation
- [x] M2-T7 — CLI (2026-09-12, 24b8338) — binaire `prophet`, sous-commandes
- [x] M2-T8 — Application noyau (2026-09-12, 24b8338) — règles Landlock, domaines, profil seccomp

### M3 — ledger : Event Bus et Ledger

- [x] M3-T1 — Stockage (2026-09-12, 24b8338) — stockage JSONL par jour, index, réouverture
- [x] M3-T2 — API (2026-09-12, 24b8338) — requêtes filtrées, bus de diffusion
- [x] M3-T3 — Scellement (2026-09-12, 24b8338) — scellement ed25519, vérification autonome
- [x] M3-T4 — CLI et rejeu (2026-09-12, 24b8338) — `prophet log tail|replay|verify`, lisible sans daemon

### M4 — sfs : Semantic FS v0

- [x] M4-T1 — Disposition (2026-09-12, 24b8338) — ADR-0004, détection de dorsale sans privilège
- [x] M4-T2 — Opérations (2026-09-12, 24b8338) — 50 fichiers modifiés, validés, annulés à l'octet près
- [x] M4-T3 — Provenance (2026-09-12, 24b8338) — provenance en attributs étendus, dégradation propre
- [x] M4-T4 — Transactions multi-fichiers (2026-09-12, 24b8338) — transactions hors arbre de travail, balayage des restes
- [x] M4-T5 — Mode dégradé (2026-09-12, 24b8338) — repli portable, limites annoncées

### M5 — sandboxd : Sandbox Manager

- [x] M5-T1 — Niveau 0 (bwrap + Landlock + seccomp) (2026-09-12, 24b8338) — 8 tests d'évasion réels, démarrage en 2,6 ms
- [x] M5-T2 — Niveau 1 (gVisor) (2026-09-12, 3c0b7cd) — vérifié sur matériel réel en intégration continue : exécution effective sous gVisor et absence d'interface réseau, tests `needs_gvisor` verts
- [x] M5-T3 — Niveau 2 (Firecracker) (2026-09-12, faca93c) — vérifié sur matériel réel en intégration continue : microVM démarrée avec noyau et racine d'invité, et refus explicite plutôt que repli quand le niveau est inatteignable
- [ ] M5-T4 — Pool de snapshots — plus bloqué : le niveau 2 démarre sur le coureur d'intégration ; reste à écrire, avec l'objectif de 100 ms depuis instantané à mesurer
- [x] M5-T5 — Cycle de vie et quotas (2026-09-12, 24b8338) — gel global de 8 sandboxes en 124 µs
- [x] M5-T6 — Sélection automatique (2026-09-12, 24b8338) — sélection de niveau, microVM imposée pour tout code
- [x] M5-T7 — CLI (2026-09-12, 24b8338) — sonde de capacités et rapport

### M6 — egress et vault

- [x] M6-T1 — Proxy (2026-09-12, 24b8338) — politique par hôte, méthode, volume ; IP littérales refusées
- [x] M6-T2 — Détection d'exfiltration (2026-09-12, 24b8338) — motifs de secrets bloquants, signaux faibles portés à l'humain
- [x] M6-T3 — Vault (2026-09-12, 24b8338) — coffre chiffré, références jamais valeurs
- [x] M6-T4 — Injection dans le proxy (2026-09-12, 24b8338) — substitution au dernier moment, hôte vérifié
- [x] M6-T5 — Sous-volumes d'identifiants des clients officiels (2026-09-12, 24b8338) — répertoires privés par pilote et par utilisateur
- [x] M6-T6 — Identité réseau d'agent (2026-09-12, 8091f4d) — en-tête signé, utilisateur sous empreinte salée par machine

### M7 — mcp-system : serveurs MCP système

- [x] M7-T1 — `fs` (2026-09-12, 8091f4d) — lecture, écriture, liste, stat, recherche ; double contrôle outil puis cible
- [x] M7-T2 — `proc` (2026-09-12, 8091f4d) — exécution et arrêt ; microVM imposée hors liste blanche
- [x] M7-T3 — `http` (2026-09-12, 8091f4d) — sortie par le proxy uniquement
- [x] M7-T4 — `task` (2026-09-12, 8091f4d) — état et diff de la tâche courante
- [x] M7-T5 — `approval` (2026-09-12, 8091f4d) — demande et attente ; résumé obligatoire
- [x] M7-T6 — `ledger` (2026-09-12, 8091f4d) — lecture limitée à la tâche courante
- [x] M7-T7 — `memory` (2026-09-12, 8091f4d) — enregistrement et recherche par espace
- [x] M7-T8 — `secrets` (2026-09-12, 8091f4d) — références seules, valeurs jamais rendues
- [x] M7-T9 — `clock`, `notify` (2026-09-12, 8091f4d) — horloge rejouable, notification hors bande
- [x] M7-T10 — Registre (2026-09-12, 8091f4d) — couverture de la spécification vérifiée par un test

### M8 — agentd et providers

- [x] M8-T1 — Cycle de vie de tâche (2026-09-12, 24b8338) — machine à états, table de transitions testée en entier
- [x] M8-T2 — Budgets et quotas (2026-09-12, 24b8338) — budgets multidimensionnels, quotas d'abonnement
- [x] M8-T3 — Hiérarchie (2026-09-12, 24b8338) — hiérarchie bornée, budget prélevé sur le parent
- [x] M8-T4 — Pilote `claude-code` (2026-09-12, 24b8338) — ligne de commande, environnement, détection de session
- [x] M8-T5 — Pilote `codex` (2026-09-12, 24b8338) — pilote Codex CLI
- [x] M8-T6 — Pilote `gemini` (2026-09-12, 24b8338) — pilote Gemini CLI
- [ ] ⛔ M8-T7 — Moteurs locaux — bloqué : exige un GPU et un modèle du catalogue
- [x] M8-T8 — Pilote `prophet-agent` (2026-09-12, 24b8338) — boucle native : points de reprise, fork, rejeu
- [x] M8-T9 — Sélection de pilote (2026-09-12, 24b8338) — sélection expliquée, confidentialité locale respectée
- [x] M8-T10 — CLI (2026-09-12, 24b8338) — `prophet provider ls|login`
- [x] M8-T11 — Démo M8 (2026-09-12, 24b8338) — démonstration sur trois pilotes

### M9 — image bootable

- [x] M9-T1 — Modules NixOS (2026-09-12, 24b8338) — modules NixOS, un service durci par daemon
- [x] M9-T2 — Noyau (2026-09-12, 24b8338) — exigences noyau documentées et conséquences d'une absence
- [x] M9-T3 — Immuabilité et A/B (2026-09-12, 24b8338) — racine A/B, bascule automatique
- [x] M9-T4 — Chiffrement (2026-09-12, 24b8338) — LUKS2, TPM avec repli par phrase de passe
- [x] M9-T5 — Installeur (2026-09-12) — `image/installateur/prophet-installer.sh` : partitionnement GPT, LUKS2 sur l'état et les données, deux racines A/B, montage. Exercé en intégration continue sur un disque en boucle, y compris ses refus — travail d'intégration vert : refus d'une mauvaise confirmation sans toucher au disque, refus d'un disque trop petit, puis préparation réelle dont chaque étiquette correspond à ce qu'`immutable.nix` attend
- [ ] ⛔ M9-T6 — Démo M9 — bloqué : exige une image construite et démarrée en machine virtuelle

### M10 — browser-bridge et SUP v0

- [x] M10-T1 — Spécification SUP v0 (2026-09-12, 24b8338) — arbre, actions typées, niveaux de détail
- [x] M10-T2 — Registre SUP (2026-09-12, 24b8338) — registre cloisonné, différentiels
- [x] M10-T3 — Pont navigateur (2026-09-12, 24b8338) — réservation sans capture d'écran, 1 929 octets
- [x] M10-T4 — Adaptateur AT-SPI (2026-09-12, 8091f4d) — correspondance des rôles, confiance annoncée, actions réellement offertes seulement
- [x] M10-T5 — Application native de référence (2026-09-12, 8091f4d) — éditeur publiant SUP nativement, `send` irréversible et externe
- [x] M10-T6 — Repli vision (2026-09-12, 24b8338) — capture d'écran réservée, hors défaut

### M11 — memoryd

- [x] M11-T1 — Stockage (2026-09-12, 24b8338) — espaces cloisonnés, provenance
- [x] M11-T2 — API MCP (2026-09-12, 24b8338) — recherche hybride, rappel vérifié
- [x] M11-T3 — Mémoire épisodique (2026-09-12, 8091f4d) — résumé dérivé du journal, refus retenus, confiance selon l'issue
- [x] M11-T4 — Édition humaine (2026-09-12, 24b8338) — `prophet memory ls|search|forget`

### M12 — shell-tui

- [x] M12-T1 — Barre d'intentions (2026-09-12, 8091f4d) — proposition étroite, élargissements posés en questions
- [x] M12-T2 — Timeline (2026-09-12, 24b8338) — timeline groupée par étape
- [x] M12-T3 — Centre d'approbations (2026-09-12, 24b8338) — centre d'approbations lisible en cinq secondes
- [x] M12-T4 — Undo (2026-09-12, 24b8338) — `prophet task undo`, sans daemon
- [x] M12-T5 — Gel d'urgence (2026-09-12, 24b8338) — `prophet freeze`

### M13 — bench et adversarial

- [x] M13-T1 — Suite de tâches (2026-09-12, 8091f4d) — 8 tâches, 5 familles, vérificateur par tâche
- [ ] ⛔ M13-T2 — Ligne de base « pixels » — bloqué : exige un agent de référence exécutable
- [x] M13-T3 — Suite adversariale (2026-09-12, 24b8338) — 20 scénarios, 20 sans conséquence
- [x] M13-T4 — Rapport de phase 0 (2026-09-12, 24b8338) — docs/reports/phase0.md

## Blocages

Le conteneur de construction n'a ni KVM, ni Nix, ni Landlock, ni cgroups v2. Ce n'est plus le
dernier mot : le job `isolation` de l'intégration continue installe gVisor, Firecracker et les
images d'invité sur un coureur Ubuntu muni de KVM, et y exerce les quatre tests matériels. C'est
ainsi que M5-T2 et M5-T3 ont été vérifiés.

Ce qu'aucune des deux machines n'offre encore, et qui bloque les tâches marquées ⛔ ci-dessus :
Nix (M9-T5, M9-T6), un GPU avec un modèle du catalogue (M8-T7), un agent de référence exécutable
(M13-T2). `docs/reports/phase0.md` section 5 dit, pour chacune, ce qu'il faut pour la vérifier.

Un rappel qui a coûté cher le 12 septembre : une machine hôte peut refuser ce qu'elle paraît
offrir. Ubuntu 24.04 interdit par AppArmor d'exécuter dans un espace de noms non privilégié, et
`/dev/kvm` peut être présent sans être ouvrable. Voir ADR-0006 ; `prophet status` le signale
désormais, et `tools/install-isolation.sh` le traite sans rien modifier sans autorisation.

Les pilotes de clients officiels sont testés jusqu'à la limite de ce qui est vérifiable sans compte : construction de la ligne de commande, environnement transmis, détection de session, messages d'erreur. L'exécution de bout en bout exige une connexion réelle.

## Backlog (hors tâche courante, à ne pas faire maintenant)

_Vide._

## Incidents (demandes de violation des invariants, refusées)

_Aucun._
