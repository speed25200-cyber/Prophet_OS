# Publication des fichiers — 13 septembre 2026

Travail après `8ea4c7f`, lié à M4-T2, M4-T4, M8-T10 et M12-T4. Le moteur de bibliothèque
publie les versions examinées, conserve les fichiers remplacés et reprend une application
ou une annulation interrompue. Le parcours d'approbation depuis la supervision reste à intégrer.

## Défauts reproduits

Avant modification, dix tests échouent : application malgré une édition humaine, collision
d'ajout, suppression d'un fichier retouché, sortie de périmètre, parent remplacé par un lien,
annulation qui écrase une retouche, supprime un ajout retravaillé ou écrase un fichier recréé,
permissions perdues et sauvegarde altérée acceptée. La première version corrigée perd encore
les attributs étendus ; deux nouveaux tests le reproduisent avant leur correction.

La revue suivante reproduit trois erreurs de consultation : suivi d'un lien `meta.json`,
identifiant différent du dossier accepté et publication cachée si son manifeste disparaît.
Les lectures sont désormais bornées et ancrées, et un journal incomplet est signalé.
Un autre test constate que le diagnostic promet des snapshots natifs sur btrfs alors que
le moteur copie les fichiers ; cette annonce est corrigée.

## Vérifications

| Contrôle final | Résultat local |
| --- | --- |
| `nix develop --command just check` | 663 réussis, 0 échec, 29 ignorés ; format, clippy, binaires, services, durcissement, documentation des travaux et recherche de secrets réussis |
| Cinq binaires SFS du même build, sous UID/GID 65534 | 61 réussis, 0 échec, 0 ignoré |
| Interruptions réelles, comprises dans le test de reprise | 18 scénarios réussis : neuf à l'application, neuf à l'annulation |

Les résultats proviennent de `publication-check-final.log` et
`publication-unprivileged-final.log` dans le répertoire local de construction. Le test unitaire
SFS contient 18 cas, les conflits 23, les aperçus 4, les captures 4 et les espaces de travail 12.
La vérification complète du dépôt est exécutée sous le compte de construction root ; seule
la seconde série SFS constitue ici une preuve d'exécution sous un compte sans privilèges.

Les tests de bibliothèque exercent les conflits avant publication, les versions examinées,
les copies figées, les ACL humaines et héritées, les attributs et dates, les appels répétés,
les états corrompus, les collisions concurrentes et la conservation des fichiers déplacés.
Les tests existants couvrent aussi 50 modifications suivies d'une annulation avec comparaison
des contenus, les captures autorisées et les aperçus.

Un test lance un véritable sous-processus puis lui envoie SIGKILL à neuf étapes, successivement
pendant l'application et l'annulation : journal initial, copie synchronisée, version prête,
intention synchronisée, avant changement de nom, changement effectué, répertoires synchronisés,
progression enregistrée et journal terminé. Les 18 reprises vérifient ajout, modification et
suppression. Une modification humaine après la reprise est conservée lors d'un nouvel appel.
Les points d'interruption n'existent que dans le binaire de test.

Les commandes Rust sont exécutées dans `nix develop`, avec la chaîne épinglée. Une exécution
complémentaire des mêmes binaires sous `nobody` (UID/GID 65534) vérifie l'absence de dépendance
aux privilèges root. Son premier lancement utilisait par erreur le répertoire temporaire privé
du shell Nix root ; il échouait avant toute opération SFS. L'essai corrigé emploie un répertoire
temporaire appartenant au compte de test. Les essais utilisent ext4 sous WSL2, sans disque de production.

## Limites de cette preuve

Le journal permet de reprendre après arrêt du processus. Ces essais ne simulent pas une perte
d'alimentation, un contrôleur défaillant ou une matrice complète de systèmes de fichiers.
L'atomicité porte sur chaque changement de nom, jamais sur tous les documents à la fois.
Une édition concurrente tardive reste conservée dans un fichier déplacé ; elle peut nécessiter
une résolution explicite avant de poursuivre. Les répertoires et temporaires privés ne sont pas nettoyés.

La bibliothèque ne vérifie pas le consentement humain. L'identité créatrice, les permissions
capd, le manifeste approuvé, son transport entre services et l'écriture sous l'UID humain
restent à raccorder. Les nouveaux fichiers utilisent encore le groupe du parent ; la politique
doit distinguer le parent SGID et le groupe de l'écrivain avant l'intégration générale. Les
répertoires parents modifiés concurremment demandent leurs propres contrôles ; le verrou
actuel exclut les publications coopérantes, sans sérialiser toutes les opérations historiques
sur le cycle de vie d'une capture. L'inode, ctime
et atime ne sont pas restaurés. Les liens, fichiers spéciaux, capacités, bits SUID/SGID et
créations SELinux non configurées sont refusés. Les API historiques `Transaction` restent
hors de ce mécanisme et ne fournissent pas une transaction atomique sûre pour des chemins non fiables.

Ce jalon ne valide ni le parcours graphique approuvé, ni l'annulation complète d'une mission
installée, ni les critères FRONTIER. La conception détaillée figure dans
l'[ADR 0022](../adr/0022-publication-et-conflits.md).
