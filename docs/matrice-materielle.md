# Matrice matérielle de Prophet OS

Ce que l'image embarque pour un PC ordinaire, et ce qui en a été **vu**. La colonne « Vu » ne
ment pas : « VM » veut dire vu dans la machine virtuelle de l'intégration continue (QEMU, OVMF
ou SeaBIOS, disque virtio, réseau virtio, carte virtio-gpu sans accélération), « hôte CI » sur
le coureur GitHub avec KVM, « non » que personne ne l'a encore constaté. **Rien n'a jamais
démarré sur un vrai PC** ; ce document est une déclaration de ce que l'image contient, pas
une liste de machines qui marchent. Le relevé que l'installeur affiche avant d'effacer le
disque (`docs/installation.md`, section 3) dit, depuis la clé, ce qu'une machine donnée offre.

## Processeur et mémoire

| Élément | Dans l'image | Vu |
|---|---|---|
| x86-64, Intel ou AMD | noyau NixOS, microcode Intel et AMD mis à jour au démarrage | VM (le processeur du coureur de la CI, virtualisé) |
| Virtualisation matérielle (VT-x, AMD-V) | modules `kvm-intel`, `kvm-amd`, `vhost_vsock` ; sans elle, le niveau 2 d'isolation (microVM Firecracker) est refusé et dit | hôte CI (niveau 2 joué) ; VM sans KVM imbriqué : refus vérifié |
| Mémoire | 8 Gio au minimum pour le bureau et un modèle local ; en dessous, l'installeur le dit | VM du bureau à 4 Gio |
| Disque | 80 Gio au minimum : deux racines de 24, 32 d'état chiffré, le reste en données chiffrées ; SATA (chipsets AMD, Intel, NVIDIA, VIA, SiI de 2010 et après), NVMe, USB (UHCI à xHCI), lecteurs de cartes SDHCI et Realtek dans l'initrd | VM (virtio) ; disque en boucle pour l'installeur ; installation complète et redémarrage avec phrase de passe dans QEMU le 15 septembre 2026 |

## Modèles locaux sur processeur

Vus sur le coureur de la CI (quatre cœurs x86-64, sans carte), travail « Poids du catalogue
servis (réels) » : chaque poids tiré de Hugging Face par egress, vérifié par son empreinte,
servi par le routeur épinglé et interrogé. Débits et réponses de `c44a349` (fenêtre 4 096) ;
mémoire à la fenêtre 2 048 : l'estimation de `providers::memory` (ADR 0047) face à la mémoire
résidente de l'instance du moteur une fois la réponse rendue — avec projection des poids
(`89b1b3a`, dont la part anonyme entre parenthèses) puis sans, comme l'image les charge
désormais (`227a63e`).

| Entrée du catalogue | Taille | Réponse | Génération | Mémoire estimée | Résidente projetée (anonyme) | Résidente sans projection | Vu |
|---|---|---|---|---|---|---|---|
| `qwen3-8b-q4` (Qwen3 8B, Q4_K_M) | 5,0 Go | « bonjour », 2,8 s | 7,4 tokens/s (relevé sur `4ecf15c`) | 5,8 Go | 8,8 Go (3,7 Go) | 5,5 Go | hôte CI |
| `granite-3.3-2b-q4` (IBM Granite 3.3 2B) | 1,5 Go | « Paris. », 2,5 s | 21 tokens/s | 2,0 Go | 3,0 Go (1,5 Go) | 1,9 Go | hôte CI |
| `smollm2-1.7b-q4` (SmolLM2 1.7B) | 1,1 Go | « Paris », 1,2 s | 31 tokens/s | 1,8 Go | 2,4 Go (1,3 Go) | 1,6 Go | hôte CI |
| `phi-3-mini-q4` (Phi-3 mini 4k) | 2,4 Go | « Paris », 0,9 s | 13 tokens/s | 3,5 Go | 4,6 Go (2,1 Go) | 3,3 Go | hôte CI |
| `llama-3.2-3b-q4` (Llama 3.2 3B) | 2,0 Go | « Paris. », 2,2 s | 17 tokens/s | 2,7 Go | 3,9 Go (1,8 Go) | 2,5 Go | hôte CI |
| `qwen3-1.7b-q8`, `qwen3-0.6b-q8` (modèles du relais) | 1,8 et 0,6 Go | missions réelles | — | — | — | — | VM (mission locale sous NixOS) |
| Tout modèle, sur carte (Vulkan) | — | — | — | — | — | — | non |

