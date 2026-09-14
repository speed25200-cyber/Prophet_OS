# Installer Prophet OS sur un PC

**Version de développement.** Le bureau et sa variante installée passent leur parcours en
machine virtuelle, dans l'intégration continue ; rien n'a encore démarré sur un vrai PC. ChatGPT
conserve un échec de compatibilité connu. Les résultats d'une ancienne révision ne valident pas l'image
courante. Consultez [STATUS.md](STATUS.md) et [FRONTIER.md](FRONTIER.md) avant tout essai.

Ce document suppose que vous partez d'un PC sous Windows dont vous acceptez de perdre tout le
contenu. Il dit aussi, à la fin, ce qui ne marche pas encore : un système qu'on installe en
effaçant son disque doit annoncer ses manques avant, pas après.

## Ce qu'il vous faut

- Une clé USB d'au moins 2 Gio, dont le contenu sera effacé.
- Un PC x86-64, avec **UEFI** (tout PC livré avec Windows 10 ou 11 en a un) **ou un simple
  BIOS** (un PC de 2012 démarre aussi : l'installeur y pose GRUB au lieu de systemd-boot),
  **80 Gio** de disque au minimum, **8 Gio** de mémoire, et une connexion réseau au moment de
  l'installation, qui télécharge aussi le modèle local par défaut (Qwen3-1.7B, 1,83 Go, depuis
  huggingface.co).
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
| Construire le système installé | chacun des sept services pointe vers un programme qui existe, le matériel détecté par `nixos-generate-config` et le mode BIOS composent avec la configuration, **et** `nixos-install` pose réellement ce système sur la disposition que l'installeur crée |
| Voir l'image démarrer | la clé USB démarre jusqu'à l'invite, en UEFI (OVMF) **et** sans UEFI (SeaBIOS) |
| Les sept services sous systemd | les daemons tournent sous leur utilisateur, avec leur durcissement, et `sandboxd` isole vraiment |
| **Le système installé démarre** | secours du kiosque historique, puis session du nouveau bureau : systemd-boot, paramètres du noyau, connexion PAM, supervision sous le compte humain, sept services, fenêtres des clients, fichiers, presse-papiers et reprise après verrouillage |
| Le système installé démarre sans UEFI | la même configuration, amorcée par GRUB sous SeaBIOS : paramètres du noyau et sept services, pour les PC qui n'ont qu'un BIOS |
| Question ouverte : la racine en lecture seule | rien — c'est une **question**, pas une garantie, et elle ne part plus qu'à la demande. Sa réponse est « non » depuis le 12 septembre 2026 : voir `image/tests/racine-en-lecture-seule.nix`. Pour la reposer, déclenchez le workflow à la main en cochant « Rejouer l'expérience de la racine en lecture seule » |

Les sept premiers partent à chaque poussée et doivent être verts. Le huitième ne part qu'à la
demande : sa réponse est connue, et un rouge permanent dans un tableau qu'on demande de lire avant
de graver une image n'apprend rien — il entraîne à ignorer le rouge.

Le workflow des composants comporte en plus le test **ChatGPT sous NixOS (compatibilité)**.
Son erreur Fontconfig reste bloquante pour annoncer ChatGPT pleinement compatible, même si
le client affiche son écran de connexion dans le test du bureau.

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
| CSM / Legacy BIOS | **indifférent** | L'installeur pose systemd-boot si la clé a démarré en UEFI, GRUB sinon. Sur une machine qui a les deux, préférez l'UEFI : systemd-boot y est sans éditeur, et Secure Boot pourra s'y ajouter. |
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

