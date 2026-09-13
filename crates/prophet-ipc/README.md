# Transport IPC Prophet

JSON-RPC 2.0 sur socket Unix, un message UTF-8 terminé par une nouvelle ligne. Le serveur
transmet l'identité du pair attestée par `SO_PEERCRED` au handler. Le handler applique les
droits ; ce transport ne remplace pas une autorisation par méthode et par utilisateur.

Le client sérialise les appels d'une même connexion. Il borne ses requêtes et les réponses
lues à 8 Mio, vérifie version et identifiant, et exige exactement un champ `result` ou `error`.
`result: null` est valide. Une ligne tronquée ou une réponse incohérente échoue sans acquittement.
Les délais relèvent de l'appelant. Pour une commande dont la confirmation est perdue, l'appelant
doit relire l'état métier ; le transport ne fournit ni idempotence ni nouvel envoi automatique.

L'inspecteur de mission ouvre une connexion neuve pour chaque appel et applique un délai de
cinq secondes. Après une annulation de future sur un client partagé, ce dernier peut encore
recevoir une ancienne réponse : la corrélation la refuse, sans établir l'issue de la commande.

Le serveur limite la taille après lecture de la ligne ; sa lecture et le nombre de connexions
restent à borner avant exposition à des pairs moins fiables. Voir [la spécification IPC](../../docs/specs/ipc.md).
