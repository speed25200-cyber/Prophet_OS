# Prophet OS — Avancement

> Réévaluation du 12 septembre 2026 : les coches historiques ci-dessous décrivent parfois une
> bibliothèque ou une simulation, pas le parcours installé complet. Les exigences de livraison
> sont désormais suivies dans [FRONTIER.md](FRONTIER.md). Le moteur local possède un client HTTP
> concret avec un essai Qwen3/CPU et fichier vérifié (`c3b0c08`). La conversation native en flux
> est implémentée ; son raccordement à l'exécution agentique via MCP/agentd reste à réaliser.
> Voir [le rapport d'inférence](reports/local-inference-2026-09-12.md) et les
> [captures et vérifications de l'espace natif](reports/espace-natif-2026-09-12.md).

Jalons d'intégration réellement exercés le 12 septembre 2026 :

- `c3b0c08` — moteur local réel, CLI et gel d'un processus possédé par sandboxd.
- `766ce9e` — espace natif Wayland, conversation locale en flux, neuf tests de rendu réussis,
  première image en fenêtre WSLg et capture d'une vraie réponse Qwen3.
- `2abcf3f` — préparation MCP : le registre refuse désormais les exigences inconnues, les cibles absentes,
  les niveaux d'isolation insuffisants et les contextes de tâche incohérents. La session exige
  son initialisation et borne ses entrées. Quatre tests de régression ont d'abord échoué sur
  l'ancien comportement. Le [guide du composant](../crates/mcp-system/README.md) précise les
  accès fichiers et les raccordements aux daemons qui restent à corriger avant activation.
  Validation locale après correction : `nix develop --command just check`, 569 tests réussis,
  aucun échec, 16 ignorés ; format, clippy et contrôles du dépôt réussis.
- `0f3f307` — clients officiels : Codex et Claude Code exigés par la configuration d'image. Versions et
  connexion sondées par les vrais clients, sans inspection des fichiers d'identifiants ; profils
  privés dans le répertoire utilisateur, reprise Codex et options de flux Claude corrigées.
  Les capacités ne déclarent plus des fonctionnalités agentiques non raccordées. Validation
  locale : `nix develop --command just check`, 575 tests réussis, aucun échec, 17 ignorés.
  Test explicite supplémentaire sur les vrais binaires : Codex 0.153.4 et Claude Code 2.1.266,
  versions reconnues et connexion requise dans des profils vierges ; commandes CLI doctor,
  login et ls JSON exercées avec succès. La CI de cette révision a réussi ses trois travaux,
  puis la construction de l'ISO, son démarrage, la construction du système installé, son
  démarrage et les tests des services. Le bureau humain, l'application
  ChatGPT et les sessions authentifiées restent à intégrer. Voir le
  [guide des pilotes](components/providers.md) et l'[ADR 0009](adr/0009-clients-officiels-et-bureau.md).
- `f8e263e` et correctifs de compatibilité — paquet ChatGPT Linux expérimental construit depuis
  le `.deb` officiel 26.908.40834 ; empreinte du binaire principal inchangée. La VM NixOS sous
  KVM confirme une fenêtre XWayland visible et l'écran de connexion par reconnaissance de texte,
  avec le binaire officiel sous UID 1000. La copie des plugins est corrigée, leur initialisation
  se termine. **Le test graphique strict reste en échec** sur une erreur Fontconfig dans un
  renderer secondaire ; le paquet reste hors de l'image installée. Aucun compte n'est connecté.
  Un test de régression protège aussi les valeurs d'options de Claude Code. Validation locale
  des composants : `just check` dans Nix, 576 tests réussis, aucun échec, 17 ignorés. La CI de
  `f8e263e` réussit les composants, l'isolation, la surface, les services, l'installeur, l'ISO et
  les deux démarrages ; son travail ChatGPT a échoué sur la classe de fenêtre, corrigée depuis.
  Les résultats de cette révision ne valident pas les correctifs suivants. Voir le
  [rapport ChatGPT Linux](reports/chatgpt-linux-2026-09-12.md).

La CI de `f132518` a depuis réussi les composants, l'isolation, le rendu de la surface, les
services, l'installeur, les constructions et le démarrage de l'ISO. Le travail ChatGPT reste en
échec. Le test du système installé a aussi échoué lors de l'ouverture de session du propriétaire
après son délai de 900 secondes ; sa cause reste à diagnostiquer. Cette observation précède la
refonte Iris et ne constitue pas une validation du système installé pour cette révision.

Jalon d'interface du 13 septembre 2026 : **Iris** remplace la présentation de l'espace natif
par une composition centrée, une sculpture irisée native, un dock flottant, Inter embarquée
et des contrôles adaptés à la taille de la fenêtre. Les captures finales incluent une vraie
conversation Qwen locale et des formats de 640 × 480 à 1920 × 1080. Validation locale :
`just check`, 576 tests réussis, aucun échec, 18 ignorés ; dix tests graphiques explicites
réussis et première image soumise à Wayland sous WSLg. La nouvelle révision reste à valider
en CI. Ce jalon ne résout pas les échecs installés et ChatGPT décrits ci-dessus. Voir le
[rapport Iris et ses captures](reports/interface-iris-2026-09-13.md).

Le même jour, l'utilisateur rejette Iris et demande un espace réellement conçu pour les agents
et la supervision humaine. La nouvelle direction supprime la sculpture, adopte un thème clair
et place les missions, leur contexte et les décisions au premier plan. Les filtres, la sélection,
le retour aux missions sur petit écran et l'examen explicite sont implémentés. Validation locale :
`just check`, 577 tests réussis, aucun échec, 20 ignorés ; douze tests graphiques explicites
réussis (six parcours natifs et six tests du rendu historique). Les commandes d'agents, livrables,
diffs, permissions détaillées et acquittements restent à intégrer. La validation de cette nouvelle
révision en CI reste à réaliser. Voir le [rapport de supervision](reports/supervision-2026-09-13.md)
et l'[ADR 0011](adr/0011-supervision-humaine.md). La qualité visuelle reste à apprécier par
l'utilisateur ; ce jalon ne constitue pas une certification SOTA ni une équivalence avec Apple.

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
- [ ] M8-T7 — Moteurs locaux — client HTTP, flux annulable, interface de conversation et essai Qwen3/CPU réalisés ; restent le service de modèles, le raccordement à agentd, les budgets de tokens/VRAM et la matrice GPU/modèles
- [x] M8-T8 — Pilote `prophet-agent` (2026-09-12, 24b8338) — boucle native : points de reprise, fork, rejeu
- [x] M8-T9 — Sélection de pilote (2026-09-12, 24b8338) — sélection expliquée, confidentialité locale respectée
- [x] M8-T10 — CLI (2026-09-12, 24b8338) — `prophet provider ls|login`
- [x] M8-T11 — Démo M8 (2026-09-12, 24b8338) — démonstration sur trois pilotes

### M9 — image bootable

L'ISO se construit depuis le 12 septembre 2026 : le travail « Support d'amorçage » de l'intégration
continue exerce l'installeur sur un disque en boucle, puis produit `prophet-os-installeur-*.iso`.
Six options du Nix n'avaient jamais été évaluées avant ce jour et l'empêchaient — elles sont
corrigées, et la liste est dans `docs/reports/phase0.md`.

- [x] M9-T1 — Modules NixOS (2026-09-12) — modules NixOS, un service durci par daemon. **Cochée à tort jusqu'au 12 septembre au soir** : les sept services déclaraient un `ExecStart` vers un programme que l'atelier ne produisait pas. Une machine installée aurait démarré avec sept unités en échec. Les sept programmes existent désormais, et `tools/verifier-les-services.sh` refuse l'écart — il tourne dans `just check`
- [x] M9-T2 — Noyau (2026-09-12, 24b8338) — exigences noyau documentées et conséquences d'une absence
- [x] M9-T3 — Immuabilité et A/B (2026-09-12, 24b8338) — racine A/B, bascule automatique
- [x] M9-T4 — Chiffrement (2026-09-12, 24b8338) — LUKS2, TPM avec repli par phrase de passe
- [x] M9-T5 — Installeur (2026-09-12) — `image/installateur/prophet-installer.sh` : partitionnement GPT, LUKS2 sur l'état et les données, deux racines A/B, montage. Exercé en intégration continue sur un disque en boucle, y compris ses refus — travail d'intégration vert : refus d'une mauvaise confirmation sans toucher au disque, refus d'un disque trop petit, puis préparation réelle dont chaque étiquette correspond à ce qu'`immutable.nix` attend
- [x] M9-T6 — Démo M9 (2026-09-12) — **l'image démarre, et cela a été vu** : micrologiciel UEFI, menu d'amorçage, noyau, espace utilisateur, `serial-getty`, message d'accueil, connexion automatique. Journal complet dans l'artefact « demarrage-vm ». L'intégration continue le refait à chaque construction. Reste non vérifié : le matériel réel — carte graphique, carte réseau, micrologiciel d'un PC donné
- [x] M9-T7 — **Le système installé démarre** (2026-09-12) — et c'est une autre question que M9-T6.
  Le support d'amorçage est une configuration à part : racine en lecture-écriture, session ouverte
  automatiquement. Ce qu'il installe n'avait jamais démarré, seulement été construit.
  `image/tests/installe.nix` le démarre **par son chargeur d'amorçage**, en UEFI, depuis un vrai
  disque : `bootctl status` confirme que `systemd-boot` l'a lancé, avec `lockdown=integrity` et
  `module.sig_enforce=1` sur la ligne de commande — les deux candidats les plus plausibles à un
  refus de démarrer

### Une correction à mon propre message de commit (12 septembre 2026)

Le commit `63e0d99` déplace `allowUnfreePredicate` du module vers `flake.nix`, et donne comme
raison que NixOS refuserait qu'un module touche à `nixpkgs.config` quand `pkgs` vient du cadre de
test — « Your system configures nixpkgs with an externally created instance ». **Ce n'est pas ce
que le journal dit.** L'erreur réelle était :

```
The option `nixpkgs.config.allowUnfreePredicate` has conflicting definition values
Use `lib.mkForce value` ou `lib.mkDefault value` …
```

Une **définition en conflit**, pas une instance externe : le cadre de test pose déjà cette option
pour ses nœuds, à partir du `pkgs` qu'on lui donne, et le module en posait une seconde.

Le déplacement reste la bonne correction — il supprime l'une des deux définitions, et l'unique
qui subsiste vient du `pkgs` construit dans `flake.nix`, lequel sert aussi bien aux tests qu'à
l'image. Mais j'avais écrit le mécanisme avant de l'avoir lu, et c'est exactement ce que ce dépôt
reproche partout ailleurs. Le commit reste tel quel — réécrire l'histoire pour se donner raison
après coup serait pire — et la correction vit ici.

### Une leçon de la journée, écrite pour la prochaine

Une exécution de « Support d'amorçage » occupe six coureurs pendant une demi-heure, et le groupe de
concurrence ajouté ce jour-là **annule l'exécution en cours à chaque poussée**. C'est ce qu'on veut
quand on enchaîne des corrections ; c'est exactement ce qu'on ne veut pas quand on attend un
verdict. Deux exécutions ont ainsi été annulées par la poussée suivante, dont l'une portait la
correction dont on attendait la réponse.

La règle qui en découle : **quand on attend la réponse d'un test de trente minutes, on ne pousse
plus rien qui touche `flake.nix`, `image/`, `crates/` ou `iso.yml`.** `docs/` et `tools/` sont hors
du filtre de chemins et restent libres.

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


### Daemons — les programmes que les services déclaraient

Écrits le 12 septembre 2026, après qu'un garde-fou a montré que les sept `ExecStart` de
`image/modules/prophet.nix` ne nommaient aucun programme existant. Chacun a un test qui lance le
**binaire** et lui parle par son socket, parce que c'est le binaire que systemd lancera.

- [x] `prophet-capd` — `cap.check`, `cap.mint`, `cap.revoke`, `approval.*`. Un jeton signé par une
  autre clé est refusé, et le motif nomme la signature
- [x] `prophet-ledger` — seul écrivain du journal, scellement tous les 256 événements. Un appelant
  ne peut pas choisir sa place dans la chaîne
- [x] `prophet-vault` — `secrets.use` n'est servi qu'au compte du proxy de sortie ; sans ce compte,
  personne n'obtient de valeur
- [x] `prophet-memoryd` — espaces cloisonnés ; une recherche sans espace échoue au lieu de chercher
  partout
- [x] `prophet-egress` — le relais, écrit ici : jeton, `cap.check`, détection d'exfiltration, puis
  seulement la sortie. Un `capd` injoignable ferme la sortie
- [x] `prophet-sandboxd` — un niveau que la machine ne tient pas est refusé, jamais abaissé
- [x] `prophet-agentd` — demande ses jetons à `capd` et pousse son journal vers `ledger` ; il
  n'émet ni n'écrit lui-même. Ses tâches survivent à un redémarrage, écriture atomique en 0600
- [x] `surface::reel` — la surface lit `agentd`, `capd` et `sandboxd` au lieu d'afficher une scène
  d'exemple. Un daemon muet vide sa part du champ plutôt que de laisser la précédente : montrer
  d'anciennes tâches comme si elles couraient encore serait faux *et* crédible
- [x] `prophet-daemon` — la part commune : socket, état, clés, et surtout **à qui un daemon accepte
  de parler**, écrite une fois pour que les sept copies ne divergent pas

### Ce que le premier démarrage sous systemd a montré (12 septembre 2026)

Le test `image/tests/services.nix` a fait tourner les sept services sous systemd, avec leurs
utilisateurs et leur durcissement. Trois défauts sont apparus, qu'aucun test de daemon pris
isolément ne pouvait voir.

- [x] `/run/prophet` en `0750` : le groupe ne pouvait pas y **écrire**. Seul le premier service
  démarré créait son socket ; les six autres bouclaient sur un « Permission denied ». Corrigé en
  `0770`, avec `RuntimeDirectoryPreserve` — sans quoi l'arrêt d'un seul service emportait les six
  autres sockets
- [x] La règle du groupe ne regardait que le `gid` attesté par `SO_PEERCRED`, c'est-à-dire le
  groupe **principal**. Un compte déclaré dans `prophet-system` par `extraGroups` y appartient
  réellement et se faisait pourtant refuser — **la surface était dans ce cas**, et aurait affiché
  un champ vide sur une machine saine. L'appartenance est maintenant aussi cherchée dans
  `/etc/group`. `root` est accepté : le refuser ne protégeait rien, puisqu'il lit les clés de
  signature dans `/var/lib/prophet`, et rendait `prophet status` inutilisable pour le propriétaire
- [x] Ma première correction lisait `/etc/group` **au démarrage**. Le test en machine virtuelle l'a
  refusée aussitôt : il crée son compte après le démarrage des daemons, comme le fait un
  `nixos-rebuild switch`, qui ne les redémarre pas. Un refus qui dépend de l'heure à laquelle un
  service a démarré ne se diagnostique jamais. Le fichier est maintenant lu au moment de la
  question, et seulement pour un pair qui serait sinon refusé : les sept daemons se reconnaissent
  par leur groupe principal, `root` par son `uid`, et rien n'est ouvert sur le chemin fréquent
- [x] `prophet status` ne rendait plus la main — quinze minutes, sans rien afficher. `egress` est
  un proxy HTTP : un `ping` JSON-RPC est pour lui une requête tronquée, et il attendait la fin
  d'en-têtes qui ne viendraient jamais. Il n'était pas en faute ; la sonde l'était. Elle lui parle
  maintenant sa langue — une requête sans jeton, refusée par `407` avant toute sortie, ce qui
  prouve davantage qu'un `pong`. Toutes les sondes ont un délai de deux secondes

Les trois ont un test qui échoue sur le code d'avant : trois dans `crates/prophet-cli`
(`sondes::*`), deux dans `crates/prophet-daemon`, et quatre sous-tests dans
`image/tests/services.nix`.

### Ce que `agentd` promettait sans pouvoir le tenir (12 septembre 2026)

- [x] `agentd` déclarait `ReadWritePaths = [ "/home/prophet" … ]` pendant que `ProtectHome = true`,
  hérité du modèle commun, rendait `/home` inaccessible et vide dans son espace de montage. Le
  sous-test qui interroge **depuis l'intérieur** de cet espace, par `nsenter`, a rendu
  `agentd voit /home/prophet : refusé`. Une tâche qui ouvre son espace de travail aurait échoué
  sur un « Read-only file system » très loin de cette ligne. `ProtectHome` est désormais désactivé
  pour ce seul service ; `ProtectSystem = "strict"` reste, donc tout `/home` demeure en lecture
  seule sauf les deux chemins déclarés — ce que la déclaration prétendait déjà

### Le gardien borné par les règles du prisonnier (12 septembre 2026)

- [x] `sandboxd` recevait `CAP_SETUID`, `CAP_SETGID` et `CAP_SYS_ADMIN`, et de l'autre main le
  filtre d'appels système hérité des six autres daemons : `@system-service` moins `@privileged`.
  Or `@system-service` ne contient pas `@mount`, et `~@privileged` retire `setuid`, `setgid`,
  `setgroups` et `pivot_root` — le travail exact de ce service. `RestrictSUIDSGID` implique par
  ailleurs `NoNewPrivileges`, que le même bloc désactive trois lignes plus haut. Le test des
  services l'a montré en toutes lettres :

  ```
  confinement impossible : écriture de uid_map : Operation not permitted
  ```

  **Aucune tâche ne pouvait donc être isolée sur la machine installée**, et l'invariant « tout
  processus non fiable tourne sous `sandboxd` au niveau requis » était inapplicable. Le filtre du
  service borne maintenant le gestionnaire ; celui que subit une tâche reste posé par `sandboxd`
  dans son enfant, après le confinement, et beaucoup plus étroit.

  Vérifié à la main sur le conteneur de construction, hors systemd : `sandbox.start` au niveau 0
  rend `{"task": "task:essai-local", "pid": …, "level": 0}` et le journal dit « sandbox démarrée ».
  Le code du confinement n'était pas en cause ; seule l'entrave du service l'était.

- [x] `tools/verifier-le-durcissement.sh` — le garde-fou qui dit en une seconde ce que le test en
  machine virtuelle a mis sept minutes à apprendre. Il cherche deux contradictions et rien
  d'autre : un service à qui l'on accorde `CAP_SETUID`, `CAP_SETGID` ou `CAP_SYS_ADMIN` et dont le
  filtre retire `@privileged` ou n'ajoute pas `@mount` ; et `RestrictSUIDSGID` gardé en même temps
  que `NoNewPrivileges = false`, que le premier implique. Vérifié en remettant la configuration
  d'avant la correction : il rend les deux défauts et sort en 1. Il ne remplace pas le test — lui
  seul exerce le durcissement réel — mais il évite d'y aller pour une faute qui se lit dans le
  fichier. Ajouté à `just check` et au travail `check` de l'intégration continue

- [ ] `image/tests/services.nix` demande aussi, désormais, si `agentd` peut écrire là où sa
  configuration le prétend. `ReadWritePaths = [ "/home/prophet" … ]` et `ProtectHome = true` se
  contredisent en apparence, et c'est systemd qui tranche sans que le fichier dise dans quel sens.
  Le contrôle regarde depuis l'intérieur de l'espace de montage du service, par `nsenter` : le
  jour où la promesse serait fausse, une tâche échouerait sur « Read-only file system » loin de
  cette ligne, et personne ne remonterait jusqu'à elle

- [x] L'image n'embarquait **aucun client officiel**. `prophet provider login claude-code`
  répondait « lancez `claude login` » sur une machine où `claude` n'existe pas — découverte à
  faire après avoir formaté son disque, c'est-à-dire au seul moment où il est trop tard. Claude
  Code et Gemini CLI sont maintenant embarqués tels quels, par `lib.optional (pkgs ? …)`
  pour qu'un renommage en amont retire le client sans casser l'image. Le test vérifie non pas leur
  présence — ils viennent de nixpkgs et peuvent en disparaître — mais que `provider ls` dise la
  vérité sur ceux qui y sont : annoncer un client absent est pire que de dire qu'il manque.
  Codex CLI est laissé de côté : `pkgs.codex` est un nom générique, `lib.optional (pkgs ? …)`
  protège d'un attribut absent mais pas d'un attribut qui n'est pas le bon, et livrer un binaire
  étranger sous un nom auquel l'OS fait confiance serait pire que de ne rien livrer

- [x] **La capacité qui ne se devine pas.** Après la correction du filtre, le refus persistait,
  identique. Les diagnostics ajoutés au test ont écarté les capacités (`CapEff = 0x2000c0`, les
  trois attendues), `NoNewPrivileges` (`0`) et le filtre (`setuid`, `mount`, `pivot_root` présents).
  Le groupe principal a été écarté en reproduisant les deux cas à la main. Le refus a finalement
  été **reproduit hors systemd** avec `capsh --drop`, en recréant le jeu de capacités exact du
  service, puis localisé par **bissection sur les trente-huit capacités** : `CAP_SETFCAP`.

  Depuis Linux 5.12, projeter l'**uid 0** dans un espace de noms exige `CAP_SETFCAP` dans l'espace
  parent — pas `CAP_SETUID`. Vérifié dans les deux sens sur cette machine : sans elle le refus,
  avec elle `{"task": "task:confirme", "pid": 792, "level": 0}`. Noté en ADR-0005, ajouté au
  garde-fou, et le refus lui-même nomme désormais laquelle de ses quatre causes s'applique

- [ ] **Le maillon jamais exercé : `nixos-install` lui-même.** Le travail « installeur » s'arrête
  au montage ; le travail « système installé » démarre une configuration que le cadre de test
  fabrique. Entre les deux, personne n'avait jamais posé ce système sur la disposition que
  l'installeur crée. Le travail `systeme` reprend maintenant là où l'installeur s'arrête, avec la
  fermeture qu'il vient de construire (`--system`, donc sans la reconstruire), et vérifie ce qui
  atterrit réellement sur le disque : le magasin, `run/current-system`, le compte `prophet` dans
  `/etc/passwd`, et le haché du mot de passe en `0600`. `--no-bootloader` parce qu'un coureur
  GitHub ne démarre pas en UEFI — que le chargeur fonctionne est vérifié ailleurs

### Le compte sans lequel personne ne se connecte (12 septembre 2026)

- [x] La machine installée ne créait **aucun** compte humain. `nixos-install --no-root-password`
  laisse `root` verrouillé, `systemd-boot` est configuré sans éditeur, et `cfg.user` — « prophet »
  — était référencé dans `ReadWritePaths` sans avoir jamais été déclaré. On installait donc un
  système sur lequel il était impossible d'ouvrir une session, et impossible de se rattraper.
  Rien ne pouvait le voir : seul le support d'amorçage avait jamais démarré, et lui ouvre une
  session automatiquement. Le compte est maintenant déclaré, dans `wheel` et `prophet-system` ;
  l'installeur demande son mot de passe **avant** d'écrire quoi que ce soit sur le disque, refuse
  en dessous de huit caractères, et ne pose que le haché, en `0600`

### Ce qui rendait l'ISO non reproductible (12 septembre 2026)

- [x] `flake.nix` suivait la **branche** `nixos-unstable`, et le dépôt n'a pas de `flake.lock`.
  Deux gravures de la même ISO à quinze jours d'écart installaient donc deux systèmes différents,
  et un travail d'intégration continue vert la veille pouvait être rouge le lendemain sans qu'une
  ligne du dépôt ait changé. Pour un système qu'on installe après avoir formaté son disque, « ce
  qu'on installe est ce qu'on a gravé » est la propriété qui permet de revenir en arrière.
  Épinglé à `8ce4ef6`, la révision du canal du 12 septembre — celle contre laquelle tout est vert
- [x] Le travail `services` échouait à l'étape qui rend `/dev/kvm` ouvrable, **après** l'avoir
  rendu ouvrable : il demandait la cible `microvm`, qui installe aussi Firecracker et interroge
  l'API de GitHub, laquelle répond `403` sur un coureur partagé quand la limite est atteinte. Une
  cible `kvm` existe maintenant pour ceux qui veulent seulement faire tourner une machine
  virtuelle, et la recherche de version de Firecracker se rabat sur la redirection de
  `releases/latest` quand l'API se tait

### Le système installé, démarré pour la première fois (12 septembre 2026)

M9-T6 a montré le **support d'amorçage** démarrer. Ce que ce support installe est une autre
configuration, et elle n'avait jamais été démarrée — seulement construite. Entre les deux,
`immutable.nix` ajoute précisément ce qui peut empêcher une machine de démarrer : racine en
lecture seule alors que l'activation de NixOS écrit `/etc/passwd` et `/etc/shadow` à chaque
démarrage, `systemd-boot` sans éditeur donc sans secours, `lockdown=integrity` et
`module.sig_enforce=1`.

**Première réponse, obtenue le 12 septembre.** Le sous-test « la machine a démarré par son
chargeur d'amorçage » est **passé** : `systemd-boot` a lancé la configuration installée, en UEFI,
avec `lockdown=integrity` et `module.sig_enforce=1` sur la ligne de commande. Ces deux paramètres
étaient les candidats les plus plausibles à un refus de démarrer — un noyau qui exige des modules
signés et n'en trouve aucun ne monte pas sa racine. Ce n'est pas ce qui se produit.

- [ ] `image/tests/installe.nix` — démarre la configuration installée **par son chargeur
  d'amorçage**, en UEFI, depuis un vrai disque, et vérifie dans l'ordre : le chargeur a bien
  lancé le système, les comptes ont été écrits, aucune unité n'a échoué, les sept services
  tournent, `prophet-surface` a au moins été lancée, le propriétaire ouvre une session sur `tty1`
  avec son mot de passe, et les paramètres du noyau sont ceux demandés. Écrit avant de savoir ce
  qu'il dira : c'est le seul moyen d'apprendre quelque chose

**Ce que ce test ne peut pas vérifier, et qu'il ne faut pas croire vérifié.** Le cadre de test
NixOS fournit son propre disque et redéfinit `fileSystems` à une priorité qui l'emporte sur celle
de `immutable.nix`. La **racine en lecture seule n'est donc pas exercée**, et c'est la question la
plus dangereuse pour quelqu'un qui vient d'effacer son disque :

> L'activation de NixOS écrit `/etc/passwd`, `/etc/shadow`, `/etc/group` et tout l'arbre de liens
> de `/etc` à **chaque** démarrage, et crée des répertoires sous `/var`. `immutable.nix` monte
> `/home` et `/var/lib/prophet` depuis des volumes séparés, mais `/etc`, `/var/log`, `/var/lib` et
> `/tmp` restent sur la racine. Si celle-ci est vraiment en lecture seule, l'activation échoue et
> la machine part en mode de secours — sauf que `systemd-boot` est configuré sans éditeur, donc il
> n'y a pas de mode de secours utilisable.

**RÉPONSE, le 12 septembre 2026 : non.** L'expérience a rendu

```
RuntimeError: Shell disconnected
```

La machine ne garde même pas un interpréteur vivant. `immutable.nix` **ne monte plus la racine en
lecture seule** : livrer cela aurait donné, sur un PC dont on vient d'effacer Windows, une machine
qui ne démarre pas — et `systemd-boot` étant configuré sans éditeur, sans aucun rattrapage.

Ce que cela coûte, dit franchement : **la promesse d'immuabilité n'est pas tenue aujourd'hui.** Les
mises à jour A/B, le chiffrement et le verrouillage du noyau le sont ; la racine en lecture seule
ne l'est pas, et `docs/installation.md` le dit. La tenir demande une conception —
`system.etc.overlay`, un `/var` porté par un volume inscriptible, `boot.tmp.useTmpfs` — pas un
réglage. Le test reste, et redeviendra le garde-fou qui empêche de défaire ce travail le jour où
il sera fait.

- [x] `image/tests/racine-en-lecture-seule.nix` — pose la question à la machine au lieu de la
  raisonner. Il force l'option `ro` là où le cadre de test pose la racine, démarre, et raconte ce
  qu'il trouve : les unités en échec, l'état de `systemd-tmpfiles-setup`, les erreurs du journal.
  Son travail d'intégration continue est en `continue-on-error` — c'est une **question**, pas une
  garantie, et un échec n'y signale pas une régression mais donne la réponse

La correction, si la réponse est « non », n'est pas un réglage mais une décision de conception. La
piste que NixOS documente pour ce cas précis : `system.etc.overlay` — qui exige l'initrd systemd,
déjà activé — un `/var` porté par un volume inscriptible plutôt que par la racine, et
`boot.tmp.useTmpfs`. Elle se prendra en la prenant. **À traiter avant de déclarer l'ISO
installable.**

### Le serveur, état réel au 12 septembre 2026 à 15 h 52

Le travail qui agit a été déclenché **par l'API**, sur la branche de travail. Ce qui a
effectivement changé sur la machine, et comment le défaire :

| Fait | Comment revenir en arrière |
|---|---|
| Les unités `hermes*` sont **arrêtées et désactivées** | `systemctl enable --now hermes…` — le journal du workflow nomme les unités. Les fichiers de `/root/hermes` n'ont pas été touchés |
| Prophet OS est déposé dans `/root/prophet_os` | `rm -rf /root/prophet_os` |
| gVisor est installé — `runsc release-20260907.0` | le paquet reste ; `runsc` s'enlève à la main |
| La restriction AppArmor des espaces de noms est **levée** (15 h 54, après la correction d'ordre) | `sysctl -w kernel.apparmor_restrict_unprivileged_userns=1`, et retirer le fichier posé sous `/etc/sysctl.d/` |

Les niveaux 0 et 1 sont donc désormais atteignables sur cette machine : les espaces de noms sont
utilisables, et gVisor est en place. Le niveau 2 restera hors d'atteinte — pas de `/dev/kvm` sur ce
VPS, c'est une machine virtuelle sans virtualisation imbriquée.

« Atteignables » est ce que la configuration permet. Ce que la machine **tient réellement** est
une autre question. Elle a été posée à 15 h 56, en compilant `sandboxd` sur le serveur et en
lançant pour de vrai :

```
test niveau_un_execute_reellement_sous_gvisor ... ok
test niveau_un_n_a_pas_de_reseau ... ok
```

**Le niveau 1 fonctionne sur cette machine** : un programme s'exécute réellement sous gVisor, et la
sandbox n'a aucune interface réseau. Ce ne sont pas des sondes de présence — l'une lance un
programme et regarde ce qu'il rend, l'autre essaie de sortir et constate qu'elle ne peut pas.

Non vérifiable ici, et dit comme tel plutôt que compté comme réussi ou échoué :

| | |
|---|---|
| `niveau_deux_demarre_une_microvm` | `needs_kvm` — il manque l'accès à KVM, Firecracker et les images d'invité. Définitif sur ce VPS |
| `le_niveau_deux_ne_retombe_jamais_sur_le_niveau_zero` | `needs_kvm`, même raison |
| les six tests de la surface | `needs_gpu` — aucun périphérique Vulkan utilisable ; un nœud `/dev/dri` ne suffit pas |

Le rapport complet est dans l'artefact `rapport-serveur` du run `34703605599`.

La cause de l'étape sautée était dans le workflow, pas sur la machine :
`install-isolation.sh gvisor` répond à deux questions — installer gVisor, et signaler la
restriction — et sortait en 1 sur la seconde après avoir réussi la première. L'étape qui devait
lever la restriction a donc été sautée, alors qu'elle était demandée. Corrigé : la restriction est
levée **avant** la préparation, et la préparation juge sur `command -v runsc` plutôt que sur le
code de sortie d'un outil qui répond à deux questions.

### Ce que le run 48 a appris, et ce qui a été corrigé (12 septembre 2026, 16 h 30)

Le run `34703888974` a rendu son verdict : quatre travaux verts, dont **« Le système installé
démarre »** et **« Voir l'image démarrer »**. Deux rouges, tous deux réels, tous deux corrigés ici.

**`prophet log` ne trouvait pas le journal.** Le test des services est allé beaucoup plus loin
qu'avant — `sandbox démarrée tache=task:essai-sandbox niveau=0`, la correction `CAP_SETFCAP` tient
— puis a buté sur ceci :

```
$ prophet log tail -n 20
aucun journal sur cette machine
```

La commande lisait `~/.prophet/ledger`, et rien d'autre. Or `prophet-ledger` écrit dans
`/var/lib/prophet/ledger` : deux journaux existent, et la commande d'audit ne connaissait que
celui du développement. Elle répondait donc « il n'y a rien » devant un journal plein, ce qui est
la pire des trois réponses possibles — pas « je ne sais pas », mais une négation.

Corrigé : `prophet log` interroge d'abord **le service**, qui seul connaît l'état courant et qui
seul sert les membres de `prophet-system` (l'état du daemon est en 0700, pour que personne ne
réécrive l'histoire par le fichier) ; à défaut, il lit les fichiers, en essayant
`/var/lib/prophet/ledger` avant `~/.prophet/ledger` ; et quand il ne trouve rien, il dit **où il a
regardé et pourquoi chaque tentative a échoué**. Quatre tests dans `crates/prophet-cli`, dont un
qui échoue sur l'ancien code.

**Le travail « Construire le système installé » se trompait de question.** Il cherchait
`/mnt/run/current-system` et `/mnt/etc/passwd` après un `nixos-install --no-bootloader`, et
déclarait l'installation ratée de ne pas les trouver. Il avait tort sur les deux : `/run` est un
tmpfs créé au démarrage, il n'existe sur aucun disque ; et `--no-bootloader` ne saute pas
seulement `bootctl`, il saute le `switch-to-configuration boot` tout entier — donc l'activation,
donc `/etc`. Le travail échouait sur une installation réussie, ce qui use la confiance qu'on
accorde aux verts.

Corrigé : il vérifie maintenant ce qu'un tel `nixos-install` produit réellement — le profil système
pointant vers la fermeture exacte qu'on vient de construire, les sept unités et le binaire
`prophet` **sur le disque cible** et non sur le coureur, le haché en 0600 — et il **dit** que les
comptes ne sont pas de son ressort, en nommant le travail qui en répond.

**Les deux dettes notées à 16 h 00 sont payées** : le commentaire périmé d'`installe.nix` (et
celui de `racine-en-lecture-seule.nix`, qui parlait au présent d'une option retirée), et le travail
de la racine en lecture seule, désormais à déclenchement manuel — entrée
`reposer_la_question_de_la_racine`. Un rouge permanent dans un tableau que le guide d'installation
demande de lire avant de graver une image n'est pas une information : c'est un entraînement à
ignorer le rouge.

### La surface refusait le bon mot de passe du propriétaire (12 septembre 2026, 17 h)

Le run `34705399751` a rendu cinq travaux bloquants sur six. Le sixième — « Le système installé
démarre », vert au tour précédent — a échoué, et sa cause n'est pas un aléa.

`prophet-surface` tenait `/dev/tty1` avec `TTYVHangup` et `Restart = "always"`. Sur une machine
sans pilote graphique, elle redémarre cinq fois en une minute, et **chaque tentative raccroche le
terminal où le propriétaire tape son mot de passe**. Le journal, à vingt-deux millisecondes près :

```
16:42:00.600  machine: sending keys 'essai-prophet\n'
16:42:00.686  prophet-surface.service: Scheduled restart job, restart counter is at 4
16:42:00.708  unix_chkpwd: password check failed for user (prophet)
```

Le mot de passe était le bon. Sur un vrai PC, le propriétaire aurait lu « Login incorrect » sans
écran graphique pour lui dire pourquoi, sur une machine dont il vient d'effacer le disque. Il n'y a
pas de pire moment pour donner à un système l'air de refuser son propriétaire.

`docs/components/surface.md` posait déjà la question — « le terminal disputé » — et nommait les deux
sorties possibles. Elle est prise : **la surface vit sur `tty7`**, celui que les serveurs graphiques
occupent depuis toujours et où NixOS ne fait naître aucun `getty` (il n'en crée que sur tty1 à
tty6). Le service de repli reste sur `tty1`, là où un humain regarde.

Le test était complice : il attendait que la surface atteigne `active` ou `failed`, or `active` est
traversé une fraction de seconde à **chaque** relance d'une unité en `Restart = "always"`. Il
déclarait donc la surface posée alors qu'elle en était à sa quatrième tentative, puis se connectait
dans la course — et gagnait une fois sur deux. Un test qui dépend d'une course ne protège de rien :
celui-ci avait déclaré la machine bonne au tour précédent. Deux corrections : l'attente exige
maintenant un état qui **tient** (`failed` est définitif ; un `active` qui survit dix secondes est
un vrai `active`), et un sous-test neuf lit `TTYPath` — il ne court pas, il constate.

Ce qui reste inconnu et n'est pas réglé : que `cage` bascule réellement sur `tty7` et y affiche
quelque chose. Aucun coureur n'a d'adaptateur graphique, et c'était déjà invérifiable sur `tty1`.
Le déménagement ne dégrade rien de vérifié ; il supprime un mal, lui, mesuré.

### La première ISO installable, et ce qu'elle ne tient pas (12 septembre 2026, 17 h 16)

Run `34707081688` : **les six travaux bloquants sont verts**, le septième ignoré comme voulu. La
correction de la surface tient — le sous-test de connexion, qui mettait 900 s à expirer, a rendu
la main en 1,04 s, et le propriétaire voit ses sept services depuis sa session :

```
machine: (finished: waiting for \$|prophet@ to appear on tty 1, in 1.04 seconds)
  Services
    ✓ capd  ✓ ledger  ✓ vault  ✓ egress  ✓ sandboxd  ✓ memoryd  ✓ agentd
```

Et `egress` a refusé la sonde de `prophet status`, sur la machine installée comme sur le serveur :
`WARN requête sans jeton hote=sonde.prophet.invalid`.

**Ce que le même test a montré, et qu'il ne faut pas laisser passer : le verrouillage du noyau
n'a pas lieu.**

```
initrd=… lockdown=integrity module.sig_enforce=1 … lsm=landlock,yama,bpf
lockdown : absent
```

Les deux paramètres sont bien sur la ligne de commande — c'est ce que le sous-test affirme, et il a
raison. Mais le noyau démarre avec `lsm=landlock,yama,bpf`, où `lockdown` ne figure pas, et
`/sys/kernel/security/lockdown` n'existe pas : le LSM n'est pas actif, donc `lockdown=integrity`
ne fait rien. `module.sig_enforce=1` est vraisemblablement inerte de même, puisque la machine
charge ses modules sans se plaindre.

Le sous-test **affichait** déjà `lockdown : absent` sans en conclure quoi que ce soit, et c'était
la bonne façon de ne pas mentir. Mais `docs/installation.md` annonçait « le verrouillage du noyau »
parmi ce qui est tenu, et `immutable.nix` le répète. Corrigé dans le guide : un durcissement
annoncé qui n'a pas lieu est pire qu'un durcissement absent, parce qu'on compte dessus.

Ce n'est pas un défaut de démarrage et cela ne retarde pas l'image. C'est une dette, nommée.
La payer demande d'ajouter `lockdown` à la liste `lsm=` — et de vérifier ce que cela casse, car
`module.sig_enforce=1` devenu effectif sur des modules NixOS non signés empêcherait une machine
réelle de charger ses pilotes.

### Prophet OS tourne sur le serveur (12 septembre 2026, 17 h 09)

Run `34707355297`, vert. Les sept daemons sont `active (running)` et `enabled` sur
`ubuntu-2gb-fsn1-2`, chacun sous son compte, avec le durcissement de l'image. Ce ne sont pas des
sondes de présence — chacun a écrit dans le journal ce qu'il fait :

```
prophet-capd    : clé créée /var/lib/prophet/capd/signing.key
                  politiques locales chargées nombre=1
                  capd écoute socket=/run/prophet/capd.sock
prophet-ledger  : clé créée /var/lib/prophet/ledger/seal.key
                  ledger écoute cle=ed25519:R633QnvkrUans5I6Kxqf/e0tOB/Ra2qxd/UPEeWARJ0=
prophet-egress  : egress écoute ; rien ne sort sans un jeton que capd approuve
                  WARN requête sans jeton hote=sonde.prophet.invalid   (×2)
prophet-sandboxd: niveau maximal atteignable : 1, Landlock ABI 8, gVisor /usr/bin/runsc
prophet-agentd  : les jetons viennent de capd, le journal part vers ledger
```

Les deux lignes d'`egress` valent d'être lues : c'est `prophet status` qui l'interroge, et le proxy
**refuse sa requête faute de jeton**. L'invariant « toute sortie réseau passe par egress » n'est pas
seulement déclaré sur cette machine, il est exercé — et la sonde d'état prouve davantage qu'un
`pong` en se faisant refuser.

Ce que cela n'est pas, et qui doit rester écrit : le serveur n'est pas devenu Prophet OS. Noyau
d'Ubuntu (`7.0.0-22-generic`), racine inscriptible, pas d'emplacements A/B, pas de chiffrement posé
par nous. Ce sont les daemons qui tournent, pas le système. Le niveau 2 y restera hors d'atteinte :
pas de `/dev/kvm`.

Ce qui a changé sur la machine, et comment le défaire :

| Fait | Comment revenir en arrière |
|---|---|
| `/root/hermes` **supprimé** | `tar xf /root/hermes-sauvegarde-<date>.tar -C /root` |
| Sept services dans `/etc/systemd/system/prophet-*.service`, démarrés et activés | `sudo /root/prophet_os/tools/lancer-sur-l-hote.sh --retirer` |
| Programmes dans `/usr/local/lib/prophet`, `prophet` dans `/usr/local/bin` | idem |
| Comptes `capd`, `ledger`, `vault`, `egress`, `memoryd`, `agentd` et groupe `prophet-system` | conservés par `--retirer` ; `userdel` à la main |
| État dans `/var/lib/prophet/<daemon>`, en 0700 | conservé par `--retirer` |

### Hermes est supprimé ; mon propre garde a empêché le lancement (12 septembre 2026, 17 h 06)

Le run `34707083853` a fait ce qu'on lui demandait d'abord : **`/root/hermes` est supprimé**, après
archivage et relecture. Les unités systemd et les entrées cron à son nom sont parties avec.

Puis le lancement a échoué — sur mon propre contrôle, et pour la faute exacte qu'il existe pour
empêcher. `systemd-analyze verify` ne relit pas une unité isolée : il charge tout le graphe de
dépendances et rapporte au passage ce qu'il a à reprocher aux unités de la distribution. Sur ce
serveur :

```
/usr/lib/systemd/system/xfs_scrub_all.service:26: Support for option CPUAccounting= has been
removed and it is ignored
```

Rien à voir avec Prophet OS. Mon filtre ne retirait que les lignes `not found`, donc il a pris ces
reproches pour les siens et refusé de démarrer les sept services. **Une sonde qui conclut sur autre
chose que ce qu'elle prétend mesurer** — c'est la faute que ce dépôt traque partout, écrite cette
fois dans l'outil chargé de l'attraper.

Corrigé : seules les lignes qui **nomment l'unité examinée** sont retenues ; les autres sont
comptées et signalées, parce qu'un avertissement qu'on écarte sans le montrer est un avertissement
qu'on a caché. Vérifié des deux côtés sur une machine : une unité portant
`SystemCallFilter=~@privileged ~@resources` est toujours refusée, et la sortie exacte du serveur
ne bloque plus rien.

Ce que la machine a répondu malgré l'échec, et qui vaut d'être noté :

```
  Isolation
  niveau maximal atteignable : 1 (0 confiné, 1 noyau utilisateur, 2 microVM)
    Landlock      : ABI 8
    gVisor        : /usr/bin/runsc
    /dev/kvm      : absent
```

Et `prophet log tail` a répondu « journal vide » au lieu de « aucun journal sur cette machine » : le
correctif de la recherche du journal fonctionne sur une vraie machine — il a trouvé
`/var/lib/prophet/ledger`, que le script venait de créer.

### L'adresse du serveur était publique (12 septembre 2026, 17 h)

Elle était posée en **variable** de dépôt `VPS_HOST`. GitHub masque la valeur d'un secret, pas
celle d'une variable : il l'imprime dans le bloc `env:` de chaque étape, et les journaux d'Actions
d'un dépôt public sont publics. L'adresse d'une machine dont ce dépôt documente qu'elle accepte
`root` par mot de passe s'est donc retrouvée en clair dans plusieurs exécutions — et une version
antérieure de la sonde l'affichait même en toutes lettres, `VPS_HOST = …`, en croyant rendre
service.

Corrigé : le workflow cherche d'abord le **secret** `VPS_HOST`, masque l'adresse dès sa première
étape, et ne l'imprime plus jamais — « posée » ou « absente », et rien d'autre. La variable reste
acceptée pour que rien ne casse, mais tant qu'elle est une variable, elle fuit une fois par
exécution dans le bloc `env:` de la première étape. **À faire par le propriétaire : déplacer
`VPS_HOST` dans les secrets**, et considérer l'adresse comme connue.

### L'archivage d'Hermes était trop lent pour finir (12 septembre 2026, 17 h)

L'étape a été coupée à sa limite de trente minutes, sans avoir fini. **Rien n'a été supprimé** :
l'effacement vient après la relecture de l'archive, jamais avant, et la relecture n'a pas eu lieu.
`/root/hermes` est intact.

La faute était `tar czf`. Compresser des données déjà compressées — historiques de marché, parquet,
journaux gzippés — coûte tout le temps du monde pour quelques pour cent. Et rien n'était mesuré
avant de commencer : on ne pouvait même pas dire s'il restait une minute ou une heure.

Corrigé : l'archive n'est plus compressée (`tar cf` va à la vitesse du disque) ; la taille et le
nombre de fichiers sont mesurés **avant** de commencer et affichés ; la place libre est vérifiée
avec une marge d'un dixième, parce qu'une archive qui remplit le disque casse la machine qu'on
essayait de préserver ; et la relecture **compte les fichiers** au lieu de se contenter que `tar`
n'ait pas protesté — une archive tronquée au premier bloc se relit sans se plaindre.

### Faire tourner Prophet OS sur une machine qui n'est pas Prophet OS (12 septembre 2026)

`tools/lancer-sur-l-hote.sh` installe les sept daemons en services systemd sur un hôte Ubuntu et
les démarre. Rien ne le faisait jusqu'ici : `verify-on-host.sh` sonde sans rien installer,
`setup-ubuntu-host.sh` pose des dépendances.

Ce que cela **n'est pas**, et qui doit être dit avant qu'on le découvre : le serveur ne devient pas
Prophet OS. Son noyau reste celui d'Ubuntu, sa racine reste inscriptible, il n'y a ni emplacements
A/B ni chiffrement posé par nous. Ce qui tourne, ce sont les daemons, avec le durcissement de
l'image. C'est la différence entre « Prophet OS est installé » et « Prophet OS tourne ici », et sur
un VPS qu'on ne réinstalle pas, seule la seconde est disponible.

Un piège trouvé en écrivant les unités à la main, et qui ne se voit pas dans le module NixOS :

```
SystemCallFilter=~@privileged ~@resources    # ne fait pas ce qu'on lit
```

systemd ne prend le `~` qu'en tête de valeur, puis lit chaque mot comme un nom d'appel système. Le
second `~@resources` n'est pas un groupe nié mais un nom invalide : il est écarté avec un simple
avertissement, et le filtre posé est plus large que voulu. `systemd-analyze verify` le dit —
« System call ~@resources is not known, ignoring » — et le script le lui demande désormais sur les
sept unités **avant** de démarrer quoi que ce soit. NixOS écrit une ligne par élément de liste, ce
qui masque le piège ; à la main, il faut le connaître.

### Hermes et le lancement, câblés dans le workflow du serveur (12 septembre 2026)

Deux entrées neuves, toutes deux à « false » par défaut :

- `supprimer_hermes` — archive `/root/hermes` dans `/root/hermes-sauvegarde-<date>.tar.gz`,
  **relit l'archive** (`tar tzf`), et n'efface qu'ensuite ; puis retire les unités systemd et les
  entrées cron à son nom. L'archive reste sur le serveur : la rapatrier la ferait passer par un
  artefact d'un dépôt public, et un moteur de trading contient des clés d'API. Une archive qu'on
  n'a pas ouverte n'est pas une sauvegarde, c'est un fichier dont on espère quelque chose ;
- `lancer_les_services` — lance `tools/lancer-sur-l-hote.sh`, puis relève ce que la machine répond
  (`prophet status`, `task ls`, `log tail`, l'état et le journal de chaque service) dans l'artefact
  `prophet-sur-le-serveur`.

L'en-tête du workflow disait « rien ici ne touche /root/hermes ». Ce n'est plus vrai, et il le dit
maintenant : le laisser écrit aurait été pire que de ne rien écrire.

### Une correction à ce que ce fichier affirmait encore

Le paragraphe « Le serveur de l'utilisateur reste inatteint » ci-dessous portait deux erreurs, dont
une de ma main. `workflow_dispatch` **fonctionne par l'API sur une branche de travail** : c'est le
bouton de l'interface qui exige la branche par défaut, pas le déclenchement. J'ai affirmé le
contraire pendant des heures, sur la foi d'un unique 404, et demandé trois fois à l'utilisateur une
modification qui n'était pas nécessaire. L'essai a rendu `204 No Content`. Le serveur n'est plus
inatteint : le run `34703605599` y a tourné, et le secret `VPS_PASSWORD` est posé.

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

**L'autorisation au niveau du socket est grossière.** Un pair est accepté s'il appartient au
groupe `prophet-system`, et il a alors accès à *toutes* les méthodes système du daemon. C'est
suffisant entre daemons, qui se font mutuellement confiance par construction, mais la surface doit
elle aussi en faire partie pour lire les tâches — et elle obtient du même coup un accès qu'elle
n'utilise pas. Restreindre demanderait une notion de méthode autorisée par pair que `prophet-ipc`
n'a pas. À faire avant qu'un programme moins fiable qu'un afficheur ne parle à un daemon.

**Les sept daemons tournent sous systemd**, dans une machine NixOS de test que `just test-vm`
démarre et que l'intégration continue exerce : chacun sous son utilisateur, avec le durcissement
du module, ses sockets en 0660 dans un répertoire en 0750, et la chaîne complète qui planifie une
tâche. Ce qui reste non vérifié est le matériel réel — la carte graphique, la carte réseau et le
micrologiciel d'un PC donné.

**Le serveur de l'utilisateur est atteint** (12 septembre, 15 h 52). Le workflow
`.github/workflows/verifier-sur-le-serveur.yml` y tourne, déclenché **par l'API sur la branche de
travail**. Ce paragraphe a longtemps dit le contraire, et c'était mon erreur : le bouton de
l'interface GitHub exige la branche par défaut, le déclenchement par l'API non. Le secret
`VPS_PASSWORD` est posé — et il reste la seule chose qu'aucun agent ne doit écrire : ni dans le
fichier, ni dans un commit, ni dans une entrée qu'il remplirait lui-même.

Une tentative de contourner le premier point — faire de la poussée elle-même le déclencheur, avec
l'intention écrite dans un fichier versionné — a été refusée, à raison : cela rendait un `git push`
capable d'arrêter un moteur de production et de changer un réglage du noyau sans qu'un humain
tranche au moment où cela arrive. Le workflow reste donc à déclenchement manuel.

## Backlog (hors tâche courante, à ne pas faire maintenant)

_Vide._

## Incidents (demandes de violation des invariants, refusées)

_Aucun._
