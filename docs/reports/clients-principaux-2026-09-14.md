# Claude et ChatGPT, modèles principaux — 14 septembre 2026

L'humain a tranché le matin : « les modèles principaux seront ChatGPT et Claude, oublie le
local ». Ce rapport dit ce qui a été fait dans la journée pour que Claude Code et Codex soient
les cerveaux de Prophet OS, par leurs clients officiels, sans clé API et sans que l'OS touche à
leurs identifiants ; ce qui est prouvé ; ce qui ne l'est pas.

## Ce qui est fait

- **Un client officiel est le modèle d'une mission** (complément de l'ADR 0035, `a5ac295`).
  `task.prepare` admet `codex` ou `claude-code` comme modèle, si le contexte l'admet et si le
  lanceur de la session le dit connecté — sinon la préparation dit comment se connecter.
  `task.start` lance le client par le lanceur, sans le moteur local ; le client rejoint la
  mission par le pont, travaille sous son jeton, se retire ; un fil du service conclut ce qu'il
  aurait laissé ouvert. Le catalogue propose les clients connectés en tête des modèles de
  chaque contexte, et tous les contextes de l'image les préfèrent ; le modèle local n'est plus
  qu'un secours. Les rôles suivent : Claude Code réfléchit et relit, Codex code et exécute.
- **Les deux cerveaux ensemble** (`64abe00`). Codex mène une mission, écrit, confie la
  relecture à Claude Code par `task.delegate {role: "review"}` ; Claude Code rejoint sa
  sous-mission, rend son avis ; Codex conclut avec. Chacun dans sa mission contrôlée, compté
  sous son nom.
- **L'espace de travail partagé du relais** (ADR 0039, `cca45fc`). Une sous-mission part de
  l'espace de travail de son parent — elle voit ce qu'il vient d'écrire — et, finie, y rapporte
  son diff ; le parent publie le tout, examiné d'un seul tenant ; une sous-mission ne se publie
  plus seule. Sans cela, le relecteur lisait un fichier absent.
- **Un client nommé dans une délégation** (`68ad481`) : `task.delegate {model: "codex"}` ; sans
  modèle ni rôle, une mission menée par un client confie à ce même client.
- **L'humain coupe** (`239753b`) : `task.cancel` d'une mission menée par un client conclut sa
  séance puis `pilot.stop` tue le client et tout son groupe de processus, sur-le-champ.
- **Lisible** : `prophet task options` nomme les clients (« codex — Codex (ChatGPT) ») et dit
  leur connexion ; la surface nomme qui mène une mission et liste, sous « Confiées », les
  sous-missions qu'elle a déléguées (à qui, quoi, où chacune en est) ; le guide d'installation
  montre le chemin au terminal (`4b0c564`, `ab96339`, `475cf84`). Une mission dont le client
  est lancé mais pas encore attaché ne se relance pas : elle s'annule (`bd80bf3`).
- **Les paliers de modèles** (ADR 0040, `b18ebaf`) : `driver:claude-code@opus` pour réfléchir et
  coder, `@sonnet` pour relire après Codex, `@haiku` pour exécuter ; le lanceur passe le palier
  au client par `--model` (Claude Code) ou `-m` (Codex, Gemini) ; réglables par
  `prophet.localEngine.paliers`. C'est l'économie de tokens demandée, chez les clients.
- **L'atelier logiciel de bout en bout** (`6f777e6`, `478736a`) : un client écrit un outil Python
  dans l'espace de la mission, l'exécute par `proc.exec` — en microVM, par le vrai sandboxd —,
  la sortie et le fichier écrit par l'outil reviennent. L'essai a révélé que le contexte ne
  pouvait pas démarrer (mission montée au niveau 2 par le planificateur, approbation par
  commande que rien ne pouvait donner) : corrigé, ADR 0031 complété. Puis un coureur à
  virtualisation imbriquée a montré un disque de travail sans assez d'inodes : corrigé.
- **Deux rouges de la CI corrigés** : l'invité microVM monte l'espace de travail au chemin de
  l'hôte par une surcouche overlay (`4ade307` ; l'hôte de la CI a son répertoire temporaire
  sous `/home`, que la racine squashfs ne laissait pas créer) ; le contrôle des profils admet
  `proc.kill` avec `proc.exec` (le catalogue de l'image l'employait, agentd refusait de
  démarrer).

