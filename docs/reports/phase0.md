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

## 6. Un défaut trouvé après coup, et corrigé

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

## 7. Écarts par rapport au plan

| Point du plan | Ce qui a été fait | Raison |
|---|---|---|
| MCP par la bibliothèque `rmcp` | protocole implémenté directement | même codec que l'IPC interne, surface utilisée petite et stable, une dépendance de moins |
| Projection des identifiants par le processus lui-même | poignée de main avec le gestionnaire | le noyau la refuse dans les environnements conteneurisés (ADR-0005) |
| Index vectoriel `sqlite-vec` | vecteurs et similarité en Rust sur SQLite | pas d'extension à charger, entièrement testable ; l'interface d'embedding reste abstraite |
| Interface en mode texte plein écran | vues pures rendues par la ligne de commande | les vues sont la partie qui porte la valeur et se teste ; l'habillage interactif reste à faire |

## 8. Recommandations pour la phase 1

1. **Rejouer ce rapport sur une machine complète**, par `just verify-host`. Le script sonde la
   machine, lance la suite complète y compris les tests marqués `needs_gvisor` et `needs_kvm`, et
   écrit un rapport. Les lignes de la section 5 sont la première dette du projet ; le défaut de la
   section 6 montre pourquoi elle se paie vite.
2. **Mesurer la ligne de base par captures d'écran.** Le rapport de coût repose aujourd'hui sur un
   modèle de tour de boucle ; il faut le confronter à un agent réel.
3. **Brancher un vrai client d'éditeur.** C'est le seul moyen de figer les noms d'options des
   clients, qui varient d'une version à l'autre, et de valider la délégation des permissions.
4. **Élargir la suite adversariale** à mesure que des composants arrivent, en particulier le
   compositeur et les adaptateurs d'applications existantes : chaque nouvelle voie d'entrée de
   contenu extérieur est une nouvelle famille de scénarios.
5. **Écrire `kernel/reduce.sh`.** L'image utilise aujourd'hui le noyau LTS tel quel ; la réduction
   de surface annoncée n'existe pas encore.
