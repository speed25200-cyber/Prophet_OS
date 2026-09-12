# Rapport de phase 0

> État du système au terme de la première passe de construction. Ce rapport dit ce qui marche, ce
> qui est mesuré, et ce qui ne l'est pas. Les chiffres proviennent tous de tests reproductibles du
> dépôt, pas d'estimations.

## 1. Ce qui existe

| Jalon | Composant | État | Vérifié par |
|---|---|---|---|
| M0 | Workspace, outillage, intégration continue | fait | `just check` |
| M1 | Types fondamentaux, jetons, journal, IPC | fait | 52 + 11 tests |
| M2 | `capd` : broker de capacités et politiques Cedar | fait | 41 tests |
| M3 | `ledger` : journal chaîné et scellé | fait | 13 tests |
| M4 | `sfs` : espace de travail par tâche, diff, annulation | fait | 23 tests |
| M5 | `sandboxd` : isolation graduée | niveau 0 vérifié ; 1 et 2 écrits, non exerçables ici | 17 tests |
| M6 | `vault` et `egress` : secrets, sortie réseau, identité d'agent | fait | 54 tests |
| M7 | `mcp-system` : serveurs MCP système | fait, liste normative couverte | 34 tests |
| M8 | `agentd` et `providers` : runtime et pilotes | fait | 38 tests |
| M9 | Image amorçable | écrite, non construite | — |
| M10 | `sup`, `browser-bridge`, `app-editor` : interface sémantique | fait | 51 tests |
| M11 | `memoryd` : mémoire cloisonnée et épisodique | fait | 25 tests |
| M12 | `shell` et `prophet` : interface humaine | fait | 42 tests |
| M13 | `bench` : suite de tâches, adversariale, coût | fait ; ligne de base pixels non mesurable ici | 22 tests |

**439 tests**, tous verts. **24 471 lignes** de Rust. Aucun avertissement de `clippy`, aucun `unsafe` hors d'une sonde
d'appel système documentée.

**81 des 88 tâches du plan sont faites.** Les 9 restantes, marquées ⛔ dans `docs/STATUS.md`,
dépendent de matériel ou de logiciel absents de l'environnement de construction ; la section 5 dit
pour chacune ce qu'il faut pour la vérifier.

## 2. Mesures

Toutes relevées sur la machine de construction, un conteneur sans KVM, sans Landlock et sans
cgroups v2.

| Mesure | Objectif du plan | Relevé | Verdict |
|---|---|---|---|
| Contrôle de capacité (`cap.check`) | < 200 µs | **11,6 µs** | tenu, avec un facteur 17 de marge |
| Démarrage d'une sandbox de niveau 0 | < 10 ms (p50) | **2,6 ms** | tenu |
| Gel d'urgence de toutes les tâches | < 50 ms | **124 µs** pour 8 sandboxes | tenu, avec un facteur 400 |
| Débit de l'IPC interne | 100 000 allers-retours < 5 s | **386 ms** pour 10 000, soit ~3,9 s pour 100 000 | tenu |
| Observation d'une page web | ordre du kilooctet | **1 929 octets** | tenu |
| Suite adversariale | 0 attaque aboutie | **0 sur 20** | tenu |

### Coût d'une observation

Une capture d'écran de bureau coûte environ 1 500 tokens et 900 Ko. L'arbre sémantique de la page
de réservation du test M10 coûte 1 929 octets, soit environ 640 tokens, et une réobservation
différentielle bien moins.

La comparaison honnête porte sur un tour complet de boucle : une boucle par capture doit
réobserver pour vérifier ce qu'elle vient de faire, une boucle sémantique reçoit le résultat avec
l'action. Sur une tâche de cinq étapes, le rapport mesuré par `bench::cost` dépasse **4 fois moins
de tokens**, et l'écart se creuse avec la longueur de la tâche.

## 3. Suite adversariale

Vingt scénarios, six familles d'attaque, exécutés contre les composants réels. **Chaque scénario
suppose que le modèle a entièrement cédé à l'injection** : le test n'appelle aucun modèle. Ce qui
est mesuré est donc la résistance du système, jamais la prudence d'un modèle.

