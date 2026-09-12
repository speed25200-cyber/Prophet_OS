# `prophet-surface` — surface d'observation

- **Affichage** : `cage -s` sur `/dev/tty7`, sans gestionnaire de session ni de fenêtres ; `tty1` reste à la connexion
- **Utilisateur** : `surface`, groupes `video input render prophet-system`
- **Lit** : `agentd` (tâches), `capd` (décisions), `sandboxd` (isolation)
- **Crate** : `crates/surface`

Ce que la machine montre en s'allumant. Un champ de courants : chaque tâche est un filament qui
traverse l'écran, à la vitesse de son débit d'étapes, et dont la clarté dit ce qui reste de budget.
Rien n'y est décoratif — si une particule bouge, c'est qu'une étape a été franchie.

Pas de barre de navigation : une barre de navigation suppose qu'on navigue.

## Ce qu'elle montre, et ce qu'elle ne montre pas

Elle lit le système. Quand un daemon ne répond pas, **sa part du champ se vide** et la ligne
d'isolation dit pourquoi. Elle ne garde pas la dernière image connue : l'écran montrerait des
tâches en train de courir alors que plus rien ne tourne — faux, et crédible, la pire combinaison.

Une scène d'exemple reste accessible par `--demonstration` ou `--capture`, pour une revue ou une
capture. Jamais par défaut, et jamais en remplacement d'une panne.

## Interaction

Deux touches, et c'est tout : <kbd>Entrée</kbd> accepte la décision montrée, <kbd>Échap</kbd> la
refuse. La réponse part vers `capd`, sur un fil séparé pour qu'un broker lent ne gèle pas l'écran.
Un échec de transmission est journalisé en erreur : une décision humaine perdue est exactement ce
qu'un système d'approbation ne doit jamais faire en silence.

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

**Ce qui reste inconnu, et qu'il ne faut pas croire réglé.** Que `cage` bascule effectivement sur
`tty7` et y affiche quelque chose n'est vérifié nulle part : aucun coureur d'intégration continue
n'a d'adaptateur graphique utilisable, et les six tests de la surface sont marqués `needs_gpu`.
C'était déjà vrai quand elle était sur `tty1` — on n'a jamais vu cette surface à l'écran. Le
déménagement ne dégrade donc rien de vérifié ; il supprime un mal, lui, mesuré. Si la bascule ne se
fait pas sur une machine réelle, le propriétaire aura sous les yeux une invite de connexion
utilisable et le message du service de repli, ce qui est très exactement le comportement voulu
quand l'écran ne peut pas s'allumer.

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
