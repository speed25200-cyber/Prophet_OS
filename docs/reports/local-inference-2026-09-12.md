# Première exécution avec un modèle local réel

Exécutée le 12 septembre 2026, sous Ubuntu 24.04 / WSL2, noyau 6.6.87.2,
AMD Ryzen 7 5825U, 7,5 Gio visibles dans la VM. Serveur llama.cpp 0.4.0 fourni par
le nixpkgs épinglé dans flake.nix, quatre threads CPU, contexte de 4096 tokens.

Poids : `Qwen/Qwen3-0.6B-GGUF`, fichier `Qwen3-0.6B-Q8_0.gguf`, téléchargé depuis
le dépôt officiel Qwen sur Hugging Face. SHA-256 :
`9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031`.
Le modèle et le serveur restent des dépendances externes ; les poids ne sont pas dans le dépôt.

Le test `un_modele_reel_choisit_un_outil_ecrit_et_termine` a réussi en 12,44 secondes.
Le client a découvert le modèle dans `/v1/models`, envoyé une intention et le schéma d'un outil,
reçu un appel généré par le modèle, fait écrire un fichier temporaire, transmis le résultat au
modèle et reçu sa réponse finale. Le contenu du fichier et une consommation non nulle sont
vérifiés. Aucun tour du modèle n'est scripté dans ce test. Le nombre d'événements observé est huit.

```sh
llama-server -m /chemin/Qwen3-0.6B-Q8_0.gguf --alias qwen3-0.6b \
  --host 127.0.0.1 --port 18080 -c 4096 -t 4 --jinja --reasoning-budget 0

PROPHET_TEST_ENDPOINT=http://127.0.0.1:18080/v1 \
PROPHET_TEST_MODEL=qwen3-0.6b \
nix develop --command cargo test -p providers --test local_real -- --ignored --nocapture
```

Le test expose volontairement un seul outil écrivant un seul fichier temporaire. Il valide le
transport, l'inférence et la boucle native ; il ne prouve pas l'intégration des daemons, des droits
système, de l'interface ou de tous les modèles locaux. Ce temps unique est un résultat d'essai,
pas une mesure de performance comparative ni une promesse de latence.

Commandes ajoutées pour l'usage direct du moteur (sans outils système) :

```sh
prophet provider models --endpoint http://127.0.0.1:18080/v1
prophet provider chat --model qwen3-0.6b --endpoint http://127.0.0.1:18080/v1 "Bonjour /no_think"
```

Le client refuse les adresses distantes, les proxies d'environnement, les redirections, les
réponses trop grandes, les réponses tronquées, les outils inconnus et les consommations absentes.
Les moteurs doivent respecter le contrat Chat Completions pris en charge ; la compatibilité
avec Ollama/vLLM reste à exercer séparément. Un appel d'outil par tour est accepté pour l'instant.

Validation complémentaire : `nix develop --command just check` a réussi sous WSL2
(format, clippy avec avertissements interdits, tests du workspace et contrôles du dépôt).
Le test explicite `le_gel_global_arrete_le_processus_possede_par_le_daemon` a aussi réussi :
après un RPC au daemon, `/proc/PID/status` confirme l'état arrêté d'un vrai processus confiné.
Cela ne prouve pas encore le gel de tous ses descendants ; le contrôleur cgroup reste à intégrer.
