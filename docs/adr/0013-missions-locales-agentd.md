# ADR-0013 — Lancer les missions locales par agentd

- **Statut** : accepté
- **Date** : 2026-09-13
- **Tâches liées** : M7-T1, M8-T8, M8-T9 ; intégration de FRONTIER.md

## Contexte

Le daemon savait planifier une tâche, sans méthode pour l'exécuter. La boucle historique
synchrone ne convenait pas au service : elle ne permettait pas l'annulation pendant une
inférence et comptait le budget après les actions. Les tests du registre utilisaient un
Broker et un journal en mémoire. Ces garanties ne suffisaient pas à alimenter la supervision.

## Décision

Séparer planification et lancement. Conserver le plan, le jeton et le résultat dans l'état
d'agentd. `task.start` réserve au plus deux missions actives puis démarre un thread dédié.
Ce thread possède une boucle Tokio pour son transport HTTP ; les outils synchrones appellent
les vrais daemons par des connexions Unix courtes, hors de Tokio, bornées à cinq secondes.
L'interface IPC du service reste disponible pendant l'inférence.

Le lancement n'accepte que les plans locaux au niveau 0. Il exécute les cinq outils fichiers
natifs, filtrés par le jeton, sans créer de processus non fiable. Les autres niveaux et pilotes
sont refusés. Les chemins doivent appartenir au périmètre conservé dans le plan, en plus des
grants de capd. Le moteur ne choisit ni son jeton, ni le home, ni le répertoire de travail.

Préparer la copie SFS par des ouvertures relatives sans liens, avec contrôle de lecture par
descendant et pendant la copie. Refuser une tâche existante, les périmètres chevauchants,
les fichiers spéciaux et liens multiples ; borner profondeur, nombre et volume. La résolution
d'un périmètre absent ne crée aucun dossier dans les documents de l'utilisateur.

Compter les tokens d'entrée et de sortie avant l'appel d'outil, y compris quand le moteur
rend une génération tronquée. Appliquer la durée réelle et le plafond d'étapes. L'annulation
abandonne la requête HTTP et interdit les prochaines actions aux contrôles d'autorité.
Un appel noyau ou un RPC déjà en cours peut retarder l'arrêt ; aucune latence maximale
générale n'est revendiquée.

Exiger une confirmation du journal avant l'action et après son résultat. Si la confirmation
du résultat manque, l'action peut déjà avoir eu lieu : bloquer les appels suivants, rendre
un échec et ne jamais réessayer automatiquement. Cela n'offre pas une transaction commune
entre les fichiers, le journal et l'état de tâche.

Écrire l'état sous le verrou du runtime, dans un fichier créé en 0600, synchronisé puis
renommé, avec synchronisation du parent. Une corruption bloque le démarrage et préserve le
fichier. Après redémarrage, une tâche active devient échouée avec une raison et un résultat
consultables ; aucun redémarrage automatique d'action n'est effectué.

## Limites

La boucle historique `Runtime::run` reste utilisée par ses tests de bibliothèque ; le service
utilise `local::Mission`. L'autorisation interservices par méthode, l'approbation de manifestes,
la révocation persistante et une outbox durable restent à construire. La file historique des
événements de planification peut encore perdre des événements si ledger est injoignable.

Les racines doivent rester sous contrôle du lanceur et les services sont de confiance. Un
acteur partageant leurs droits peut interférer avec les chemins de métadonnées et de diff ;
la capture par descripteurs ne fournit pas un espace de noms privé. Les fichiers non copiés
peuvent être lus dans la vue d'origine par MCP si les contrôles l'autorisent : il ne s'agit pas
d'une vue intégralement immuable du home.

Une fin normale signifie que le modèle a terminé sa boucle et que SFS a calculé un diff.
Elle ne garantit pas que l'intention est satisfaite. Le vérificateur de résultat, la validation
humaine du diff, les commits/undo robustes, les checkpoints et les contrôles de l'interface
restent nécessaires. Le serveur de modèles et l'isolation des clients officiels conservent
leurs propres exigences d'intégration. Ce jalon ne qualifie pas Prophet OS de SOTA.
