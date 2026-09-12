# Espace de travail Prophet

Le binaire `prophet-surface` fournit une interface native Wayland : accueil, sélection de modèle,
conversation locale en flux, copie du texte, interruption et activité des services. Les tâches
et décisions viennent d'agentd/capd ; les conversations directes n'utilisent pas d'outils système.

L'accueil est l'espace de supervision : missions sélectionnables, filtres, contexte et examen
des décisions humaines. Préparer un objectif ouvre le dialogue local, sans lancer d'agent.
Le thème clair utilise Inter 4.1 embarquée sous licence SIL OFL et supprime la sculpture Iris.
Les [captures et vérifications](../../docs/reports/supervision-2026-09-13.md) proviennent du
binaire natif ; les scènes d'exemple sont explicitement marquées.

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
Les tests de l'ancien champ de courants restent disponibles sous `--observation`.
