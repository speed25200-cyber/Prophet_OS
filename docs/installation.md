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

### Savoir qu'une ISO est bonne avant de formater son disque

L'onglet *Actions* garde aussi les images produites par des exécutions **partiellement** vertes,
et rien sur le fichier ne les distingue. Avant de prendre une image, ouvrez l'exécution qui l'a
produite et vérifiez que **tous** ses travaux sont verts, à la seule exception de celui qui porte
« Question ouverte » dans son nom :

| Travail | Ce qu'il garantit |
|---|---|
| Installeur sur disque en boucle | l'installeur formate, chiffre et monte pour de vrai, et refuse ce qu'il doit refuser — mauvaise confirmation, disque trop petit, mot de passe trop court |
| Construire l'ISO | l'image se construit, et chacun de ses fichiers est celui de la révision gravée |
| Construire le système installé | chacun des sept services pointe vers un programme qui existe, **et** `nixos-install` pose réellement ce système sur la disposition que l'installeur crée |
| Voir l'image démarrer | la clé USB démarre jusqu'à l'invite |
| Les sept services sous systemd | les daemons tournent sous leur utilisateur, avec leur durcissement, et `sandboxd` isole vraiment |
| **Le système installé démarre** | secours graphique sans interruption de la connexion, puis démarrage du système : chargeur d'amorçage, paramètres du noyau, compte ouvrable, session sur `tty1`, et les sept services vus par le propriétaire depuis sa session |
| Question ouverte : la racine en lecture seule | rien — c'est une **question**, pas une garantie, et elle ne part plus qu'à la demande. Sa réponse est « non » depuis le 12 septembre 2026 : voir `image/tests/racine-en-lecture-seule.nix`. Pour la reposer, déclenchez le workflow à la main en cochant « Rejouer l'expérience de la racine en lecture seule » |

Les six premiers partent à chaque poussée et doivent être verts. Le septième ne part qu'à la
demande : sa réponse est connue, et un rouge permanent dans un tableau qu'on demande de lire avant
de graver une image n'apprend rien — il entraîne à ignorer le rouge.

Le sixième est le seul qui réponde à ce qui compte une fois le disque effacé : ce que la clé
installe démarre-t-il ? Une image produite par une exécution où ce travail manque ou échoue n'a
été vue démarrer que depuis la clé, ce qui est une autre configuration.

`tools/verifier-la-doc-des-travaux.sh` vérifie que ce tableau ne prend pas de retard sur le
workflow : un travail ajouté sans être décrit ici ferait croire qu'une image est bonne alors
qu'un contrôle manque.

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
| Secure Boot | **désactivé** | Prophet OS ne signe pas encore son chargeur d'amorçage. C'est une dette connue, écrite dans `image/modules/immutable.nix` : elle demande d'ajouter `lanzaboote` aux entrées du flake et d'enrôler les clés depuis l'installeur. Tant qu'elle n'est pas payée, le micrologiciel refuserait de démarrer. |
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
demande ensuite deux choses, chacune deux fois : la **phrase de passe du chiffrement**, puis le
**mot de passe de votre compte**.

Ce sont deux secrets différents, et il faut les deux. La phrase ouvre les volumes chiffrés au
démarrage ; le mot de passe ouvre votre session et sert à `sudo`. Le compte `root` reste
verrouillé, et le chargeur d'amorçage n'a pas d'éditeur : il n'y a donc pas de démarrage de
secours, et une installation sans mot de passe utilisable donnerait une machine à réinstaller.
L'installeur refuse plutôt que de la produire — huit caractères au minimum.

Le mot de passe n'est écrit nulle part en clair : seul son haché est posé sur le disque installé,
en `0600`. Ce dépôt est public, et un mot de passe écrit dans une configuration versionnée est un
mot de passe connu.

Comptez vingt minutes à une heure : le système est téléchargé depuis `cache.nixos.org` et
assemblé sur place.

Pour voir la disposition qu'il produirait sans rien installer :

```sh
sudo prophet-installer --disque /dev/VOTRE_DISQUE --jusqu-au-montage
```

## Ce que cette version ne tient pas encore

**La racine n'est pas en lecture seule.** Elle devait l'être — c'est écrit partout dans la
conception — et l'expérience du 12 septembre 2026 a montré qu'une machine ainsi montée ne garde
même pas un interpréteur vivant : l'activation de NixOS écrit `/etc/passwd`, `/etc/shadow` et tout
l'arbre de `/etc` à chaque démarrage, et seuls `/home` et `/var/lib/prophet` sont sur des volumes
séparés.