## Preuves

Toutes avec les vrais capd, ledger, agentd, `prophet-pilotd`, sfs et la CLI ; les clients sont
des scripts de remplacement lancés par le vrai lanceur, sur le même chemin que les vrais
(pont, séance, CLI, sortie finale au format du client). Machine : WSL2, CPU seul.

| Preuve | Où |
|---|---|
| Mission préparée sur `codex`, lancée, écrite par la séance, texte revenu, moteur local jamais sollicité ; refus sans lanceur ; client non admis refusé | `crates/agentd/tests/pilot.rs` |
| Codex mène, confie la relecture à Claude Code, qui lit le code que Codex vient d'écrire et dépose son verdict chez Codex ; le parent porte le compte de l'enfant | idem |
| `task.delegate {model: "codex"}` ; `gemini` non admis = erreur d'argument sans sous-mission | idem |
| Annulation : le faux Codex qui s'attarde trente secondes est tué (son attente disparaît, vérifié par `pgrep`) ; une autre mission sur lui se lance et finit aussitôt | idem, et `crates/pilotd/src/lib.rs` (client avec sous-processus arrêté en moins d'une seconde) |
| Le faux Claude Code reçoit `--model sonnet` pour la relecture ; le lanceur place `--model`/`-m` au bon endroit pour chaque client | idem, et `crates/pilotd/src/lib.rs` |
| Sous-tâche depuis l'espace du parent, rapport (modifié, supprimé, ajouté), périmètre hors du parent laissé chez l'enfant, publication du parent, parent fermé refusé | `crates/sfs/tests/workspace.rs` |
| Niveau 2 depuis un répertoire absent de la racine de l'invité (`/root`) : 4 s la première fois, 1,8 s ensuite | `crates/sandboxd/tests/enforcement.rs` (`needs_kvm`) |
| Catalogue de l'image (`proc.kill`) chargé | `crates/agentd/tests/preparation.rs` ; job « sept services » |
| Un client écrit `somme.py`, l'exécute en microVM (niveau 2, vrai sandboxd), « somme 7 » et `resultat.txt` reviennent — 2,9 s | `crates/agentd/tests/pilot.rs` (`needs_kvm`) |

CI de `a5ac295` : tout vert — `check`, isolation sur l'hôte (les trois essais `needs_kvm` avec
l'invité à surcouche), parole, surface, ChatGPT, moteur, mission locale réelle, installeur,
ISO, sept services, système installé et ses deux démarrages — sauf « Voir l'image démarrer »
sous UEFI : le noyau de l'invité QEMU s'est planté au chargement des modules avant tout
terminal, le même support ayant démarré sous SeaBIOS dans le même travail ; ce démarrage était
vert sept fois de suite avant. Le travail rejoue désormais une fois après un tel plantage, la
trace du premier essai conservée. CI de `239753b` (clients principaux, espace partagé, client
nommé, `pilot.stop`) : **les deux chaînes entièrement vertes** (14 sept. 12:40 UTC), ISO UEFI
comprise. CI de `0d3fa49` (« Confiées », mission en main, second essai UEFI) : verte des deux côtés
(13:20 UTC). CI de `ead38a8` (paliers de modèles) : verte des deux côtés (14:05 UTC).

## Ce qui n'est pas prouvé

- Le vrai Claude Code et le vrai Codex. Leur connexion appartient à l'humain, sur une machine
  installée ; la forme de leur sortie finale, leur configuration MCP et leur conduite devant un
  refus de capd sont construites d'après leur documentation. C'est l'essai `needs_claude_login`
  / `needs_chatgpt_login`, le premier à mener quand une machine connectée existe.
- Le client n'est pas confiné (ADR 0026) : il tourne dans la session, sous l'identité de
  l'humain ; seuls ses appels par la mission sont tranchés par capd.
- Le rapport au parent est une copie de fichiers, pas une fusion (ADR 0039).

## Pour la session suivante

1. Lire la CI de `d81da7c` et suivants (surface : « Confiées », mission en main ; CI : second
   essai UEFI).
2. Une machine avec Claude Code ou Codex connecté : `prophet task prepare --model codex`, et
   regarder le vrai client rejoindre la mission.
3. Confiner le client (ADR 0026) ; montrer dans la surface ce que chaque client a rapporté.
