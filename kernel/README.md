# Noyau

Prophet OS n'écrit pas de noyau : il en configure un (ADR-0001). Ce répertoire rassemble ce qui
doit être vrai du noyau pour que les garanties du système tiennent.

## Fonctions exigées

| Option | Pourquoi |
|---|---|
| `CONFIG_SECURITY_LANDLOCK` | restriction de chemins sous le processus, niveau 0 |
| `CONFIG_SECCOMP_FILTER` | filtrage d'appels système, tous niveaux |
| `CONFIG_USER_NS` | espaces de noms utilisateur non privilégiés |
| `CONFIG_CGROUPS` + hiérarchie unifiée | quotas de ressources par tâche |
| `CONFIG_KVM` | microVM du niveau 2 |
| `CONFIG_VSOCKETS` + `CONFIG_VHOST_VSOCK` | dialogue avec l'invité d'une microVM |
| `CONFIG_BTRFS_FS` | sous-volumes par tâche (ADR-0004) |
| `CONFIG_IO_URING` | entrées-sorties de l'indexation et du cache de modèles |
| `CONFIG_BPF_SYSCALL` | observation et filtrage réseau du proxy |
| `CONFIG_SECURITY_LOCKDOWN_LSM` | `lockdown=integrity` |
| `CONFIG_MODULE_SIG_FORCE` | aucun module non signé |

## Fonctions retirées

Tout ce qui n'est ni nécessaire aux cibles matérielles de la version 1, ni à ce tableau. En
particulier : pilotes de matériel ancien, systèmes de fichiers exotiques, protocoles réseau
inutilisés, et `CONFIG_DEVMEM`.

## Vérification

`just test-vm` démarre l'image et vérifie que chaque fonction du tableau est réellement présente,
plutôt que de supposer qu'une option écrite dans un fichier a été prise en compte. Une fonction
absente n'est pas une erreur de démarrage : elle abaisse le niveau d'isolation maximal annoncé par
`prophet status`, et les tâches qui exigeraient davantage sont refusées.

## État

La configuration elle-même est produite à partir de la configuration du noyau LTS de nixpkgs,
réduite par le script `kernel/reduce.sh`, qui reste à écrire. Tant qu'il n'existe pas, l'image
utilise le noyau LTS de nixpkgs tel quel : toutes les fonctions du tableau y sont présentes, mais
la surface est plus large que la cible.
