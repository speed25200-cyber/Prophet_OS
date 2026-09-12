# Installer Prophet OS sur un PC

Ce document suppose que vous partez d'un PC sous Windows dont vous acceptez de perdre tout le
contenu. Il dit aussi, à la fin, ce qui ne marche pas encore : un système qu'on installe en
effaçant son disque doit annoncer ses manques avant, pas après.

## Ce qu'il vous faut

- Une clé USB d'au moins 2 Gio, dont le contenu sera effacé.
- Un PC avec **UEFI** (tout PC livré avec Windows 10 ou 11 en a un), **80 Gio** de disque au
  minimum, et une connexion réseau au moment de l'installation.
- Le fichier `prophet-os-installeur-*.iso`, produit par le travail « Support d'amorçage » de
  l'intégration continue. Il se télécharge depuis l'onglet *Actions* du dépôt, dans les artefacts
  de la dernière exécution réussie — artefact `prophet-os-iso`, environ 1,4 Gio, accompagné de son
  empreinte. GitHub les livre dans une archive ZIP qu'il faut d'abord extraire.

Vérifiez l'empreinte du fichier téléchargé — elle est publiée à côté de lui. Dans PowerShell :

```powershell
Get-FileHash .\prophet-os-installeur-*.iso -Algorithm SHA256
```

## 1. Écrire l'image sur la clé

Depuis Windows, avec [Rufus](https://rufus.ie) ou
[balenaEtcher](https://etcher.balena.io) : sélectionnez l'image, sélectionnez la clé, écrivez.

Si Rufus propose un mode, prenez **image DD** plutôt que ISO : l'image est hybride et se copie
telle quelle.

## 2. Préparer le micrologiciel

Redémarrez et entrez dans le menu du micrologiciel — souvent <kbd>F2</kbd>, <kbd>F10</kbd>,
<kbd>Suppr</kbd> ou <kbd>F12</kbd> selon la marque, pressé dès l'allumage.

Trois réglages :

| Réglage | Valeur | Pourquoi |
|---|---|---|
| Secure Boot | **désactivé** | Prophet OS ne signe pas encore son chargeur d'amorçage. C'est une dette connue, notée en ADR-0006 ; tant qu'elle n'est pas payée, le micrologiciel refuserait de démarrer. |
| CSM / Legacy BIOS | **désactivé** | L'installeur exige un démarrage UEFI et refusera de continuer sinon. |
| Ordre de démarrage | la clé USB en premier | — |

Si votre PC a le *Fast Startup* de Windows, désactivez-le avant, ou éteignez la machine avec
`shutdown /s /t 0` : sinon Windows laisse le disque dans un état verrouillé.

## 3. Installer

Démarrez sur la clé. L'écran d'accueil rappelle les trois commandes.

```sh
# 1. Le réseau, si vous n'êtes pas en filaire
nmtui

# 2. Repérer le disque cible
lsblk

# 3. Installer
sudo prophet-installer --disque /dev/nvme0n1
```

Remplacez `/dev/nvme0n1` par ce que `lsblk` affiche pour **votre** disque. Un portable récent a
généralement un seul disque NVMe ; une tour peut en avoir plusieurs, et se tromper de disque est
la seule erreur irréparable de cette procédure.

L'installeur vous montre ce qu'il va effacer, nomme les systèmes d'exploitation qu'il détecte, et
**vous demande de recopier le nom du disque**. Rien n'est écrit avant cette confirmation. Il
demande ensuite une phrase de passe pour le chiffrement, deux fois.

Comptez vingt minutes à une heure : le système est téléchargé depuis `cache.nixos.org` et
assemblé sur place.

Pour voir la disposition qu'il produirait sans rien installer :

```sh
sudo prophet-installer --disque /dev/VOTRE_DISQUE --jusqu-au-montage
```

## 4. Premier démarrage

Retirez la clé, redémarrez. La phrase de passe vous est demandée pour ouvrir les volumes chiffrés.

```sh
prophet status                        # les services, l'isolation, les limites de la machine
prophet provider login claude-code    # connecter votre abonnement
```

`prophet status` commence par la liste des sept services et dit lesquels répondent. C'est la
première chose à regarder : un système dont `capd` est muet affiche une isolation parfaite et un
journal intact, et ne peut rien faire. `docs/components/` dit ce que chaque absence emporte avec
elle.

Pour ne plus taper la phrase à chaque démarrage, enrôlez-la dans le TPM :

```sh
systemd-cryptenroll --tpm2-device=auto /dev/disk/by-label/prophet-home-luks
```

Gardez la phrase malgré tout : elle reste le seul recours si la carte mère change.

## Ce que cette version ne fait pas encore

Dit franchement, parce que vous aurez effacé un disque pour l'essayer.

- **Pas de Secure Boot.** Le chargeur n'est pas signé ; il faut désactiver Secure Boot dans le
  micrologiciel. Cela réduit la garantie que ce qui démarre est bien ce qui a été installé.
- **La surface montre le système, mais peu de choses au début.** L'environnement graphique démarre
  seul sur le premier écran : un champ de courants où chaque tâche est un filament, sans bureau ni
  fenêtres. Il lit les tâches réelles, les décisions en attente et ce que la machine sait isoler.
  Tant que vous n'avez lancé aucune tâche, le champ est donc vide — c'est normal, et c'est
  préférable à une démonstration qui ressemblerait à un système en marche.
- **Les sept services démarrent pour la première fois sur votre machine.** Jusqu'au 12 septembre
  au soir, ils étaient déclarés sans exister : l'image installée aurait démarré avec sept unités en
  échec. Ils existent désormais, chacun avec un test qui lance le programme et lui parle. Ils n'ont
  jamais tourné ensemble ailleurs que dans ces tests.
- **La bascule A/B n'est pas exercée.** Les deux racines sont créées et le système sait démarrer
  sur la première ; le service de mise à jour qui écrit dans la seconde est une esquisse.
- **Le niveau 2 d'isolation exige des images d'invité** qui ne sont pas dans l'image installée.
  `prophet status` dira ce qui manque. Les niveaux 0 et 1 fonctionnent, et ont été vérifiés sur
  du matériel réel.
- **Aucune démonstration en machine virtuelle n'a été faite** (M9-T6). L'ISO est construite et son
  installeur est exercé automatiquement sur un disque en boucle, mais personne n'a encore vu cette
  image démarrer de bout en bout sur une vraie machine. Vous serez le premier.

Ce dernier point mérite d'être pesé. Si vous voulez réduire le risque : essayez d'abord l'ISO dans
une machine virtuelle (VirtualBox, VMware ou Hyper-V, avec l'UEFI activé et un disque de 80 Gio),
puis sur le vrai PC une fois que vous l'aurez vue démarrer.

## Revenir à Windows

Il n'y a pas de retour en arrière : l'installation efface la table de partitions. Prévoyez une
clé d'installation Windows et votre licence — elle est presque toujours dans le micrologiciel sur
un PC de marque, et se retrouve donc toute seule — avant de commencer.