Résultat : **20 sur 20 sans conséquence pour l'utilisateur**, dont 19 refus francs et une
approbation humaine exigée.

Les blocages viennent de six couches différentes : le jeton de capacité, la politique, le proxy de
sortie, la sandbox, le coffre et le journal. Aucune ne consulte un modèle, aucune ne peut être
influencée par du texte. C'est ce qui rend le résultat stable quel que soit le modèle employé.

Deux distinctions que la suite tient explicitement :

- une requête portant un **motif de secret reconnaissable** est refusée, jamais soumise à
  approbation : laisser un humain fatigué approuver le départ d'une clé n'est pas une protection ;
- un **téléversement volumineux ou encodé** est porté à l'humain sans être bloqué, parce que ces
  formes ont des usages légitimes et qu'un refus d'office produirait des faux positifs coûteux.

## 4. La démonstration du jalon M8

La même tâche, les mêmes outils, les mêmes permissions, sur trois pilotes, sans changer une ligne
et sans clé d'API :

| Pilote | Étapes | Appels d'outils | Tokens | État |
|---|---|---|---|---|
| `local:qwen3-8b` (boucle native) | 4 | 3 | 180 | terminée |
| `driver:claude-code` (abonnement Claude) | 4 | 3 | 150 | terminée |
| `driver:codex` (abonnement ChatGPT) | 4 | 3 | 150 | terminée |

## 5. Ce qui n'est pas vérifié

Cette section importe autant que les précédentes.

| Élément | Pourquoi ce n'est pas vérifié | Ce qu'il faut pour le vérifier |
|---|---|---|
| Sandbox de niveau 1 (gVisor) | `runsc` absent de l'environnement de construction | une machine avec gVisor installé ; `just verify-host` lance les tests marqués `needs_gvisor` |
| Sandbox de niveau 2 (microVM) | `/dev/kvm` et images d'invité absents | une machine avec KVM, Firecracker et les images ; `just verify-host` lance les tests marqués `needs_kvm`. L'objectif de 100 ms depuis instantané reste à mesurer |
| Landlock | absent de ce noyau ; la restriction de chemins repose sur la racine minimale seule | un noyau avec `CONFIG_SECURITY_LANDLOCK` |
| Image amorçable | ni Nix ni virtualisation dans l'environnement | `nixos-rebuild build-vm`, puis `just demo M8` dans la machine virtuelle |
| Pilotes de clients officiels, de bout en bout | exigent une session d'abonnement Claude ou ChatGPT | une connexion réelle ; la construction de la ligne de commande, l'environnement transmis et la détection de session sont testés, l'exécution ne l'est pas |
| Moteurs de modèles locaux | ni GPU ni modèle téléchargé | une machine avec GPU et un modèle du catalogue |
| Quotas de ressources par tâche | cgroups v2 absents | un système avec la hiérarchie unifiée |
| Comparaison avec un agent par captures d'écran | aucun agent de référence exécutable ici | la ligne de base M13-T2, à mesurer sur une machine complète |

Le système **annonce** ces limites plutôt que de les masquer : `prophet status` affiche le niveau
d'isolation réellement atteignable, et `sandboxd` refuse une tâche qui exigerait davantage au lieu
de la dégrader en silence.

## 6. Ce qu'une relecture a trouvé après coup, et corrigé

### 6.1 La sandbox retombait silencieusement au niveau 0

La première version de ce rapport annonçait les niveaux 1 et 2 comme « écrits, non exerçables
ici ». C'était trop indulgent. Une relecture a montré que le gestionnaire ignorait purement et
simplement le niveau demandé : il vérifiait que la machine pouvait l'atteindre, puis lançait le
confinement de niveau 0 dans tous les cas.

La conséquence n'aurait pas été visible dans cet environnement, où aucun niveau supérieur n'est
atteignable. Elle l'aurait été **sur une machine équipée** : une demande de niveau 2 aurait été
acceptée et exécutée en niveau 0, c'est-à-dire avec une isolation bien moindre que celle annoncée.
C'est exactement la dégradation silencieuse que le reste de la conception interdit.

