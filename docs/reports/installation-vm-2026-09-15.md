# L'ISO installée et démarrée dans une machine virtuelle — 15 septembre 2026

L'humain a demandé d'installer l'OS et de l'essayer. Fait dans QEMU 11.1 avec KVM, sous WSL,
sur la machine de développement (AMD Ryzen 7 5825U, 15 Gio, WSL limité à 7 Gio). C'est le
premier passage complet de bout en bout hors de l'intégration continue : ISO → installeur →
disque chiffré → redémarrage sur le disque seul → session du bureau.

## Ce qui a été fait

| Étape | Résultat |
|---|---|
| ISO du run `34897838743` (révision `9abc822`), 1,67 Go, empreinte SHA-256 vérifiée | conforme |
| Démarrage de l'ISO en UEFI (OVMF du magasin Nix, QEMU 11.1) | **ne démarre pas** : ni écran ni série, avec ou sans KVM ; la CI démarre la même ISO en UEFI avec l'OVMF d'Ubuntu — c'est cet OVMF-là qui est en cause, pas l'image |
| Démarrage de l'ISO sous SeaBIOS | 30 s jusqu'à l'invite ; ISOLINUX, session `nixos` ouverte d'elle-même sur la console série |
| `sudo prophet-installer --disque /dev/vda` : relevé de la machine, confirmation, phrase de passe, mot de passe, six partitions, deux volumes LUKS2, formatage, montage | 20 s |
| Copie du dépôt, mot de passe haché, détection du matériel, relevé, GRUB | voir les trois fautes ci-dessous |
| `nixos-install` avec la configuration `prophet-ci` et la fermeture construite sur l'hôte servie en cache local | 13 min |
| Redémarrage sur le disque seul : GRUB, noyau, demande de la phrase de passe (`prophet-state`), réutilisée pour `prophet-home` | oui |
| Écran de connexion « Prophet OS » (utilisateur « Propriétaire de cette machine », session « Prophet OS »), mot de passe | oui |
| Session du bureau | **morte aussitôt** la première fois, retour à l'écran de connexion sans un mot (faute 3) ; avec le rendu logiciel permis : supervision à l'écran, lanceur, terminal |
| Console texte (tty2) : `prophet status`, sept services `active`, aucun service en échec, relevé de l'installeur relu | oui |

Captures : `session-logiciel-55s` (la supervision sur le système installé), `luks` (la demande
de phrase), `connexion` et `mot-de-passe` (l'écran de connexion), `tty2-status`,
`tty2-services` (dans le dossier de travail de la session ; à verser ici si utile).

## Trois fautes bloquantes, invisibles pour la CI

Le travail « Installeur sur disque en boucle » s'arrêtait au montage ; le test du bureau pose
lui-même sa variable de rendu. Les trois fautes vivaient juste derrière.

1. **La copie du dépôt prenait le lien, pas le contenu** (`762e30d`). Sur le support,
   `/etc/prophet/source` est un lien vers le magasin Nix ; `cp -r` copiait le lien, et le
   `chmod -R u+w` qui suit échouait sur le magasin en lecture seule. Sur un PC : disque formaté,
   rien d'installé. Corrigé par `cp -rL --no-preserve=mode`.
2. **La carte graphique du relevé n'existait plus à l'étape suivante** (`8dc0c2e`, faute de la
   veille) : posée dans une fonction exécutée en sous-shell, `CARTE` était vide et
   `set -u` tuait l'installeur après la détection du matériel. La sonde Vulkan se fait avant
   le relevé.
3. **Le bureau ne démarrait pas sans accélération graphique** (`16e8bab`). Sans nœud de rendu
   (`/dev/dri/renderD*`), EGL retombe sur le rendu logiciel et wlroots refuse de créer son
   moteur tant que `WLR_RENDERER_ALLOW_SOFTWARE` n'est pas posée ; la session mourait en
   silence. La session la pose ; avec une vraie carte, rien ne change. Vérifié dans la même
   VM : la supervision s'affiche.

Et deux choses que la CI ne montrait pas non plus : `ping` pour sonder le cache Nix échoue
partout où l'ICMP est bloqué (le coureur de la CI, bien des réseaux d'entreprise) — sonde en
HTTPS désormais (`21cd46a`) ; et l'installeur a maintenant `--sans-installation`, que la CI
joue sur un disque neuf jusqu'à la veille de `nixos-install` (`770e4ed`).

## Ce que cet essai ne prouve pas

- **Le matériel réel** : toujours rien. La VM a une carte `bochs-drm` sans Vulkan, un réseau
  virtio, ni son ni micro ; le relevé de l'installeur le dit, et `prophet status` aussi.
- **L'UEFI** : installé et démarré sous SeaBIOS (GRUB), le chemin « PC sans UEFI » de l'ADR 0032.
  La CI démarre l'installé en UEFI ; ici l'OVMF disponible ne démarrait pas.
- **La configuration complète** : `prophet-ci`, sans la suite bureautique ni le modèle local
  (la fermeture complète pèse 21 Gio et le réseau de la VM donne 0,7 Mio/s : un premier essai
  a été coupé après 58 min quand le disque de l'hôte s'est rempli). Le niveau 2 d'isolation
  n'est donc pas dans ce système installé (`prophet status` : niveau 0 seul).
- **La mise à jour A/B, la voix, les clients avec un compte** : non exercés.

## Comment refaire

Les scripts vivent dans le dossier de travail de la session (`scratchpad/vm/`) : `build-cache.sh`
(construit la fermeture sur l'hôte, l'exporte en image ext4), `vm-install.py` (pilote
l'installeur par la console série, ajuste l'installeur de l'ISO par un script sed), `vm-boot.sh`
(démarre le disque installé), `vm-mon.py` (clavier, capture d'écran, OCR), `vm-test.py` et
`tty2b.sh` (phrase de passe, connexion, requêtes sur tty2). À verser dans `tools/` une fois
débarrassés de leurs chemins fixes.
