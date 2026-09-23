# ADR-0046 — Télécharger les poids du catalogue du système, par egress, vérifiés avant d'être posés

- **Statut** : accepté
- **Date** : 2026-09-23
- **Tâche liée** : M8-T7

## Contexte

Le plan veut un « catalogue signé (`/var/lib/prophet/models/catalog.json`), téléchargement
vérifié via `egress` » et, pour critère, `prophet model pull qwen3-8b-q4` puis
`prophet model serve`. Jusqu'ici, un poids n'arrivait sur la machine que par la configuration
du système : le modèle par défaut est téléchargé par `nixos-install` (ADR 0033), un autre se
pose à la main sous `/var/lib/prophet/models`. L'humain ne pouvait pas ajouter un modèle depuis
la machine, ni retirer celui qu'il n'emploie plus.

Trois invariants bornent la réponse : toute sortie réseau passe par egress ; tout droit vient
d'un jeton émis par capd sous une politique Cedar ; aucun fichier n'est cru avant d'être
vérifié — un poids est une donnée que llama-server analyse, et un GGUF fabriqué est une entrée
hostile.

## Décision

- **Le catalogue fait partie du système.** `crates/providers/catalogue.json`, compilé dans les
  binaires (`providers::catalogue`). Chaque entrée donne une adresse épinglée sur une révision,
  l'empreinte SHA-256 publiée, les hôtes par lesquels le téléchargement peut passer, redirections
  comprises (`huggingface.co`, `*.huggingface.co`, `*.hf.co`), et, quand on la connaît, la taille
  exacte. Une entrée n'y entre qu'avec une empreinte relevée : aujourd'hui les deux modèles du
  relais, Qwen3 1.7B et 0.6B en Q8_0, dont le flake porte déjà l'empreinte. `PROPHET_MODEL_CATALOG`
  remplace le catalogue — une variable du service, posée par la configuration du système (l'essai
  des services en pose un), jamais par une tâche.
- **agentd télécharge** (`model.pull {id}`, puis `model.pulls`, `model.cancel`, `model.remove`,
  `model.catalog`). Il demande à capd un jeton de six heures sous un manifeste qui ne permet que
  `net.egress` vers les hôtes de l'entrée, et envoie chaque requête, redirections comprises, au
  proxy de sortie, jeton dans `Proxy-Authorization`. Une redirection vers un hôte que l'entrée ne
  permet pas arrête le téléchargement avant toute requête vers lui.
- **Rien n'est posé sans être vérifié** (`providers::pull`). Le fichier s'écrit sous un nom caché
  (`.<fichier>.part`) ; taille, empreinte SHA-256 et en-tête GGUF sont vérifiés avant qu'il prenne
  son nom, lisible par le moteur local (`0644`). Une empreinte fausse, un fichier plus long que
  prévu ou un en-tête illisible effacent le fichier ; une connexion coupée ou un arrêt le gardent,
  et le téléchargement suivant reprend par `Range` (ou repart de zéro si le serveur l'ignore).
- **Un dossier à lui.** `/var/lib/prophet/models/catalogue`, à agentd, lu par le moteur. C'est le
  seul que le téléchargement écrit et que `model.remove` touche ; les poids que la configuration
  pose ailleurs ne se retirent pas par là. `prophet model ls` et la page Modèles le lisent avec le
  reste.
- **Un poids déjà fourni n'est pas retéléchargé.** Le modèle par défaut vit dans `/nix/store`
  (ADR 0033), sous l'empreinte même que porte le catalogue ; agentd reçoit `PROPHET_WEIGHTS` et
  dit d'une entrée dont la configuration pose le fichier (même nom, ou `<empreinte>-<nom>` du
  magasin) qu'elle est fournie par le système (`provided`) ; `model.pull` la refuse.
- **La page Modèles** montre le catalogue : fourni, téléchargé et vérifié, en cours (barre de
  progression relue toutes les demi-secondes), échoué avec son motif, interrompu ; un geste par
  entrée — Télécharger, Reprendre, Arrêter, Retirer.
- **Le journal le dit** : `model.pulled` et `model.removed` (`id`, `file`, `sha256`, `bytes`),
  sous l'acteur qui l'a demandé.
- La CLI : `prophet model catalog`, `prophet model pull <id>` (progression, `--detach`),
  `prophet model cancel <id>`, `prophet model rm <id>`.

## Alternatives écartées

- **Un catalogue signé posé sous `/var/lib`** : il faudrait une clé de signature, sa rotation et
  sa révocation, pour obtenir ce que le système a déjà — un fichier que rien sur la machine ne
  réécrit, livré avec les binaires dans `/nix/store`. Un catalogue distant signé reviendra si le
  catalogue doit évoluer plus vite que le système.
- **Télécharger depuis la CLI de l'humain** : elle n'a pas le droit d'émettre un jeton (ADR 0044),
  et le fichier doit arriver là où le moteur, sous un autre compte, le lit.
- **Un tunnel `CONNECT` vers le dépôt** : egress y verrait l'hôte et rien d'autre ; la requête en
  forme absolue, terminée par le proxy, lui laisse voir l'adresse et appliquer sa détection.
- **Suivre les redirections sans borne d'hôte** : le jeton limite déjà la sortie aux hôtes de
  l'entrée ; refuser avant d'envoyer dit mieux pourquoi, et n'envoie rien.
- **Inscrire `qwen3-8b-q4`, l'exemple du plan** : son empreinte publiée n'a pas été relevée (le
  dépôt de poids n'est pas joignable d'ici) ; une entrée sans empreinte serait un téléchargement
  que rien ne vérifie.

## Conséquences

- Egress applique à ces requêtes sa détection ordinaire. Les adresses signées du CDN du dépôt
  sont longues ; au-delà de 2 000 caractères, egress les refuserait comme une exfiltration, et le
  téléchargement échouerait avec ce motif. L'essai `needs_network`
  `un_vrai_poids_du_catalogue_arrive_de_hugging_face_par_egress` le tranche : Qwen3 0.6B tiré de
  Hugging Face par le vrai egress, TLS et redirections compris, empreinte publiée vérifiée ; le
  travail d'isolation de la CI le lance quand le dépôt répond.
- `prophet model serve` n'est pas encore là : un poids téléchargé est au catalogue installé, il
  n'est pas servi tant que la configuration du moteur ne le nomme pas. Le mode routeur de
  llama-server sait lire un dossier de modèles ; le brancher sur `models/catalogue` est la suite.
- Le catalogue grandit par le dépôt, avec une empreinte relevée par entrée.
- Vérifié : essais unitaires du téléchargement (redirection, reprise, empreinte, en-tête, taille,
  refus du proxy, arrêt), parcours `agentd` avec les vrais capd, ledger et egress
  (`crates/agentd/tests/poids.rs`), et, sous systemd, l'essai des services télécharge d'un dépôt
  local par le vrai proxy sous le compte de l'humain.
