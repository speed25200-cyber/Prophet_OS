# 0008 — Espace de travail natif et moteur local réel

Date : 12 septembre 2026. Statut : accepté pour l'implémentation en cours.

Le nouvel objectif exige un système utilisable par une personne et par des agents. L'ancienne
surface ne proposait que deux réponses à une décision ; elle ne permettait ni d'écrire une
demande, ni de choisir un modèle, ni de lire une conversation. Le pilote local était simulé.

La surface devient un espace natif winit/wgpu avec egui pour les widgets, la saisie Unicode,
le presse-papiers, le défilement et l'arbre d'accessibilité. Son thème, sa navigation et sa
sculpture dorée sont propres à Prophet. L'ancien renderer reste accessible en capture avec
`--observation` pour conserver ses tests, sans constituer le parcours ordinaire.

Le premier transport réel utilise Chat Completions sur HTTP en boucle locale. Les clients
refusent les adresses distantes, les identifiants dans les URL, les proxies et les redirections.
Le service de surface reçoit AF_INET/AF_INET6 avec IPAddressDeny=any et IPAddressAllow=localhost.
La conversation seule n'expose aucun outil système ; l'exécution d'actions reste à raccorder à
agentd, capd et sandboxd. Cette étape ne doit pas donner des droits supplémentaires aux modèles.

Les réponses arrivent par un transport asynchrone annulable. Le fil graphique lit des messages,
préserve les réponses partielles et ne marque un tour réussi qu'après une fin et une consommation
valides. Fermer le transport demande au serveur d'arrêter ; le comportement de chaque moteur
doit être exercé. Les réponses annulées ne sont pas réinjectées comme des échanges réussis.

Le contexte est borné en taille avec un refus explicite. Cette borne ne remplace pas une mesure
du nombre de tokens selon le tokenizer du modèle : la planification du contexte et de la VRAM
reste une exigence de livraison. Les conversations restent en mémoire pour cette étape ; leur
persistance sûre et la gestion des utilisateurs ne sont pas encore livrées.

Le rendu ne tourne plus en boucle active sans attente. Les fragments et les entrées réveillent
la fenêtre ; la sculpture est plafonnée à environ 30 images/s et le mode mouvement réduit retire
son animation. Les performances sur GPU et les lecteurs d'écran restent à mesurer sur matériel.
