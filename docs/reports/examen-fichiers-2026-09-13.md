# Examen des fichiers — 13 septembre 2026

Révision de travail après `1640e1c`, dans le commit portant ce rapport.

Validation finale : **`just check` dans Nix, 631 tests réussis, aucun échec, 26 ignorés**,
avec format, clippy, construction des programmes et contrôles du dépôt. Les **16 tests
graphiques explicites passent** sur la présentation finale : six parcours de l'espace natif,
quatre parcours de mission avec vrais services et modèle contrôlé, six contrôles du renderer
historique. Le modèle réel est exercé séparément, avec trois réussites avant le dernier
ajustement visuel et une quatrième après celui-ci.

## Parcours livré

Dans le résultat d'une mission terminée, **Examiner** ouvre le fichier dans l'onglet **Fichiers**.
L'humain lit l'état de départ et la proposition, leurs lignes, leurs tailles et leurs permissions.
Il peut changer de fichier, copier le texte exact de la proposition et actualiser la lecture.
Les originaux restent intacts. Aucun bouton d'application n'est raccordé à cette vue.

Les deux contenus viennent des versions de la mission. La capture initiale est conservée ;
le service vérifie les empreintes avant de livrer les textes. Si le travail a été altéré, l'ancien
aperçu disparaît à l'actualisation et la vue explique le refus. Les fichiers binaires ou supérieurs
à 64 Kio reçoivent une explication, sans troncature silencieuse. Le calcul et la lecture se font
hors du thread graphique ; le défilement ne compose que les lignes visibles.

L'accès à `task.change` est réservé au pair Unix créateur, enregistré lors de la préparation
ou de la planification et conservé au redémarrage. Le champ utilisateur déclaré ne suffit pas.
Les anciennes missions sans versions ou sans propriétaire constaté n'obtiennent pas d'aperçu.
Les autres méthodes d'agentd conservent leur autorisation globale au socket : ce jalon n'est
pas une validation de l'ensemble des permissions interservices.

## Régressions et diagnostic

- Les premiers tests SFS ne compilaient pas avant l'ajout des API de revue. Les quatre scénarios
  passent ensuite : original modifié indépendamment, ajouts/suppressions, binaire et gros texte,
  contenu altéré même à taille identique, version supprimée réapparue, chemins invalides et liens.
- Un test de comparaison de 2 401 lignes échouait : les 2 400 lignes communes étaient indiquées
  comme supprimées puis ajoutées. Il passe après conservation des extrémités communes et calcul
  sur le seul milieu. La comparaison simplifiée des grands blocs reste explicitement nommée.
- Le test interservices a découvert une erreur de son propre redémarrage : il omettait le home
  et les sockets configurés au premier lancement. Le helper conserve désormais ce contexte.
  Le résultat et les versions sont relus après le redémarrage du vrai agentd.
- La CI graphique de `1640e1c` a échoué sur un bouton de lancement encore absent. Le plan était
  reçu, mais la liste des tâches attendait sa prochaine lecture. La capture de l'artefact montre
  le message de synchronisation. Le test attend désormais le bouton effectif et activé ; le
  comportement produit n'est pas modifié pour raccourcir cette attente.

