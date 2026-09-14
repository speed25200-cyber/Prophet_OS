# ADR-0037 — Accélérer les modèles locaux par Vulkan, sur demande

- **Statut** : accepté
- **Date** : 2026-09-14
- **Tâche liée** : M8-T7

## Contexte

Le moteur local ne tourne que sur processeur : `llama-server` de nixpkgs sans backend
graphique, `--gpu-layers 0`, et une unité systemd qui cache les périphériques
(`PrivateDevices`). Sur le coureur d'intégration continue, Qwen3-1.7B répond à 13 tokens par
seconde ; sur un portable ordinaire, ce n'est pas la fluidité que le plan demande, et un modèle
plus grand y est hors de portée. L'image porte déjà Mesa et Vulkan pour la surface (RADV pour
AMD, ANV pour Intel, NVK pour les NVIDIA au module ouvert) : la voie graphique la plus large
est là, sans pilote propriétaire, que le verrouillage du noyau refuserait de toute façon.

## Décision

Le paquet `llama-cpp` de l'image existe en deux variantes : la variante CPU, inchangée, et
`llama-cpp-vulkan`, le même `llama-cpp` de nixpkgs avec son backend Vulkan et le même
correctif du protocole d'appels. L'option `prophet.localEngine.gpu.enable` (faux par défaut)
fait servir la variante Vulkan, passe `--gpu-layers` (`gpu.layers`, 999 par défaut : tout le
modèle) au moteur et dans le fichier de préréglages du routeur, et ouvre à l'unité les seuls
périphériques DRM (`DeviceAllow = char-drm`, groupes `video` et `render`) ; tout le reste du
durcissement tient. L'intégration continue construit la variante Vulkan à chaque poussée, pour
qu'elle compile toujours ; aucun coureur n'a de carte, elle n'y est pas exercée.

## Alternatives écartées

- CUDA : NVIDIA seulement, et le pilote propriétaire ne se charge pas sous `lockdown=integrity`.
- ROCm : AMD seulement, une fermeture de plusieurs gigaoctets pour une partie des cartes que
  Vulkan sert déjà.
- L'accélération par défaut : jamais mesurée sur une vraie carte, et une puce intégrée faible
  peut être plus lente que le processeur ; le chemin CPU est prouvé, il reste le défaut.

## Conséquences

- L'installeur active l'option lui-même : si `vulkaninfo`, sur le support d'amorçage muni des
  pilotes Vulkan de Mesa, voit un périphérique qui n'est pas le rastériseur logiciel, il écrit
  `image/machine/acceleration.nix` (troisième fichier propre à la machine, après ceux de
  l'ADR 0032) ; sinon le fichier reste vide et les modèles tournent sur processeur. Une
  machine peut aussi poser l'option à la main.
- L'essai réel est marqué `needs_gpu` : vitesse, mémoire vidéo occupée et chute sur processeur
  quand la carte manque sont à mesurer sur du matériel, pas en machine virtuelle.
- Le test du moteur local en VM reste sur processeur ; la variante Vulkan n'est vérifiée que
  par sa construction.
