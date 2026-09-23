# La mémoire des poids locaux — 23 septembre 2026

Un agent ou un humain peut désormais télécharger et servir n'importe quel poids du catalogue du
système (ADR 0046). Charger un modèle qui ne tient pas en mémoire ne produit pas d'erreur : le
noyau pagine, le bureau gèle pendant l'inférence, puis l'OOM tue un processus. Ce rapport dit
ce que le système sait désormais prévoir, ce qu'il en montre, ce qu'il refuse, et ce que la
mesure sur le vrai moteur a appris — dont un défaut de chargement corrigé dans l'image.
L'[ADR 0047](../adr/0047-la-memoire-des-poids-estimee-avant-de-charger.md) porte les décisions.

Environnement des mesures : coureur `ubuntu-latest` de la CI, quatre cœurs x86-64, sans carte
graphique ; llama.cpp épinglé (`v0.4.0`, nixpkgs du `flake.lock`), routeur lancé comme l'image
le lance, fenêtre de 2 048 tokens. Rien ici ne mesure une carte graphique ni la VRAM.

## 1. Estimer avant de charger

L'en-tête GGUF donne les couches, les têtes KV, la dimension des clés et des valeurs, et la
taille du vocabulaire. `providers::memory::need` en déduit ce que llama.cpp réservera : le
fichier, le cache KV de toute la fenêtre (couches × têtes KV × (clé + valeur) × 2 octets), les
logits d'un micro-lot (vocabulaire × 512 × 4 octets) et 192 Mio pour le moteur. Le catalogue
porte ces nombres pour chaque entrée, relevés dans les 16 premiers Mio de chaque fichier ; un
essai `needs_network` les relit par le vrai egress à la révision épinglée (vert sur `b2f97f9`).

| Entrée | Cache KV par token | Vocabulaire | Déclare outils / réflexion |
|---|---|---|---|
| Qwen3 0.6B, 1.7B | 112 Kio | 151 936 | oui / oui |
| Qwen3 4B, 8B | 144 Kio | 151 936 | oui / oui |
| IBM Granite 3.3 2B | 80 Kio | 49 159 | oui / oui |
| SmolLM2 1.7B | 192 Kio | 49 152 | non / non |
| Phi-3 mini 4k | 384 Kio (sans GQA) | 32 064 | non / non |
| Llama 3.2 3B | 112 Kio | 128 256 | oui / non |

À fenêtre égale, Phi-3 mini demande trois fois et demie le cache de Llama 3.2 3B pour un
fichier à peine plus gros : la taille du fichier seule ne prévoit rien.

## 2. Ce que le système en fait

- **Le dire** : `prophet model ls` (colonnes mémoire et outils), `prophet model catalog` (avant
  de télécharger), `prophet status`, `model.list` pour les agents (`memory`, `fit`,
  `template`, `resident`), la page Modèles (une jauge par poids : la machine, ce que d'autres
  occupent, ce que ce poids demande ; un repère de ce que le moteur tient pour le poids servi).
- **Refuser** : `prophet model serve` ne charge pas un poids qui ne tiendrait pas, sauf
  `--force` ; agentd ne prépare ni ne lance de mission locale sur un tel modèle, puisque le
  routeur le chargerait à la demande ; `prophet model pull` ne commence pas un téléchargement
  qui ne tient pas sur le disque.

## 3. Ce que la mesure a appris

Premier passage (`89b1b3a`, reproduit sur `b2f97f9` à 0,1 % près) : l'estimation couvrait la
mémoire anonyme de chaque instance, mais la mémoire résidente totale la dépassait de 30 à 50 %.

| Famille | Estimée | Résidente | dont anonyme | dont fichier |
|---|---|---|---|---|
| Qwen3 8B | 5,8 Go | 8,8 Go | 3,7 Go | 5,0 Go |
| Granite 3.3 2B | 2,0 Go | 3,0 Go | 1,5 Go | 1,6 Go |
| SmolLM2 1.7B | 1,8 Go | 2,4 Go | 1,3 Go | 1,1 Go |
| Phi-3 mini | 3,5 Go | 4,6 Go | 2,1 Go | 2,4 Go |
| Llama 3.2 3B | 2,7 Go | 3,9 Go | 1,8 Go | 2,0 Go |

Le fichier entier restait projeté alors que la part anonyme dépassait déjà cache et calcul
d'environ les deux tiers du fichier. La source épinglée l'explique (`src/llama-model-loader.cpp`) :
un tenseur qui ne vit pas dans le tampon projeté — la copie réarrangée que le processeur reçoit
— est recopié **depuis la projection**, et le chargement fini ne libère que le début et la fin
de celle-ci. Les pages du milieu restent résidentes et comptées à l'instance, y compris par
l'OOM. L'image charge donc les poids sans projection : `--load-mode none` en mode simple,
`load-mode = none` dans chaque préréglage du relais (`--no-mmap` est déprécié dans cette
version ; un préréglage nomme l'option sans tirets). Le contrôle `llama-router` vérifie que
l'instance reçoit l'option.

| Famille | Estimée | Résidente avec projection | Résidente sans projection | Gain |
|---|---|---|---|---|
| Qwen3 8B | 5,84 Go | 8,76 Go | 5,46 Go | −38 % |
| Granite 3.3 2B | 2,02 Go | 3,03 Go | 1,88 Go | −38 % |
| SmolLM2 1.7B | 1,76 Go | 2,36 Go | 1,61 Go | −32 % |
| Phi-3 mini | 3,47 Go | 4,57 Go | 3,29 Go | −28 % |
| Llama 3.2 3B | 2,72 Go | 3,85 Go | 2,50 Go | −35 % |

Sans projection (`227a63e`), la mémoire résidente est presque toute anonyme (20 à 37 Mo de
fichiers projetés : le binaire et ses bibliothèques) et l'estimation la dépasse de 5 à 10 %.
Les cinq familles répondent toujours juste ; servir Qwen3 8B prend 6,3 s au lieu de 2,9, les
5 Go étant lus d'un coup.

## 4. Vérification

| Élément | Où | Verdict |
|---|---|---|
| Estimation, verdict, lecture de `/proc`, gabarits | `cargo test -p providers --lib -- memory weights` | vert |
| Refus de `serve`, colonnes de `ls`, catalogue | `crates/prophet-cli/tests/modeles.rs` | vert |
| Mission refusée sans contacter le moteur | `crates/agentd/tests/local_daemon.rs` | vert |
| Jauge, repère du poids servi, capacités | `crates/surface/tests/bureau.rs` (`needs_gpu`, lavapipe) | vert |
| En-têtes du catalogue relus à la source | essai `needs_network`, travail d'isolation, `b2f97f9` | vert |
| Estimation face au vrai moteur, avec projection | travail « Poids du catalogue servis (réels) », `89b1b3a`, `b2f97f9` | vert (écart expliqué ci-dessus) |
| Estimation face au vrai moteur, sans projection | même travail, `227a63e` | vert : estimation 5 à 10 % au-dessus |
| L'instance reçoit `--load-mode none` | contrôle `llama-router`, `227a63e` | vert |

## 5. Ce qui reste

- La VRAM et les couches déchargées sur une carte (`needs_gpu`) : la même estimation vaudra
  pour elles, à vérifier sur une machine qui en a une.
- Les architectures à fenêtre glissante ou à attention latente : l'estimation les surestime,
  du côté sûr ; aucune n'est encore au catalogue.
