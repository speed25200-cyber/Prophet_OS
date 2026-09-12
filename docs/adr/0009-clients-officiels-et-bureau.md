# 0009 — Clients officiels et session de bureau

Date : 12 septembre 2026. Statut : accepté ; intégration en cours.

L'objectif inclut explicitement ChatGPT et Claude Code, en plus des modèles locaux. Trois
parcours doivent être livrés : l'application ChatGPT pour l'humain, les clients officiels dans
un terminal humain, et leur intégration agentique contrôlée par Prophet. Une commande de version
réussie ne valide pas ces trois parcours.

Codex et Claude Code sont des dépendances obligatoires de l'image. Leur source reste le nixpkgs
épinglé ; les applications ne sont pas modifiées. Le premier jalon vérifie leur version, leur
présence et leur état de connexion par leurs propres commandes. Les identifiants restent gérés
par les clients ; Prophet n'en inspecte jamais les fichiers. Aucun abonnement n'est déduit de
la simple présence d'un fichier ni d'un code de succès d'authentification.

Pour l'intégration graphique de Codex, la cible est le protocole bidirectionnel documenté
`codex app-server` : initialisation, fils, tours, événements et décisions d'approbation. Le
constructeur `codex exec` reste un profil de commande, pas un remplacement du canal de décisions.
Claude Code utilise ses interfaces officielles de flux, reprise et permission ; les protections
du client sont conservées. Le raccordement doit être exercé avec capd, sandboxd et egress avant
d'annoncer un pilote disponible dans l'orchestrateur.

L'application ChatGPT Linux est en aperçu. La liste de distributions officiellement prises en
charge ne comprend pas NixOS ; son paquet de compatibilité reste à construire et tester.
XWayland est la première cible de compatibilité, le mode Wayland natif étant expérimental.
Source : [application ChatGPT Linux](https://learn.chatgpt.com/docs/linux/linux-app).

Le kiosque Cage actuel, exécuté sous un compte de service avec accès réseau limité à localhost,
ne constitue pas un bureau pour ces applications. Il faut une session humaine authentifiée,
un compositeur avec plusieurs fenêtres, un terminal, un navigateur pour les connexions, des
portails de fichiers et un trousseau utilisateur. Les identités et droits des services système
doivent rester distincts de ceux des applications. Ce bureau reste à implémenter ; affaiblir
globalement l'unité de surface pour y lancer les clients ne satisfait pas cette décision.

Références : [Codex App Server](https://learn.chatgpt.com/docs/app-server),
[authentification Codex](https://learn.chatgpt.com/docs/auth),
[CLI Claude Code](https://code.claude.com/docs/en/cli-reference).
