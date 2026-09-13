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
Le 13 septembre, la direction visuelle Iris remplace l'accueil et la navigation : dock flottant,
sculpture 3D, typographie embarquée et composition adaptée à la taille de fenêtre. Les
[captures et contrôles de cette refonte](reports/interface-iris-2026-09-13.md) portent sur le
binaire natif ; la session humaine avec plusieurs applications reste à intégrer.
La demande suivante remplace Iris par un [espace de supervision des missions](reports/supervision-2026-09-13.md) :
sélection et filtres, contexte, examen volontaire des décisions et thème clair sans sculpture.
Les diffs, livrables, commandes d'agents et permissions détaillées restent à raccorder.
Le registre MCP refuse maintenant les exigences impossibles à vérifier et la session applique
le cycle d'initialisation. Le 13 septembre, les accès fichiers sont ancrés sur des descripteurs
Linux, les liens refusés et les descendants recontrôlés. Un [essai Qwen3 réel avec le registre](reports/mcp-fichiers-2026-09-13.md)
produit un fichier de travail et un diff SFS. Le binaire reste à raccorder aux services, avec
des contextes de tâche fiables, des racines privées et un journal durable ; les commits SFS
concurrents et le lancement depuis l'interface restent à sécuriser et à intégrer.
Le raccordement suivant ajoute les [missions locales par agentd](reports/missions-locales-2026-09-13.md) :
plan conservé, lancement en arrière-plan, capd et ledger réels, scopes contrôlés, budget avant
action, annulation de l'inférence et résultats persistants. Le lanceur accepte uniquement les
outils natifs de confiance au niveau 0 ; les processus isolés restent à raccorder. Trois essais
avec Qwen3-0.6B échouent sur une génération incomplète. La réussite de la chaîne de service avec
un modèle réel reste donc à établir ; le lancement, les diffs et l'arrêt depuis l'interface
ne sont pas encore livrés à ce jalon. Le raccordement suivant ajoute un
[inspecteur avec commandes explicites](reports/commandes-supervision-2026-09-13.md) : lecture
cohérente du plan et du résultat, lancement et arrêt, réponses tardives écartées et missions
terminées conservées. La liste des fichiers provient des métadonnées du diff ; leur contenu,
validation et annulation restent à intégrer, ainsi que la création d'un plan depuis le dialogue.
Le jalon suivant ajoute la préparation depuis une intention humaine et depuis une demande du
dialogue : profils configurés côté service, découverte réelle du modèle, plan persistant et
lancement distinct. La récupération d'une réponse perdue utilise une lecture de la référence
conservée. Ce raccordement nécessite encore l'installation des profils et du moteur ; il ne
valide pas la planification par un modèle réel ni la chaîne installée entière. Voir l'[ADR 0015](adr/0015-intention-et-profils-de-mission.md).
Aucune exigence complète ci-dessus n'est cochée pour ces jalons.
Codex et Claude Code sont maintenant obligatoires dans la configuration de l'image ; leurs vrais
binaires répondent aux sondes de version et d'authentification en profils vierges. Le diagnostic
ne confond plus fichiers présents, connexion et pilote agentique disponible. Le paquet expérimental
ChatGPT affiche son écran de connexion sous NixOS/XWayland, mais une erreur de polices secondaire
maintient son test strict en échec et le paquet reste hors de l'image installée. Le bureau humain
et les exécutions authentifiées restent à intégrer et à vérifier.
