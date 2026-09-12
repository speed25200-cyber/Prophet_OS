# Vérifier Prophet OS sur le serveur

Ce document dit comment faire tourner la vérification sur le serveur Hetzner, et pourquoi elle
ne part pas toute seule.

## Pourquoi un workflow, et pas une session

Une session Claude n'atteint pas cette machine. La politique de sortie de l'organisation refuse
l'hôte — le message exact est « request blocked: no rule or allowlist entry allows host ». Ce
refus vient de l'organisation, pas du serveur, et il ne se contourne pas : le faire serait
exactement ce que la politique cherche à empêcher.

Un runner GitHub, lui, atteint la machine. C'est déjà par ce chemin que le dépôt Hermes y déploie.
Le workflow `.github/workflows/verifier-sur-le-serveur.yml` est donc l'endroit où la vérification
sur matériel réel peut avoir lieu — comme c'est déjà le cas pour gVisor et Firecracker, vérifiés
par le job `isolation` de l'intégration continue.

## Ce que la sonde a trouvé (12 septembre 2026)

Elle a tourné, et c'est la première fois que cette machine est décrite par une mesure plutôt que
par son nom.

| | Mesuré | Ce que cela veut dire |
|---|---|---|
| Système | Ubuntu 26.04 LTS, noyau 7.0.0 | récent, rien à faire |
| Mémoire | **7740 Mio**, plus 2047 Mio d'échange | le nom de la machine dit « 2gb » ; il a tort. Rien à ajouter pour compiler |
| Disque | 21 Gio libres | suffisant pour l'atelier, pas pour des images de microVM |
| `/dev/kvm` | **absent** | le niveau 2 est hors d'atteinte sur ce serveur, définitivement : c'est une machine virtuelle sans virtualisation imbriquée |
| Espaces de noms | restreints par AppArmor (`= 1`) | **les niveaux 0 et 1 échouent aussi**, tant que la case `lever_userns` n'a pas été cochée une fois (ADR-0006) |
| Rust, gVisor | absents | la case `preparer` les installe |
| Hermes | intact | la sonde n'a rien touché |

Deux conséquences pour ce que « Prophet OS tourne sur ce serveur » peut vouloir dire.

**Le niveau 2 n'y sera jamais disponible.** Pas de `/dev/kvm`, donc pas de microVM Firecracker.
`sandboxd` le dira plutôt que de faire semblant — c'est précisément ce que `max_level` existe pour
annoncer. Une tâche qui exige le niveau 2 sera refusée sur cette machine, et acceptée sur un PC
avec KVM.

**Aucun niveau ne fonctionne avant `lever_userns`.** La restriction d'Ubuntu laisse créer l'espace
de noms puis refuse d'y exécuter quoi que ce soit. Une sonde qui s'arrête à la création conclut à
tort que tout va bien : c'est exactement ADR-0006.

## Ce qu'il reste à faire, et par qui

| | Qui | État |
|---|---|---|
| Poser le secret `VPS_PASSWORD` | vous | **fait** — la sonde s'est connectée le 12 septembre |
| Poser la variable `VPS_HOST` | vous | **fait** |
| Porter le workflow sur `main` | **vous** | reste à faire |

Les deux premiers suffisaient pour que la **sonde** tourne — elle se connecte, regarde, et repart
sans rien toucher. Elle est partie, elle a abouti, et ce qu'elle a rapporté est dans la section
précédente. Le chemin fonctionne : le découvrir au moment où l'on veut arrêter un moteur de
production aurait été le pire moment.

Le troisième est nécessaire pour **agir** : arrêter Hermes, installer, vérifier. GitHub ne propose
`workflow_dispatch` que pour un fichier présent sur la branche par défaut, et son API répond `404`
pour les autres. Ce travail-là ne part jamais sur une poussée, et la garde est posée sur le travail
entier plutôt que sur chaque étape — une condition oubliée sur une seule étape suffirait à faire ce
qu'on voulait empêcher.

Ce n'est pas moi qui l'amène sur `main` : la consigne de cette session est de ne pousser que sur
`claude/ai-optimized-os-design-djq7iw`, et amener la branche sur `main` est une décision qui se
prend, pas un effet de bord d'un commit.

## Les deux choses à faire, une fois

Elles demandent toutes deux la main d'un humain. La première pour une raison de plateforme, la
seconde pour une raison de principe.

### 1. Poser le secret **et** l'adresse

Dépôt → *Settings* → *Secrets and variables* → *Actions*. Deux onglets, deux choses.

| Onglet | Nom | Valeur | Pourquoi là |
|---|---|---|---|
| **Secrets** | `VPS_PASSWORD` | le mot de passe root | chiffré, jamais affiché |
| **Variables** | `VPS_HOST` | l'adresse du serveur | pas un secret, mais ce dépôt est public |

