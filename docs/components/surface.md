# `prophet-surface` — espace de travail natif

- **Affichage** : `cage -s` sur `/dev/tty7`, sans gestionnaire de session ni de fenêtres ; `tty1` reste à la connexion
- **Utilisateur** : `surface`, groupes `video input render prophet-system`
- **Lit** : `agentd` (tâches), `capd` (décisions), `sandboxd` (isolation)
- **Crate** : `crates/surface`

La surface propose un accueil, une conversation en flux, une sélection de modèle et les tâches
des services. La saisie, le presse-papiers, le défilement et les événements d'accessibilité passent
par egui/winit ; wgpu dessine l'interface. L'espace principal rassemble les missions reçues,
leur contexte et les décisions à examiner, dans un thème clair avec Inter embarquée. Les
compteurs viennent du moteur et des services, et les réponses du modèle sélectionné.

Les filtres et la sélection pilotent l'inspecteur. À petite taille, celui-ci remplace la liste
avec un retour aux missions. Une décision attend dans une bande persistante ; l'humain ouvre
son examen, lit les conséquences puis choisit. Les [captures et limites de la supervision](../reports/supervision-2026-09-13.md)
remplacent la direction Iris précédente. L'inspecteur lit maintenant `task.inspect` : plan et
accès, état exact, réponse finale, métadonnées de changements et parcours des états. Les missions
terminées restent accessibles. Les contrôles de lancement et d'arrêt envoient une commande
explicite hors de la boucle de rendu, affichent son acquittement ou son erreur et relisent l'état.
L'onglet Fichiers lit les deux versions par `task.change`, les compare en arrière-plan et propose
une copie exacte du texte final. Un travail altéré fait échouer l'actualisation ; les textes trop
grands et les binaires sont explicites. L'accès dépend du créateur Unix constaté par agentd.
L'application approuvée et le journal détaillé restent à intégrer. Voir l'[ADR 0018](../adr/0018-examen-des-versions.md).

La préparation d'une mission possède maintenant son propre écran : objectif humain, contexte
configuré dans le service, modèle disponible et limites. `task.prepare` conserve le plan, qui
est sélectionné dans la supervision dès réception ; il n'est jamais lancé automatiquement.
Le dialogue peut transmettre sa demande humaine au brouillon. Après une réponse perdue,
la référence conservée permet de relire le plan sans renvoyer la création. La fermeture de
l'interface ne conserve pas encore ces brouillons. Les profils du service doivent être configurés.

## Ce qu'elle montre, et ce qu'elle ne montre pas

Elle lit le système. Quand un daemon ne répond pas, **sa part du champ se vide** et la ligne
d'isolation dit pourquoi. Elle ne garde pas la dernière image connue : l'écran montrerait des
tâches en train de courir alors que plus rien ne tourne — faux, et crédible, la pire combinaison.

Une scène d'exemple reste accessible par `--demonstration`, avec ce mot inscrit dans l'image.
`--capture` seul ne fabrique plus de tâches. `--observation` conserve l'ancien renderer pour ses
tests visuels. Les commandes de capture et de démarrage figurent dans le README du crate.

## Interaction

Ctrl+Entrée envoie une demande au moteur choisi ; Entrée seule insère une ligne. La conversation
se déroule sur un fil réseau séparé, reste navigable pendant la génération et dispose d'une
interruption. Les réponses partielles restent marquées comme telles. La nouvelle conversation
écarte les événements tardifs de la précédente. Les décisions capd ont des boutons explicites.
Une erreur de transmission d'approbation est encore journalisée ; son acquittement visible dans
l'interface reste à intégrer.

Le modèle doit déjà être servi sur une adresse HTTP de boucle locale, configurable avec
`--endpoint` ou `PROPHET_MODEL_ENDPOINT`. Le service systemd permet la boucle locale et refuse
les autres adresses IP. La conversation directe n'expose pas d'outils système. Le téléchargement,
le démarrage des modèles, la persistance des conversations et le parcours agentique complet
restent à raccorder. Voir l'ADR 0008 et les exigences FRONTIER.

Une seule décision est montrée à la fois, la plus ancienne. Faire patienter quelqu'un est déjà
désagréable ; changer d'avis sur ce qu'on lui demande pendant qu'il patiente le serait davantage.

## Pourquoi elle appartient à `prophet-system`

