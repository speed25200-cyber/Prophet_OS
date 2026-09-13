# 0021 — Une session humaine Wayland avec les clients officiels

Date : 13 septembre 2026. Statut : implémenté, parcours du bureau vérifié en VM ; variante installée à valider.

L'[ADR 0009](0009-clients-officiels-et-bureau.md) exige une session humaine pour ChatGPT,
Claude Code et Codex, distincte de leur future exécution comme agents sous sandboxd. Le
kiosque Cage sous UID de service ne peut pas porter cette session. Il ne possède ni gestion
de fenêtres ni identité du propriétaire, pourtant nécessaire à l'inspection des missions.

Le module `desktop.nix` ajoute une connexion PAM par greetd/ReGreet et un compositeur
SwayFX exécuté sous le compte `prophet.user`. La supervision devient une application
Wayland identifiable par `org.prophet.Supervision`, lancée par le gestionnaire systemd de
cet utilisateur. Les services système gardent leurs comptes et leurs protections. Le module
désactive le kiosque historique ; `prophet.desktop.enable = false` permet de le conserver.
La console tty2 reste disponible indépendamment de l'écran de connexion sur tty1.

Quatre espaces distinguent supervision, travail, dialogue et recherche. Un lanceur accessible
par Super + Espace ou par la barre propose la supervision, ChatGPT, Claude Code, Codex,
fichiers, terminal, navigateur, verrouillage et déconnexion. Les commandes proviennent d'une
liste fixe. Les terminaux des clients s'ouvrent dans `~/Documents/Prophet` et utilisent leurs
profils privés existants, sous `~/.local/state/prophet/providers/<pilote>/<utilisateur>`.
Le lanceur n'inspecte aucun fichier de connexion. Les applications gèrent leur authentification.
L'ouverture d'un client CLI présente l'atelier en onglets ; chaque terminal conserve toute la
largeur utile. Super + W et Super + B permettent de choisir les onglets ou les fenêtres côte
à côte. L'option officielle `foot --hold` garde le terminal visible après la fin du client :
un diagnostic de démarrage ne disparaît plus avec sa fenêtre. L'humain peut la fermer avant
de relancer l'application ; cette persistance ne signifie pas que le client tourne encore.

Le lancement d'une application est redemandé au compositeur après le choix de son espace.
Le premier parcours a révélé un terminal créé mais invisible : conserver le contexte du
raccourci initial le rattachait à l'espace quitté. Le code de
[lancement de Sway](https://github.com/swaywm/sway/blob/master/sway/commands/exec_always.c)
crée un contexte et un jeton d'activation par `exec`. Le lanceur demande ce contexte au bon
moment et transmet séparément le répertoire de travail du terminal.

Les portails de fichiers et le trousseau GNOME appartiennent à cette même session. Le
verrouillage utilise PAM ; fermer la supervision ne doit pas arrêter les missions d'agentd.
Le rendu logiciel forcé est une configuration de VM, pas le réglage livré au matériel réel.

Le paquet expérimental ChatGPT entre dans ce bureau pour exercer les applications ensemble.
Cela remplace la décision provisoire de le garder hors de l'image : **son défaut Fontconfig
connu reste un défaut de livraison**. Le test strict `chatgpt-desktop` est conservé sans
affaiblissement. Une fenêtre de connexion dans le test de bureau ne peut pas transformer ce
test rouge en compatibilité complète. Aucun binaire du client n'est modifié par ce jalon,
aucune sandbox n'est désactivée et aucun compte cloud n'est utilisé par le test.

Ces lancements sont des applications interactives du propriétaire. Ils ne constituent pas
un chemin d'exécution d'agent autorisé par capd/sandboxd/egress. Les pilotes ne doivent pas
annoncer ce chemin prêt tant que son protocole, ses permissions et son confinement ne sont
pas raccordés. Le réseau des applications humaines n'est pas mesuré ici comme réseau d'agent.

Le critère écrit avant l'implémentation est `nix build .#checks.x86_64-linux.desktop-session` :
connexion avec mot de passe, supervision sous UID humain, commandes des services, fenêtres
coexistantes, fichier et presse-papiers réels, processus officiels et reprise après verrouillage.
Le test `installe` reprend ce parcours avec systemd-boot et les modules de l'image. Il exige
un magasin sur le disque ext4 de l'invité et vérifie les empreintes de ses contenus avec
`nix-store --verify --check-contents`, sans réparation automatique. Ce contrôle supplémentaire
répond aux diagnostics de lecture émis par `cptofs` pendant la fabrication du disque.
Le test `surface-rescue` conserve séparément le contrôle du secours historique. Les résultats effectifs
et les limites doivent être consignés dans le rapport avant de déclarer ce critère réussi.

Ce bureau fournit l'infrastructure des applications ; il ne suffit pas à satisfaire la qualité
visuelle demandée, l'accessibilité, les mesures matérielles, les clients authentifiés ni la
supervision agentique complète. Les exigences de `FRONTIER.md` restent ouvertes.
