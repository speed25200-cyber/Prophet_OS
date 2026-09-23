# ADR-0047 — Estimer la mémoire qu'un poids demande, et ne pas charger ce qui ne tient pas

- **Statut** : accepté ; estimation vérifiée contre le vrai moteur en CI (travail « Poids du
  catalogue servis (réels) », `227a63e` : 5 à 10 % au-dessus de la mémoire résidente)
- **Date** : 2026-09-23
- **Tâche liée** : M8-T7 (FRONTIER : moteurs locaux, « mémoire »)

## Contexte

Depuis l'ADR 0046, un humain ou un agent peut télécharger et servir n'importe quel poids du
catalogue : de 0,6 Go à 5 Go de fichier. Charger un modèle qui ne tient pas ne fait pas
d'erreur : le noyau pagine, le bureau gèle pendant l'inférence, puis l'OOM tue un processus —
peut-être la surface, peut-être un service. La taille du fichier ne suffit pas à le prévoir :
le cache KV que llama.cpp alloue pour toute la fenêtre dépend de l'architecture (Phi-3 mini,
sans GQA, en demande 1,6 Go à 4 096 tokens ; Qwen3 8B, 0,6 Go), et le tampon de calcul, du
vocabulaire. Le plan demande « mémoire, VRAM » sans dire comment ; la VRAM exige une carte, la
mémoire vive non.

## Décision

- **Estimer depuis l'en-tête GGUF** (`providers::memory::need`) : fichier + cache KV
  (couches × têtes KV × (dimension des clés + des valeurs) × 2 octets f16 × fenêtre) + logits
  d'un micro-lot (vocabulaire × 512 × 4 octets) + 192 Mio pour le moteur. Les têtes KV valent
  les têtes d'attention quand l'en-tête ne les distingue pas, la dimension d'une tête la
  largeur divisée par les têtes ; sans de quoi calculer le cache, pas d'estimation (on ne
  devine pas). La fenêtre est celle du moteur de la machine : `PROPHET_LOCAL_CONTEXT`, que le
  module du moteur local pose à partir de `contextSize`.
- **Confronter à `/proc/meminfo`** : `fits` si `MemAvailable` suffit, `tight` si la machine
  vide la tiendrait, `too_large` si elle ne la tient pas en gardant 1,5 Gio au système
  (bureau, services).
- **Le dire partout où l'on choisit un modèle** : `prophet model ls` (colonne mémoire, verdict),
  `model.list` pour les agents (`memory` par poids, `system_memory`, `local_context`), la page
  Modèles (une jauge par poids : la machine entière, ce que d'autres occupent, ce que ce poids
  demande, en orange quand il déborde).
- **Refuser de charger ce qui ne tient pas** : `prophet model serve` refuse un poids
  `too_large`, en disant poste par poste ce qu'il demande et ce que la machine a ; `--force`
  charge quand même, en connaissance de cause. `tight` charge : la mémoire peut se libérer.
- **Ne pas lancer de mission sur ce qui ne tient pas** : le routeur charge à la demande le
  modèle qu'une requête nomme ; agentd refuse donc la préparation (`task.prepare`) et le
  lancement (`task.start`) d'une mission locale dont le modèle, retrouvé parmi les poids
  installés par le nom que le routeur lui donne (son fichier sans `.gguf`), est `too_large`.
  La mission reste planifiée ; le refus dit ce que le modèle demande. Aucune requête au moteur
  n'est faite pour cela.
- **Le dire avant de télécharger** : chaque entrée du catalogue porte le cache KV par token
  et le vocabulaire relevés dans l'en-tête de son fichier (`kv_bytes_per_token`,
  `vocabulary`) ; `model.catalog` rend pour chacune ce qu'elle demandera (`memory`), que
  `prophet model catalog` et la page Modèles disent (« ≈ 6,1 Go en mémoire », « Trop grand
  pour cette machine »). L'essai `needs_network` des empreintes lit, par le vrai egress,
  les 16 premiers Mio de chaque fichier à sa révision épinglée (`providers::pull::get_prefix`,
  une plage d'octets, redirections bornées aux hôtes de l'entrée) et exige que le catalogue
  porte ce que l'en-tête dit. Relevé le 23 septembre sur les huit entrées : de 80 Kio par token
  (Granite 3.3 2B) à 384 Kio (Phi-3 mini, sans GQA) ; revérifié par la CI (`b2f97f9`, travail
  d'isolation, vert).
- **Montrer ce que le moteur tient vraiment** : `providers::memory::engine_instances` lit dans
  `/proc` les instances de llama-server, le poids que chacune a chargé (`--model`) et leur
  mémoire résidente et anonyme ; la page Modèles en fait un repère sur la jauge du poids servi
  (« le moteur en tient 8,8 Go résidents, dont 3,7 anonymes »), `model.list` le rend aux agents
  (`resident`), `prophet model ls` l'ajoute au poids servi.
- **Refuser de télécharger ce qui ne tient pas sur le disque** : `providers::pull` compare ce
  qui reste à recevoir à la place libre du dossier (`statvfs`) avant toute requête.

## Alternatives écartées

- **La taille du fichier seule** : elle manque le cache KV, qui varie du simple au triple
  d'une architecture à l'autre à fenêtre égale.
- **Demander au moteur** (`/props` après chargement) : il faut avoir chargé, c'est-à-dire déjà
  payé la pagination qu'on voulait éviter.
- **Refuser aussi `tight`** : la mémoire disponible bouge ; un navigateur fermé la libère.
  Le dire suffit.
- **Borner par un cgroup mémoire du moteur** : utile en plus (l'unité systemd du moteur est
  durcie), mais un OOM dans le cgroup reste un échec tardif ; l'estimation dit avant.

## Conséquences

- L'estimation est une prévision : le travail « Poids du catalogue servis (réels) » relève,
  pour chaque famille, la mémoire résidente de l'instance que le routeur a lancée
  (`/proc/<pid>/status`) et exige que l'estimation ne manque pas la mémoire anonyme (cache KV,
  calcul, poids recopiés par le moteur) ni ne dépasse 1,5 fois ce que le moteur tient. Les
  constantes se recalibrent sur ces relevés.