Projetés, les poids restaient résidents deux fois : llama.cpp recopie depuis la projection les
tenseurs réarrangés pour le processeur et garde celle-ci (source épinglée, ADR 0047). L'image
les charge désormais sans projection (`--load-mode none`) : la mémoire résidente baisse de 28 à
38 %, et l'estimation la dépasse de 5 à 10 %. Mémoire minimale : Qwen3 8B à 4 096 tokens
demande environ 6,1 Go, que 8 Gio tiennent en laissant 1,5 Gio au système. La VRAM n'est pas
mesurée.

Concurrence et annulation (`dd5ce52`, SmolLM2, une place par instance comme l'image) : deux
requêtes simultanées aboutissent (1,3 s et 1,9 s) ; une génération abandonnée par son client
libère l'instance, et la requête suivante répond en 0,5 s.

## Affichage

| Carte | Pilote | Ce que ça donne | Vu |
|---|---|---|---|
| AMD Radeon GCN 1 et 2 (HD 7000, HD 8000, R7/R9 200) | `amdgpu` forcé (`si_support`, `cik_support`) | Vulkan par RADV : le bureau et les modèles locaux accélérés (ADR 0032, 0037) | non |
| AMD Radeon GCN 3 et après, RDNA | `amdgpu` | Vulkan par RADV | non |
| Intel HD/Iris/Xe (Broadwell et après) | `i915` / `xe`, `intel-media-driver` | Vulkan par ANV | non |
| NVIDIA | `nouveau` (pilote libre du noyau) | affichage ; Vulkan seulement pour les cartes que NVK couvre ; **pas de pilote propriétaire** (incompatible avec le verrouillage du noyau visé) | non |
| Aucune carte reconnue, ou carte sans nœud de rendu | — | rendu logiciel (llvmpipe) : le bureau tourne, lentement ; les modèles locaux sur processeur | VM (c'est le mode de la CI) ; système installé sur `bochs-drm` dans QEMU le 15 septembre 2026, bureau affiché une fois le rendu logiciel permis (`16e8bab`) |

Le rendu et la consommation de la surface sur une carte réelle restent à mesurer (ADR 0037).

## Réseau

| Élément | Dans l'image | Vu |
|---|---|---|
| Filaire (Intel, Realtek, Broadcom, Atheros…) | pilotes du noyau ; micrologiciels redistribuables | VM (virtio) |
| Wi-Fi (Intel, Realtek, Atheros, Broadcom, MediaTek…) | pilotes du noyau ; micrologiciels redistribuables (`linux-firmware`) ; NetworkManager, `nmtui` sur la clé | non |
| Bluetooth | micrologiciels présents ; aucun service configuré | non |
| Pare-feu | aucun port ouvert ; toute sortie des agents par `egress` | VM |

## Son, micro, parole

| Élément | Dans l'image | Vu |
|---|---|---|
| Carte son (HDA Intel/AMD, USB) | PipeWire, PulseAudio par PipeWire | VM sans carte son : la parole est prouvée avec des fichiers |
| Micro | entrée PipeWire ; l'écoute de l'atelier et de `prophet listen` (Whisper local) | non (fichiers seulement) |
| Voix | Piper local | VM (fichier produit et vérifié) |

## Micrologiciel et sécurité matérielle

| Élément | Dans l'image | Vu |
|---|---|---|
| UEFI | systemd-boot, sans éditeur | VM (OVMF) |
| BIOS / CSM | GRUB sur la partition d'amorçage BIOS, toujours créée | VM (SeaBIOS) ; installé et redémarré ainsi dans une VM QEMU le 15 septembre 2026 |
| Secure Boot | **non pris en charge** : chargeur non signé ; l'installeur le dit s'il le voit activé | — |
| TPM 2 | `tpm2-tss` ; enrôlement de la phrase de passe après l'installation (`systemd-cryptenroll`) | non |
| Verrouillage du noyau | annoncé, **inactif** (voir `docs/installation.md`) | VM (constaté inactif) |

## Ce qui n'est pas dans l'image

- Pilotes propriétaires (NVIDIA, Broadcom `wl`) : incompatibles avec l'objectif de noyau
  verrouillé ; une machine qui en dépend l'ajoute dans sa propre configuration, en
  connaissance de cause.
- Imprimantes, scanners, tablettes : rien de configuré.
- Processeurs ARM : l'image est x86-64 seulement.

## Comment faire avancer ce document

Chaque « non » se lève par un essai sur une machine réelle : démarrer la clé, lire le relevé
de l'installeur, et rapporter ici la carte, le pilote lié et ce que le bureau a donné. Un
rapport dans `docs/reports/` avec le relevé et une capture suffit.
