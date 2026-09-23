# ADR-0047 — Estimer la mémoire qu'un poids demande, et ne pas charger ce qui ne tient pas

- **Statut** : accepté ; estimation vérifiée contre le vrai moteur en CI (travail « Poids du
  catalogue servis (réels) »)
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
- Les architectures à fenêtre glissante (Gemma 3) ou à attention latente (DeepSeek) ont un
  cache plus petit que la formule : l'estimation les surestime, du côté sûr.
- La VRAM reste à mesurer sur une carte (`needs_gpu`) : la même estimation vaudra pour les
  couches déchargées, à vérifier.
