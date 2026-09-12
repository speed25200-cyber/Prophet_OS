# Prophet OS

Un système d'exploitation PC conçu pour que des agents IA (Claude, GPT, Gemini, LLM locaux…) l'utilisent de façon rapide, sûre, observable et réversible, avec l'humain aux commandes.

**Pourquoi ?** Les OS actuels sont faits pour un humain avec des yeux et une souris. Un agent y travaille en aveugle : captures d'écran, clics aux coordonnées, permissions tout-ou-rien, aucun retour arrière. Prophet OS fait de l'agent un utilisateur de première classe : interface sémantique au lieu de pixels, capacités fines au lieu d'identité empruntée, snapshots et undo global, journal d'audit signé, routage vers n'importe quel modèle.

**Comment ?** Noyau Linux LTS durci et configuré sur mesure ; tout l'espace utilisateur repensé de zéro (runtime d'agents, broker de capacités, sandbox graduée, système de fichiers sémantique, protocole d'UI sémantique, routeur de modèles, mémoire, ledger).

📄 **[Plan complet](docs/PLAN.md)** : diagnostic, principes, architecture, spécification des composants, sécurité, feuille de route, équipe, métriques, risques, MVP.

🛠️ **[Plan d'exécution pour l'agent constructeur](docs/BUILD_PLAN.md)** : 14 jalons, 79 tâches avec critères d'acceptation vérifiables, à suivre dans l'ordre de [`docs/STATUS.md`](docs/STATUS.md). Instructions de travail dans [`CLAUDE.md`](CLAUDE.md), décisions dans [`docs/adr/`](docs/adr/), spécifications gelées dans [`docs/specs/`](docs/specs/).

---

## État du code

Le système est construit. **433 tests** verts, aucun avertissement de `clippy`. **81 des 88 tâches du plan** sont faites ; les 9 restantes exigent du matériel absent de l'environnement de construction et sont nommées dans [`docs/STATUS.md`](docs/STATUS.md).

```
prophet status          # ce que la machine sait faire, et ce qu'elle ne sait pas
prophet provider ls     # pilotes disponibles et sessions d'abonnement
prophet task ls         # tâches, même sans daemon en service
prophet task undo <id>  # défaire une tâche déjà validée
prophet log verify      # vérifier l'intégrité du journal
```

### Ce que fait le système, mesuré

| Propriété | Mesure |
|---|---|
| Contrôle de capacité | 11,6 µs par appel, pour 200 µs visés |
| Démarrage d'une sandbox de niveau 0 | 2,6 ms |
| Gel d'urgence de toutes les tâches | 124 µs |
| Observation d'une page web | 1 929 octets, contre environ 900 Ko pour une capture d'écran |
| Suite adversariale | 20 attaques sur 20 sans conséquence |
| La même tâche sur Claude, ChatGPT et un modèle local | mêmes outils, mêmes permissions, aucune clé d'API |

Le détail, y compris **ce qui n'est pas vérifié et pourquoi**, est dans [`docs/reports/phase0.md`](docs/reports/phase0.md).

### Organisation

| Crate | Rôle |
|---|---|
| `prophet-types` | jetons de capacité, manifestes, événements, sérialisation canonique |
| `prophet-ipc` | JSON-RPC sur socket Unix, identité du pair attestée par le noyau |
| `capd` | broker de capacités, politiques Cedar, approbations |
| `ledger` | journal en ajout seul, chaîné et scellé |
| `sfs` | espace de travail par tâche, diff, annulation |
| `sandboxd` | isolation graduée, du confinement à la microVM |
| `vault`, `egress` | secrets jamais vus par un modèle, unique voie réseau |
| `mcp-system` | outils système, contrôlés au même endroit pour tous |
| `agentd`, `providers` | cycle de vie des tâches, pilotes d'abonnement et boucle native |
| `sup`, `browser-bridge` | interface sémantique, navigation sans pixels |
| `memoryd` | mémoire cloisonnée par espace, épisodes dérivés du journal |
| `app-editor` | application de référence publiant SUP nativement |
| `shell`, `prophet-cli` | vues et binaire destinés à l'humain |
| `bench` | suite adversariale et mesure de coût |
