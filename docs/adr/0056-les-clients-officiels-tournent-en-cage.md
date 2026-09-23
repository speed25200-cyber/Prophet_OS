# ADR-0056 — Les clients officiels lancés en mission tournent en cage

- **Statut** : accepté (phase 1 : fichiers, services, session ; phase 2 : réseau par egress)
- **Date** : 2026-09-23
- **Tâche liée** : M8-T4 à M8-T6 (« lance le binaire officiel dans une sandbox niveau 1 ») ;
  FRONTIER (« clients officiels effectivement exécutés dans le confinement requis ») ; ADR 0026,
  0035, 0044

## Contexte

M8-T4 à M8-T6 étaient cochés pour la ligne de commande, l'environnement et la détection de
session, mais `prophet-pilotd` lançait Claude Code, Codex et Gemini **sans confinement**, sous
l'identité de l'humain, avec le répertoire de travail dans `~/Documents/Prophet`. Un client
d'une mission pouvait donc :

- lire et écrire toute la maison de l'humain, y poser un fichier de démarrage exécuté à la
  session suivante, et écrire dans ses documents **sans passer par l'examen** de SFS ;
- joindre `capd.sock` et trancher une approbation (ADR 0044), et joindre agentd sous l'identité
  de l'humain pour n'importe quelle méthode : `task.spawn` d'un manifeste de sa main,
  `task.apply`, `task.halt` ;
- parler au bus de session, à sway (`swaymsg exec`), à `systemd --user`, et lancer ainsi un
  programme hors de tout espace.

Deux invariants étaient en défaut : « tout processus non fiable tourne sous sandboxd au niveau
requis » et « toute sortie réseau passe par egress ».

## Décision

**Phase 1, en place.** Chaque client lancé en mission passe par `prophet-pilot-cage`, qui
réutilise les primitives de `sandboxd` (racine minimale, Landlock) :

- espaces de noms utilisateur, montage, **processus**, IPC et nom d'hôte ; même uid et gid que
  l'humain (rien de plus) ; le processus 1 de la cage recueille les enfants, et tuer la cage
  tue tout ce qu'elle contient ;
- racine minimale : le système en lecture seule (`/nix/store`, `/etc`, le système courant),
  le **profil privé** du client en écriture (ses identifiants, que l'OS ne lit jamais : il les
  lui monte), et, propres à la mission, une maison, un temporaire et un répertoire de travail,
  retirés après elle ; Landlock borne le tout, rien ne s'exécute hors du système ;
- un seul socket : un relais du lanceur, qui ne laisse passer vers agentd que la séance de
  **cette** mission (`task.attach`, `task.tools`, `task.call`, `task.detach` avec son
  identifiant) ; ni capd, ni le journal, ni `/run/prophet` ne sont visibles ;
- un environnement vidé : langue, fuseau, identité, autorités de certification et proxy de
  l'humain passent ; le bus de session, sway, Wayland et `XDG_RUNTIME_DIR` non ;
- **pas de cage, pas de client** : sans `prophet-pilot-cage` ou si la cage ne se pose pas,
  `pilot.run` échoue en le disant (`SandboxError`).

Aucun filtre d'appels système n'est posé dans la cage : Codex confine lui-même ses commandes
avec les espaces de noms, et le filtre du niveau 0 les refuserait.

**Phase 2, à faire.** Le réseau de l'hôte reste au client, qui joint son éditeur directement :
l'invariant « toute sortie réseau passe par egress » n'est pas encore tenu pour les clients
officiels. Le tenir demande un espace réseau propre à la cage, un relais dans la cage vers le
socket d'egress (le client ne parle que TCP), le jeton de la mission ajouté hors de la cage, et
une politique des hôtes de chaque éditeur (API, authentification, télémétrie) que seule
l'épreuve d'un client connecté permet d'arrêter sans casser son fonctionnement.

## Alternatives écartées

- **Lancer les clients par sandboxd (niveau 1, gVisor)** : sandboxd tourne sous son propre
  compte ; le client doit tourner sous l'identité de l'humain pour lire son profil, et gVisor
  n'est pas sur toutes les machines. La cage réutilise ses primitives sous la bonne identité.
- **Masquer seulement `/run/prophet`** : le client garderait la maison, le bus de session et
  sway, par lesquels il sort de tout espace (constat de STATUS, « Écart relevé »).
- **Un interrupteur pour lancer sans cage** : ce serait contourner l'invariant « même pour le
  test » ; les essais tournent en cage, et se taisent là où les espaces de noms manquent.
- **Laisser le répertoire de travail dans `~/Documents/Prophet`** : les outils natifs du client
  y écriraient sans examen ; il travaille dans la cage, et ses écritures durables passent par
  la séance (`fs.write` → SFS → l'humain applique).

## Conséquences

- Les essais des clients (`pilotd --test cage`, `agentd --test pilot`) exigent des espaces de
  noms utilisateur : ils se taisent sur « check » et sont exigés par le coureur d'isolation
  (`tools/verify-on-host.sh`, `PROPHET_EXIGER_ESPACES_DE_NOMS=1`).
- Un client installé hors du système (dans la maison) doit être déclaré par
  `PROPHET_PILOT_READ_ONLY`, en lecture seule, sans quoi la cage ne le voit pas.
- Les sondes d'état (`claude auth status`, `codex login status`) et la connexion
  (`prophet provider login`) restent hors cage : ce sont les commandes du client, lancées pour
  l'humain, sans objectif de mission. Le lanceur interactif « Claude Code · mission » du bureau
  reste la session de l'humain lui-même.
- À vérifier avec un client connecté (`needs_codex_login`, `needs_claude_login`) : que Claude
  Code et Codex trouvent tout ce qu'il leur faut dans la cage (profil, certificats, résolveur).
