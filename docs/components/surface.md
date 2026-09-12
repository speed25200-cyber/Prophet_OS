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

`/run/prophet` est en 0750 pour ce groupe et les sockets en 0660 : sans lui, la surface ne
joindrait aucun daemon et afficherait un champ vide en permanence. Cela lui donne, au niveau du
socket, le même accès qu'un daemon — plus qu'elle n'en utilise. Le restreindre demande une notion
de méthode autorisée par pair que `prophet-ipc` n'a pas encore.

## Diagnostic d'un écran noir

```sh
journalctl -u prophet-surface -n 50
vulkaninfo --summary        # un adaptateur est-il disponible ?
fc-list | head              # une police est-elle installée ?
```

La surface refuse de démarrer sans police plutôt que de se dessiner sans un mot et de paraître
fonctionner.