`/run/prophet` est en 0770 pour ce groupe et les sockets en 0660 : sans lui, la surface ne
joindrait aucun daemon et afficherait un champ vide en permanence. Elle y est par `extraGroups`,
c'est-à-dire comme membre déclaré et non comme groupe principal — ce que `SO_PEERCRED` n'atteste
pas. Chaque daemon la refusait donc, et l'écran serait resté vide sans qu'aucune panne n'existe.
La règle du groupe, dans `prophet-daemon`, lit maintenant aussi `/etc/group`. Cela lui donne, au niveau du
socket, le même accès qu'un daemon — plus qu'elle n'en utilise. Le restreindre demande une notion
de méthode autorisée par pair que `prophet-ipc` n'a pas encore.

## Ce qui la fait démarrer, et qui tient à deux lignes

Le service est voulu par `graphical.target`. Cette machine n'a ni serveur X ni gestionnaire de
session : `services.xserver.enable` vaut `false`, et la cible par défaut de systemd est donc
`multi-user.target`. Une seconde ligne, quatre-vingt-dix lignes plus bas dans le même fichier,
rattache `graphical.target` à `multi-user.target` — et c'est elle qui fait que la surface démarre.

Les deux ne tiennent qu'ensemble, et rien à la construction ne le signalerait : retirer le
rattachement laisserait l'écran noir sans une ligne de journal, parce qu'une unité jamais lancée
n'échoue pas, ne journalise rien, et ne déclenche pas son service de repli.

`image/tests/installe.nix` démarre une vraie machine et vérifie que la surface est bien lancée.
C'est ce qui empêche qu'une des deux lignes parte sans l'autre.

**Le terminal disputé, et ce qu'il a coûté.** Cette section posait une question ouverte : la
surface réclamait `/dev/tty1` avec `TTYVHangup = true`, et `getty@tty1` le réclamait aussi.
Chaque tentative de la surface raccrochait le terminal, et on estimait que le propriétaire
attendrait une minute avant de pouvoir taper.

C'était optimiste. Le test du système installé a montré pire, le 12 septembre 2026, à vingt-deux
millisecondes près :

```
16:42:00.600  machine: sending keys 'essai-prophet\n'
16:42:00.686  prophet-surface.service: Scheduled restart job, restart counter is at 4
16:42:00.708  unix_chkpwd: password check failed for user (prophet)
```

Le mot de passe était le bon. Raccrocher le terminal pendant que quelqu'un tape ne lui fait pas
perdre une minute : cela lui fait lire **« Login incorrect »** alors qu'il n'a pas fait d'erreur,
sur une machine dont il vient d'effacer le disque, sans écran graphique pour lui dire pourquoi. Il
n'y a pas de pire moment pour donner à un système l'air de refuser son propriétaire.

**La seconde sortie a été prise** : la surface vit sur `/dev/tty7`, et `tty1` reste à la connexion.
Le septième terminal est celui que les serveurs graphiques occupent depuis toujours, et pour cette
raison exacte — NixOS ne fait naître de `getty` que sur `tty1` à `tty6`, donc personne ne se
connecte sur `tty7`. Une surface qui s'y débat ne prend plus en otage la seule porte d'entrée de la
machine. `image/tests/installe.nix` le garde, par un sous-test qui lit `TTYPath` plutôt que de
courir contre une relance : « la surface ne prend pas en otage le terminal de connexion ».

Le service de repli, lui, **reste sur `tty1`** : c'est là que le propriétaire regarde. Expliquer un
écran noir sur cet écran noir n'aurait servi à personne.

Le rendu Vulkan hors écran et une fenêtre Wayland sous WSLg ont depuis été exercés ; les tests
graphiques tournent aussi sur llvmpipe dans la CI. Ces preuves sont décrites dans le
[rapport de l'espace natif](../reports/espace-natif-2026-09-12.md) et le
[rapport Iris](../reports/interface-iris-2026-09-13.md). La bascule de Cage vers tty7 et le rendu
sur les pilotes d'un PC physique restent à vérifier. Le repli conserve une invite de connexion
et un message utilisable lorsque l'écran graphique ne peut pas démarrer.

## Si l'écran reste noir

La surface réessaie cinq fois en une minute, puis s'arrête — marteler toutes les deux secondes
remplirait le journal d'une seule erreur répétée, ce qui la rend plus difficile à trouver, pas plus
facile.

Un second service prend alors le relais et **écrit sur le terminal** ce qui s'est passé, où
chercher, et les deux commandes qui marchent quand même. Sans lui, le diagnostic partirait au
journal — que personne ne peut lire, puisqu'il n'y a pas d'écran.

## Diagnostic d'un écran noir

```sh
journalctl -u prophet-surface -n 50
vulkaninfo --summary        # un adaptateur est-il disponible ?
fc-list | head              # une police est-elle installée ?
```

La surface refuse de démarrer sans police plutôt que de se dessiner sans un mot et de paraître
fonctionner.
