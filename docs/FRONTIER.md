# Prophet OS — exigences de la version complète

L'objectif est un OS Linux de nouvelle génération pour les agents et les modèles locaux :
fonctionnel de bout en bout, performant, contrôlable, avec une interface graphique soignée et
fluide. La qualification SOTA exige des comparaisons reproductibles. Les tests de composants,
les maquettes et les démonstrations simulées ne suffisent pas à la revendiquer.

Cette liste complète STATUS.md et corrige ses critères trop faibles. Une case ne peut être cochée
qu'avec une commande reproductible et le résultat de l'exécution correspondante.

- [ ] Exécution locale complète depuis l'interface : intention, sélection de modèle, plan,
  droits, outils, résultat vérifié, validation et annulation. Inclure les erreurs et la reprise.
- [ ] Moteurs locaux effectivement intégrés, catalogue et cycle de vie des poids, découverte des
  capacités, génération en flux, budgets de contexte, mémoire, VRAM, concurrence et annulation.
  Valider des modèles de plusieurs familles sur CPU et GPU, puis les modalités annoncées.
- [ ] MCP livré opérationnel : initialize, outils disponibles et appels réels contrôlés par capd.
  Aucun outil annoncé comme fonctionnel ne peut se limiter à un message d'indisponibilité.
- [ ] Clients officiels effectivement exécutés dans le confinement requis, événements et demandes
  de permission traduits à partir de leurs protocoles documentés. Sessions réelles pour les essais.
- [ ] ChatGPT graphique et Claude Code utilisables dans une session humaine complète : connexion,
  projets locaux, terminal, presse-papiers, fichiers, reprise et mise à jour. Vérifier la
  compatibilité de l'application ChatGPT Linux avec NixOS et XWayland. Mesurer les latences et
  la consommation face aux mêmes clients sur une distribution officiellement prise en charge.
- [ ] Isolation appliquée : espaces de noms, Landlock, seccomp, cgroups, gVisor et microVM selon
  le niveau demandé. Vérifier des commandes dans l'invité, deux VM simultanées, quotas, arrêt des
  descendants et absence de sortie réseau non autorisée. La présence d'un périphérique ne suffit pas.
- [ ] Permissions interservices par méthode et identité, limitation de la surface d'observation,
  émission à partir de manifestes approuvés, révocation persistante, approbations humaines liées
  à l'action exacte. Aucune clé ni valeur de secret dans le contexte des modèles.
- [ ] Journalisation durable avec reprise et idempotence ; commits de fichiers et récupération
  après interruption testés ; undo qui respecte les modifications intervenues depuis le commit.
- [ ] Installation, système installé, chiffrement, mises à jour, secours et retour automatique
  après échec testés sur des disques de test. Secure Boot/lockdown/immuabilité mesurés si annoncés.
- [ ] Interface native : espace de commande, conversations, modèles, tâches, fichiers/diffs,
  approbations et arrêt d'urgence. Navigation clavier et souris, état vide/chargement/erreur,
  redimensionnement, accessibilité, mouvement réduit et absence de blocage pendant l'inférence.
  Captures des états réels et mesures des temps de rendu, mémoire et consommation au repos.
- [ ] Matrice matérielle publiée : Intel/AMD, GPU supportés, mémoire minimale, réseau et firmware.
  Ne pas confondre le rendu logiciel de la CI avec l'accélération d'inférence.
- [ ] Version installable reproductible, documentation correspondant au binaire, tests de bout en
  bout bloquants, comparaison avec une base Linux utilisant les mêmes modèles et les mêmes tâches.
  Mesurer réussite, latences médiane/p95, tokens, RAM/VRAM, énergie et interventions humaines.

Priorité d'intégration : moteur local réel → outils et capd → agentd → interface native →
durabilité et confinement complets → distribution et matériel → comparaison de performances.
Les défauts de sécurité connus restent bloquants avant de confier des données ou tâches sensibles.

Le 12 septembre : le client local a passé une génération avec un modèle Qwen3 réel et une écriture
de fichier décidée par le modèle. Cela valide le pilote, pas encore la chaîne installée entière.
Le transport en flux et l'espace natif de conversation sont maintenant implémentés. Le raccordement
de cette interface à l'exécution agentique complète via MCP/agentd reste à réaliser.
Le registre MCP refuse maintenant les exigences impossibles à vérifier et la session applique
le cycle d'initialisation. Son binaire reste à raccorder aux services ; les accès fichiers doivent
être durcis contre les liens symboliques et la recherche contrôlée pour chaque descendant.
Codex et Claude Code sont maintenant obligatoires dans la configuration de l'image ; leurs vrais
binaires répondent aux sondes de version et d'authentification en profils vierges. Le diagnostic
ne confond plus fichiers présents, connexion et pilote agentique disponible. Le paquet expérimental
ChatGPT affiche son écran de connexion sous NixOS/XWayland, mais une erreur de polices secondaire
maintient son test strict en échec et le paquet reste hors de l'image installée. Le bureau humain
et les exécutions authentifiées restent à intégrer et à vérifier.