- Premier passage (`89b1b3a`, fenêtre 2 048, quatre cœurs sans carte) : **vert**. L'estimation
  couvre la mémoire anonyme de chaque instance (Qwen3 8B : 5,8 Go estimés, 3,7 Go anonymes),
  mais la mémoire résidente totale la dépasse de 30 à 50 % (8,8 Go pour Qwen3 8B ; rapport
  estimation / résident de 0,66 à 0,76 sur les quatre autres familles) : la part anonyme dépasse
  cache KV et calcul d'environ les deux tiers du fichier, et le fichier entier reste projeté.
  Hypothèse : le processeur reçoit une copie réarrangée des poids Q4_K (« repack » de llama.cpp)
  pendant que la projection du fichier reste résidente ; ses pages propres se récupèrent sous
  pression, et la mémoire de travail vaut alors l'estimation. L'essai relève désormais le bilan
  que le moteur écrit dans son journal (tampons projetés et réarrangés, cache KV, calcul) pour
  la confirmer ou la corriger ; si elle tient, charger sans projection (`--no-mmap`) rendrait la
  mémoire résidente égale à l'estimation, et l'OOM ne compterait plus deux fois les poids.
- **Complément (même jour) : l'image charge les poids sans projection.** La source épinglée
  (llama.cpp `v0.4.0`, celle du nixpkgs du `flake.lock`) tranche l'hypothèse :
  `llama-model-loader.cpp` recopie depuis la projection tout tenseur qui ne vit pas dans le
  tampon projeté — c'est le cas de la copie réarrangée pour le processeur — et, le chargement
  fini, ne libère que le début et la fin de la projection ; les pages du milieu restent
  résidentes, comptées à l'instance (et par l'OOM). `--no-mmap` y est déprécié au profit de
  `--load-mode` (`auto`, `none`, `mmap`, `mlock`, `dio`) ; un préréglage le nomme par l'option
  sans tirets (`preset.cpp`, `get_map_key_opt`). Le module du moteur local passe donc
  `--load-mode none` en mode simple et `load-mode = none` à chaque section du mode relais,
  section globale `[*]` comprise ; le contrôle `llama-router` vérifie que l'instance le reçoit.
  Coût : les poids se lisent par `read` au lieu d'être projetés — le même disque, et le cache
  de pages garde le fichier, propre et récupérable, hors de la mémoire du moteur. L'essai des
  familles tourne désormais ainsi, et exige que l'estimation couvre au moins 90 % de la mémoire
  anonyme sans dépasser 1,3 fois la résidente. Le journal du routeur, tamponné, était perdu
  quand l'essai le tuait (SIGKILL) : il est arrêté par SIGTERM avant d'être lu.
  Verdict (`227a63e`) : **vert**. La mémoire résidente tombe de 28 à 38 % et devient presque
  toute anonyme (fichier projeté : 20 à 37 Mo, le binaire et ses bibliothèques) ; l'estimation
  la dépasse de 5 à 10 %, du côté sûr. Qwen3 8B : 5,46 Go résidents au lieu de 8,76, pour
  5,84 estimés ; Granite 3.3 2B 1,88 au lieu de 3,03 ; SmolLM2 1,61 au lieu de 2,36 ; Phi-3
  mini 3,29 au lieu de 4,57 ; Llama 3.2 3B 2,50 au lieu de 3,85. Les cinq répondent juste.
  Prix payé : Qwen3 8B met 6,3 s à être servi au lieu de 2,9 (5 Go lus d'un coup au lieu d'être
  projetés à la demande). Le contrôle `llama-router` est vert : l'instance reçoit
  `--load-mode none` de la section `[*]`. Cette version du moteur n'écrit pas ses tampons à la
  verbosité par défaut ; la mémoire résidente suffit à juger l'estimation.
- Les architectures à fenêtre glissante (Gemma 3) ou à attention latente (DeepSeek) ont un
  cache plus petit que la formule : l'estimation les surestime, du côté sûr.
- La VRAM reste à mesurer sur une carte (`needs_gpu`) : la même estimation vaudra pour les
  couches déchargées, à vérifier.
