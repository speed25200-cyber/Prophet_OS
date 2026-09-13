# Séance d'outils MCP pour les clients de l'humain — 13 septembre 2026

## Ce qui est livré

- **`agentd` tient une séance d'outils** sur une mission préparée : `task.attach` ouvre pour le
  créateur exactement ce qu'une mission native ouvre (jeton capd, travail SFS capturé sur les
  périmètres, registre d'outils sous contrôle et journal), sans modèle ; `task.tools` rend les
  outils que le jeton couvre ; `task.call` exécute un appel compté comme une étape ;
  `task.detach` scelle les versions et conclut la mission en `done`, prête pour `task.change`,
  `task.apply`, `task.undo`. `task.cancel` conclut une séance en `cancelled`. Ces méthodes
  exigent l'UID créateur persisté et tournent sur un thread propre, hors de Tokio.
- **`prophet-mcp` est un pont stdio** : `initialize` attache la mission avec le nom du client,
  `tools/list` et `tools/call` sont relayés, la fin de l'entrée retire le client. Il ne tient
  aucun jeton, n'ouvre aucun fichier et n'exécute rien lui-même.
- **Une mission pour un client se prépare sans moteur local** : `task.prepare` avec
  `client: true` garde l'exigence du profil (modèle admis) mais dispense de la découverte ;
  `task.options` rend les modèles admis par profil (`preferred`).
- **Depuis un terminal** : `prophet task options`, `prophet task prepare [--client]`,
  `prophet task mcp-config`. **Depuis le bureau** : « Claude Code · mission » prépare la mission,
  écrit la configuration MCP dans le répertoire d'exécution de la session et lance le client
  avec `--mcp-config`.

## Preuves

- `crates/agentd/tests/mcp_client.rs`, avec les vrais capd, ledger, agentd et le vrai pont :
  outils limités au jeton (`fs.read`, `fs.write` pour le profil d'exemple), écriture dans le
  travail et non dans le home, refus hors périmètre sans fuite du contenu, retrait qui scelle
  les versions, examen puis publication par le créateur ; annulation pendant la séance ;
  mission inconnue refusée sans rien créer.
- `crates/agentd/tests/preparation.rs` : un modèle admis mais non découvert est refusé pour une
  mission native et accepté pour un client ; un modèle non admis reste refusé.
- `crates/prophet-cli/tests/task_service.rs` : rendu du catalogue, paramètres exacts envoyés
  par `prepare` (avec et sans `--client`), configuration MCP refusée pour une mission terminée.
- L'entrée du lanceur du bureau est attendue par le test du bureau dans `prophet-ouvrir --liste` ;
  elle n'a pas été exécutée localement (pas de Nix).

## Limites

- Le client reste un processus de l'humain avec ses propres outils natifs : la séance ajoute des
  outils contrôlés, elle ne le confine pas. Le pilote lancé par le service dans une sandbox
  (M8-T4) reste à livrer.
- Une séance vit avec le service : un redémarrage la perd et la mission passe en échec.
- Codex n'a pas d'entrée de lanceur ; sa configuration MCP passe par sa propre configuration.
- L'expérience réelle avec Claude Code connecté (`needs_claude_login`) n'a pas été exercée ici.
