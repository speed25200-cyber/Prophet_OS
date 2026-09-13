# SFS — versions pour l'examen humain

La mission locale utilise la bibliothèque SFS dans agentd pour capturer le contexte autorisé,
conserver une version initiale privée et indexer le travail final. La copie et la comparaison
utilisent des ouvertures relatives à des descripteurs Linux. Les empreintes enregistrées
permettent de refuser un aperçu dont les octets ont changé depuis la mission.

La surface lit ces versions par `task.change` ; l'autorisation vient du propriétaire constaté
par agentd, et non d'une identité fournie dans les paramètres. Le chemin doit appartenir à
l'index final. La publication et son annulation passent par `task.apply` et `task.undo`, sous
la même identité ; voir l'[ADR 0023](../adr/0023-approbation-et-publication-par-agentd.md).

La bibliothèque possède maintenant une publication de l'index exact, une annulation qui
refuse les modifications humaines ultérieures et un journal de reprise. Les versions et
métadonnées sont conservées avant chaque échange ; les publications coopérantes sont exclues
par verrou. L'[ADR 0022](../adr/0022-publication-et-conflits.md) décrit les limites, notamment
les lots partiellement visibles et les conflits tardifs. Le parcours approuvé relie maintenant
l'index examiné et le créateur constaté ; les droits capd liés à l'index et un écrivain sous
l'identité humaine restent à livrer. Les captures privées d'agentd et son ACL de lecture sur
les documents ne deviennent pas des droits de publication.

Le [guide de la bibliothèque](../../crates/sfs/README.md) donne les interfaces et la commande
de test. L'[ADR 0018](../adr/0018-examen-des-versions.md) décrit les bornes, l'autorisation,
le compromis systemd nécessaire et les limites de récupération après interruption.