Trois changements :

1. Chaque niveau a désormais son propre chemin de lancement : programme d'amorçage pour le
   niveau 0, `runsc` pour le niveau 1, Firecracker pour le niveau 2.
2. La sonde exige, pour annoncer le niveau 2, non seulement KVM et le binaire mais aussi les
   **images d'invité** : un binaire sans les images qu'il lui faut ne démarre aucune machine
   virtuelle, et l'annoncer reviendrait à promettre une protection inexistante.
3. `missing_for` nomme ce qui manque, et le refus le dit à l'utilisateur.

Un test, `le_niveau_deux_ne_retombe_jamais_sur_le_niveau_zero`, existe désormais pour attraper
cette faute si elle revenait.

La leçon vaut au-delà de ce défaut : un composant qu'on ne peut pas exercer doit être décrit comme
**non vérifié**, jamais comme « prêt ». La section 5 est écrite dans cet esprit.

### 6.2 L'appareil de vérification confondait « non vérifiable » et « en échec »

Le premier défaut portait sur le système ; celui-ci porte sur l'instrument censé le juger, ce qui
est plus insidieux : un instrument faux ne se signale pas, il se contente de rendre un verdict.

`verify-on-host.sh` lançait `cargo test --workspace -- --ignored` en un seul bloc. Sur une machine
qui a gVisor mais pas KVM — le cas de n'importe quel serveur d'hébergeur en nuage, puisque la
virtualisation imbriquée n'y est pas offerte — les deux tests `needs_kvm` échouent par construction,
l'étape entière passe au rouge, et le rapport ne permet plus de voir que le niveau 1, le seul que
cette machine ait réellement débloqué, fonctionne. Le script promettait pourtant le contraire dans
son propre texte : « les tests correspondants seront ignorés et signalés comme tels ».

Trois changements :

1. Les tests matériels sont dispatchés **marqueur par marqueur**. Les marqueurs sont lus dans les
   sources (`#[ignore = "needs_gvisor"]`) plutôt que tenus dans une liste à part, pour qu'un test
   ajouté demain soit pris en compte sans toucher au script. Un marqueur inconnu n'est jamais
   supposé satisfait : il est rangé en « non vérifiable », car un vert obtenu par défaut est
   précisément ce qu'on cherche à éviter.
2. Trois issues distinctes remplacent deux : réussi, échoué, **non vérifiable ici**. La dernière a
   sa propre section dans le rapport et ne compte dans aucun des deux totaux.
3. Un mode `--niveaux` (`just verify-levels`) ne compile que `sandboxd` et s'arrête après les
   niveaux d'isolation. Sur une machine à mémoire courte, où la compilation de l'atelier entier
   risque d'être tuée, c'est la différence entre obtenir la réponse qui compte et ne rien obtenir.
   En mode complet, les niveaux passent désormais **avant** la compilation longue, pour la même
   raison.

Les diagnostics des tests de niveau 1 ont été repris dans la foulée. Ils affirmaient `sortie :
{stdout}` sur une chaîne vide, et repliaient un compte d'interfaces illisible sur la valeur `99`,
ce qui fait lire un échec de lancement comme une mesure de réseau. Ils rapportent maintenant le
code de sortie et l'erreur standard du moniteur, et distinguent « le compte est mauvais » de « rien
n'a tourné ». Sur une machine distante dont on ne lit que la sortie collée, cette distinction est
la moitié du diagnostic.

Les deux moitiés du mécanisme ont été exercées : matériel absent, les quatre tests sont déclarés
non vérifiables et aucun ne passe au vert ; un `runsc` factice placé sur le chemin, les deux tests
`needs_gvisor` sont réellement lancés, échouent, et rapportent `runsc: unable to start container:
permission denied`.

### 6.3 Le vert silencieux, une famille entière

En cherchant où faire tourner les tests matériels, une évidence : **l'intégration continue n'avait
jamais été regardée**. Les sept exécutions de la branche étaient rouges, dont deux pour des lints
que le `clippy` local, plus ancien, ne signale pas. Un rapport de phase qui annonce « tout est
vert » sans avoir ouvert la page des exécutions ne dit rien de plus que « c'était vert chez moi ».