La [CI de `1640e1c`](https://github.com/speed25200-cyber/Prophet_OS/actions/runs/34736553935)
réussit composants, isolation et protocole du moteur. Le travail NixOS charge Qwen3 sous le
compte séparé, puis échoue avant toute génération avec `Function not implemented` dans la capture.
Ce défaut est reproduit sous systemd avec le test SFS : `RestrictSUIDSGID=yes` bloque `openat2`
avec l'erreur 38 ; le même test passe avec cette seule restriction désactivée. La correction
est limitée à agentd, comme décrit dans l'[ADR 0018](../adr/0018-examen-des-versions.md).
L'évaluation Nix confirme son UID `agentd`, ses capacités vides, `NoNewPrivileges=true` et
`ProtectSystem=strict`. La VM ajoute désormais la lecture des versions et leur relecture après
redémarrage. Cette nouvelle exécution complète reste à établir en CI.
La sonde ajoute aussi un refus de lecture sous l'UID 0 avant et après redémarrage. La syntaxe
Python des deux scripts et l'évaluation de la dérivation Nix finale sont vérifiées localement.

Le travail ChatGPT du même run ouvre et ferme la fenêtre officielle sous l'UID 1000, puis
échoue au contrôle Fontconfig. Le problème reste ouvert ; aucune session authentifiée n'est validée.

## Essais avec un modèle réel

Le moteur corrigé et Qwen3-1.7B-Q8_0 exécutent trois missions graphiques consécutives avec
trois contenus aléatoires différents. Le fichier exact est lu depuis la proposition puis
vérifié dans l'aperçu de la surface. Les originaux restent intacts. Les trois essais passent.

| Essai | Contenu exact | Du lancement à l'aperçu vérifié | Images composées pendant l'exécution |
|---|---|---:|---:|
| 1 | `mission-2046180100` | 13,999 s | 114 |
| 2 | `mission-2964627990` | 13,158 s | 107 |
| 3 | `mission-2622248763` | 15,189 s | 126 |

Ces essais précèdent le dernier ajustement de présentation : l'en-tête de l'onglet Fichiers
est ensuite compacté pour laisser davantage de place au document en 1280 × 800.
Les durées des commandes complètes sont 90,098 / 18,456 / 19,964 s ; la première inclut la
compilation du test. Les images comptées ne mesurent pas une fréquence de rendu interactive :
le pilote du test impose des pauses entre les compositions.

Un **quatrième essai sur la présentation finale réussit** avec `mission-1719979888` :
18,546 secondes du lancement à l'aperçu vérifié, 160 images composées pendant l'exécution.
Sa commande complète dure 73,057 secondes, compilation comprise. La
[capture finale avec Qwen](../images/surface-mission-reel-fichier-1440.png) montre ce contenu exact.
Le serveur est arrêté par le vérificateur à la fin de chaque série. Une lecture du journal
WSL a expiré pendant la validation finale ; les tests ont continué et la connexion a repris,
sans redémarrage forcé de la distribution. La cause de ce délai d'observation n'est pas établie.

Environnement : WSL Ubuntu 24.04, Nix, CPU, rendu Vulkan llvmpipe, mêmes services réels sous
l'UID de développement. Le paquet est
`/nix/store/828hffb09kkgrb27f2pm11k1kc7r7r0m-llama-cpp-0.4.0` ; les poids portent l'empreinte
SHA-256 `061b54daade076b5d3362dac252678d17da8c68f07560be70818cace6590cb1a`.
Le [rapport du moteur](grammaire-locale-2026-09-13.md) donne leur provenance et les échecs
Qwen3-0.6B qui restent distincts. Trois répétitions d'une écriture ne valident pas la fiabilité
générale, plusieurs familles de modèles ni les performances d'un OS installé.

Reproduction, après construction des programmes du workspace et avec les poids déjà présents :

```sh
nix develop --command just check
VK_DRIVER_FILES=/chemin/vers/lvp_icd.x86_64.json \
PROPHET_CAPTURE_DIR=/chemin/vers/captures \
  nix develop --command cargo test -p surface -- --ignored --test-threads=1 --nocapture
VK_DRIVER_FILES=/chemin/vers/lvp_icd.x86_64.json \
  nix develop --command python3 tools/verifier-moteur-local.py \
    --llama-server /chemin/vers/llama-server \
    --weights /chemin/vers/Qwen3-1.7B-Q8_0.gguf --model qwen3-1.7b \
    --output /chemin/vers/un-nouveau-repertoire --repetitions 3 --surface
```

## Captures natives

La comparaison avec moteur contrôlé est capturée en
[1440 × 1000](../images/surface-mission-fichier-1440.png),
[1280 × 800](../images/surface-mission-fichier-1280.png) et
[640 × 900](../images/surface-mission-fichier-640.png).
Le [refus après altération](../images/surface-mission-fichier-altere-1440.png) retire le texte
précédemment confirmé. Les clics sur Examiner, Actualiser et Copier sont exercés dans les
widgets natifs avec les vrais services. L'en-tête compact laisse la place aux lignes à examiner.

## Limites

Le contenu peut être examiné ; l'application approuvée, les conflits avec les originaux, l'undo,
les checkpoints et la vérification sémantique de l'objectif restent à réaliser. La copie de départ
augmente l'espace disque nécessaire. Une empreinte valide ne certifie pas la qualité du travail.

Le graphisme reste une interface de supervision fonctionnelle. Cette étape ne livre pas encore
la direction spectaculaire demandée. Les captures hors écran ne démontrent ni une session humaine
installée complète, ni sa fluidité sur une matrice matérielle. Aucun critère complet de FRONTIER
n'est coché et aucune qualification SOTA n'est revendiquée.
