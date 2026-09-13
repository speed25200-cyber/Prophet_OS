# ADR 0015 — Préparer une mission à partir d'une intention humaine

- Date : 13 septembre 2026
- Statut : adopté

## Contexte

L'interface peut lancer un plan existant, mais sa commande « Préparer un objectif » ouvrait
une conversation sans outils. Demander au modèle de fabriquer un manifeste ou de choisir ses
propres droits introduirait un autre problème : le contexte généré deviendrait une autorité.

## Décision

Le service charge un catalogue explicite de profils depuis `PROPHET_MISSION_PROFILES`. Chaque
profil fixe un manifeste, les périmètres fichiers et les modèles admis. Le catalogue est limité
à 1 Mio et 32 profils ; il est validé entièrement au démarrage. Pour ce lanceur, seuls les
modèles locaux et outils fichiers natifs de niveau 0 sont admissibles. Chaque périmètre de
capture exige un droit de lecture ; les droits d'écriture peuvent être plus étroits. Les
chemins parents, globaux et l'état interne `.prophet` sont refusés comme périmètres.

`task.options` expose les noms, contextes, limites et droits demandés, sans manifeste ni jeton.
Les modèles proposés sont l'intersection des préférences du profil et de `/models`, interrogé
auprès du moteur configuré dans agentd. Le moteur du dialogue peut être différent. Une erreur
de découverte est explicite ; elle ne devient pas un catalogue vide prétendument réussi.

`task.prepare` accepte uniquement `id`, `intent`, `profile`, `model`. L'identité est dérivée de
SO_PEERCRED (`uid:<nombre>`). Le service vérifie de nouveau le modèle, demande le jeton à capd,
conserve le plan puis le rend. Il ne lance aucune génération et n'ouvre pas le travail SFS.
Le lancement reste une commande humaine distincte après examen des droits effectivement émis.
Ce plan est un contrat d'exécution ; il ne prétend pas être une décomposition du travail par IA.

La surface conserve le brouillon, reçoit les réponses hors du fil graphique et garde l'ULID
envoyé. Elle refuse un second envoi du même brouillon. Après une réponse incertaine, « Retrouver
le plan » fait uniquement une inspection de cette référence. Un nouveau brouillon requiert une
action explicite. La reprise de la même référence ne crée donc pas automatiquement une seconde
mission. Une confirmation dont l'identité, l'objectif ou le modèle diffèrent est rejetée.

L'écran sépare la direction humaine du formulaire de travail ; le parcours Définir → Examiner
→ Superviser → Relire indique les passages de responsabilité. Depuis le dialogue, seule la
demande humaine est reprise comme brouillon. Le texte de la réponse du modèle ne sélectionne
aucun profil et ne déclenche aucun lancement.

## Conséquences et limites

Un administrateur doit encore installer les profils et démarrer le moteur local. Le catalogue
est une configuration de confiance, pas un registre d'éditeurs dont les signatures seraient
vérifiées. La validation cryptographique des manifestes reste ouverte, comme pour `task.spawn`.
Cette ancienne méthode conserve aussi ses paramètres historiques ; cette évolution ne corrige
pas à elle seule l'autorisation interservices de l'ensemble du système.

Les brouillons et références incertaines ne survivent pas encore à la fermeture de la surface.
Les tâches et plans confirmés sont persistants dans agentd. Le journal de planification garde
les limites de son transport historique ; une réponse perdue ne prouve ni absence ni réussite.
L'interface donne accès au résultat et aux métadonnées du diff ; validation de contenu, application,
undo robuste, processus isolés et clients officiels restent des exigences de livraison distinctes.