En la réparant, trois endroits sont apparus où un test pouvait passer sans rien vérifier, tous du
même genre que le défaut 6.1 :

| Où | Ce qui se taisait | Ce qui l'empêche désormais |
|---|---|---|
| Tests de niveau 0 | sans espaces de noms, ils se taisent et passent | la CI exige `unshare --user` avant de les lancer |
| Tests du pont CDP | sans navigateur, ils se taisent et passent | `PROPHET_EXIGER_NAVIGATEUR=1`, posé par la CI, transforme l'absence en échec |
| Lancement de microVM | `spawn` réussi comptait pour un démarrage | Firecracker doit vivre et créer son socket d'API, sinon le lancement échoue en disant pourquoi |

Le dernier méritait mieux qu'une correction de confort. Créer un processus n'est pas démarrer une
machine virtuelle : une configuration refusée tue Firecracker dans la milliseconde suivante, et
rendre `Ok` à cet instant annonçait une isolation de niveau 2 inexistante — le défaut 6.1 à
nouveau, déplacé d'un cran.

Une course a été trouvée dans la même passe. Le pont CDP réservait un port libre, le relâchait,
puis confiait le numéro au navigateur : entre les deux, un autre processus pouvait le prendre. Le
défaut ne se manifestait que dans la suite complète, où plusieurs binaires démarrent ensemble,
jamais quand on lançait le fichier seul — la forme même du défaut qu'on classe à tort en
« hasard ». `Browser::launch_auto` absorbe la course en réessayant, et un test lance deux
navigateurs simultanément pour la provoquer plutôt que de l'attendre.

Enfin, le niveau 2 est vérifiable là où on ne l'attendait pas. Le journal de la CI se plaignait du
binaire Firecracker et des images, jamais de KVM : **les coureurs GitHub offrent la virtualisation
imbriquée**. Un serveur d'hébergeur en nuage ne le fera jamais ; l'intégration continue, si.

## 7. Écarts par rapport au plan

| Point du plan | Ce qui a été fait | Raison |
|---|---|---|
| MCP par la bibliothèque `rmcp` | protocole implémenté directement | même codec que l'IPC interne, surface utilisée petite et stable, une dépendance de moins |
| Projection des identifiants par le processus lui-même | poignée de main avec le gestionnaire | le noyau la refuse dans les environnements conteneurisés (ADR-0005) |
| Index vectoriel `sqlite-vec` | vecteurs et similarité en Rust sur SQLite | pas d'extension à charger, entièrement testable ; l'interface d'embedding reste abstraite |
| Interface en mode texte plein écran | vues pures rendues par la ligne de commande | les vues sont la partie qui porte la valeur et se teste ; l'habillage interactif reste à faire |

## 8. Recommandations pour la phase 1

1. **Rejouer ce rapport sur une machine complète**, par `just verify-host` — ou `just
   verify-levels` d'abord si la machine est petite, ce qui répond à la seule question qui compte
   sans compiler l'atelier entier. Le script sonde la machine, lance chaque test matériel dont le
   matériel est présent, et range les autres en « non vérifiable » sans les compter. Les lignes de
   la section 5 sont la première dette du projet ; les deux défauts de la section 6 montrent
   pourquoi elle se paie vite, et que l'instrument de mesure fait partie de la dette.
2. **Mesurer la ligne de base par captures d'écran.** Le rapport de coût repose aujourd'hui sur un
   modèle de tour de boucle ; il faut le confronter à un agent réel.
3. **Brancher un vrai client d'éditeur.** C'est le seul moyen de figer les noms d'options des
   clients, qui varient d'une version à l'autre, et de valider la délégation des permissions.
4. **Élargir la suite adversariale** à mesure que des composants arrivent, en particulier le
   compositeur et les adaptateurs d'applications existantes : chaque nouvelle voie d'entrée de
   contenu extérieur est une nouvelle famille de scénarios.
5. **Écrire `kernel/reduce.sh`.** L'image utilise aujourd'hui le noyau LTS tel quel ; la réduction
   de surface annoncée n'existe pas encore.
