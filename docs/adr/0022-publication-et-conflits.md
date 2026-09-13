# 0022 — Publier les versions examinées avec un journal de reprise

- **Statut** : moteur de bibliothèque implémenté ; commandé par agentd depuis l'[ADR 0023](0023-approbation-et-publication-par-agentd.md) ; écrivain sous l'identité humaine ouvert
- **Date** : 2026-09-13
- **Tâches liées** : M4-T2, M4-T4, M8-T10, M12-T4

L'ancien commit copie directement le travail sur les documents actuels. L'ancien undo restaure
les sauvegardes sans vérifier les modifications ultérieures. Dix régressions reproduisent des
écrasements, suppressions, changements de permissions perdus, sorties de périmètre et suivi de lien.
Ces API ne peuvent pas servir de fondation au bouton d'approbation de la supervision.

La publication doit porter sur l'index exact présenté à l'humain. Elle vérifie tous les originaux
et toutes les versions conservées avant la première mutation, refuse les liens et les fichiers
spéciaux, puis conserve un journal synchronisé sur disque avant chaque remplacement. Une annulation
compare les documents actuels aux versions effectivement publiées, jamais au travail courant.

Les publications coopérantes d'un home prennent un verrou exclusif sur son descripteur. Les éditeurs
ordinaires ne prennent pas ce verrou : une nouvelle vérification avant `rename` ne suffit donc pas.
Le remplacement échange atomiquement la proposition et le fichier déplacé, puis contrôle ce dernier.
Un ajout utilise `RENAME_NOREPLACE`. Le fichier déplacé reste conservé, y compris en cas de conflit
tardif. Une divergence interrompt le lot avec un état de récupération explicite ; elle ne déclenche
pas une restauration aveugle qui écraserait une nouvelle modification humaine.

Chaque intention en cours conserve les identités de fichiers attendues. Après interruption, la
reprise distingue une opération non effectuée d'un échange déjà effectué et refuse les états
ambigus. Les fichiers et les répertoires concernés sont synchronisés. L'atomicité porte sur chaque
changement de nom : le lot complet peut être visible partiellement pendant son exécution ou après
interruption. Le journal et l'interface doivent montrer cette situation, sans prétendre à une
transaction multi-fichiers instantanée.

Le moteur de fichiers n'est pas une autorité d'approbation. Le futur raccordement doit lier le
créateur constaté, les droits capd et l'index approuvé. L'écriture finale doit aussi conserver
l'identité et les droits d'accès humains ; l'ACL de lecture d'agentd sur Documents ne sera pas
élargie pour contourner ce problème. Le transport des versions, l'exécution sous l'identité humaine,
la journalisation interservices et les commandes graphiques restent à vérifier avant livraison.

Les critères de validation comprennent les conflits avant mutation, la modification du travail
après examen, les échanges concurrents, les interruptions réelles d'un processus puis reprise,
la sauvegarde altérée, l'identité des fichiers et les refus de liens. Une réussite de ces tests
de bibliothèque ne prouvera pas à elle seule le parcours approuvé depuis le bureau.

Le manifeste immuable contient les versions et leurs métadonnées ; son empreinte figure dans
un petit curseur de progression muni d'une somme de contrôle. Le curseur est réécrit à chaque
étape sans recopier tout l'index. Ces empreintes détectent une altération ; elles n'authentifient
pas l'auteur. Les états `applying`, `undoing` et `conflict` restent consultables après redémarrage.
Une reprise terminée est idempotente et ne retouche pas les documents modifiés ensuite.

Les fichiers remplacés gardent leur UID, GID et attributs humains, notamment les ACL POSIX et
la provenance antérieure. L'annulation restaure aussi la date de modification. Les attributs
du travail de l'agent ne remplacent pas ceux de l'humain. La provenance du nouvel acte provient
de l'appelant. Les ajouts héritent de l'ACL par défaut du parent, ajustée au mode examiné ; le
mode peut donc réduire les droits effectifs de cette ACL. La création sous SELinux exige encore
une politique explicite. Les fichiers spéciaux, liens multiples, capacités de fichier et bits
SUID/SGID sont refusés. Un échec de conservation des métadonnées interrompt la préparation.

Les ouvertures exigent Linux `openat2`, sans liens ni traversées de montage. Le lot comporte au
plus 10 000 changements, 64 niveaux et 1 Gio cumulé de versions initiales et proposées ; les
enregistrements sont bornés à 8 Mio, les attributs à 128 noms et 1 Mio par fichier. Le budget du
manifeste est vérifié pendant son accumulation. La présence de btrfs ne change pas cette
implémentation par copies : le diagnostic ne promet plus de snapshots natifs.

Les règles de changement de nom s'appuient sur [rename(2)](https://man7.org/linux/man-pages/man2/rename.2.html).
La synchronisation explicite des répertoires suit [fsync(2)](https://man7.org/linux/man-pages/man2/fsync.2.html).
Les essais tuent réellement un processus à neuf frontières de l'application puis de l'annulation.
Ils vérifient la reprise après arrêt du processus, pas une coupure d'alimentation ou un disque défaillant.

Les limites restantes incluent le transport des versions entre UID, les droits et l'approbation
liés au manifeste exact, le traitement graphique des conflits, la concurrence sur les répertoires
parents, la politique de groupe des nouveaux fichiers et le nettoyage des copies privées.
Les anciennes API `Transaction` ne sont pas remplacées par cette décision et ne constituent pas
une transaction multi-fichiers atomique utilisable avec des paramètres non fiables.
