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
Le [correctif du moteur local](reports/grammaire-locale-2026-09-13.md) élimine ensuite une
répétition d'espaces dans sa grammaire et respecte la cardinalité des appels. Le paquet Nix
réussit 60 vérifications de protocole. Avec ce paquet, Qwen3-1.7B réussit trois missions
d'écriture exacte avec les vrais services et persistance après redémarrage ; Qwen3-0.6B
n'en réussit qu'une sur trois, les deux autres altérant le contenu. Cette preuve réelle
sur une tâche ne couvre ni les objectifs variés, ni les autres familles, ni l'image installée,
ni la validation sémantique par agentd. Les critères complets restent ouverts.
Codex et Claude Code sont maintenant obligatoires dans la configuration de l'image ; leurs vrais
binaires répondent aux sondes de version et d'authentification en profils vierges. Le diagnostic
ne confond plus fichiers présents, connexion et pilote agentique disponible. Le paquet expérimental
ChatGPT affiche son écran de connexion sous NixOS/XWayland, mais une erreur de polices secondaire
maintient son test strict en échec et le paquet reste hors de l'image installée. Le bureau humain
et les exécutions authentifiées restent à intégrer et à vérifier.

Le [raccordement système suivant](reports/moteur-installe-2026-09-13.md) configure le moteur,
le profil documentaire et le home du propriétaire. Trois essais avec la surface native et
Qwen3-1.7B réels réussissent : intention, préparation, lancement et contenu exact du travail.
Le test des unités installées avec poids réels est ajouté à la CI, mais son résultat reste
à établir. La session humaine graphique complète, la direction visuelle attendue, les
performances matérielles et les exigences de sécurité et de durabilité restent ouvertes.

L'[examen des fichiers](reports/examen-fichiers-2026-09-13.md) ajoute ensuite la lecture des
versions capturées, leur comparaison, la copie exacte et le refus d'un travail altéré. La
méthode exige l'UID créateur enregistré ; les autres méthodes restent à sécuriser séparément.
La CI de `1640e1c` charge le modèle installé puis échoue sur l'interdiction d'`openat2` par
systemd. Le retrait ciblé de `RestrictSUIDSGID` pour agentd corrige la reproduction locale ;
une nouvelle VM reste nécessaire. L'application approuvée, l'undo, la session humaine complète
et la refonte graphique restent à livrer. Aucun critère complet n'est coché pour cet examen.

La CI de `d20ef74` réussit ensuite la mission Qwen3 avec les unités NixOS, le modèle sous un
compte distinct, la lecture des versions et le redémarrage d'agentd. Le test ChatGPT reste en
échec sur Fontconfig. L'[atelier graphique](reports/atelier-2026-09-13.md) ajoute une galerie,
la Focale, la recherche clavier et corrige le mélange des transparences. La qualité visuelle
attendue, la fluidité matérielle et la session humaine complète restent à valider ; aucun
critère complet n'est coché.

Le [correctif d'accès humain](reports/acces-humain-2026-09-13.md) permet ensuite à la CLI de
consulter agentd malgré les captures privées. Une VM KVM vérifie que le secours graphique
ne masque plus l'invite et n'interrompt pas la connexion. Cette preuve de secours ne livre
pas le bureau humain ; le test installé complet de la nouvelle révision reste à vérifier.

La CI de `ddd375b` confirme ensuite le secours, la connexion console, les services et le modèle
réel. Le [nouveau bureau humain](reports/bureau-humain-2026-09-13.md) remplace le kiosque de l'image
par une connexion PAM et des fenêtres sous l'identité du propriétaire. Le parcours du bureau
réussit en VM en 188,07 s. La variante sur disque installé s'arrête localement sur une erreur
KVM/SMM avant les services. La CI de `8ea4c7f` vérifie ensuite le démarrage, les empreintes du
magasin ext4, la session et le travail humain, puis échoue au relevé du processus bref de
Claude Code après son diagnostic réseau. Son parcours complet reste donc en échec.
ChatGPT y est inclus à titre expérimental, avec son contrôle strict toujours rouge.
Cette intégration ne coche pas le critère des clients authentifiés ni celui de l'interface complète.

La [publication journalisée](reports/publication-2026-09-13.md) ajoute les contrôles des
versions exactes, les conflits d'application et d'annulation, la conservation des métadonnées
et la reprise après 18 interruptions réelles de processus. La bibliothèque ne délivre aucune
approbation ; l'intégration interservices sous l'identité humaine et la résolution graphique
des conflits restent ouvertes. Cette preuve partielle ne coche pas le critère de durabilité complète.

L'[approbation depuis le service](reports/approbation-2026-09-13.md) relie ensuite cet index
au créateur constaté : `task.apply` et `task.undo` dans agentd, `prophet task apply` / `undo`
et les boutons de l'atelier, avec journal sous l'acteur `user` et reprise d'une intention
interrompue. Avant d'écrire, capd tranche sur un jeton neuf borné aux chemins de l'index exact ;
une révocation après la mission bloque la publication. Quatre tests réels sous un seul UID le
vérifient. Sur l'image installée, le service n'a pas `CAP_CHOWN` : remplacer un document du
propriétaire n'est pas livré. Le critère d'exécution locale complète reste donc ouvert.

Le [navigateur et le web](reports/navigateur-2026-09-13.md) arrivent ensuite : `http.fetch`
réel par egress, `web.open`/`web.tree`/`web.act` par l'arbre d'un Chromium piloté, X et un
navigateur à profil Prophet dans le bureau. Les preuves sont des tests avec les vrais services
et un vrai navigateur, sous un seul UID, sans VM. La sortie réseau propre du navigateur n'est pas
relayée par egress et son confinement n'est pas livré ; le critère « MCP livré opérationnel »
progresse sans être coché.

Les [instruments de l'atelier](reports/instruments-2026-09-13.md) donnent ensuite à la surface
une identité qui se lit sans mots : anneaux, monogrammes, bandes d'état, rail éclairé, sans
animation au repos, vérifiés en rendu logiciel par les tests de propriétés et d'inspecteur.
Les captures d'états réels sont régénérées ; les temps de rendu, la mémoire et la consommation
sur écran physique attendus par le critère d'interface restent à mesurer.

La [direction Réacteur](reports/interface-reacteur-2026-09-13.md) remplace ensuite la
présentation de l'atelier : nuit, plaques de verre à crochets, jauges graduées, cinq accents de
couleur au choix conservés dans la configuration, et un champ GPU où chaque mission reçue est
un ruban qui avance au rythme réel de ses étapes. Les parcours graphiques existants passent
inchangés et deux tests de propriétés s'y ajoutent. `prophet-surface --mesure` mesure les
temps par image et la mémoire résidente, GPU attendu ; en build release sur llvmpipe, cinq
missions à 1920 × 1080 coûtent 24,44 ms en médiane (champ allégé pour rastériseur logiciel),
51,22 ms avec le champ complet, 8,06 ms sans mission, pour 150 à 160 Mio résidents ; un écran
inchangé n'est plus redessiné. Les mêmes mesures sur un écran physique avec carte graphique,
et la consommation au repos, restent à établir pour cocher le critère d'interface.