L'installeur dit d'abord **ce qu'il voit de la machine** — processeur et mémoire, virtualisation
(KVM, sans laquelle le niveau 2 d'isolation reste refusé), carte graphique et le pilote que le
noyau lui a lié (Vulkan ou rendu logiciel), interfaces réseau filaires et Wi-Fi avec leur pilote
et leur état, carte son et présence d'une entrée micro, Secure Boot, TPM. Une ligne rouge est un
manque : c'est le moment de renoncer, tant que Windows est encore là, si l'écran, le réseau ou le
micro ne sont pas reconnus. Le système installé a les mêmes pilotes que la clé et verra la même
chose ; ce relevé est gardé avec la machine (`image/machine/inventaire.txt` dans sa source).
Puis il vous montre ce qu'il va effacer, nomme les systèmes d'exploitation qu'il détecte, et
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

Comptez vingt minutes à une heure : le système est téléchargé depuis `cache.nixos.org`, le
modèle local depuis `huggingface.co`, et le tout est assemblé sur place. Au premier démarrage,
l'agent local répond sans compte ni clé d'API ; un abonnement Claude Code, Codex ou Gemini s'y
ajoute par `prophet provider login`.

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

Les deux emplacements A/B existent, mais la mise à jour suivie d'un retour automatique après
échec reste à exercer. L'installeur teste le formatage et le montage LUKS2 sur disque factice ;
le parcours complet de déverrouillage au redémarrage reste distinct. Les tests des services
exercent les comptes, le durcissement et certains chemins d'isolation. Ces preuves ne couvrent
pas encore toutes les méthodes IPC ni tout programme lancé comme agent.

## 4. Premier démarrage

Retirez la clé, redémarrez. La phrase de passe vous est demandée pour ouvrir les volumes chiffrés,
puis l'écran de connexion **Prophet OS** est prévu sur le premier terminal virtuel.

Identifiant : **`prophet`**. Mot de passe : celui que vous avez choisi à l'installation. Ce compte
appartient à `wheel` — donc `sudo` — et à `prophet-system`, ce qui lui permet de parler aux sept
daemons. La session Wayland ouvre la supervision et permet d'utiliser plusieurs applications.
Le [choix de session](adr/0021-session-humaine-wayland.md) est implémenté et son parcours réussit
en VM. Le [rapport](reports/bureau-humain-2026-09-13.md) distingue ce résultat de la validation
encore ouverte sur disque installé.

| Action | Accès |
|---|---|
| Ouvrir une application | **Super + Espace**, ou « Prophet » dans la barre |
| Terminal dans votre répertoire personnel | **Super + Entrée** |
| Fichiers du projet documentaire | **Super + E** |
| Supervision, atelier, dialogue, recherche | **Super + 1**, **2**, **3**, **4** |
| Déplacer une fenêtre vers un espace | **Super + Maj + 1**, **2**, **3**, **4** |
| Présenter l'espace en onglets / côte à côte | **Super + W** / **Super + B** |
| Fermer la fenêtre active | **Super + Maj + Q** |
| Verrouiller la session | **Super + L**, ou « Verrouiller » dans la barre |
| Fermer la session avec confirmation | **Super + Maj + E** |
| Console de secours | **Ctrl + Alt + F2** ; retour à l'écran graphique avec **Ctrl + Alt + F1** |

Le lanceur propose **ChatGPT**, **Claude Code**, **Codex**, les fichiers, le navigateur et
**Outils** : les programmes que l'atelier logiciel a écrits à votre demande et que vous avez
publiés dans `~/Documents/Prophet/outils` (un fichier `.py` ou `.sh`, ou un dossier avec un
`main.py`), ouverts dans un terminal qui reste affiché, sous votre identité — ils sont à vous
depuis que vous les avez examinés et publiés ; l'agent, lui, ne les exécute qu'en microVM.
`prophet-ouvrir outils --liste` les nomme, `prophet-ouvrir outils <nom>` en ouvre un.
Claude Code et Codex s'ouvrent dans `~/Documents/Prophet`, avec des profils privés propres
au pilote et au propriétaire. Leur espace passe en onglets pour conserver une largeur lisible.
Le terminal reste ouvert après l'arrêt du client afin de garder son diagnostic à l'écran ;
fermez cette fenêtre avant de relancer le client. Connectez-vous dans leurs interfaces officielles. Prophet
n'inspecte pas leurs fichiers d'identifiants. Le lancement interactif n'est pas encore une
intégration d'agent contrôlée par capd et sandboxd.

La supervision utilise les services sous votre identité. Fermer sa fenêtre ne supprime pas
les missions du service ; « Supervision » dans le lanceur la rouvre. Le modèle local nécessite
des poids installés : consultez le [guide du moteur](../crates/providers/README.md).

```sh
prophet status                        # les services, l'isolation, les limites de la machine
prophet provider ls                   # quels clients sont là, et lesquels sont connectés
prophet provider login claude-code    # connecter votre abonnement
```

Une fois connectés, Claude Code et Codex sont les modèles principaux des missions : le
service les propose en tête de chaque contexte et le modèle local ne sert que de secours.
Une mission se confie à l'un d'eux comme à un modèle, par son identifiant :

```sh
prophet task options                                   # contextes, modèles, clients connectés
prophet task prepare --profile documents --model codex "Résumer les notes de la semaine"
prophet task prepare --profile atelier --model claude-code@opus "Concevoir l'outil de tri"   # un palier de modèle, au choix
prophet task start <id>                                # Codex est lancé dans la mission, sous votre identité
prophet task inspect <id>                              # son avancement, puis son résultat
prophet task cancel <id>                               # coupe : la séance est conclue, le client tué
prophet cap approvals                                  # ce qui attend votre décision, avec le motif du modèle (ADR 0041)
prophet cap approve <id> --scope task                  # accorder, pour toute la mission ; `prophet cap deny <id>` refuse
```

Un client non connecté est refusé à la préparation, en disant comment se connecter.

Dans un contexte, les rôles ont leurs paliers de modèles (ADR 0040) : la réflexion et le code
au palier `opus` de Claude Code, la relecture à Codex puis au palier `sonnet`, les étapes simples
au palier `haiku` — le lanceur passe le palier au client par son option de modèle. Pour en
changer, dans la configuration de la machine (`image/machine/`) :

```nix
prophet.localEngine.paliers = { reflect = "opus"; code = "opus"; review = "sonnet"; execute = "haiku"; };
```

Tout cela est prouvé avec des clients de remplacement ; le vrai client, lui, ne peut l'être que
par vous, une fois connecté. Une commande le fait, dans votre session, depuis le dépôt :

```sh
PROPHET_TEST_CLIENT=codex PROPHET_TEST_PILOT_STATE=$HOME/.local/state/prophet   cargo test -p agentd --test pilot -- --ignored needs_codex_login
```

Elle prépare une mission sur le client, le laisse la rejoindre, écrire un fichier par l'outil
`fs.write`, se retirer, et vérifie le résultat. Son journal dit ce que le client a fait ou
refusé.

`prophet provider ls` distingue clients présents, connexion et exécution agentique disponible.
Codex et Claude Code sont des dépendances obligatoires du nixpkgs épinglé. Gemini CLI est
également fourni lorsque ce nixpkgs le propose. Les sondes utilisent les commandes des clients,
sans déduire une connexion de la présence d'un fichier. Voir le [guide des pilotes](components/providers.md).

Claude Code est distribué sous les conditions de son éditeur — nixpkgs le marque « unfree ».
L'image l'autorise nommément dans sa liste de paquets propriétaires. Le paquet ChatGPT Linux
provient du `.deb` officiel, avec un environnement de compatibilité FHS. Son écran de connexion
a été observé en VM ; son renderer secondaire conserve une erreur Fontconfig. L'application
est expérimentale dans ce bureau et aucune session ChatGPT authentifiée n'est encore validée.

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
- **La supervision reste incomplète.** L'atelier présente les missions réelles, leur résultat
  et les versions des fichiers proposées. L'application approuvée de ces changements et leur
  annulation robuste ne sont pas encore raccordées. Les conversations restent en mémoire.
- **Le parcours sur disque installé passe en CI, pas encore sur un vrai PC.** Le test du bureau
  et sa variante installée réussissent la connexion, les fenêtres, le verrouillage et la
  reconnexion, en machine virtuelle. Aucun client cloud authentifié n'est validé : Claude Code
  et Codex n'ont jamais tourné dans une mission avec un vrai compte, seuls des clients de
  remplacement l'ont fait sur le même chemin.
- **La bascule A/B n'est pas exercée.** Les deux racines sont créées et le système sait démarrer
  sur la première ; le service de mise à jour qui écrit dans la seconde est une esquisse.
- **Le niveau 2 d'isolation (microVM) est dans l'image (ADR 0038), mais n'a tourné que sur
  l'hôte de la CI.** Il exige la virtualisation matérielle (KVM), qu'une machine virtuelle sans
  virtualisation imbriquée n'offre pas ; `prophet status` dit quels niveaux sont disponibles et
  ce qui manque sur la machine.
- **La matrice matérielle reste à établir.** Les démarrages en VM, UEFI comme BIOS, ne valident
  pas votre carte graphique, votre réseau ni votre micrologiciel. Le système installé embarque
  désormais les micrologiciels redistribuables et le matériel que l'installeur détecte, et met
  les Radeon HD 7000/8000 sous `amdgpu` pour avoir Vulkan (ADR 0032) ; que votre carte, votre
  Wi-Fi et votre BIOS s'en satisfassent reste à constater sur la machine — l'inventaire que
  l'installeur affiche avant d'effacer le disque le dit pour l'essentiel (pilote d'affichage,
  Vulkan, réseau, micro), depuis la clé, sans rien risquer. Les performances
  d'inférence GPU et les comparaisons avec les distributions prises en charge restent aussi à
  mesurer.

Ce dernier point mérite d'être pesé. Si vous voulez réduire le risque : essayez d'abord l'ISO dans
une machine virtuelle (VirtualBox, VMware ou Hyper-V, en UEFI ou en BIOS, avec un disque de 80 Gio),
puis sur le vrai PC une fois que vous l'aurez vue démarrer chez vous.

## Revenir à Windows

Il n'y a pas de retour en arrière : l'installation efface la table de partitions. Prévoyez une
clé d'installation Windows et votre licence — elle est presque toujours dans le micrologiciel sur
un PC de marque, et se retrouve donc toute seule — avant de commencer.
