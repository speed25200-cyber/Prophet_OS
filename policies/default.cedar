// --- Interdits absolus : aucun jeton ne peut les lever. ---

// Les chemins sensibles (clés, identifiants, configuration système) ne sont jamais accessibles
// à un agent, même si son jeton les couvre.
forbid(principal, action, resource)
when { resource has sensitive && resource.sensitive == true };

// L'exécution de code arbitraire n'est permise qu'en microVM (niveau 2).
forbid(principal, action == Prophet::Action::"proc.exec", resource)
unless { context has sandbox_level && context.sandbox_level >= 2 };

// --- Classes d'actions automatiques. ---

// Lecture locale et observation : automatique, journalisée.
permit(principal, action in [
    Prophet::Action::"fs.read",
    Prophet::Action::"fs.list",
    Prophet::Action::"ui.read",
    Prophet::Action::"ledger.read",
    Prophet::Action::"memory.read",
    Prophet::Action::"model.use"
], resource);

// Écriture locale : automatique, car réversible par le sous-volume de tâche.
permit(principal, action in [
    Prophet::Action::"fs.write",
    Prophet::Action::"memory.write"
], resource);

// Sortie réseau, appel d'outil, action d'interface : autorisées par la politique ; la couche
// d'approbation traite séparément les cas irréversibles ou externes.
permit(principal, action in [
    Prophet::Action::"net.egress",
    Prophet::Action::"tool.call",
    Prophet::Action::"ui.act"
], resource);

// Exécution : permise ici, mais le `forbid` ci-dessus la restreint au niveau 2.
permit(principal, action == Prophet::Action::"proc.exec", resource);

// Sous-tâches et délégation.
permit(principal, action in [
    Prophet::Action::"task.spawn",
    Prophet::Action::"cap.delegate"
], resource);

// Lecture du journal de toutes les tâches : réservée, refusée par défaut (aucun permit).
// Capture d'écran : réservée, refusée par défaut (aucun permit).
