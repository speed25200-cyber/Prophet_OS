# Espace de travail Prophet

Le binaire `prophet-surface` fournit une interface native Wayland : accueil, sélection de modèle,
conversation locale en flux, copie du texte, interruption et activité des services. Les tâches
et décisions viennent d'agentd/capd ; les conversations directes n'utilisent pas d'outils système.

L'accueil est un atelier de supervision : galerie de missions, filtres, recherche avec Ctrl+K
et examen des décisions humaines. La Focale agrandit le travail sélectionné ; une mission seule
dispose directement de l'inspecteur. La navigation graphite devient une barre inférieure à
petite largeur. Préparer un objectif ouvre un brouillon de mission : intention, contexte
et modèle effectivement disponible auprès du service. Le plan est préparé sans lancer d'agent.
Une mission déjà planifiée par agentd peut être examinée puis lancée depuis son plan. Son
inspecteur affiche les accès, l'état exact, le résultat textuel et les fichiers préparés.
L'onglet Fichiers compare les versions initiales et proposées, vérifiées par le service ; il
permet de copier le texte exact et d'actualiser l'examen. L'arrêt est explicite et sa confirmation vient du service. Les missions terminées
restent consultables. Une erreur de commande est affichée sans renvoi automatique.
Une mission terminée offre à son créateur « Appliquer à mes documents », puis « Annuler la
publication » ; l'état de publication lu dans SFS est affiché, y compris une intention
interrompue à reprendre ou un conflit conservé.
La direction Réacteur ([ADR 0025](../../docs/adr/0025-direction-visuelle-reacteur.md)) donne à
l'atelier une nuit, des plaques de verre à crochets, des jauges graduées, Inter embarquée en trois
graisses, un champ GPU où chaque mission reçue est un ruban de lumière qui avance au rythme réel
de ses étapes, et cinq accents de couleur au choix (`--accent arc|or|plasma|jade|nacre`,
`PROPHET_SURFACE_ACCENT`, ou la page Système, qui conserve le choix dans
`$XDG_CONFIG_HOME/prophet/surface.json`). Les [captures et vérifications](../../docs/reports/interface-reacteur-2026-09-13.md)
proviennent du binaire natif ; les scènes d'exemple sont explicitement marquées.

```sh
nix develop --command cargo run -p surface --bin prophet-surface -- \
  --fenetree --endpoint http://127.0.0.1:8080/v1
```

Le moteur doit déjà être lancé. Son téléchargement et son cycle de vie ne sont pas encore gérés
par cette interface. Les conversations restent en mémoire pendant la session. Ctrl+Entrée envoie
une demande, Entrée ajoute une ligne. Les actions de décision ont des boutons explicites.

Captures :

```sh
prophet-surface --capture espace.png --demonstration
prophet-surface --capture decision.png --demonstration --decision
prophet-surface --capture conversation.png --endpoint http://127.0.0.1:8080/v1 \
  --modele MON_MODELE --prompt "Bonjour"
```

`--demonstration` est explicite et inscrit dans l'image. Une capture ordinaire lit l'état réel.

Mesure du rendu, sans fenêtre ni capture, GPU attendu à chaque image :

```sh
prophet-surface --demonstration --mesure 120 --largeur 1920 --hauteur 1080
```

Elle imprime l'adaptateur, la scène, la médiane, le p95 et le maximum par image, et la
mémoire résidente. Sur un rastériseur logiciel elle mesure le processeur ; sur une carte
graphique, la carte. La fenêtre ne redessine un écran que si la scène a changé ou si
l'interface l'a demandé (champ vivant, saisie, transition) : au repos, le GPU dort.
Les tests de l'ancien champ de courants restent disponibles sous `--observation`.

Le raccordement de l'inspecteur utilise `PROPHET_AGENTD_SOCKET` (sinon `/run/prophet/agentd.sock`).
Le moteur des missions est configuré dans **agentd** par `PROPHET_LOCAL_ENDPOINT` ; `--endpoint`
configure uniquement la conversation directe. Le lanceur actuel accepte les outils natifs de
confiance au niveau 0. Un plan exigeant un pilote isolé reste non lançable depuis cette vue.
Le catalogue de contextes est configuré dans agentd par `PROPHET_MISSION_PROFILES` ; voir
le [guide de configuration](../agentd/README.md). Une demande humaine du dialogue peut devenir
un brouillon ; les réponses du modèle ne choisissent ni profil ni droits. Une préparation
incertaine conserve sa référence et peut être retrouvée par lecture, sans nouvelle création.
Les brouillons restent en mémoire ; les plans confirmés sont persistants dans le service.
La comparaison ligne par ligne est calculée en arrière-plan et son rendu est limité aux lignes
visibles. Les aperçus binaires, trop grands, manquants ou altérés ont des états explicites.
L'onglet Parcours relit dans le journal ce que l'agent a touché : chaque appel d'outil avec sa cible contrôlée et son issue, jamais le contenu. Voir les ADR
[0014](../../docs/adr/0014-inspection-et-commandes-de-mission.md),
[0015](../../docs/adr/0015-intention-et-profils-de-mission.md) et
[0018](../../docs/adr/0018-examen-des-versions.md) et [0019](../../docs/adr/0019-atelier-et-focale.md).
