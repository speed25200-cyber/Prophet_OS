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
| Disque | 80 Gio au minimum : deux racines de 24, 32 d'état chiffré, le reste en données chiffrées ; SATA (chipsets AMD, Intel, NVIDIA, VIA, SiI de 2010 et après), NVMe, USB (UHCI à xHCI), lecteurs de cartes SDHCI et Realtek dans l'initrd | VM (virtio) ; disque en boucle pour l'installeur |

## Affichage

| Carte | Pilote | Ce que ça donne | Vu |
|---|---|---|---|
| AMD Radeon GCN 1 et 2 (HD 7000, HD 8000, R7/R9 200) | `amdgpu` forcé (`si_support`, `cik_support`) | Vulkan par RADV : le bureau et les modèles locaux accélérés (ADR 0032, 0037) | non |
| AMD Radeon GCN 3 et après, RDNA | `amdgpu` | Vulkan par RADV | non |
| Intel HD/Iris/Xe (Broadwell et après) | `i915` / `xe`, `intel-media-driver` | Vulkan par ANV | non |
| NVIDIA | `nouveau` (pilote libre du noyau) | affichage ; Vulkan seulement pour les cartes que NVK couvre ; **pas de pilote propriétaire** (incompatible avec le verrouillage du noyau visé) | non |
| Aucune carte reconnue | — | rendu logiciel (llvmpipe) : le bureau tourne, lentement ; les modèles locaux sur processeur | VM (c'est le mode de la CI) |

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
| BIOS / CSM | GRUB sur la partition d'amorçage BIOS, toujours créée | VM (SeaBIOS) |
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
