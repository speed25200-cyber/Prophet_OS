# providers

Transports et contrats des moteurs locaux, de la boucle native et des clients officiels.

Le client HTTP local et son flux sont exercés avec un modèle réel. Les profils Codex, Claude
Code et Gemini décrivent les commandes ; leurs méthodes d'exécution agentique ne sont pas encore
raccordées à sandboxd. Leurs capacités d'exécution restent donc à faux et leur catalogue de
modèles vide. La version ou la connexion d'un client n'atteste pas la disponibilité du pilote.

`OfficialDriver::diagnostic` résout un exécutable, interroge sa version et utilise sa commande
d'état de connexion. Il ne lit jamais les fichiers d'identifiants. La sonde attend cinq secondes
au maximum, borne la version à 4 Kio et ne capture aucune sortie d'authentification. Un client
muet est arrêté et attendu. Les variables d'environnement portant des clés API ne sont pas
transmises.

`jev` porte le décideur rapide de TypeSafe AI : protocole (`Question`, `Request`, `Response`,
vérification des réponses), transport par le proxy de sortie (`egress::EgressTransport`, jeton
de la tâche et référence de secret, jamais la clé), opérateur d'interface (`operator::Operator`,
un `ModelClient` qui décide `web.act` sur l'arbre SUP et rend la main au modèle génératif par
`DriverError::HandOver` ; `operator::Cascade` enchaîne les deux) et routeur
(`router::Router`, qui départage les candidats admissibles de `selection::eligible`). Jev est
optionnel et ne parle jamais au réseau directement. Voir la
[spécification](../../docs/specs/jev-decisions.md) et l'[ADR 0042](../../docs/adr/0042-jev-decideur-rapide.md).

`catalogue` est le catalogue des poids du système (`catalogue.json`, compilé dans les
binaires : adresse épinglée, empreinte SHA-256, hôtes permis) ; `pull` télécharge une entrée
par le socket d'egress, suit les redirections vers les seuls hôtes permis, et ne pose le
fichier qu'une fois taille, empreinte et en-tête GGUF vérifiés ; une coupure reprend par
`Range` ([ADR 0046](../../docs/adr/0046-les-poids-geres-par-le-catalogue-du-systeme.md)).

`memory` estime ce que le moteur réservera pour servir un poids — fichier, cache KV de la
fenêtre, calcul — depuis son en-tête (`weights`) ou son entrée au catalogue, le confronte à
`/proc/meminfo` (`fits`, `tight`, `too_large`) et lit dans `/proc` ce que les instances de
llama-server tiennent vraiment ; `weights` lit aussi ce que le gabarit de conversation
déclare (outils, réflexion) ([ADR 0047](../../docs/adr/0047-la-memoire-des-poids-estimee-avant-de-charger.md)).

Commandes de vérification, depuis `nix develop` :

```sh
cargo test -p providers
cargo test -p providers --lib -- jev            # protocole, transport, opérateur, routeur
cargo test -p providers --lib -- catalogue pull # catalogue et téléchargement vérifié
cargo test -p providers --lib -- memory weights # mémoire estimée, instances, gabarits
cargo test -p providers --lib needs_official_clients_versions_et_sessions_vierges -- --ignored --nocapture
```

Le second test requiert `PROPHET_TEST_CODEX` et `PROPHET_TEST_CLAUDE`, chemins absolus des vrais
clients. Il crée des configurations temporaires vierges. Il ne se connecte à aucun compte,
n'importe aucun identifiant et ne lance aucune génération.

Voir [le guide des pilotes](../../docs/components/providers.md),
[l'essai d'inférence](../../docs/reports/local-inference-2026-09-12.md) et
[les exigences de livraison](../../docs/FRONTIER.md).

Le paquet du moteur se construit avec `nix build .#llama-cpp`. Le contrat de grammaire
se vérifie avec `just test-local-engine` dans le shell Nix, sans poids ni inférence.
L'[ADR 0016](../../docs/adr/0016-grammaire-du-moteur-local.md) décrit le correctif qui
préserve la cardinalité demandée et interdit les répétitions vides d'appels.
Le [rapport](../../docs/reports/grammaire-locale-2026-09-13.md) distingue cette preuve
de la qualité des générations et du parcours réel agentd.

## Moteur de l'image

Le module NixOS `prophet.localEngine` installe le paquet corrigé de llama.cpp et expose un
premier profil documentaire dans la supervision. Le fichier de poids reste un choix explicite :

```nix
prophet.localEngine = {
  weights = "/var/lib/prophet/models/Qwen3-1.7B-Q8_0.gguf";
  model = "qwen3-1.7b";
  threads = 4;
};
```

Après avoir appliqué cette configuration, poser le GGUF vérifié dans ce répertoire avec un
mode `0644`, puis démarrer `prophet-local-engine.service`. Le modèle doit être compatible avec
les paramètres du service ; cette première configuration est exercée avec Qwen3 sur CPU.
Elle ne télécharge aucun modèle au démarrage. Sans `weights`, aucun moteur n'est lancé.

La surface et agentd partagent le port configuré, `8080` par défaut, sur `127.0.0.1` seulement.
Le contexte `Documents Prophet` prépare les fichiers dans un espace de travail privé sous le
home du propriétaire ; les originaux de `~/Documents/Prophet` restent à examiner et appliquer.
Le choix des poids depuis l'interface et l'accélération GPU restent à intégrer.

`just test-local-engine-vm` exerce les services installés avec les vrais poids. Le script
`tools/verifier-moteur-local.py --surface` exerce séparément le parcours graphique natif.
Voir le [rapport](../../docs/reports/moteur-installe-2026-09-13.md) pour les preuves et limites.
