# SFS — versions pour l'examen humain

La mission locale utilise la bibliothèque SFS dans agentd pour capturer le contexte autorisé,
conserver une version initiale privée et indexer le travail final. La copie et la comparaison
utilisent des ouvertures relatives à des descripteurs Linux. Les empreintes enregistrées
permettent de refuser un aperçu dont les octets ont changé depuis la mission.

La surface lit ces versions par `task.change` ; l'autorisation vient du propriétaire constaté
par agentd, et non d'une identité fournie dans les paramètres. Le chemin doit appartenir à
l'index final. Cette intégration ne fait pas intervenir les anciennes méthodes de commit/undo
du daemon SFS et n'applique aucun fichier au home.

Le [guide de la bibliothèque](../../crates/sfs/README.md) donne les interfaces et la commande
de test. L'[ADR 0018](../adr/0018-examen-des-versions.md) décrit les bornes, l'autorisation,
le compromis systemd nécessaire et les limites de récupération après interruption.