Livrer cela vous aurait donné, après avoir effacé Windows, un PC qui ne démarre pas — sans
rattrapage, le chargeur d'amorçage étant configuré sans éditeur. La racine est donc inscriptible,
et l'immuabilité reste une promesse à tenir.

**Le verrouillage du noyau n'est pas actif non plus.** `lockdown=integrity` et
`module.sig_enforce=1` sont bien passés au noyau — le test le lit sur `/proc/cmdline` — mais le
noyau démarre avec `lsm=landlock,yama,bpf`, où `lockdown` ne figure pas, et
`/sys/kernel/security/lockdown` n'existe pas. Un paramètre que le noyau ignore ne protège rien, et
le croire posé est pire que de savoir qu'il ne l'est pas. C'est écrit dans `docs/STATUS.md` ; ce
n'est pas un défaut de démarrage, c'est un durcissement annoncé qui n'a pas lieu.

Ce qui **est** tenu : les deux emplacements A/B avec bascule automatique, le chiffrement LUKS2 des
données et de l'état, Landlock et seccomp, les sept services durcis sous leur propre compte, et
`egress` qui refuse toute requête sans jeton.

## 4. Premier démarrage

Retirez la clé, redémarrez. La phrase de passe vous est demandée pour ouvrir les volumes chiffrés,
puis une invite de connexion apparaît.

Identifiant : **`prophet`**. Mot de passe : celui que vous avez choisi à l'installation. Ce compte
appartient à `wheel` — donc `sudo` — et à `prophet-system`, ce qui lui permet de parler aux sept
daemons.

```sh
prophet status                        # les services, l'isolation, les limites de la machine
prophet provider ls                   # quels clients sont là, et lesquels sont connectés
prophet provider login claude-code    # connecter votre abonnement
```

`prophet provider ls` d'abord : il dit quels clients officiels sont **réellement présents** sur
cette machine. L'image embarque Claude Code et Gemini CLI tels quels, quand nixpkgs les fournit —
un client peut y être renommé ou en disparaître, et l'image se construit alors sans lui plutôt que
de refuser. La liste dit la vérité dans les deux cas ; `prophet provider login` vous enverrait
sinon lancer une commande qui n'existe pas.

Codex CLI n'y est pas encore : son nom dans nixpkgs est un mot générique, et livrer un binaire
étranger sous un nom auquel l'OS fait confiance serait pire que de ne rien livrer. Vous pouvez
l'installer vous-même ; `prophet provider ls` le verra.

Claude Code est distribué sous les conditions de son éditeur — nixpkgs le marque « unfree ».
L'image l'autorise **nommément**, et non par un `allowUnfree` global qui laisserait entrer
n'importe quel paquet propriétaire sans que personne ne s'en aperçoive. La liste est dans
`image/modules/prophet.nix` et se lit en une ligne ; la retirer donne une image sans aucun paquet
propriétaire, où `provider ls` dira simplement que le client est absent.

Prophet OS ne lit jamais les fichiers d'identifiants de ces clients. Il monte leur répertoire de
session dans leur sandbox, et c'est tout — vous vous connectez avec `claude login` comme sur
n'importe quelle machine.

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
- **Cette image démarre — cela a été vu, pas supposé.** L'intégration continue la démarre en
  machine virtuelle UEFI à chaque construction : le micrologiciel la trouve, le noyau part,
  l'espace utilisateur monte, et l'écran d'accueil qui dit quoi taper apparaît. Ce qu'elle ne dit
  pas : si *votre* carte graphique, *votre* carte réseau et *votre* micrologiciel s'entendent avec
  elle. C'est la seule inconnue qui reste, et elle ne peut être levée que chez vous.

Ce dernier point mérite d'être pesé. Si vous voulez réduire le risque : essayez d'abord l'ISO dans
une machine virtuelle (VirtualBox, VMware ou Hyper-V, avec l'UEFI activé et un disque de 80 Gio),
puis sur le vrai PC une fois que vous l'aurez vue démarrer chez vous.

## Revenir à Windows

Il n'y a pas de retour en arrière : l'installation efface la table de partitions. Prévoyez une
clé d'installation Windows et votre licence — elle est presque toujours dans le micrologiciel sur
un PC de marque, et se retrouve donc toute seule — avant de commencer.
