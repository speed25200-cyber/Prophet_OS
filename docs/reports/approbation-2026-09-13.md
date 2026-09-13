# Approbation depuis le service — 13 septembre 2026

Travail après `b8a7aa5`, lié à M4-T2, M8-T10 et M12-T4. Le moteur de publication de l'[ADR 0022](../adr/0022-publication-et-conflits.md)
est maintenant commandé par agentd, sous l'identité du créateur, depuis la CLI et l'atelier.

## Ce qui est raccordé

- `task.apply` et `task.undo` dans `prophet-agentd` : UID créateur exigé, mission `done`,
  index exact relu par SFS avant la première mutation, provenance de la mission posée sur les
  fichiers, reprise d'une intention interrompue par la même commande, une publication à la fois.
- `task.inspect` rend `publication` (état SFS), `can_apply` et `can_undo` ; les anciens clients
  qui ignorent ces champs continuent de lire la réponse.
- `prophet task apply <id>` et `prophet task undo <id>` passent par le service ; `prophet task show`
  affiche l'état de publication et la commande suivante. L'ancien undo sur disque est retiré.
- L'inspecteur de l'atelier affiche l'état de publication et deux boutons, « Appliquer à mes
  documents » et « Annuler la publication », avec reprise nommée quand une intention est
  interrompue. Les acquittements sont vérifiés comme pour le lancement et l'arrêt.
- Le journal reçoit `fs.commit`, ou `fs.undo` puis `task.rolled_back`, sous l'acteur `user`,
  sans le contenu des fichiers.
- Avant la première mutation, `task.apply` demande à capd un jeton neuf borné aux chemins de
  l'index exact et soumet chaque chemin à `cap.check` : plafond du manifeste, politique Cedar
  et révocation de la mission s'appliquent au moment de publier. Le manifeste est conservé
  avec le plan à cet effet. Un refus est journalisé (`policy.deny`, étape `publish`).

## Vérifications

| Contrôle | Résultat local |
| --- | --- |
| `cargo test -p agentd --test local_daemon` | 14 réussis, 0 échec, 1 ignoré (modèle réel) |
| `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings` | réussis |
| `cargo test --workspace --no-fail-fast` | 666 réussis, 1 échec, 29 ignorés ; l'échec est propre à l'environnement (ci-dessous) |
| `tools/verifier-les-services.sh`, `verifier-le-durcissement.sh`, `verifier-la-doc-des-travaux.sh` | réussis |

Trois nouveaux tests d'intégration lancent les vrais `prophet-capd`, `prophet-ledger`,
`prophet-agentd` et la CLI, avec une réponse HTTP contrôlée jouant le modèle :

1. Refus avant la fin ; mission terminée sans rien publier ; `open` puis `committed` ;
   fichier réel écrit ; second `apply` refusé ; `undo` retire l'ajout et rend `rolled_back` ;
   événements `fs.commit`, `fs.undo`, `task.rolled_back` avec l'acteur `user` ; contenu absent
   du journal ; état relu après redémarrage du service ; nouvel `apply` refusé en nommant l'état.
2. Une retouche humaine après publication fait refuser `undo`, laisse le document intact,
   conserve `committed` et n'inscrit aucun `fs.undo`.
3. La CLI publie et annule par le service, et `task show` nomme la commande suivante.
4. Une révocation par `cap.revoke` après la fin de la mission fait refuser `apply` par capd :
   aucun fichier n'est écrit, SFS reste `open`, le journal porte `policy.deny` avec le chemin
   et aucun `fs.commit`.

Les commandes Rust utilisent la chaîne épinglée `1.97.1`, hors `nix develop` : le conteneur
de cette session n'a pas Nix. Aucune VM n'a été lancée. L'unique échec,
`official::tests::un_fichier_de_configuration_ne_prouve_pas_une_connexion` dans `providers`,
tient au conteneur : un client `claude` y est installé et connecté par un fournisseur géré par
l'hôte, si bien que la sonde de session répond « connecté » pour un répertoire vide. Le code de
ce test et du pilote n'a pas changé ; le travail `check` de la CI de `b8a7aa5` le passe.

## Limites de cette preuve

Les tests tournent sous un seul UID : le service, le créateur et le propriétaire des fichiers
sont la même identité. Sur l'image installée, `agentd` n'a pas `CAP_CHOWN` ; la conservation
du propriétaire d'un document remplacé échouera avant toute mutation, et le remplacement d'un
fichier du propriétaire n'est donc pas livré. L'ajout d'un fichier neuf y appartiendra au
service. Un écrivain sous l'identité humaine reste le prochain jalon. La révocation est
prouvée ; le refus d'un chemin sensible par la politique n'est vérifié que par les tests
de capd, pas par une mission entière. Le conflit tardif laisse SFS en `conflict` ; l'atelier
le montre, sans le résoudre. Les boutons de l'atelier sont vérifiés par les tests du contrôleur, pas par une
capture ni par une session Wayland. Aucun critère de FRONTIER n'est coché.
