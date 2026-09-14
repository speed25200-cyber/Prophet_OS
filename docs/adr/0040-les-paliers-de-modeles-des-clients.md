# ADR-0040 — Les paliers de modèles des clients : le meilleur pour réfléchir, le moins cher pour exécuter

- **Statut** : accepté
- **Date** : 2026-09-14
- **Tâche liée** : M8-T7

## Contexte

Le relais (ADR 0034) répartit une mission entre des rôles — réflexion, code, relecture,
exécution — pour que les tours coûteux soient rares et les tours nombreux bon marché. Depuis
l'ADR 0035 et son complément, ces rôles vont aux clients officiels de l'humain, Claude Code
et Codex. Mais un client n'est pas un modèle : Claude Code sert plusieurs modèles, du plus
capable au moins cher, choisis par son option `--model` ; nommer `driver:claude-code` pour
tous les rôles revenait à payer chaque étape au même prix, et à laisser au client le choix
du modèle pour la réflexion la plus exigeante. L'humain veut le contraire : les meilleurs
modèles pour la réflexion profonde et le code, des modèles peu coûteux pour l'intégration.

## Décision

1. **Une référence de client peut porter un palier.** `driver:<client>@<palier>` nomme un
   client officiel et le modèle que le lanceur lui demandera : `driver:claude-code@opus`,
   `driver:claude-code@haiku`. Le palier est un alias ou un identifiant que l'option de
   modèle du client accepte ; il est passé tel quel, jamais interprété par l'OS. Le manifeste
   le valide (lettres, chiffres, `.`, `_`, `-`), la sélection et le relais jugent la
   disponibilité sur le client seul (connecté ou non), et le catalogue admet un rôle à palier
   dès que le profil admet le client.
2. **Le lanceur passe le palier au client.** `pilot.run` reçoit `model` et l'ajoute à la
   ligne de commande : `--model` avant le séparateur pour Claude Code, `-m` après `exec` pour
   Codex, `-m` pour Gemini ; un client de remplacement le reçoit en fin d'arguments. Sans
   palier, le client garde son modèle par défaut.
3. **Les contextes de l'image répartissent les paliers.** Claude Code au palier `opus` pour
   la réflexion et le code, Codex puis Claude Code au palier `sonnet` pour la relecture (un
   autre regard que l'auteur, à moindre coût), Claude Code au palier `haiku` puis Codex pour
   l'exécution ; le modèle local ferme chaque liste. Les paliers se changent par
   `prophet.localEngine.paliers` (`reflect`, `code`, `review`, `execute`) sans toucher au
   reste — pour nommer un modèle plus récent dès que le client l'accepte.
4. **Lisible partout.** Le plan, l'inspection, la surface et la CLI nomment le client et son
   palier (« Claude Code (Anthropic) · opus ») ; `task.prepare` et `task.delegate {model}`
   acceptent `claude-code@opus` comme ils acceptent `claude-code`.

## Conséquences

- L'économie de tokens du relais devient réelle chez les clients : la réflexion et le code
  au modèle le plus capable, les étapes simples au moins cher, la relecture entre les deux.
  Preuves : la validation du manifeste, la sélection et la résolution des rôles avec un
  palier ; le lanceur qui place `--model`/`-m` au bon endroit pour chaque client et le passe
  au client de remplacement ; et, avec les vrais services, le faux Claude Code qui reçoit
  `--model sonnet` pour la relecture confiée par Codex.
- Limites : les alias (`opus`, `sonnet`, `haiku`) et l'option de Codex sont ceux que leur
  documentation décrit ; leur acceptation par les vrais clients reste à voir sur une machine
  connectée. Un palier refusé par le client fait échouer le lancement, ce que le client dit
  dans sa sortie et la mission dans sa raison. Codex garde son modèle par défaut tant que ses
  identifiants de modèles ne sont pas vérifiés.
