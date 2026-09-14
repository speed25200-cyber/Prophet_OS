# ADR 0035 — Claude Code et Codex comme rôles du relais, lancés dans la session de l'humain

Date : 2026-09-13. Statut : accepté, première livraison.

## Contexte

Le relais de modèles (ADR 0034) fait avancer plusieurs modèles ensemble par rôles : réflexion,
exécution, code. Les modèles les plus capables de l'utilisateur sont ceux de ses abonnements
Claude et ChatGPT, qui n'entrent dans Prophet OS que par leurs clients officiels (Claude Code,
Codex), sans clé API (invariants du dépôt). Or le service ne peut pas les lancer : leurs
identifiants sont ceux de l'humain, dans le répertoire privé de chaque client, et l'OS ne les
lit ni ne les copie. Jusqu'ici, un client ne rejoignait une mission que si l'humain ouvrait
lui-même une séance (ADR 0026) ; un modèle ne pouvait pas *confier* une étape à Claude ou à
Codex.

## Décision

1. **Un lanceur de pilotes dans la session : `prophet-pilotd`.** Service utilisateur de la
   session graphique, sous l'identité de l'humain, sur un socket du répertoire des services
   que seul `agentd` peut appeler (`SO_PEERCRED`, groupe système). Deux méthodes :
   `pilot.status` (chaque client : installé, connecté, version, sondés par la commande du
   client lui-même) et `pilot.run {task, driver, intent, wall_time_s}` : écrit en 0600 la
   configuration MCP qui raccorde le pont `prophet-mcp` à la séance de `task`, lance le client
   **sans modification** en mode non interactif avec son profil privé (`CLAUDE_CONFIG_DIR`,
   `CODEX_HOME`, `GEMINI_CONFIG_DIR`) et cette configuration, attend sa fin (tué au délai),
   et rend sa réponse finale, bornée. Il ne décide d'aucun droit.
2. **Les rôles peuvent nommer un client.** `model.roles` et `model.preferred` admettent
   `driver:claude-code`, `driver:codex`, `driver:gemini` dans un profil de mission, à condition
   que sa confidentialité ne soit pas `local-only`. `task.options` ne propose un tel rôle que si
   le lanceur dit le client prêt ; sans lanceur, le rôle retombe sur le modèle local suivant de
   sa liste.
3. **La délégation vers un client est une séance.** `task.delegate {role}` résolu en `driver:x`
   prépare la sous-mission comme toute autre (jeton délégué par capd, filiation, budget prélevé,
   même propriétaire, rôle), mais le service n'y lance aucun modèle : il demande au lanceur de
   lancer le client, qui rejoint la séance par le pont, y appelle les outils sous le jeton
   délégué (chaque appel tranché par capd, journalisé, compté sous `client:<nom>`), se retire,
   et son texte revient au parent comme le résultat d'un outil. Si le client part sans se
   retirer, le service conclut la séance avec son texte ; s'il ne l'a jamais rejointe, la
   sous-mission échoue en le disant.
4. **Le service attend, sous l'identité de l'humain rien ne lui est prêté.** Le parent est
   bloqué le temps du client, comme pour une sous-mission locale ; le lanceur tourne dans la
   session, pas dans le service ; les identifiants restent dans le profil privé du client.

## Conséquences

- Un profil peut dire « Claude Code réfléchit, Codex code, Qwen exécute », et une mission
  locale peut confier du code à Codex et une réflexion à Claude Code, chacun dans une séance
  contrôlée par capd, avec le compte par modèle qui dit ce que chacun a coûté (en tours pour les
  clients, qui ne rendent pas leurs compteurs).
- Preuve livrée : un test avec les vrais capd, ledger, agentd, `prophet-pilotd` et CLI, où un
  client de remplacement joue Codex par le même chemin (séance, pont ou CLI, retrait) ; un
  second test montre le repli sur le modèle local sans lanceur. Le vrai Claude Code et le vrai
  Codex ne sont pas installés sur la machine de construction et leur connexion appartient à
  l'humain : leur exécution réelle est un essai `needs_claude_login` / `needs_chatgpt_login`,
  à mener sur une machine où l'humain s'est connecté.
- Complément du 14 septembre 2026 — **les clients sont les modèles principaux.** L'humain a
  tranché : Claude et ChatGPT mènent, le modèle local n'est qu'un secours. `task.prepare` admet
  donc un client officiel comme modèle d'une mission de premier niveau, nommé comme un modèle
  (`codex`, `claude-code`, ou `driver:codex`) : le profil doit l'admettre, le lanceur doit le
  dire connecté (sinon la préparation le refuse en disant comment se connecter), le plan se fait
  sur `driver:<client>` sans découverte du moteur local ; `task.start` le lance alors par le
  lanceur, comme une délégation mais sans parent — le client rejoint la mission par le pont,
  travaille sous son jeton, se retire, et un fil du service conclut ce qu'il aurait laissé
  ouvert. `task.options` propose les clients connectés en tête des modèles de chaque contexte,
  et tous les contextes de l'image les préfèrent (`driver:claude-code`, `driver:codex`, puis le
  local) ; les rôles suivent : Claude Code réfléchit et relit, Codex code et exécute. Sans
  lanceur ou sans client connecté, rien ne change : le modèle local reste proposé et lançable,
  ce qui garde les essais de la CI sans clients. Preuve : le test `une_mission_demarre_
  directement_sur_le_client_officiel_connecte` (faux Codex, vrais services : préparation sur
  `codex`, lancement, écriture par la séance, texte revenu, moteur local jamais sollicité) et
  son contraire sans lanceur. Et les deux cerveaux ensemble, sans aucun modèle du service
  (`codex_mene_la_mission_et_confie_la_relecture_a_claude_code`) : Codex mène la mission,
  écrit, confie la relecture par `task.delegate {role: "review"}` à Claude Code, qui rejoint
  sa sous-mission par le pont, rend son avis, et Codex conclut avec cet avis ; chaque client
  dans sa propre mission contrôlée, compté sous son nom, le parent portant le compte de
  l'enfant. Une limite vue en l'écrivant : une sous-mission travaille dans son propre espace,
  pris sur les fichiers de l'humain, et ne voit pas ce que le parent a écrit sans l'avoir
  publié ; le relecteur juge donc ce que l'auteur cite dans l'intention (ou ce qui est déjà
  publié), pas l'espace de travail du parent. Donner à l'enfant une vue en lecture de l'espace
  du parent est la prochaine marche du relais — faite le jour même par l'ADR 0039 : l'enfant
  part de l'espace du parent et son travail y revient.
- Limites : le client n'est pas confiné par la séance (ADR 0026) ; l'argument `-c` de Codex
  pour ses serveurs MCP est construit d'après sa documentation et non vérifié sur le binaire ;
  Gemini n'a pas de configuration MCP raccordée ; le parent ne voit du client que son texte
  final. L'image installe le lanceur dans la session (`prophet-pilotd`) et nomme son socket à
  `agentd` ; cette partie attend la CI.
