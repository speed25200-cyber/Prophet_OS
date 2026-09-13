# Contextes web et navigateur sondé — 13 septembre 2026

## Ce qui est livré

- **Le catalogue de « Nouvel objectif » consulte le web.** `agentd::preparation` admet les
  hôtes de sortie (`net.egress`, y compris `*`), l'interface du navigateur piloté (`ui.read` et
  `ui.act` sur `browser`, rien d'autre) et les outils `http.fetch`, `web.open`, `web.tree`,
  `web.act`. Un outil réseau sans hôte est refusé au chargement. Le cadre de l'ADR 0015 tient
  par ailleurs : niveau 0, modèles locaux, périmètres relatifs au home couverts par une lecture.
- **Un contexte « Recherche sur le web »** dans `examples/missions/profils-locaux.json` et dans
  le catalogue de l'image (`image/modules/local-engine.nix`), sur `*` : chaque hôte reste
  tranché par capd à la requête, inscrit au journal avec sa cible, et un envoi de formulaire
  ou une méthode qui modifie attend la décision humaine.
- **Le service sonde son navigateur au démarrage.** `Browsing::probe` lance le programme sur
  une page vierge, dans un profil jetable, sans sortie réseau, et lui demande sa version.
  `task.options` rend `browser` (`program`, `ready`, `detail`) et, par profil, `web` ; la
  surface l'affiche dans le cadre de la mission et ne propose pas de lancer un contexte web
  sans navigateur qui répond ; `task.prepare` le refuse avant d'émettre un jeton ;
  `prophet status` porte le même verdict.
- **L'image nomme Chromium pour `agentd`** (`prophet.navigateur`, `null` pour retirer les
  outils web) et lève pour ce seul service deux entraves qui tuent un navigateur en silence :
  `MemoryDenyWriteExecute` (code généré par V8) et le `SIGSYS` du filtre commun sur `setrlimit`
  (`SystemCallErrorNumber=EPERM`).

## Preuves

- `crates/agentd/tests/preparation.rs` : catalogue (hôtes, navigateur, outils refusés sans
  hôte), service sans navigateur (état absent, préparation refusée avec la raison), service
  avec le vrai Chromium (sonde prête, contexte web préparé sans exécution).
- `crates/mcp-system/tests/web.rs` : la sonde rend la version ou nomme l'échec ; son profil
  jetable ne survit pas aux processus auxiliaires du navigateur.
- `crates/shell` : le statut dit si le navigateur répond ou ce que son absence coûte.
- `image/tests/services.nix` : la sonde répond « prêt » sous l'unité réelle, le contexte « web »
  est annoncé avec ses droits, `prophet status` le montre.
- `image/tests/local-engine.nix` : avec le modèle réel et le catalogue installé, une mission
  « Recherche sur le web » ouvre un témoin HTTP par le navigateur piloté, le relais, egress et
  capd, puis `task.inspect` relit l'adresse et le titre où l'agent a navigué.

Les tests Nix n'ont pas été exécutés localement (pas de Nix dans cet environnement) ; leur
verdict est celui de la CI de la révision qui porte ce rapport.

## Limites

- Le navigateur piloté tourne dans le service `agentd`, pas au niveau 2 (ADR 0024) ; les deux
  entraves levées le sont pour tout le service.
- La sonde prouve que le programme démarre et répond au protocole ; une page réelle n'est
  rendue que par une mission.
- Le catalogue reste une configuration de confiance, sans signature vérifiée.
- Le binaire `prophet-mcp` (stdio) refuse toujours de servir : les clients d'éditeurs n'ont
  pas encore les outils de Prophet OS par MCP.
