# `prophet-surface` — surface d'observation

- **Affichage** : `cage -s` sur `/dev/tty1`, sans gestionnaire de session ni de fenêtres
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

**Question restée ouverte :** la surface réclame `/dev/tty1` avec `StandardInput = "tty-force"`,
et `getty@tty1` le réclame aussi. Sur une machine sans écran, `cage` échoue et le conflit ne dure
que le temps des cinq tentatives ; sur une machine avec écran, personne ne l'a encore vu. Retirer
le getty de `tty1` rendrait la machine plus cohérente et moins rattrapable — c'est le seul
terminal où l'on puisse taper quand tout le reste manque.

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
