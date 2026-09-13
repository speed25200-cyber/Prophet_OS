# Outils MCP système

La bibliothèque fournit le registre, le contrôle des capacités, les outils et une session MCP
sur des flux délimités par des sauts de ligne. Le binaire `prophet-mcp` n'est pas encore raccordé
aux daemons : il refuse explicitement de servir. La présence d'un outil dans la bibliothèque
ne prouve pas que son service est intégré ; plusieurs outils renvoient encore une indisponibilité.

Chaque appel du registre vérifie le droit `tool.call`, puis la capacité sur la ressource concrète.
Une exigence inconnue, une cible de ressource absente, un niveau de sandbox inférieur au minimum
ou une tâche différente du sujet du jeton provoquent un refus avant l'exécution de l'outil.

La session exige `initialize`, puis `notifications/initialized`, avant `tools/list` et
`tools/call`. La version prise en charge est `2025-06-18`. Le transport limite chaque message
entrant à 1 Mio, refuse les lignes tronquées et ne traite jamais une notification comme un
appel d'outil. Les réponses sont exclusivement du JSON-RPC ; les diagnostics vont sur stderr.

```sh
nix develop --command cargo test -p mcp-system
```

Les outils fichiers ouvrent les racines `home` et `home/.prophet/tasks/<tâche>/work` sans lien
symbolique, puis résolvent les chemins relativement à leurs descripteurs avec `openat2`.
Les traversées de liens, sorties de racine et passages vers des montages descendants sont
refusés. Les lectures refusent aussi les fichiers spéciaux et les fichiers ayant plusieurs
liens physiques. Les écritures publient un fichier temporaire par remplacement atomique dans
le travail de la tâche. Elles ne tronquent jamais la cible d'un lien physique existant.

La recherche et la liste refont le contrôle des droits sur chaque descendant ; la révocation
et l'expiration sont revérifiées pendant le parcours. Les deux vues sont fusionnées, avec
priorité au travail. Un lien de travail invalide ne provoque pas de repli vers le fichier
d'origine. Les noms privés `.prophet` et `.prophet-write-*` sont inaccessibles.

Une lecture rend au plus 256 Kio ; une écriture accepte au plus 1 Mio de contenu. Les parcours
ont des bornes de nombre, de profondeur, de volume lu et de résultats, ainsi qu'un budget de
temps vérifié entre les opérations. `truncated` signale un plafond atteint ; `scoped` rappelle
que liste et recherche ne portent que sur le périmètre autorisé. Ce budget ne constitue pas
un délai maximal pour un appel noyau bloqué. Voir les [bornes exactes](../../docs/specs/mcp-system-tools.md).

`native::RegistryExecutor` permet à `providers::NativeDriver` de passer par ce même registre.
Un test explicite avec Qwen3 réel vérifie un appel contrôlé, la création dans le travail et le
diff SFS correspondant, sans appliquer le changement au fichier utilisateur. Chaque exécuteur
doit être créé pour une seule tâche par un lanceur de confiance : le modèle ne fournit jamais
son jeton, ses racines ou son niveau d'isolation.

`services::Services` fournit maintenant l'autorité distante et un journal confirmé par les
daemons capd et ledger. `agentd::local::Mission` l'utilise sur un thread dédié : le registre
ne fabrique plus de Broker local pour ce parcours. Un échec de journal interdit de poursuivre ;
si le résultat d'une écriture n'est pas confirmé, celle-ci peut déjà avoir eu lieu et ne doit
pas être répétée automatiquement. Voir le [contrat agentd](../../docs/components/agentd.md).
Cette intégration de bibliothèque n'active pas le binaire MCP autonome.

```sh
PROPHET_TEST_ENDPOINT=http://127.0.0.1:18080/v1 PROPHET_TEST_MODEL=qwen3-0.6b \
  nix develop --command cargo test -p mcp-system --test fichiers_isoles \
  un_modele_reel_ecrit -- --ignored --nocapture
```

Ces garanties supposent des racines fournies et protégées par le lanceur de confiance. Elles
ne remplacent pas une identité de service authentifiée, des espaces de noms privés, ni le
durcissement des commits/undo SFS face aux modifications concurrentes. Un descripteur reste
attaché à son répertoire même si un acteur privilégié déplace celui-ci. Le contrôleur et le
journal de cet essai MCP isolé sont en processus ; son journal n'est pas durable. Le nouveau
parcours agentd utilise les vrais services ; leur autorisation fine et la reprise durable
restent à réaliser avant d'activer `prophet-mcp` et le lancement depuis l'interface.
Voir l'[ADR 0012](../../docs/adr/0012-acces-fichiers-mcp.md)
et le [rapport de vérification](../../docs/reports/mcp-fichiers-2026-09-13.md).

La négociation suit le [cycle de vie MCP 2025-06-18](https://modelcontextprotocol.io/specification/2025-06-18/basic/lifecycle).
