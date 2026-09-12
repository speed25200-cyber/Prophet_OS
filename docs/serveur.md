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

## Les deux choses à faire, une fois

Elles demandent toutes deux la main d'un humain. La première pour une raison de plateforme, la
seconde pour une raison de principe.

### 1. Poser le secret `VPS_PASSWORD`

Dépôt → *Settings* → *Secrets and variables* → *Actions* → *New repository secret*.

| Nom | Valeur |
|---|---|
| `VPS_PASSWORD` | le mot de passe root du serveur |

Les secrets ne traversent pas les dépôts : celui d'Hermes ne vaut pas ici, il faut le reposer.

Aucun agent n'écrit ce mot de passe — ni dans le fichier du workflow, ni dans un commit, ni dans
une entrée qu'il remplirait lui-même. Le workflow le masque dans son journal dès qu'il l'a retenu.
Il n'y a pas d'autre endroit où il puisse apparaître.

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

## Une remarque sur le mot de passe

Il a été collé en clair dans une conversation. Quel que soit le reste, il vaut mieux le changer,
puis mettre à jour le secret. Tant qu'il ne l'est pas, il faut le considérer comme connu.