Le nom du secret compte exactement. Un secret **Codespaces**, **Dependabot** ou **d'environnement**
n'est pas visible par un workflow d'Actions : il existe, et la variable reste vide — les deux cas
se ressemblent parfaitement vus du workflow. La sonde affiche donc lesquels de six noms plausibles
elle voit, sans jamais montrer de valeur.

Les secrets ne traversent pas les dépôts : celui d'Hermes ne vaut pas ici, il faut le reposer.

Aucun agent n'écrit ce mot de passe — ni dans le fichier du workflow, ni dans un commit, ni dans
une entrée qu'il remplirait lui-même. Le workflow le masque dans son journal dès qu'il l'a retenu.
Il n'y a pas d'autre endroit où il puisse apparaître.

Le secret est lu tel quel : le workflow n'essaie **qu'une seule forme**. Une version antérieure en
essayait deux et expliquait en commentaire comment la seconde se dérivait de la première — dans un
dépôt public, écrire la règle de dérivation d'un mot de passe revient à en donner la moitié.

À défaut de secret, l'entrée `root_password` du déclenchement accepte une saisie à la main. Elle
convient pour un essai ; le secret convient pour la suite.

### 2. Mettre le workflow sur la branche par défaut

GitHub ne propose le bouton *Run workflow* que pour les workflows présents sur la branche par
défaut, et son API répond `404` pour les autres. `main` n'a aujourd'hui qu'un commit initial :
tout le travail est sur `claude/ai-optimized-os-design-djq7iw`.

Il suffit donc d'y amener la branche — une fusion, ou une *pull request* fusionnée. Le workflow
apparaît alors dans l'onglet *Actions*, et peut être déclenché sur n'importe quelle branche.

Une variante avait été tentée pour éviter cette étape : faire de la poussée elle-même le
déclencheur, l'intention étant écrite dans un fichier versionné. Elle a été refusée, et le refus
est juste — cela aurait rendu un `git push` capable d'arrêter un moteur de production et de
changer un réglage du noyau sans qu'un humain tranche au moment où cela arrive. Une décision
pareille se prend en la prenant, pas en poussant un commit.

## Déclencher

*Actions* → *Vérifier Prophet OS sur le serveur* → *Run workflow*, sur la branche de travail.

Quatre cases, toutes fermées par défaut. Fermées, le workflow se connecte, dépose le dépôt dans
`/root/prophet_os` et sonde la machine sans rien y modifier.

| Case | Ce qu'elle fait | Comment revenir en arrière |
|---|---|---|
| `arreter_hermes` | Désactive les unités `hermes*` et arrête les processus. Les fichiers de `/root/hermes` ne sont jamais touchés. | `systemctl enable --now hermes…` — le journal du workflow nomme les unités qu'il a désactivées. |
| `preparer` | Installe de quoi compiler, gVisor, et ajoute 4 Gio d'échange (2 Gio de mémoire ne suffisent pas à compiler). | `swapoff /swapfile.prophet && rm /swapfile.prophet` ; les paquets restent. |
| `lever_userns` | Rend les espaces de noms non privilégiés utilisables (ADR-0006). Sans cela, les niveaux 0 et 1 d'isolation échouent sur Ubuntu 24.04. | `sysctl -w kernel.apparmor_restrict_unprivileged_userns=1`, et retirer le fichier posé sous `/etc/sysctl.d/`. |
| `verifier_niveaux` | Compile `sandboxd` et exerce réellement les niveaux d'isolation. Comptez une heure. | rien à défaire. |

Le rapport est rapatrié en artefact `rapport-serveur`.

## Ce que le workflow ne fait jamais

- Il ne touche pas à `/root/hermes`, ni aux fichiers, ni à la configuration.
- Il ne redémarre pas la machine.
- Il n'écrit aucun identifiant nulle part.
- Prophet OS vit dans `/root/prophet_os` et nulle part ailleurs.

## Deux remarques sur ce mot de passe

**Il a été collé en clair dans une conversation.** Quel que soit le reste, il vaut mieux le
changer, puis mettre à jour le secret. Tant qu'il ne l'est pas, il faut le considérer comme connu.

**Ce dépôt est public.** L'adresse du serveur n'y figure donc plus : elle vit dans la variable
`VPS_HOST`. Mais ce document dit, et continuera de dire, que cette machine accepte `root` par mot
de passe — c'est une information utile à qui l'administre, et une invitation pour qui la trouve.
Les deux mesures qui ferment vraiment la porte :

```sh
# Sur le serveur, une fois une clé publique installée :
sed -i 's/^#\?PermitRootLogin.*/PermitRootLogin prohibit-password/' /etc/ssh/sshd_config
sed -i 's/^#\?PasswordAuthentication.*/PasswordAuthentication no/' /etc/ssh/sshd_config
systemctl reload sshd
```

Cela demanderait de changer ce workflow pour une clé plutôt qu'un mot de passe — un secret
`VPS_SSH_KEY` à la place de `VPS_PASSWORD`. C'est la bonne direction, et elle n'est pas prise
aujourd'hui.
