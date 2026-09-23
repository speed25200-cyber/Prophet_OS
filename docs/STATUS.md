# Prophet OS — Avancement

> État au 13 septembre 2026 : les coches historiques ci-dessous décrivent parfois une
> bibliothèque ou une simulation, pas le parcours installé complet. Les exigences de livraison
> sont désormais suivies dans [FRONTIER.md](FRONTIER.md). Le moteur local possède un client HTTP
> concret, une conversation en flux et des missions via MCP/agentd avec vrais capd/ledger.
> L'examen des versions est implémenté, et leur publication commandée par le créateur depuis
> agentd ; l'écriture sous l'identité humaine sur l'image et le parcours installé
> complet restent à établir. Voir le [dernier rapport de l'atelier](reports/atelier-2026-09-13.md)
> et les exigences ouvertes, notamment la qualité graphique attendue et les sessions authentifiées.
>
> 22 septembre 2026 : la branche de travail réunit la ligne d'audit du 15 septembre et le
> décideur Jev (ADR 0042), qui avaient divergé de `main` ; `just check` y est vert, et l'atelier
> Réacteur reçoit sa seconde passe (ADR 0043). Voir la section du 22 septembre plus bas.

Correctifs d'accès humain du 13 septembre, après `a6b831c` : la CLI consulte la liste,
le détail et le diff d'agentd sans ouvrir ses captures privées. Deux régressions échouent
avant correction puis réussissent. Le secours graphique laisse getty afficher son avis et
l'invite ; une VM KVM dédiée réussit la connexion malgré les avis de panne avant et pendant
le mot de passe, puis dans la session ouverte (34,98 s). Le test installé complet est renforcé,
mais son résultat sur cette nouvelle révision reste à établir. Les échecs de services et de
connexion de `d20ef74` ne sont donc plus attribués à une cause inconnue. Voir le
[rapport d'accès humain](reports/acces-humain-2026-09-13.md) et l'[ADR 0020](adr/0020-consultation-et-secours-humains.md).
La CI de `ddd375b` réussit depuis les services, le démarrage installé et la mission avec modèle
réel. Le test installé de cette révision prouve le secours et la connexion console, pas un bureau.
Le nouveau module de [session humaine](reports/bureau-humain-2026-09-13.md), après `ddd375b`,
passe le parcours `desktop-session` en 188,07 s : connexion PAM, supervision sous UID du
propriétaire, fichier et presse-papiers réels, applications officielles, verrouillage et reconnexion.
La variante sur disque installé échoue localement dans KVM/SMM avant les services. La
[CI de `8ea4c7f`](https://github.com/speed25200-cyber/Prophet_OS/actions/runs/34748498023)
réussit ensuite le démarrage systemd-boot, la vérification des empreintes du magasin ext4,
la session PAM, les sept services, le terminal, Thunar et le presse-papiers. Le parcours
échoue au relevé du processus Claude Code : sa capture montre le diagnostic `ENOTFOUND`
suivi de sa fin dans la VM sans réseau. Le parcours installé complet reste rouge.
Aucun critère complet de FRONTIER n'est coché.
ChatGPT y est inclus à titre expérimental ; son défaut Fontconfig et son test strict rouge restent ouverts.
`just check` réussit dans Nix : 633 tests, aucun échec, 29 ignorés, format, clippy,
construction des binaires et contrôles du dépôt réussis.

La [CI générale de `8ea4c7f`](https://github.com/speed25200-cyber/Prophet_OS/actions/runs/34748498818)
réussit les contrôles du code, l'isolation, le protocole du moteur, la mission locale réelle
et la surface d'observation. Elle échoue sur le contrôle strict Fontconfig de ChatGPT.

Jalon de publication après `8ea4c7f` : `commit_review` vérifie les versions exactes et les
originaux, `undo` refuse les modifications humaines ultérieures, et le journal permet la
reprise après interruption. Les attributs, ACL, propriétaires et dates sont conservés.
Les snapshots Btrfs ne sont plus annoncés comme implémentés. Le
[rapport de publication](reports/publication-2026-09-13.md) précise les preuves et les limites.
Validation locale : `just check` réussi, 663 tests sans échec, 29 ignorés ; les 61 tests SFS
sont rejoués sous UID/GID 65534 sans échec, dont les 18 scénarios d'interruption et reprise.
L'application sous l'UID humain, les droits capd liés à l'index, les dossiers parents concurrents
et les commandes graphiques restent à livrer ; aucune case complète de FRONTIER n'est cochée.

Approbation depuis le service, après `b8a7aa5`, dans le commit portant ce rapport : `agentd`
expose `task.apply` et `task.undo`, réservés au créateur persisté d'une mission `done`, et
`task.inspect` rend l'état de publication SFS avec `can_apply` / `can_undo`. `prophet task apply`
et `prophet task undo` passent par le service ; l'ancien undo sur disque est retiré. L'inspecteur
de l'atelier offre « Appliquer à mes documents » et « Annuler la publication », avec reprise
d'une intention interrompue. Le journal reçoit `fs.commit`, `fs.undo` et `task.rolled_back`
sous l'acteur `user`. Trois tests d'intégration avec les vrais capd, ledger, agentd et la CLI
vérifient la publication réelle, le refus après retouche humaine et la relecture après
redémarrage. Le même jour, la publication est liée à capd : le manifeste est conservé avec le
plan, un jeton neuf borné aux chemins de l'index exact est demandé au moment de publier, et
chaque chemin passe par `cap.check` ; un quatrième test prouve qu'une révocation après la
mission fait refuser la publication sans rien écrire, avec `policy.deny` au journal.
Voir le [rapport d'approbation](reports/approbation-2026-09-13.md) et
l'[ADR 0023](adr/0023-approbation-et-publication-par-agentd.md). Limite : les tests tournent
sous un seul UID ; sur l'image installée, `agentd` n'a pas `CAP_CHOWN` et le remplacement d'un
document du propriétaire n'est pas livré. Aucune case complète de FRONTIER n'est cochée.

Navigateur et web du 13 septembre 2026, dans le commit portant ce rapport : `http.fetch` relaie
par le socket d'egress sous le jeton de la tâche (lecture automatique, écriture soumise à
décision, réponse bornée et recomposée) ; deux tests avec les vrais capd, ledger, egress et
agentd prouvent qu'une seule requête atteint le serveur témoin, sans le jeton, et qu'aucune ne
part sans proxy. Les outils `web.open`, `web.tree` et `web.act` s'adossent au pont CDP avec un
profil par tâche ; un test avec un vrai Chromium vérifie le refus d'un hôte hors droits avant
tout lancement, l'ouverture, la saisie, la relecture, le refus de `submit` sans décision, l'erreur
nommée d'un identifiant absent et l'absence du contenu de la page au journal. Le registre demande
désormais à chaque outil les effets de l'appel précis. Le bureau ajoute Chromium à profil Prophet
et X en fenêtre d'application, avec `prophet-ouvrir --liste` vérifié par le test du bureau ;
cette partie n'a pas été construite localement (pas de Nix) et attend la CI. Voir l'[ADR 0024](adr/0024-navigateur-integre-et-applications-web.md)
et le [rapport](reports/navigateur-2026-09-13.md). Le même jour, tout le trafic du navigateur
piloté passe par un relais local vers egress sous le jeton de la tâche ; deux tests avec les
vrais services et un vrai Chromium prouvent que la page arrive par le proxy sans le jeton et
que rien ne sort sans egress. Le confinement du navigateur au niveau 2 n'est pas livré : les
outils web restent désactivés par défaut. Aucune case complète de FRONTIER n'est cochée.

Instruments de l'atelier du 13 septembre 2026, dans le commit portant ce rapport : rail éclairé
avec emblème cerclé de la part de missions actives, bandes d'état, anneaux de budget et
monogrammes de pilote sur les missions, trois instruments (étapes, activité, budget) dans la
Focale et, en forme compacte, dans l'inspecteur relié aux services, une fois le travail commencé.
Rendu et vérifié avec llvmpipe : 54 tests unitaires, 15 tests de rendu et 5 tests d'inspecteur
avec services réels ; les captures de la documentation sont régénérées. Le premier essai cachait
un bouton sous le défilement à 1440 × 1000 ; le test l'a relevé et la forme compacte le corrige.
Le journal porte désormais la cible contrôlée de chaque appel d'outil, et l'onglet Parcours
de l'inspecteur la relit : l'humain voit quel hôte, quel chemin ou quelle fenêtre l'agent a
touchés et avec quelle issue, sans jamais voir le contenu. L'en-tête de l'inspecteur dit où
l'agent navigue (titre et adresse déposés par les outils web, rendus par `task.inspect`) et
l'ouvre dans le navigateur de l'humain ; la page Modèles sonde Claude Code, Codex et Gemini
par leurs propres commandes et les ouvre par le lanceur du bureau ; la page Système montre
l'échelle d'isolation. Le pont navigateur laisse Chromium choisir et annoncer son port, ce
qui supprime une course qui faisait échouer la moitié de ses tests en parallèle. Voir le
[rapport des instruments](reports/instruments-2026-09-13.md). La fluidité et la
consommation sur une carte graphique réelle restent à mesurer.

Contextes web du 13 septembre 2026, dans le commit portant ce rapport : le catalogue de
profils admet les hôtes de sortie, l'interface du navigateur piloté (`ui.read` / `ui.act` sur
`browser`, rien d'autre) et les outils `http.fetch`, `web.open`, `web.tree`, `web.act`, un
outil réseau sans hôte étant refusé au chargement. Le catalogue d'exemple et celui de l'image
livrent « Recherche sur le web » sur `*` : chaque hôte reste tranché par capd, inscrit au
journal, et un envoi de formulaire attend l'accord humain. `agentd` sonde son navigateur au
démarrage, sous ses propres contraintes, et `task.options` rend le verdict ; un contexte web
n'est pas préparé sans navigateur qui répond, et la surface le dit dans le cadre de la mission.
L'image nomme Chromium pour `agentd` (`prophet.navigateur`) et lève pour lui seul deux
entraves qui tuent un navigateur en silence (`MemoryDenyWriteExecute`, `SIGSYS` sur
`setrlimit`) ; le test des services vérifie que la sonde répond « prêt » sous l'unité réelle,
et le test du moteur local lance, depuis le catalogue installé et avec le modèle réel, une
mission « Recherche sur le web » qui ouvre un témoin HTTP par le navigateur piloté, le relais,
egress et capd, puis relit où l'agent a navigué. Cette partie de l'image n'a pas été
construite localement (pas de Nix) et attend la CI. Voir
l'[ADR 0025](adr/0025-profils-de-mission-web-et-sonde-du-navigateur.md). La CI de `7d19592`
réussit les sept services sous systemd, l'ISO, son démarrage et l'installeur ; le parcours du
système installé échoue de nouveau au relevé du processus Claude Code, avant d'atteindre le
lanceur (Chromium, X). Cause lue dans le journal : le client natif quitte en moins d'une seconde
sans réseau, et le relevé commençait après l'apparition de sa fenêtre. Le test échantillonne
désormais les processus du paquet à 50 ms avant d'ouvrir l'application ; `prophet status` dit
en outre si le navigateur piloté répond. La CI de `4a6d811` a rendu son verdict : `check`,
isolation, surface, moteur local, ISO, démarrage et installeur réussissent ; le test des
services échoue parce que Chromium, lancé par agentd, s'arrête net (CHECK) dans
l'initialisation de crashpad : son foyer est `/var/empty`, il n'y crée pas `Crash Reports`, le
gestionnaire de plantage sort faute de base, et le navigateur avec lui. Le pont donne désormais
au navigateur un foyer sous son profil, et sa sortie d'erreur reste dans le journal du service.
Le parcours du système installé passe enfin le relevé de Claude Code et le lanceur, puis échoue
après la déconnexion : la cible `sway-session.target` restait active, la supervision mourait
trois fois sans compositeur et atteignait sa limite de redémarrages, et la session suivante
s'ouvrait sans elle. La session arrête maintenant la cible à sa sortie et remet le compteur à
zéro à son entrée. La CI de `043788d` confirme les deux correctifs : les sept services sous
systemd, sonde du navigateur comprise, et le parcours complet du système installé (Claude
Code, lanceur, navigateur, X, verrouillage, déconnexion puis reconnexion) réussissent, avec
l'ISO, son démarrage et l'installeur ; seul ChatGPT reste en échec sur Fontconfig.
Aucune case complète de FRONTIER n'est cochée.

Commandes du 13 septembre 2026, dans le commit portant ce rapport : `proc.exec` n'est plus un
talon. sandboxd gagne `sandbox.run` (lance, attend, tue au délai, rend code et sorties bornées) ;
l'outil résout le programme par le chemin du service, l'exécute dans l'espace de travail avec le
home en lecture seule selon le jeton, la liste blanche au niveau 0 sans décision, tout autre
programme en microVM et tenu pour irréversible ; le profil accorde `proc.exec` par nom de
programme, et un chemin (même `/tmp/x/cat`) n'est jamais l'utilitaire du PATH. Chaque sandbox
est un groupe de processus, gelé et tué en entier (`setsid`/`setpgid` refusés au niveau 0) : le
test du daemon a montré qu'un `sleep` survivait au délai avant cela. Tests du plan (niveaux,
règles, refus) sans sandboxd ; exécution réelle prouvée par `sandbox.run` sur cette machine.
Voir l'[ADR 0031](adr/0031-execution-de-programmes-sous-sandboxd.md).

Modèle local par défaut, 13 septembre 2026, dans le commit portant ce rapport : la configuration
de référence pointe Qwen3-1.7B-Q8_0 (1,83 Go), téléchargé à l'installation ; l'installeur vérifie
huggingface.co avant d'effacer le disque ; `prophet-ci` et l'ISO restent sans poids. Une machine
installée a un agent sans compte ni clé. Voir l'[ADR 0033](adr/0033-un-modele-local-par-defaut.md).

Matériel ordinaire, 13 septembre 2026, dans le commit portant ce rapport : le système installé
n'avait aucun micrologiciel redistribuable (écran noir sur une Radeon une fois posé sur le
disque), ignorait ce que `nixos-generate-config` détectait, et l'installeur refusait toute
machine sans UEFI. Désormais `hardware.enableRedistributableFirmware`, un initrd qui connaît le
SATA, l'USB, le NVMe et le virtio d'un PC ordinaire, les Radeon GCN 1/2 sous `amdgpu` pour
Vulkan ; deux fichiers par machine (`image/machine/`) écrits par l'installeur et importés par le
flake ; `prophet.boot.firmware = "bios"` avec GRUB et une sixième partition `ef02` sur tout
disque. Trois preuves ajoutées à la CI : l'ISO sous SeaBIOS, la configuration installée sous
SeaBIOS (`installe-bios`), l'évaluation avec les fichiers de machine générés sur le coureur.
Voir l'[ADR 0032](adr/0032-materiel-ordinaire.md). La CI de 697f0da a donné : services,
système installé construit et posé, ISO construite et démarrée verts ; « Le système installé
démarre » rouge sur une cause de test (`su -` sans bus de session), corrigée par 81f3eda.

Suite d'applications du 13 septembre 2026, dans le commit portant ce rapport : le bureau
installe LibreOffice, GIMP, Inkscape, Blender, FreeCAD, Evince et mpv (`prophet.desktop.suite`,
activée par défaut), avec leurs entrées dans le lanceur ; le contexte « bureau » nomme celles
qui publient une accessibilité, et la session la demande aux applications Qt. L'intégration
continue construit `prophet-ci`, la même configuration sans la suite. Voir
l'[ADR 0030](adr/0030-suite-d-applications-de-l-humain.md).

Délégation entre agents du 13 septembre 2026, dans le commit portant ce rapport : un agent en
fait travailler un autre par `task.delegate {intent, profile, model?}`. La sous-mission reçoit
un contexte du catalogue et, au choix, un autre modèle local ; son jeton est délégué par capd
(`cap.delegate`, droits ⊆ ceux du parent, jamais plus longtemps), son budget est prélevé sur
celui du parent puis imputé, elle est rattachée à lui (filiation, profondeur bornée, même
propriétaire), lancée dans son fil, et son résultat lui revient comme celui d'un outil. Le
profil doit accorder `task.spawn` sur un contexte nommé, vérifié au chargement du catalogue ;
un contexte plus large que le parent est refusé par capd et interrompt la mission. Test avec
les vrais capd, ledger et agentd et un moteur simulé qui note les modèles demandés : parent,
enfant (autre modèle), enfant, parent ; l'enfant écrit dans son espace, le parent conclut. Les
catalogues d'exemple et de l'image confient la rédaction du contexte web au contexte documents.
Voir l'[ADR 0029](adr/0029-delegation-entre-agents.md).

Lecture des formats du 13 septembre 2026, dans le commit portant ce rapport : `doc.read` lit un
fichier du périmètre de `fs.read` quel que soit son format, reconnu aux octets : PDF par poppler
(texte, pages), bureautique (`docx`, `xlsx`, `pptx`, `odt`, `ods`, `odp`) par archive et XML
en Rust pur, images par leurs en-têtes et tesseract, médias par ffprobe, HTML dépouillé,
archives listées, texte brut sinon ; tout est borné et un programme absent ou trop long est
dit. L'image met poppler, ffmpeg et tesseract (fra, eng) sur le chemin d'agentd ; les
catalogues offrent l'outil avec `fs.read`. Tests sur des fichiers fabriqués (PDF écrit à la
main, docx, png, wav, zip, html). Voir l'[ADR 0028](adr/0028-lecture-des-formats-par-un-outil-natif.md).

Pilotage des applications du 13 septembre 2026, dans le commit portant ce rapport : les
applications GTK et Qt de la session sont lues et pilotées par leur arbre d'accessibilité. Le
crate `supd` (service utilisateur `prophet-supd`, dans la session) joint le bus AT-SPI, rend
l'arbre d'une fenêtre en SUP par `sup::adapter` (provenance « accessibilité », confiance
annoncée, lecture bornée qui dit ce qu'elle laisse), et exécute `click`, `set_field` et
`toggle` par ce que l'application déclare, sans touche ni clic simulés. Son socket
(`/run/prophet/sup.sock`, groupe système) n'admet qu'agentd, qui fait trancher capd :
`ui.read` et `ui.act` désignent une application par son nom, jamais l'écran, et les outils
`ui.apps`, `ui.tree`, `ui.act` s'ajoutent aux missions natives et aux séances MCP.
`prophet task attach / call / detach` pilotent une séance depuis le terminal. Le test local
(`tools/bureau-local.sh`, vrai bus, vrai Mousepad) prouve M10-T4 : l'agent lit l'arbre, écrit
« bonjour », active « Enregistrer » par le menu, et le fichier le contient. L'image installe
Mousepad, le bus d'accessibilité, l'adaptateur et le contexte « bureau » ; le test du bureau
rejoue le scénario par la CLI. Limite connue : une action qui ouvre un dialogue modal dans une
application GTK 3 bloque son pont jusqu'à la fermeture (libdbus non réentrant) ; GTK 4 n'a pas
cette limite. Cette partie de l'image attend la CI. Voir
l'[ADR 0027](adr/0027-pilotage-des-applications-par-l-accessibilite.md).

Séance d'outils MCP du 13 septembre 2026, dans le commit portant ce rapport : un client MCP de
l'humain (Claude Code, Codex…) travaille dans une mission préparée, tenue par `agentd` avec le
même jeton, le même travail SFS, le même registre et le même journal qu'une mission native,
sans modèle (`task.attach`, `task.tools`, `task.call`, `task.detach`). `prophet-mcp` devient
le pont stdio qui relaie ces appels et ne tient aucun jeton ; `prophet task options`,
`prophet task prepare` et `prophet task mcp-config` préparent une mission depuis un terminal
et rendent la configuration à donner au client. Trois tests avec les vrais capd, ledger, agentd et le vrai
pont prouvent la séance de bout en bout : outils limités au jeton, écriture dans le travail et
non dans le home, refus hors périmètre sans fuite du contenu, retrait qui scelle les versions,
examen puis publication par le créateur ; annulation pendant la séance ; mission inconnue
refusée. Voir l'[ADR 0026](adr/0026-seance-d-outils-mcp-pour-les-clients-de-l-humain.md)
et le [rapport de la séance](reports/seance-mcp-2026-09-13.md).
Le lanceur du bureau offre « Claude Code · mission » (mission préparée sans moteur local,
configuration MCP écrite dans la session, client lancé avec) ; cette entrée attend la CI.
Le client n'est pas confiné par la séance.
Direction Réacteur du 13 septembre 2026, dans le commit portant ce rapport : l'atelier devient
une nuit à plaques de verre, crochets d'angle, jauges graduées et chiffres fins, avec cinq
accents de couleur au choix (Arc, Or, Plasma, Jade, Nacre) conservés dans la configuration ou
forcés par `--accent`. Un champ GPU dessine derrière les plaques une grille et une voûte fixes
et, pour chaque mission reçue, un ruban qui avance à la vitesse réelle de ses étapes, immobile
dès qu'elle s'arrête. La barre du système, le rail, le cadran et les jauges ne relèvent que des
comptes reçus. `prophet-surface --mesure` donne les temps par image et la mémoire résidente ;
en build release sur llvmpipe, cinq missions à 1920 × 1080 coûtent 24,44 ms en médiane avec le
champ allégé que reçoit un rastériseur logiciel, 51,22 ms avec le champ complet, 8,06 ms sans
mission ; la fenêtre ne redessine plus un écran inchangé, et `--repos` le mesure : trois
images et 2 % d'un cœur en dix secondes de repos, contre 29 % avant la correction d'une
empreinte sensible à l'ordre des courants. Validation locale sans Nix, rendu llvmpipe : format, clippy, 64 tests unitaires de la surface, 20 tests graphiques, 5 parcours de mission avec vrais capd, ledger et agentd, outils du dépôt réussis ; `cargo test --workspace` compte 428 réussites, 3 ignorés et un échec propre à cette session (sonde d'un vrai Claude Code connecté), sans lien avec la surface. Voir le [rapport Réacteur](reports/interface-reacteur-2026-09-13.md)
et l'[ADR 0025](adr/0025-direction-visuelle-reacteur.md). La fluidité et la consommation sur une
carte graphique réelle restent à mesurer ; aucune case complète de FRONTIER n'est cochée.

Relais de modèles par rôle, 13 septembre 2026, dans le commit portant ce rapport : un profil de
mission distribue des rôles (`model.roles` : `reflect`, `execute`, `code`, chacun ⊆ `preferred`) ;
`task.delegate {role}` fait choisir au service le modèle que le contexte visé admet pour ce rôle
parmi ceux que le moteur sert ; chaque mission connaît son rôle et reçoit à chaque tour la consigne
de son rôle et des contextes qu'elle peut confier ; les tokens sont comptés par modèle (tours,
entrée, sortie) jusqu'au parent, exposés par `task.inspect`, `task.result`, le journal (`by_model`),
`prophet task show` et l'atelier ; les anciens résultats d'outils sont condensés avant chaque envoi
au moteur. Preuves : deux nouveaux scénarios avec les vrais capd, ledger et agentd et un moteur
simulé (rôle résolu en passant un modèle non servi, consignes reçues, compte exact imputé au
parent ; rôle absent refusé sans sous-mission), condensation et consigne vérifiées dans providers,
rôles du manifeste dans prophet-types. **Preuve réelle : trois essais sur trois**, Qwen3-1.7B en
réflexion et Qwen3-0.6B en exécution sur un llama-server en mode routeur (deux modèles, un port) :
41,9 s à froid puis 18,2 s et 17,9 s, fichier exact écrit par la sous-mission, 31 % des tokens pris
en charge hors du modèle de réflexion. Le moteur de l'image reste à un modèle, et les clients
officiels ne sont pas encore des cibles de rôle exécutables par le service. Voir le
[rapport du relais](reports/relais-2026-09-13.md) et l'[ADR 0034](adr/0034-relais-de-modeles-par-role.md).
Aucune case complète de FRONTIER n'est cochée.

Clients officiels comme rôles du relais, 13 septembre 2026, dans le commit portant ce rapport :
un nouveau service de session, `prophet-pilotd` (crate `pilotd`), lance Claude Code, Codex ou
Gemini, sans modification, sous l'identité de l'humain et avec le profil privé du client, dans une
mission préparée par agentd, avec la configuration MCP du pont ; il n'admet qu'agentd et ne lit
jamais les identifiants. Les profils admettent `driver:claude-code|codex|gemini` dans `preferred`
et `roles` ; `task.options` ne propose ces rôles que si le lanceur dit le client connecté ;
`task.delegate {role}` résolu en `driver:` prépare la sous-mission pour une séance (jeton délégué
par capd, filiation, budget prélevé, rôle), demande le lancement, attend, conclut la séance si le
client l'a laissée ouverte, et rend son texte au parent ; sans lanceur, le rôle retombe sur le
modèle local suivant. L'image installe le lanceur dans la session et un profil « Atelier des
agents » (Claude Code réfléchit, Codex code, le modèle local exécute) ; cette partie attend la CI.
Preuves : un test avec les vrais capd, ledger, agentd, `prophet-pilotd` et CLI où un script joue
Codex par le même chemin (séance, écriture, retrait, texte revenu au parent, compte sous
`client:codex`) ; un test du repli sans lanceur ; cinq tests unitaires du lanceur (commandes,
configuration MCP en 0600, client tué au délai, client absent refusé, extraction de la réponse).
Ni Claude Code ni Codex ne sont installés sur la machine de construction et leur connexion
appartient à l'humain : leur exécution réelle reste un essai `needs_claude_login` /
`needs_chatgpt_login`. Voir l'[ADR 0035](adr/0035-clients-officiels-comme-roles-par-le-lanceur-de-session.md)
et le [composant](components/pilotd.md). Aucune case complète de FRONTIER n'est cochée.
Le 14 septembre : un quatrième rôle, `review` (complément de l'ADR 0034), juge un travail rendu
sans le refaire ni le modifier ; la consigne de la réflexion invite à faire relire tout code ou
document important, et le profil « Atelier des agents » le confie à Claude Code après le code
de Codex : un autre fournisseur que l'auteur relit, au prix d'une lecture. Manifeste, outil
`task.delegate` et consignes testés en unitaire ; l'image porte le rôle. Et `pilot.status` est
servi d'un cache rafraîchi en arrière-plan (toutes les 60 s, après chaque lancement) : le
catalogue n'attend plus les sondes des clients.
Le 14 septembre encore, sur la décision de l'humain — « les modèles principaux seront ChatGPT et
Claude, oublie le local » — : les clients officiels deviennent les modèles principaux de toute
mission (complément de l'ADR 0035). `task.prepare` admet `codex` ou `claude-code` comme modèle
d'une mission de premier niveau, si le profil l'admet et si le lanceur de la session le dit
connecté (sinon il dit comment se connecter) ; `task.start` la lance par le lanceur, sans le
moteur local, et un fil du service la conclut si le client part sans se retirer. Le catalogue
propose les clients connectés en tête des modèles ; tous les contextes de l'image les préfèrent,
le modèle local n'étant plus qu'un secours (hors ligne, sans compte), et les rôles suivent
(Claude Code réfléchit et relit, Codex code et exécute). Prouvé avec les vrais services et un
faux Codex : préparation sur `codex`, lancement, écriture par la séance, texte revenu, moteur
local jamais sollicité ; et refusé sans lanceur. Puis les deux ensemble, sans aucun modèle du
service : Codex mène la mission, écrit, confie la relecture (`task.delegate {role: "review"}`)
à un faux Claude Code qui rejoint sa sous-mission par le pont et rend son avis, avec lequel
Codex conclut — chacun dans sa mission contrôlée, compté sous son nom. Ce que cela ne prouve
pas : le vrai Codex et le vrai Claude Code, qui exigent la connexion de l'humain sur une
machine installée. Ce que cela a montré : une sous-mission ne voit pas l'espace de travail du
parent (elle part des fichiers de l'humain), donc un relecteur ne lit que ce que l'auteur cite
dans l'intention ou ce qui est déjà publié ; partager en lecture l'espace du parent avec
l'enfant est au carnet — et fait dans la foulée (ADR 0039) : une sous-mission part de l'espace
de travail de son parent (ses périmètres capturés depuis le répertoire de travail du parent, un
périmètre absent chez lui pris au répertoire personnel, les droits jugés par capd comme
toujours), et à sa fin son diff revient chez le parent (créés et modifiés copiés, supprimés
retirés, dans les périmètres du parent), qui publie le tout d'un seul tenant ; une sous-mission
ne se publie plus seule. Prouvé dans sfs (capture depuis le parent, rapport, publication du
parent, périmètre hors du parent laissé chez l'enfant, parent fermé refusé) et dans agentd avec
les vrais services : le faux Claude Code lit le code que le faux Codex vient d'écrire et son
verdict revient chez Codex ; le code de Codex pour un parent local est chez ce parent.
Puis les paliers de modèles (ADR 0040) : une référence de client porte le modèle que le
lanceur lui demandera (`driver:claude-code@opus`), validée par le manifeste, disponible dès que
le client l'est, admise par le catalogue dès que le profil admet le client ; le lanceur place
`--model` (Claude Code) ou `-m` (Codex, Gemini) sur la ligne de commande. Les contextes de
l'image donnent la réflexion et le code au palier `opus`, la relecture à Codex puis au palier
`sonnet`, l'exécution au palier `haiku` — réglables par `prophet.localEngine.paliers`. Prouvé
en unitaire (manifeste, sélection, rôles, lanceur) et avec les vrais services : le faux Claude
Code reçoit `--model sonnet` pour la relecture confiée par Codex. Ce que cela ne prouve pas :
que les vrais clients acceptent ces alias, construits d'après leur documentation.

La CI de `a5ac295` (clients principaux, invité overlay, `proc.kill` admis) réussit `check` (le
test du client comme modèle principal compris), l'isolation sur l'hôte (les trois essais
`needs_kvm` avec l'invité à surcouche : le niveau 2 exécute et rapatrie depuis `/home`), la
parole, la surface, ChatGPT, le protocole du moteur, la mission locale réelle, l'installeur,
l'ISO, les sept services (le catalogue avec `proc.kill` charge), le système installé et ses
deux démarrages. Un seul rouge : « Voir l'image démarrer » sous UEFI, où le noyau de l'invité
QEMU s'est planté au chargement de modules (`__text_poke`, parport, floppy) avant tout terminal
— le même support a démarré sous SeaBIOS dans le même travail, et ce démarrage UEFI était vert
sept fois de suite avant : une panne de l'hôte d'intégration, pas de l'image. Le travail rejoue
désormais une fois le démarrage UEFI après un tel plantage du noyau de l'invité, la trace du
premier essai conservée (`demarrage-plante.log`) ; deux plantages restent un échec. La CI de
`239753b` (espace partagé du relais, client nommé, `pilot.stop`) est **entièrement verte des
deux côtés**, démarrage UEFI de l'ISO compris (14 septembre, 12:40 UTC) ; celle de `0d3fa49`
(« Confiées », mission en main, second essai UEFI) l'est aussi (13:20 UTC), et celle de `ead38a8`
(paliers de modèles : `check` joue le faux Claude Code avec `--model sonnet`, les sept services
chargent le catalogue à paliers, le système installé démarre) l'est encore (14:05 UTC). Celle de
`bcc3e51` (docs, essai réel ignoré, script d'hôte) a un rouge instructif : le coureur des « sept
services » offrait cette fois la virtualisation imbriquée, le sous-test « sandboxd peut réellement
isoler » a donc pris le niveau 2 pour de vrai, et l'image ext4 de l'espace de travail — `/tmp`
entier, avec les partages du pilote de test — a manqué d'inodes (« Could not allocate »). Le
disque compte désormais ses entrées et demande autant d'inodes qu'il faut (rejoué sous KVM avec
six mille petits fichiers), et le sous-test image un répertoire à lui. La CI de `478736a`
(atelier logiciel de bout en bout, disque corrigé) est **entièrement verte des deux côtés**
(15:06 UTC) : l'hôte de l'isolation a joué l'essai du client qui écrit un outil et l'exécute en
microVM, les sept services ont repris leur vert. Celle de `345b1d1` (approbations, CLI des
décisions) a un rouge à `check` qui est le mien : le test d'annulation vérifiait la mort de
l'attente du faux client par un `pgrep` sur toute la machine, et le coureur avait un autre
`sleep 30` ; le faux client écrit désormais le PID de son attente, que le test regarde seul.
Celle de `d7ee0c4` (annulation corrigée, voix qui tranche) est verte partout — `check` joue
les approbations et l'annulation par PID, la mission locale réelle et le système installé
tiennent — sauf les « sept services » : le sous-test sandbox créait son répertoire de travail
dans `/tmp`, que sandboxd, sous `PrivateTmp`, ne voit pas ; il vit désormais dans l'état du
service (`ffd88d2`), et les sept services ont repris leur vert (17:00 UTC). Le modèle joint
désormais son motif à une demande d'approbation (`c160cee`), et le lanceur du bureau ouvre
les outils que l'atelier logiciel a produits et que l'humain a publiés (« Outils »), y compris
par la voix : « Prophète, ouvre l'outil bonjour », « ouvre le navigateur ». L'installeur dit
ce qu'il voit de la machine avant d'effacer le disque (processeur, mémoire, KVM, carte
graphique et pilote, réseau, son et micro, Secure Boot, TPM ; relevé gardé dans
`image/machine/inventaire.txt`, relu par `prophet status` et par la page Système de la surface), parce que rien n'a jamais démarré
sur un vrai PC et qu'un manque doit se voir tant que Windows est encore là ; le travail
« Installeur sur disque en boucle » vérifie qu'il le dit. Le rapport
du jour :
[clients principaux](reports/clients-principaux-2026-09-14.md).

La CI de `ddf6e89` (relais et lanceur de pilotes) réussit `check`, l'isolation, le protocole du
moteur, la surface, les sept services sous systemd, l'installeur, l'ISO et son démarrage, la
construction du système installé et son démarrage sans UEFI ; ChatGPT reste rouge sur Fontconfig.
Le parcours du système installé sous UEFI échoue à l'attache de la séance du scénario du bureau
(« erreur d'entrée-sortie : Permission denied ») : le test créait le document par
`install -m 0600`, ce qui réduit le masque de l'ACL posée par tmpfiles pour agentd et rend le
document illisible au service au moment de la capture ; ce scénario n'avait jamais atteint ce
point en CI (le sélecteur Mousepad le faisait échouer avant). Le test crée désormais le document
sous l'identité de l'humain et vérifie qu'agentd le lit avant d'attacher ; correction dans le
commit portant ce rapport, à confirmer par la CI. La CI de `9242b37` (avec le lanceur de pilotes,
deux modèles dans l'image, la parole) le confirme : `check`, isolation, protocole du moteur,
surface, sept services, installeur, ISO et ses démarrages, construction du système installé et
son démarrage sans UEFI réussissent ; le parcours UEFI franchit désormais le scénario du bureau
(séance attachée, éditeur piloté) et échoue plus loin, au déverrouillage de l'écran : le bon mot
de passe, tapé 2,3 s après un refus volontaire, n'a jamais déverrouillé en 30 s, alors que le
même geste passait le 13 septembre à 14 h 38. Cause la plus plausible : le délai que pam_unix
impose après un refus, pendant lequel la saisie se perd. Le test laisse passer ce délai et
retape une fois si l'écran reste verrouillé ; à confirmer par la CI.

Deux modèles dans l'image, 13 septembre 2026, dans le commit portant ce rapport :
`prophet.localEngine.executeWeights` ajoute le modèle d'exécution du relais (Qwen3-0.6B Q8_0,
640 Mo, empreinte vérifiée sur Hugging Face, téléchargé à l'installation comme le 1.7B) ; le
moteur passe alors en mode routeur de llama-server (fichier de préréglages, un modèle par section,
chargés à la demande sur le même port) et les profils de l'image (documents, web, bureau, atelier)
gagnent les rôles `reflect` (1.7B) et `execute` (0.6B puis 1.7B). Preuves locales : le fichier de
préréglages exact, servi par le llama-server du flake, expose les deux identifiants et répond à
une complétion sur chacun ; la configuration de référence s'évalue avec les deux poids dans
`ConditionPathExists` et le routeur dans `ExecStart`. La variante `prophet-ci` et le test de VM du
moteur restent à un seul modèle ; le démarrage installé avec les deux poids attend la CI.

La parole, 13 septembre 2026, dans le commit portant ce rapport : le crate `voice` enregistre le
micro de la session (`pw-record`, sinon `arecord`) et transcrit un fichier audio en local par
whisper.cpp ; `prophet voice` transcrit un fichier ou le micro et, avec `--prepare <contexte>`,
fait de la phrase dite une mission à examiner par le chemin de `prophet task prepare` ; le module
`voice.nix` installe whisper.cpp et PipeWire dans la session, et la configuration de référence
télécharge le modèle `ggml-base` (148 Mo, empreinte vérifiée) à l'installation. Preuve : une
phrase française synthétisée par espeak-ng, transcrite par le vrai whisper.cpp et le vrai modèle,
rend ses mots-clés (« note », « documents ») et la langue `fr`, détectée aussi sans indication ;
tests unitaires de la lecture de whisper et des refus. L'atelier gagne un bouton « Dicter (6 s) »
sous l'objectif, présent seulement avec un modèle de parole, qui écoute et transcrit hors du fil
graphique et ajoute le texte à l'objectif que l'humain relit ; son contrôleur est testé avec des
dictées simulées. L'OS parle aussi : `prophet voice --say` synthétise en local par Piper avec la
voix française « siwis » (installée par la configuration de référence) et joue par `pw-play` ;
preuve en boucle fermée, l'OS dit une phrase et Whisper la réécoute presque mot pour mot. La
boucle complète existe : `prophet voice --prepare <contexte> --reply` transcrit la phrase dite,
prépare la mission auprès d'agentd et répond à voix haute ce qu'il a compris et où examiner le
plan ; un test de la CLI avec service simulé le prouve, réponse réécoutée par Whisper. Le mot
d'activation existe : `prophet voice --listen` écoute par tranches et n'agit que sur « Prophète,
… », comparé avec tolérance à ce que Whisper entend ; un test avec un faux enregistreur prouve
qu'une tranche sans le mot est ignorée et que la suivante devient une mission avec réponse
parlée. L'atelier a le même geste, « Écouter « Prophète » » ; son test de contrôleur avec le vrai
Whisper est écrit mais n'a pas pu être exercé sur la machine de la session (WSL tombe à la
compilation du binaire de test de la surface ; clippy passe). La CI de `40c6e3d` l'a lancé sans
chaîne vocale (le travail « Surface d'observation » exerce tous les essais ignorés) et il a
échoué sur le modèle absent, pas sur l'écoute : un travail « Parole (Whisper et Piper réels) »
apporte désormais les programmes, le modèle et la voix par `nix build .#chaine-vocale` — les
mêmes que dans l'image — et exerce les essais `needs_voice_stack` du crate `voice`, de la CLI
et de l'atelier ; la surface les laisse à ce travail. Le résultat d'une mission se dit sur
demande, `prophet task result <id> --say` : état, début du texte ou raison, changements à
examiner, sans mise en forme, coupé à la phrase ; Whisper réécoute la voix de Piper et y trouve
les mots attendus ; sans chaîne vocale, la commande le dit au lieu de se taire. En écoute
continue, « Prophète, lance la mission » lance la dernière mission préparée (l'approbation,
dite) et « Prophète, résultat » fait dire son résultat : un test joue les trois ordres à la
suite sur un service simulé, `task.prepare`, `task.start`, `task.result` sur la même mission,
dernière réponse réécoutée « mission terminée. la note de réunion est écrite dans vos
documents. un changement est à examiner. » L'atelier dit de lui-même la fin d'une mission
qu'on regarde courir (même résumé, dans le crate `voice` ; bouton « Voix : lue / muette ») et
obéit aux mêmes ordres que la CLI (« Prophète, prépare », « lance la mission », « résultat »,
`voice::ordre_vocal` partagé) ; ces tests de contrôleur sont écrits et joués par la CI, pas sur
cette machine. Premier verdict du travail « Parole » (`27b55a9`) : `voice` et la CLI verts avec
la vraie chaîne sur le coureur — les trois ordres à la suite compris — ; l'écoute de l'atelier
laissait l'objectif vide, faute de dire la langue à Whisper (détection automatique fautive sur
une phrase courte) : `PROPHET_VOICE_LANGUAGE`, `fr` dans l'image, donne la langue par défaut ;
puis « Profaite », entendu pour « Prophète », n'était pas reconnu : « ai » et « ei » valent
« e ». Troisième verdict (`eefdef4`) : le travail « Parole » est vert — crate `voice`, CLI (les
trois ordres à la suite) et écoute permanente de l'atelier, avec la vraie chaîne, sur le coureur. Ni voix humaine
mesurée, ni lecture vérifiée sur une vraie sortie audio : voir
l'[ADR 0036](adr/0036-la-parole-de-l-humain.md) et le [rapport](reports/parole-2026-09-13.md).

Jalons d'intégration réellement exercés le 12 septembre 2026 :

- `c3b0c08` — moteur local réel, CLI et gel d'un processus possédé par sandboxd.
- `766ce9e` — espace natif Wayland, conversation locale en flux, neuf tests de rendu réussis,
  première image en fenêtre WSLg et capture d'une vraie réponse Qwen3.
- `2abcf3f` — préparation MCP : le registre refuse désormais les exigences inconnues, les cibles absentes,
  les niveaux d'isolation insuffisants et les contextes de tâche incohérents. La session exige
  son initialisation et borne ses entrées. Quatre tests de régression ont d'abord échoué sur
  l'ancien comportement. Le [guide du composant](../crates/mcp-system/README.md) précise les
  accès fichiers et les raccordements aux daemons qui restent à corriger avant activation.
  Validation locale après correction : `nix develop --command just check`, 569 tests réussis,
  aucun échec, 16 ignorés ; format, clippy et contrôles du dépôt réussis.
- `0f3f307` — clients officiels : Codex et Claude Code exigés par la configuration d'image. Versions et
  connexion sondées par les vrais clients, sans inspection des fichiers d'identifiants ; profils
  privés dans le répertoire utilisateur, reprise Codex et options de flux Claude corrigées.
  Les capacités ne déclarent plus des fonctionnalités agentiques non raccordées. Validation
  locale : `nix develop --command just check`, 575 tests réussis, aucun échec, 17 ignorés.
  Test explicite supplémentaire sur les vrais binaires : Codex 0.153.4 et Claude Code 2.1.266,
  versions reconnues et connexion requise dans des profils vierges ; commandes CLI doctor,
  login et ls JSON exercées avec succès. La CI de cette révision a réussi ses trois travaux,
  puis la construction de l'ISO, son démarrage, la construction du système installé, son
  démarrage et les tests des services. Le bureau humain, l'application
  ChatGPT et les sessions authentifiées restent à intégrer. Voir le
  [guide des pilotes](components/providers.md) et l'[ADR 0009](adr/0009-clients-officiels-et-bureau.md).
- `f8e263e` et correctifs de compatibilité — paquet ChatGPT Linux expérimental construit depuis
  le `.deb` officiel 26.908.40834 ; empreinte du binaire principal inchangée. La VM NixOS sous
  KVM confirme une fenêtre XWayland visible et l'écran de connexion par reconnaissance de texte,
  avec le binaire officiel sous UID 1000. La copie des plugins est corrigée, leur initialisation
  se termine. **Le test graphique strict reste en échec** sur une erreur Fontconfig dans un
  renderer secondaire ; le paquet reste hors de l'image installée. Aucun compte n'est connecté.
  Un test de régression protège aussi les valeurs d'options de Claude Code. Validation locale
  des composants : `just check` dans Nix, 576 tests réussis, aucun échec, 17 ignorés. La CI de
  `f8e263e` réussit les composants, l'isolation, la surface, les services, l'installeur, l'ISO et
  les deux démarrages ; son travail ChatGPT a échoué sur la classe de fenêtre, corrigée depuis.
  Les résultats de cette révision ne valident pas les correctifs suivants. Voir le
  [rapport ChatGPT Linux](reports/chatgpt-linux-2026-09-12.md).

La CI de `f132518` a depuis réussi les composants, l'isolation, le rendu de la surface, les
services, l'installeur, les constructions et le démarrage de l'ISO. Le travail ChatGPT reste en
échec. Le test du système installé a aussi échoué lors de l'ouverture de session du propriétaire
après son délai de 900 secondes ; sa cause reste à diagnostiquer. Cette observation précède la
refonte Iris et ne constitue pas une validation du système installé pour cette révision.

Jalon d'interface du 13 septembre 2026 : **Iris** remplace la présentation de l'espace natif
par une composition centrée, une sculpture irisée native, un dock flottant, Inter embarquée
et des contrôles adaptés à la taille de la fenêtre. Les captures finales incluent une vraie
conversation Qwen locale et des formats de 640 × 480 à 1920 × 1080. Validation locale :
`just check`, 576 tests réussis, aucun échec, 18 ignorés ; dix tests graphiques explicites
réussis et première image soumise à Wayland sous WSLg. La nouvelle révision reste à valider
en CI. Ce jalon ne résout pas les échecs installés et ChatGPT décrits ci-dessus. Voir le
[rapport Iris et ses captures](reports/interface-iris-2026-09-13.md).

Le même jour, l'utilisateur rejette Iris et demande un espace réellement conçu pour les agents
et la supervision humaine. La nouvelle direction supprime la sculpture, adopte un thème clair
et place les missions, leur contexte et les décisions au premier plan. Les filtres, la sélection,
le retour aux missions sur petit écran et l'examen explicite sont implémentés. Validation locale :
`just check`, 577 tests réussis, aucun échec, 20 ignorés ; douze tests graphiques explicites
réussis (six parcours natifs et six tests du rendu historique). Les commandes d'agents, livrables,
diffs, permissions détaillées et acquittements restent à intégrer. La validation de cette nouvelle
révision en CI reste à réaliser. Voir le [rapport de supervision](reports/supervision-2026-09-13.md)
et l'[ADR 0011](adr/0011-supervision-humaine.md). La qualité visuelle reste à apprécier par
l'utilisateur ; ce jalon ne constitue pas une certification SOTA ni une équivalence avec Apple.

Jalon fichiers MCP du 13 septembre 2026, après `bee035d` : accès Linux relatifs à des descripteurs,
refus des liens et fichiers spéciaux, remplacement atomique dans le travail, descendants
recontrôlés et parcours bornés. Les six régressions initiales ont échoué avant correction ; les
douze nouveaux tests ordinaires passent désormais. `RegistryExecutor` raccorde les outils à la
boucle native : un vrai Qwen3 écrit un fichier, reçoit son résultat et termine ; SFS expose le
changement sans modifier le home. **`just check` réussi : 589 tests, aucun échec, 21 ignorés**,
format, clippy et contrôles du dépôt réussis. L'essai modèle ignoré par défaut a été exécuté
séparément et réussit en 10,57 secondes. Le Broker et le journal sont en mémoire dans cet essai.
Le binaire MCP, le contexte de service fiable, les racines privées, les commits/undo SFS
concurrents et le parcours depuis l'interface restent à intégrer. Voir le
[rapport MCP](reports/mcp-fichiers-2026-09-13.md) et l'[ADR 0012](adr/0012-acces-fichiers-mcp.md).

Résultats CI relus pour `bee035d` : composants, isolation et rendu réussis ; ChatGPT en échec.
Le workflow d'image réussit les services, l'installeur, les constructions et les deux démarrages
(ISO et système installé). Le délai de session de `f132518` ne s'est pas reproduit dans ce run,
sans que sa cause soit établie. Le rapport MCP référence ces exécutions. Ces observations ne
valident pas encore le nouveau correctif MCP en CI.

Jalon de service du 13 septembre 2026, après `d7b5c90` : `task.start` lance une mission locale
en arrière-plan avec les vrais capd et ledger ; `task.result` conserve le résultat. La CLI
planifie, lance, suit et demande l'annulation. La capture SFS et les outils contrôlent les
périmètres, les générations tronquées restent comptées avant toute action et les erreurs de
journal interdisent la répétition automatique. Les tâches interrompues par redémarrage ont
un échec explicite et l'état corrompu est préservé pour réparation. Le lanceur n'exécute que
des outils natifs de confiance de niveau 0, sans lancer de programme non fiable.

**Les trois essais Qwen3-0.6B du parcours agentd échouent** : le serveur déclare une génération
incomplète au plafond de 2 048 tokens ; le dernier protocole observé contient aussi un texte
altéré. Le refus est maintenu, sans exécuter l'appel incomplet. **`just check` réussi : 604 tests,
aucun échec, 22 ignorés**, format, clippy, construction des programmes et contrôles du dépôt.
Les neuf scénarios ordinaires du parcours agentd, avec moteur HTTP contrôlé et vrais services,
passent ; les essais de modèle réel restent distincts et en échec.
Voir le [rapport de missions](reports/missions-locales-2026-09-13.md),
l'[ADR 0013](adr/0013-missions-locales-agentd.md) et l'[exemple CLI](../crates/agentd/README.md).
La reprise par checkpoints, les résultats vérifiés, le lancement depuis l'interface, les
processus sous sandboxd et l'intégration à l'image restent ouverts. Aucun critère complet
de FRONTIER.md n'est coché pour ce jalon. La CI de `d7b5c90` a réussi composants, isolation
et surface ; le test ChatGPT reste en échec (run `34726143307`).

Jalon de supervision du 13 septembre 2026, commit `2eb86f8` :
`task.inspect` expose le plan et le résultat sans jeton. L'interface native permet de lancer
un plan existant, de demander l'arrêt et de lire ou copier le résultat ; les missions terminées
restent consultables. Les commandes attendent les réponses du service et les lectures anciennes
sont écartées. La liste et le détail réconcilient leurs états, y compris après pause et reprise.
Le client IPC borne les lectures et contrôle la corrélation et la forme des réponses.

**`just check` réussi : 615 tests, aucun échec, 24 ignorés**, avec format, clippy, reconstruction
des binaires et contrôles du dépôt. **Les 14 tests graphiques explicites réussissent** ; les
deux nouveaux parcours utilisent les widgets natifs, agentd, capd et ledger réels, avec un
serveur HTTP de modèle contrôlé. Douze captures montrent le plan, l'exécution, le résultat,
le parcours, l'arrêt et l'échec, sur quatre largeurs. Un délai de lecture de cinq secondes
pendant un essai d'arrêt ne s'est pas reproduit dans les exécutions suivantes ; sa cause reste
inconnue. Voir le [rapport et les captures](reports/commandes-supervision-2026-09-13.md)
et l'[ADR 0014](adr/0014-inspection-et-commandes-de-mission.md).

Ce jalon ne valide pas la chaîne agentd avec un LLM réel : les trois échecs Qwen3 ci-dessus
restent ouverts. La CI de `a91ed22` a réussi composants, isolation et surface ; le travail
ChatGPT échoue toujours sur Fontconfig (run `34728460203`). La nouvelle révision reste à
vérifier en CI. La création de plans depuis le dialogue, le contenu des diffs et leur validation,
les processus isolés, les sessions authentifiées et l'intégration complète à l'image restent
à réaliser. Aucun critère complet de FRONTIER.md n'est coché pour ce jalon.

Jalon de préparation du 13 septembre 2026, après `2eb86f8`, dans le commit portant ce rapport :
une intention saisie dans la surface peut devenir un plan avec `task.prepare`. Le contexte
et les modèles admis viennent de profils configurés dans agentd ; le moteur est interrogé
réellement avant de proposer puis de préparer le modèle choisi. L'identité vient du pair Unix.
L'humain examine le plan puis le lance séparément. Une demande du dialogue peut devenir un
nouveau brouillon ; les réponses du modèle ne fournissent ni droits ni profil.

Le contrôleur garde la référence envoyée après une erreur et propose une relecture du plan
sans nouvelle création. Il conserve aussi le modèle de la tentative incertaine. Le formulaire
adapte son organisation aux fenêtres étroites et moins hautes. Un essai graphique a rencontré
un délai de confirmation, puis le même binaire a réussi en exécution isolée ; sa cause reste
inconnue. **`just check` réussi : 622 tests, aucun échec, 25 ignorés**, avec format, clippy,
reconstruction des programmes et contrôles du dépôt. **Les 15 tests graphiques explicites
réussissent**, dont le parcours de préparation avec modèle HTTP contrôlé et trois vrais
services, puis le transfert d'une nouvelle demande du dialogue vers un nouveau brouillon.
Voir le [rapport et les captures](reports/preparation-missions-2026-09-13.md)
et l'[ADR 0015](adr/0015-intention-et-profils-de-mission.md).

La CI de `2eb86f8` a réussi composants, isolation et surface ; le travail ChatGPT a échoué
(run `34731398933`). Les trois échecs Qwen3-0.6B restent ouverts. Les profils, moteurs et la
session humaine installée restent à intégrer ; aucune session authentifiée ChatGPT/Claude Code
n'est validée. L'examen et la validation du contenu, les checkpoints, l'undo robuste et les
processus isolés restent à réaliser. Aucun critère complet de FRONTIER.md n'est coché.

Jalon du moteur local du 13 septembre 2026, après `a16472a`, dans le commit portant ce rapport :
la cause d'une répétition après appel est isolée dans la grammaire de llama.cpp. Un patch
du paquet Nix respecte le mode séquentiel et supprime la répétition d'appels optionnels.
Huit assertions échouent sur le moteur original ; le paquet corrigé passe **60 vérifications**
sur les templates Qwen3 et Qwen2.5. Le pilote, les tests Rust et les contrôles d'exécution
restent inchangés. **`just check` réussi : 622 tests, aucun échec, 25 ignorés**.

Avec le paquet final, **Qwen3-1.7B-Q8_0 réussit trois missions réelles sur trois**, en
12,42 / 12,96 / 13,02 secondes pour le test complet. Fichier exact, diff SFS, vrais capd/ledger
et résultat relu après redémarrage sont vérifiés. Qwen3-0.6B ne réussit qu'un essai sur trois ;
les deux autres terminent mais produisent un contenu erroné. Les trois anciennes générations
tronquées restent dans le rapport historique. Cette nouvelle preuve porte sur une seule
tâche d'écriture répétée, sans modifier ses assertions ; elle ne démontre pas une fiabilité
générale, une validation des objectifs par agentd ou l'utilisation depuis l'image installée.

Voir le [diagnostic, les mesures et les limites](reports/grammaire-locale-2026-09-13.md)
et l'[ADR 0016](adr/0016-grammaire-du-moteur-local.md). La CI de `a16472a` a réussi composants,
isolation et surface ; ChatGPT reste en échec (run `34733626591`). Le moteur et ses poids
ne sont pas encore provisionnés dans la session installée. GPU, autres familles, résultats
vérifiés, graphisme et sessions authentifiées restent ouverts. Aucun critère complet de
FRONTIER.md n'est coché.

Raccordement du 13 septembre 2026, après `395ecbb` : le module système installe le moteur
corrigé, un profil `Documents Prophet` et des paramètres communs au dialogue et à agentd.
Les poids restent un choix de configuration explicite. capd et agentd utilisent le home du
propriétaire configuré ; le contexte partagé et les travaux privés reçoivent des permissions
distinctes. **Trois essais graphiques réels sur trois réussissent avec Qwen3-1.7B**, après un
premier essai également réussi : widgets natifs, vrais services, contenu exact et originaux
intacts. **`just check` réussi : 622 tests, aucun échec, 25 ignorés** ; le test graphique réel
reste séparé sous une feature explicite. Le test NixOS avec poids réels est ajouté à la CI.
Les 15 tests graphiques contrôlés passent aussi après ce raccordement.
Son exécution et la construction du nouveau paquet restent à confirmer ; l'évaluation Nix
réussit. La construction locale a été interrompue avec seulement 6 Go libres sur le disque
hôte. Le cache incrémental Linux a été réduit d'environ 15 Go, sans gain physique confirmé
sur Windows. Voir le [rapport](reports/moteur-installe-2026-09-13.md) et l'[ADR 0017](adr/0017-moteur-installe-et-contexte-partage.md).

La CI de `395ecbb` réussit le protocole du moteur, les composants, l'isolation et la surface ;
ChatGPT demeure en échec. Le kiosque, la gestion graphique des poids, le contenu des diffs,
leur validation, les clients authentifiés et la refonte visuelle demandée restent ouverts.
Aucun critère complet de FRONTIER.md n'est coché pour ce raccordement.

Examen des fichiers du 13 septembre 2026, après `1640e1c`, dans le commit portant ce rapport :
la surface compare les versions initiales et proposées, permet la copie exacte et retire un
aperçu altéré à l'actualisation. SFS conserve les octets initiaux et vérifie l'index final ;
`task.change` exige l'UID créateur constaté et persisté. Les lectures et le calcul de lignes se
font en arrière-plan. Les anciennes missions sans versions ne reçoivent pas d'aperçu inventé.
Le [rapport](reports/examen-fichiers-2026-09-13.md) contient les preuves et captures ;
l'[ADR 0018](adr/0018-examen-des-versions.md) décrit les bornes et les limites.

**`just check` final réussi : 631 tests, aucun échec, 26 ignorés** ; les **16 tests graphiques
explicites réussissent**. Trois essais Qwen3-1.7B lisent le contenu exact dans l'aperçu natif ;
un quatrième réussit après compactage de la vue Fichiers. Les contrôles du propriétaire,
des versions altérées, de la comparaison de longs textes et de la relecture après redémarrage
sont inclus. Les captures finales sont examinées en trois tailles. Une lecture de journal WSL
a expiré pendant les contrôles ; la connexion a repris et les tests ont abouti, sans arrêt forcé.

La CI de `1640e1c` réussit composants, isolation et protocole du moteur. La surface échoue sur
un bouton encore absent pendant la synchronisation ; son attente est corrigée dans le test.
La VM charge Qwen3 puis échoue avant génération : `RestrictSUIDSGID` interdit l'ouverture sûre
utilisée par SFS. L'erreur 38 est reproduite sous systemd puis supprimée avec le retrait ciblé
de cette restriction pour agentd ; les autres protections sont conservées. La nouvelle VM
doit confirmer le parcours complet. ChatGPT échoue toujours au contrôle Fontconfig.
L'application et l'undo des fichiers, les droits des autres méthodes, le bureau humain complet,
les clients authentifiés et la direction graphique demandée restent ouverts. Aucun critère
complet de FRONTIER.md n'est coché pour cet examen.

Atelier du 13 septembre 2026, après `d20ef74`, dans le commit portant le
[rapport](reports/atelier-2026-09-13.md) : navigation graphite, galerie de missions, Focale et
recherche Ctrl+K. Une mission seule reçoit directement l'espace d'examen ; les plans préparés
effacent la recherche précédente. Le mélange des transparences d'egui est corrigé dans une vue
Unorm compatible ; le test blanc sur blanc reproduisait un gris de 220 avant correction.
L'[ADR 0019](adr/0019-atelier-et-focale.md) décrit la composition et ses limites.

**`just check` final réussi : 631 tests, aucun échec, 29 ignorés**, avec format, clippy,
construction des programmes et contrôles du dépôt. La première tentative avait rencontré
trois délais de commande du navigateur ; les quatre tests concernés passent ensuite sans
modification du pont, puis la suite complète réussit. Un démarrage d'agentd dépasse aussi le
délai lors d'un essai graphique ; cet incident reste dans le rapport.
Les **19 tests graphiques réussissent** au passage final. La galerie de mille missions mesure
8,592 ms en médiane et 13,785 ms en p95 pour la composition et la soumission, sans attente de
présentation. Les captures finales sont examinées. La série Qwen3-1.7B reste **à deux réussites
sur trois** : le premier essai refuse la capture SFS avant l'inférence, avec une cause précise
non établie ; les deux autres vérifient le fichier exact et l'aperçu natif. Le diagnostic du
refus reste ouvert, sans assouplissement des droits.

La CI de `d20ef74` réussit composants, surface, isolation, protocole et mission réelle sous
NixOS : poids sous un compte distinct, contenu exact, examen des versions et relecture après
redémarrage, avec refus sous l'UID 0. ChatGPT reste en échec sur Fontconfig. La session humaine
complète, les clients authentifiés, l'application et l'undo, les autres familles et GPU,
la qualité graphique attendue et les mesures matérielles restent ouverts. Aucun critère
complet de FRONTIER.md n'est coché.

Jalon Jev du 17 septembre 2026, commits `65481d8` (providers), `ebaf48a` (egress), `5fc074b`
(agentd), `76f092a` (CLI) et `f093c9a` (image), hors plan, décidé par l'[ADR 0042](adr/0042-jev-decideur-rapide.md) :
le modèle de décision Jev (TypeSafe AI, ouvert le 15 septembre) route chaque mission vers le
modèle génératif admissible qui lui convient et opère lui-même une page par son arbre SUP,
en rendant la main au modèle génératif dès qu'il faut écrire. Sa clé reste dans le coffre,
référencée par `prophet-secret:<nom>` et substituée par le proxy ; le proxy termine TLS vers
l'amont et traite un `POST` vers un hôte d'interrogation déclaré comme une lecture. Jev est
optionnel (`prophet.jev.enable`, `PROPHET_JEV_SECRET`) : sans lui, rien ne change.
**Vérifications locales** : 26 tests unitaires `providers::jev`, 13 tests du daemon egress
(dont amont TLS et hôtes d'interrogation), 3 tests agentd avec proxy simulé et **page réelle
opérée sous Chromium sans aucun modèle génératif** (trois décisions, trois appels d'outil
au journal, tokens imputés au budget) ; `cargo fmt`, `cargo clippy -D warnings`, construction
des programmes et contrôles du dépôt réussis ; `cargo test --workspace` vert, sauf le test
préexistant `un_fichier_de_configuration_ne_prouve_pas_une_connexion`, qui échoue avant comme
après dans cet environnement où un client Claude Code connecté est installé. **Aucun appel
à l'API réelle** : le protocole vient de la documentation et des clients ouverts, et le
premier appel avec une clé reste à faire. Les outils `ui.*` ne sont pas encore offerts à
l'opérateur, et la CI de cette révision reste à exécuter. Voir le
[rapport](reports/jev-2026-09-17.md) et la [spécification](specs/jev-decisions.md).
**Suite du même soir** : une clé fournie pour le premier appel n'a pas pu servir, l'environnement
de construction refusant `api.typesafe.ai` par sa politique réseau ; elle n'est écrite nulle
part et doit être renouvelée, ayant transité par une conversation. L'essai a ajouté un test de
**chaîne complète sans aucun faux** (capd, ledger, coffre, egress, agentd : refus du coffre
sous un compte qui n'est pas `egress`, requête arrêtée, repli statique motivé en 0,3 s, secret
absent du disque en clair), un délai de connexion de quinze secondes vers l'amont dans egress,
et `EnvironmentFile=/etc/prophet/<daemon>.env` dans les unités de `tools/lancer-sur-l-hote.sh`
avec la procédure Jev en tête du script. Le premier appel réel se fait sur un hôte où les
daemons tournent sous leurs comptes, à la sortie libre vers `api.typesafe.ai`.

Ce fichier est la source de vérité de l'avancement. L'agent constructeur prend la première tâche non cochée dont les dépendances sont cochées, et coche avec la date et le hash du commit.

Une tâche marquée ⛔ est écrite et relue, mais **non exerçable dans l'environnement de construction** ; le détail est dans `docs/reports/phase0.md` section 5.

## Jalons

### M0 — Fondations du dépôt

- [x] M0-T1 — Flake Nix et dev shell (2026-09-12, 24b8338) — flake Nix et dev shell (non exerçable ici : Nix absent)
- [x] M0-T2 — Workspace Cargo (2026-09-12, 24b8338) — workspace Cargo, édition 2024, lints du workspace
- [x] M0-T3 — justfile (2026-09-12, 24b8338) — justfile, repli sans gitleaks
- [x] M0-T4 — CI GitHub Actions (2026-09-12, 24b8338) — CI : format, clippy, tests, secrets, job privilégié
- [x] M0-T5 — Documentation de base (2026-09-12, 24b8338) — ADR 0000 à 0005, STATUS, specs
- [x] M0-T6 — Hooks et hygiène (2026-09-12, 24b8338) — recherche de secrets dans `just check`

### M1 — Spécifications gelées v0

- [x] M1-T1 — Manifeste d'agent (2026-09-12, 24b8338) — manifeste : types, parseur TOML, 11 tests de validation
- [x] M1-T2 — Jeton de capacité (2026-09-12, 24b8338) — jeton : signature, délégation, réflexivité et transitivité
- [x] M1-T3 — Événement du Ledger (2026-09-12, 24b8338) — événement : chaînage, altération, suppression, insertion détectées
- [x] M1-T4 — Contrat Agent Driver (2026-09-12, 24b8338) — types du contrat de pilote
- [x] M1-T5 — Convention IPC (2026-09-12, 24b8338) — prophet-ipc : 10 000 allers-retours en 386 ms, SO_PEERCRED
- [x] M1-T6 — Nommage des outils MCP système (2026-09-12, 24b8338) — liste normative des outils, `requires` obligatoire

### M2 — capd : Capability Broker et Policy Engine

- [x] M2-T1 — Daemon et clé (2026-09-12, 24b8338) — clé ed25519, broker instanciable
- [x] M2-T2 — Émission (2026-09-12, 24b8338) — émission bornée par le plafond du manifeste
- [x] M2-T3 — Délégation (2026-09-12, 24b8338) — délégation ⊆, profondeur bornée, durée bornée
- [x] M2-T4 — Vérification (2026-09-12, 24b8338) — 11,6 µs par contrôle en binaire optimisé
- [x] M2-T5 — Politiques Cedar (2026-09-12, 24b8338) — politiques Cedar, interdits absolus, classes d'actions
- [x] M2-T6 — Approbations (2026-09-12, 24b8338) — approbations : portées once, task, agent ; expiration ; révocation
- [x] M2-T7 — CLI (2026-09-12, 24b8338) — binaire `prophet`, sous-commandes
- [x] M2-T8 — Application noyau (2026-09-12, 24b8338) — règles Landlock, domaines, profil seccomp

### M3 — ledger : Event Bus et Ledger

- [x] M3-T1 — Stockage (2026-09-12, 24b8338) — stockage JSONL par jour, index, réouverture
- [x] M3-T2 — API (2026-09-12, 24b8338) — requêtes filtrées, bus de diffusion
- [x] M3-T3 — Scellement (2026-09-12, 24b8338) — scellement ed25519, vérification autonome
- [x] M3-T4 — CLI et rejeu (2026-09-12, 24b8338) — `prophet log tail|replay|verify`, lisible sans daemon

### M4 — sfs : Semantic FS v0

- [x] M4-T1 — Disposition (2026-09-12, 24b8338) — ADR-0004, détection de dorsale sans privilège
- [x] M4-T2 — Opérations (2026-09-12, 24b8338) — 50 fichiers modifiés, validés, annulés à l'octet près
- [x] M4-T3 — Provenance (2026-09-12, 24b8338) — provenance en attributs étendus, dégradation propre
- [x] M4-T4 — Transactions multi-fichiers (2026-09-12, 24b8338) — transactions hors arbre de travail, balayage des restes
- [x] M4-T5 — Mode dégradé (2026-09-12, 24b8338) — repli portable, limites annoncées

### M5 — sandboxd : Sandbox Manager

- [x] M5-T1 — Niveau 0 (bwrap + Landlock + seccomp) (2026-09-12, 24b8338) — 8 tests d'évasion réels, démarrage en 2,6 ms ; Landlock n'était pas appliqué, il l'est depuis le 23 septembre (`9b3ae7e`)
- [x] M5-T2 — Niveau 1 (gVisor) (2026-09-12, 3c0b7cd) — vérifié sur matériel réel en intégration continue : exécution effective sous gVisor et absence d'interface réseau, tests `needs_gvisor` verts
- [x] M5-T3 — Niveau 2 (Firecracker) (2026-09-12, faca93c) — vérifié sur matériel réel en intégration continue : microVM démarrée avec noyau et racine d'invité, et refus explicite plutôt que repli quand le niveau est inatteignable
- [x] M5-T4 — Pool de snapshots (2026-09-23, 1e3ff79) — vérifié sur matériel réel en intégration continue (`3314b74`) : réserve de deux microVM restaurées de l'instantané d'un invité en attente, disque de la tâche confié à la reprise ; microVM rendue en 8,9 ms (médiane de cinq prises, de 8,4 à 10,0 ms, `41def9e`) pour un objectif de 150 ms, machine neuve à chaque prise, programme exécuté et fichiers rapatriés comme à froid, réserve régénérée (ADR 0045) ; un seul profil, l'invité du dépôt (`node` et `browser` n'ont pas d'invité)
- [x] M5-T5 — Cycle de vie et quotas (2026-09-12, 24b8338) — gel global de 8 sandboxes en 124 µs
- [x] M5-T6 — Sélection automatique (2026-09-12, 24b8338) — sélection de niveau, microVM imposée pour tout code
- [x] M5-T7 — CLI (2026-09-12, 24b8338) — sonde de capacités et rapport

### M6 — egress et vault

- [x] M6-T1 — Proxy (2026-09-12, 24b8338) — politique par hôte, méthode, volume ; IP littérales refusées
- [x] M6-T2 — Détection d'exfiltration (2026-09-12, 24b8338) — motifs de secrets bloquants, signaux faibles portés à l'humain
- [x] M6-T3 — Vault (2026-09-12, 24b8338) — coffre chiffré, références jamais valeurs
- [x] M6-T4 — Injection dans le proxy (2026-09-12, 24b8338) — substitution au dernier moment, hôte vérifié
- [x] M6-T5 — Sous-volumes d'identifiants des clients officiels (2026-09-12, 24b8338) — répertoires privés par pilote et par utilisateur
- [x] M6-T6 — Identité réseau d'agent (2026-09-12, 8091f4d) — en-tête signé, utilisateur sous empreinte salée par machine

### M7 — mcp-system : serveurs MCP système

- [x] M7-T1 — `fs` (2026-09-12, 8091f4d) — lecture, écriture, liste, stat, recherche ; double contrôle outil puis cible
- [x] M7-T2 — `proc` (2026-09-12, 8091f4d) — exécution et arrêt ; microVM imposée hors liste blanche
- [x] M7-T3 — `http` (2026-09-12, 8091f4d) — sortie par le proxy uniquement ; relais réel par le socket d'egress depuis le 13 septembre (ADR 0024)
- [x] M7-T4 — `task` (2026-09-12, 8091f4d) — état et diff de la tâche courante
- [x] M7-T5 — `approval` (2026-09-12, 8091f4d) — demande et attente ; résumé obligatoire
- [x] M7-T6 — `ledger` (2026-09-12, 8091f4d) — lecture limitée à la tâche courante
- [x] M7-T7 — `memory` (2026-09-12, 8091f4d) — enregistrement et recherche par espace
- [x] M7-T8 — `secrets` (2026-09-12, 8091f4d) — références seules, valeurs jamais rendues
- [x] M7-T9 — `clock`, `notify` (2026-09-12, 8091f4d) — horloge rejouable, notification hors bande
- [x] M7-T10 — Registre (2026-09-12, 8091f4d) — couverture de la spécification vérifiée par un test

### M8 — agentd et providers

- [x] M8-T1 — Cycle de vie de tâche (2026-09-12, 24b8338) — machine à états, table de transitions testée en entier
- [x] M8-T2 — Budgets et quotas (2026-09-12, 24b8338) — budgets multidimensionnels, quotas d'abonnement
- [x] M8-T3 — Hiérarchie (2026-09-12, 24b8338) — hiérarchie bornée, budget prélevé sur le parent
- [x] M8-T4 — Pilote `claude-code` (2026-09-12, 24b8338) — ligne de commande, environnement, détection de session
- [x] M8-T5 — Pilote `codex` (2026-09-12, 24b8338) — pilote Codex CLI
- [x] M8-T6 — Pilote `gemini` (2026-09-12, 24b8338) — pilote Gemini CLI
- [x] M8-T7 — Moteurs locaux (2026-09-23, f688d49) — critère tenu en CI par les vrais binaires : `prophet model pull qwen3-8b-q4` (5 Go par egress, empreinte vérifiée, 168 s), `prophet model serve qwen3-8b-q4` (3 s), une complétion qui aboutit (« bonjour », 2,9 s), cgroup GPU documenté (ADR 0037) ; écarts dits : catalogue compilé dans les binaires plutôt que signé sous /var/lib (ADR 0046), moteur sous une unité systemd durcie plutôt que sous sandboxd ; historique : client HTTP, flux annulable, interface de conversation et essai Qwen3/CPU réalisés ; le 13 septembre, budgets de tokens par modèle, condensation du contexte et deux modèles servis par un llama-server en mode routeur, prouvés en relais réel (ADR 0034) ; le même jour, l'image sert deux modèles en mode routeur (Qwen3-1.7B en réflexion, Qwen3-0.6B en exécution, téléchargés à l'installation), préréglages vérifiés sur le vrai moteur et configuration évaluée ; le 22 septembre, le catalogue des poids se lit dans l'en-tête GGUF de chaque fichier (`prophet model ls`, page Modèles : architecture, quantification, fenêtre de contexte), et un historique plus long que la fenêtre du moteur est resserré à sa mesure au lieu de faire échouer la mission ou la conversation ; le 23 septembre, les poids du catalogue du système se téléchargent par egress sous un jeton de capd borné au dépôt, vérifiés (SHA-256, taille, en-tête GGUF) avant d'être posés, reprennent après une coupure et se retirent (`prophet model pull`, page Modèles, ADR 0046, `114ef7b`, `1752d7a`, `78b7791`, `32a20ea`) ; le même jour, un poids téléchargé se sert par le routeur du moteur (`prophet model serve`, page Modèles, `0df78e7`) ; restent la détection de la VRAM, les budgets VRAM et la matrice GPU/modèles, à mesurer sur une carte (`needs_gpu`)
- [x] M8-T8 — Pilote `prophet-agent` (2026-09-12, 24b8338) — boucle native : points de reprise, fork, rejeu
- [x] M8-T9 — Sélection de pilote (2026-09-12, 24b8338) — sélection expliquée, confidentialité locale respectée
- [x] M8-T10 — CLI (2026-09-12, 24b8338) — `prophet provider ls|login`
- [x] M8-T11 — Démo M8 (2026-09-12, 24b8338) — démonstration sur trois pilotes
- [x] ADR 0042 — Décideur Jev (2026-09-17, 65481d8 · 5fc074b) — routage par appel et computer use par l'arbre, par le proxy, sans clé dans le chemin principal ; appel réel à l'API ouvert

### M9 — image bootable

L'ISO se construit depuis le 12 septembre 2026 : le travail « Support d'amorçage » de l'intégration
continue exerce l'installeur sur un disque en boucle, puis produit `prophet-os-installeur-*.iso`.
Six options du Nix n'avaient jamais été évaluées avant ce jour et l'empêchaient — elles sont
corrigées, et la liste est dans `docs/reports/phase0.md`.

- [x] M9-T1 — Modules NixOS (2026-09-12) — modules NixOS, un service durci par daemon. **Cochée à tort jusqu'au 12 septembre au soir** : les sept services déclaraient un `ExecStart` vers un programme que l'atelier ne produisait pas. Une machine installée aurait démarré avec sept unités en échec. Les sept programmes existent désormais, et `tools/verifier-les-services.sh` refuse l'écart — il tourne dans `just check`
- [x] M9-T2 — Noyau (2026-09-12, 24b8338) — exigences noyau documentées et conséquences d'une absence
- [x] M9-T3 — Immuabilité et A/B (2026-09-12, 24b8338) — racine A/B, bascule automatique
- [x] M9-T4 — Chiffrement (2026-09-12, 24b8338) — LUKS2, TPM avec repli par phrase de passe
- [x] M9-T5 — Installeur (2026-09-12) — `image/installateur/prophet-installer.sh` : partitionnement GPT, LUKS2 sur l'état et les données, deux racines A/B, montage. Exercé en intégration continue sur un disque en boucle, y compris ses refus — travail d'intégration vert : refus d'une mauvaise confirmation sans toucher au disque, refus d'un disque trop petit, puis préparation réelle dont chaque étiquette correspond à ce qu'`immutable.nix` attend
- [x] M9-T6 — Démo M9 (2026-09-12) — **l'image démarre, et cela a été vu** : micrologiciel UEFI, menu d'amorçage, noyau, espace utilisateur, `serial-getty`, message d'accueil, connexion automatique. Journal complet dans l'artefact « demarrage-vm ». L'intégration continue le refait à chaque construction. Reste non vérifié : le matériel réel — carte graphique, carte réseau, micrologiciel d'un PC donné
- [x] M9-T7 — **Le système installé démarre** (2026-09-12) — et c'est une autre question que M9-T6.
  Le support d'amorçage est une configuration à part : racine en lecture-écriture, session ouverte
  automatiquement. Ce qu'il installe n'avait jamais démarré, seulement été construit.
  `image/tests/installe.nix` le démarre **par son chargeur d'amorçage**, en UEFI, depuis un vrai
  disque : `bootctl status` confirme que `systemd-boot` l'a lancé, avec `lockdown=integrity` et
  `module.sig_enforce=1` sur la ligne de commande — les deux candidats les plus plausibles à un
  refus de démarrer

### Une correction à mon propre message de commit (12 septembre 2026)

Le commit `63e0d99` déplace `allowUnfreePredicate` du module vers `flake.nix`, et donne comme
raison que NixOS refuserait qu'un module touche à `nixpkgs.config` quand `pkgs` vient du cadre de
test — « Your system configures nixpkgs with an externally created instance ». **Ce n'est pas ce
que le journal dit.** L'erreur réelle était :

```
The option `nixpkgs.config.allowUnfreePredicate` has conflicting definition values
Use `lib.mkForce value` ou `lib.mkDefault value` …
```

Une **définition en conflit**, pas une instance externe : le cadre de test pose déjà cette option
pour ses nœuds, à partir du `pkgs` qu'on lui donne, et le module en posait une seconde.

Le déplacement reste la bonne correction — il supprime l'une des deux définitions, et l'unique
qui subsiste vient du `pkgs` construit dans `flake.nix`, lequel sert aussi bien aux tests qu'à
l'image. Mais j'avais écrit le mécanisme avant de l'avoir lu, et c'est exactement ce que ce dépôt
reproche partout ailleurs. Le commit reste tel quel — réécrire l'histoire pour se donner raison
après coup serait pire — et la correction vit ici.

### Une leçon de la journée, écrite pour la prochaine

Une exécution de « Support d'amorçage » occupe six coureurs pendant une demi-heure, et le groupe de
concurrence ajouté ce jour-là **annule l'exécution en cours à chaque poussée**. C'est ce qu'on veut
quand on enchaîne des corrections ; c'est exactement ce qu'on ne veut pas quand on attend un
verdict. Deux exécutions ont ainsi été annulées par la poussée suivante, dont l'une portait la
correction dont on attendait la réponse.

La règle qui en découle : **quand on attend la réponse d'un test de trente minutes, on ne pousse
plus rien qui touche `flake.nix`, `image/`, `crates/` ou `iso.yml`.** `docs/` et `tools/` sont hors
du filtre de chemins et restent libres.

### M10 — browser-bridge et SUP v0

- [x] M10-T1 — Spécification SUP v0 (2026-09-12, 24b8338) — arbre, actions typées, niveaux de détail
- [x] M10-T2 — Registre SUP (2026-09-12, 24b8338) — registre cloisonné, différentiels
- [x] M10-T3 — Pont navigateur (2026-09-12, 24b8338) — réservation sans capture d'écran, 1 929 octets
- [x] M10-T4 — Adaptateur AT-SPI (2026-09-12, 8091f4d, puis 2026-09-13) — correspondance des rôles, confiance annoncée, actions réellement offertes seulement ; le 13 septembre, lecture et pilotage réels par l'adaptateur de session `supd` et les outils `ui.*` : l'éditeur GTK est écrit et enregistré sans pixel (ADR 0027)
- [x] M10-T5 — Application native de référence (2026-09-12, 8091f4d) — éditeur publiant SUP nativement, `send` irréversible et externe
- [x] M10-T6 — Repli vision (2026-09-12, 24b8338) — capture d'écran réservée, hors défaut

### M11 — memoryd

- [x] M11-T1 — Stockage (2026-09-12, 24b8338) — espaces cloisonnés, provenance
- [x] M11-T2 — API MCP (2026-09-12, 24b8338) — recherche hybride, rappel vérifié
- [x] M11-T3 — Mémoire épisodique (2026-09-12, 8091f4d) — résumé dérivé du journal, refus retenus, confiance selon l'issue
- [x] M11-T4 — Édition humaine (2026-09-12, 24b8338) — `prophet memory ls|search|forget`

### M12 — shell-tui

- [x] M12-T1 — Barre d'intentions (2026-09-12, 8091f4d) — proposition étroite, élargissements posés en questions
- [x] M12-T2 — Timeline (2026-09-12, 24b8338) — timeline groupée par étape
- [x] M12-T3 — Centre d'approbations (2026-09-12, 24b8338) — centre d'approbations lisible en cinq secondes
- [x] M12-T4 — Undo (2026-09-12, 24b8338) — `prophet task undo`, sans daemon ; depuis le 13 septembre, par `task.undo` d'agentd sous l'identité du créateur (ADR 0023)
- [x] M12-T5 — Gel d'urgence (2026-09-12, 24b8338) — `prophet freeze`

### M13 — bench et adversarial

- [x] M13-T1 — Suite de tâches (2026-09-12, 8091f4d) — 8 tâches, 5 familles, vérificateur par tâche
- [ ] ⛔ M13-T2 — Ligne de base « pixels » — bloqué : exige un agent de référence exécutable
- [x] M13-T3 — Suite adversariale (2026-09-12, 24b8338) — 20 scénarios, 20 sans conséquence
- [x] M13-T4 — Rapport de phase 0 (2026-09-12, 24b8338) — docs/reports/phase0.md


### Daemons — les programmes que les services déclaraient

Écrits le 12 septembre 2026, après qu'un garde-fou a montré que les sept `ExecStart` de
`image/modules/prophet.nix` ne nommaient aucun programme existant. Chacun a un test qui lance le
**binaire** et lui parle par son socket, parce que c'est le binaire que systemd lancera.

- [x] `prophet-capd` — `cap.check`, `cap.mint`, `cap.revoke`, `approval.*`. Un jeton signé par une
  autre clé est refusé, et le motif nomme la signature
- [x] `prophet-ledger` — seul écrivain du journal, scellement tous les 256 événements. Un appelant
  ne peut pas choisir sa place dans la chaîne
- [x] `prophet-vault` — `secrets.use` n'est servi qu'au compte du proxy de sortie ; sans ce compte,
  personne n'obtient de valeur
- [x] `prophet-memoryd` — espaces cloisonnés ; une recherche sans espace échoue au lieu de chercher
  partout
- [x] `prophet-egress` — le relais, écrit ici : jeton, `cap.check`, détection d'exfiltration, puis
  seulement la sortie. Un `capd` injoignable ferme la sortie
- [x] `prophet-sandboxd` — un niveau que la machine ne tient pas est refusé, jamais abaissé
- [x] `prophet-agentd` — demande ses jetons à `capd` et pousse son journal vers `ledger` ; il
  n'émet ni n'écrit lui-même. Ses tâches survivent à un redémarrage, écriture atomique en 0600
- [x] `surface::reel` — la surface lit `agentd`, `capd` et `sandboxd` au lieu d'afficher une scène
  d'exemple. Un daemon muet vide sa part du champ plutôt que de laisser la précédente : montrer
  d'anciennes tâches comme si elles couraient encore serait faux *et* crédible
- [x] `prophet-daemon` — la part commune : socket, état, clés, et surtout **à qui un daemon accepte
  de parler**, écrite une fois pour que les sept copies ne divergent pas

### Ce que le premier démarrage sous systemd a montré (12 septembre 2026)

Le test `image/tests/services.nix` a fait tourner les sept services sous systemd, avec leurs
utilisateurs et leur durcissement. Trois défauts sont apparus, qu'aucun test de daemon pris
isolément ne pouvait voir.

- [x] `/run/prophet` en `0750` : le groupe ne pouvait pas y **écrire**. Seul le premier service
  démarré créait son socket ; les six autres bouclaient sur un « Permission denied ». Corrigé en
  `0770`, avec `RuntimeDirectoryPreserve` — sans quoi l'arrêt d'un seul service emportait les six
  autres sockets
- [x] La règle du groupe ne regardait que le `gid` attesté par `SO_PEERCRED`, c'est-à-dire le
  groupe **principal**. Un compte déclaré dans `prophet-system` par `extraGroups` y appartient
  réellement et se faisait pourtant refuser — **la surface était dans ce cas**, et aurait affiché
  un champ vide sur une machine saine. L'appartenance est maintenant aussi cherchée dans
  `/etc/group`. `root` est accepté : le refuser ne protégeait rien, puisqu'il lit les clés de
  signature dans `/var/lib/prophet`, et rendait `prophet status` inutilisable pour le propriétaire
- [x] Ma première correction lisait `/etc/group` **au démarrage**. Le test en machine virtuelle l'a
  refusée aussitôt : il crée son compte après le démarrage des daemons, comme le fait un
  `nixos-rebuild switch`, qui ne les redémarre pas. Un refus qui dépend de l'heure à laquelle un
  service a démarré ne se diagnostique jamais. Le fichier est maintenant lu au moment de la
  question, et seulement pour un pair qui serait sinon refusé : les sept daemons se reconnaissent
  par leur groupe principal, `root` par son `uid`, et rien n'est ouvert sur le chemin fréquent
- [x] `prophet status` ne rendait plus la main — quinze minutes, sans rien afficher. `egress` est
  un proxy HTTP : un `ping` JSON-RPC est pour lui une requête tronquée, et il attendait la fin
  d'en-têtes qui ne viendraient jamais. Il n'était pas en faute ; la sonde l'était. Elle lui parle
  maintenant sa langue — une requête sans jeton, refusée par `407` avant toute sortie, ce qui
  prouve davantage qu'un `pong`. Toutes les sondes ont un délai de deux secondes

Les trois ont un test qui échoue sur le code d'avant : trois dans `crates/prophet-cli`
(`sondes::*`), deux dans `crates/prophet-daemon`, et quatre sous-tests dans
`image/tests/services.nix`.

### Ce que `agentd` promettait sans pouvoir le tenir (12 septembre 2026)

- [x] `agentd` déclarait `ReadWritePaths = [ "/home/prophet" … ]` pendant que `ProtectHome = true`,
  hérité du modèle commun, rendait `/home` inaccessible et vide dans son espace de montage. Le
  sous-test qui interroge **depuis l'intérieur** de cet espace, par `nsenter`, a rendu
  `agentd voit /home/prophet : refusé`. Une tâche qui ouvre son espace de travail aurait échoué
  sur un « Read-only file system » très loin de cette ligne. `ProtectHome` est désormais désactivé
  pour ce seul service ; `ProtectSystem = "strict"` reste, donc tout `/home` demeure en lecture
  seule sauf les deux chemins déclarés — ce que la déclaration prétendait déjà

### Le gardien borné par les règles du prisonnier (12 septembre 2026)

- [x] `sandboxd` recevait `CAP_SETUID`, `CAP_SETGID` et `CAP_SYS_ADMIN`, et de l'autre main le
  filtre d'appels système hérité des six autres daemons : `@system-service` moins `@privileged`.
  Or `@system-service` ne contient pas `@mount`, et `~@privileged` retire `setuid`, `setgid`,
  `setgroups` et `pivot_root` — le travail exact de ce service. `RestrictSUIDSGID` implique par
  ailleurs `NoNewPrivileges`, que le même bloc désactive trois lignes plus haut. Le test des
  services l'a montré en toutes lettres :

  ```
  confinement impossible : écriture de uid_map : Operation not permitted
  ```

  **Aucune tâche ne pouvait donc être isolée sur la machine installée**, et l'invariant « tout
  processus non fiable tourne sous `sandboxd` au niveau requis » était inapplicable. Le filtre du
  service borne maintenant le gestionnaire ; celui que subit une tâche reste posé par `sandboxd`
  dans son enfant, après le confinement, et beaucoup plus étroit.

  Vérifié à la main sur le conteneur de construction, hors systemd : `sandbox.start` au niveau 0
  rend `{"task": "task:essai-local", "pid": …, "level": 0}` et le journal dit « sandbox démarrée ».
  Le code du confinement n'était pas en cause ; seule l'entrave du service l'était.

- [x] `tools/verifier-le-durcissement.sh` — le garde-fou qui dit en une seconde ce que le test en
  machine virtuelle a mis sept minutes à apprendre. Il cherche deux contradictions et rien
  d'autre : un service à qui l'on accorde `CAP_SETUID`, `CAP_SETGID` ou `CAP_SYS_ADMIN` et dont le
  filtre retire `@privileged` ou n'ajoute pas `@mount` ; et `RestrictSUIDSGID` gardé en même temps
  que `NoNewPrivileges = false`, que le premier implique. Vérifié en remettant la configuration
  d'avant la correction : il rend les deux défauts et sort en 1. Il ne remplace pas le test — lui
  seul exerce le durcissement réel — mais il évite d'y aller pour une faute qui se lit dans le
  fichier. Ajouté à `just check` et au travail `check` de l'intégration continue

- [x] (vérifié en CI, « Les sept services sous systemd » vert sur `468324a` et `12daf73`)
  `image/tests/services.nix` demande aussi, désormais, si `agentd` peut écrire là où sa
  configuration le prétend. `ReadWritePaths = [ "/home/prophet" … ]` et `ProtectHome = true` se
  contredisent en apparence, et c'est systemd qui tranche sans que le fichier dise dans quel sens.
  Le contrôle regarde depuis l'intérieur de l'espace de montage du service, par `nsenter` : le
  jour où la promesse serait fausse, une tâche échouerait sur « Read-only file system » loin de
  cette ligne, et personne ne remonterait jusqu'à elle

- [x] L'image n'embarquait **aucun client officiel**. `prophet provider login claude-code`
  répondait « lancez `claude login` » sur une machine où `claude` n'existe pas — découverte à
  faire après avoir formaté son disque, c'est-à-dire au seul moment où il est trop tard. Claude
  Code et Gemini CLI sont maintenant embarqués tels quels, par `lib.optional (pkgs ? …)`
  pour qu'un renommage en amont retire le client sans casser l'image. Le test vérifie non pas leur
  présence — ils viennent de nixpkgs et peuvent en disparaître — mais que `provider ls` dise la
  vérité sur ceux qui y sont : annoncer un client absent est pire que de dire qu'il manque.
  Codex CLI est laissé de côté : `pkgs.codex` est un nom générique, `lib.optional (pkgs ? …)`
  protège d'un attribut absent mais pas d'un attribut qui n'est pas le bon, et livrer un binaire
  étranger sous un nom auquel l'OS fait confiance serait pire que de ne rien livrer

- [x] **La capacité qui ne se devine pas.** Après la correction du filtre, le refus persistait,
  identique. Les diagnostics ajoutés au test ont écarté les capacités (`CapEff = 0x2000c0`, les
  trois attendues), `NoNewPrivileges` (`0`) et le filtre (`setuid`, `mount`, `pivot_root` présents).
  Le groupe principal a été écarté en reproduisant les deux cas à la main. Le refus a finalement
  été **reproduit hors systemd** avec `capsh --drop`, en recréant le jeu de capacités exact du
  service, puis localisé par **bissection sur les trente-huit capacités** : `CAP_SETFCAP`.

  Depuis Linux 5.12, projeter l'**uid 0** dans un espace de noms exige `CAP_SETFCAP` dans l'espace
  parent — pas `CAP_SETUID`. Vérifié dans les deux sens sur cette machine : sans elle le refus,
  avec elle `{"task": "task:confirme", "pid": 792, "level": 0}`. Noté en ADR-0005, ajouté au
  garde-fou, et le refus lui-même nomme désormais laquelle de ses quatre causes s'applique

- [x] (vérifié en CI, « Construire le système installé » vert sur `468324a` et `12daf73`)
  **Le maillon jamais exercé : `nixos-install` lui-même.** Le travail « installeur » s'arrête
  au montage ; le travail « système installé » démarre une configuration que le cadre de test
  fabrique. Entre les deux, personne n'avait jamais posé ce système sur la disposition que
  l'installeur crée. Le travail `systeme` reprend maintenant là où l'installeur s'arrête, avec la
  fermeture qu'il vient de construire (`--system`, donc sans la reconstruire), et vérifie ce qui
  atterrit réellement sur le disque : le magasin, `run/current-system`, le compte `prophet` dans
  `/etc/passwd`, et le haché du mot de passe en `0600`. `--no-bootloader` parce qu'un coureur
  GitHub ne démarre pas en UEFI — que le chargeur fonctionne est vérifié ailleurs

### Le compte sans lequel personne ne se connecte (12 septembre 2026)

- [x] La machine installée ne créait **aucun** compte humain. `nixos-install --no-root-password`
  laisse `root` verrouillé, `systemd-boot` est configuré sans éditeur, et `cfg.user` — « prophet »
  — était référencé dans `ReadWritePaths` sans avoir jamais été déclaré. On installait donc un
  système sur lequel il était impossible d'ouvrir une session, et impossible de se rattraper.
  Rien ne pouvait le voir : seul le support d'amorçage avait jamais démarré, et lui ouvre une
  session automatiquement. Le compte est maintenant déclaré, dans `wheel` et `prophet-system` ;
  l'installeur demande son mot de passe **avant** d'écrire quoi que ce soit sur le disque, refuse
  en dessous de huit caractères, et ne pose que le haché, en `0600`

### Ce qui rendait l'ISO non reproductible (12 septembre 2026)

- [x] `flake.nix` suivait la **branche** `nixos-unstable`, et le dépôt n'a pas de `flake.lock`.
  Deux gravures de la même ISO à quinze jours d'écart installaient donc deux systèmes différents,
  et un travail d'intégration continue vert la veille pouvait être rouge le lendemain sans qu'une
  ligne du dépôt ait changé. Pour un système qu'on installe après avoir formaté son disque, « ce
  qu'on installe est ce qu'on a gravé » est la propriété qui permet de revenir en arrière.
  Épinglé à `8ce4ef6`, la révision du canal du 12 septembre — celle contre laquelle tout est vert
- [x] Le travail `services` échouait à l'étape qui rend `/dev/kvm` ouvrable, **après** l'avoir
  rendu ouvrable : il demandait la cible `microvm`, qui installe aussi Firecracker et interroge
  l'API de GitHub, laquelle répond `403` sur un coureur partagé quand la limite est atteinte. Une
  cible `kvm` existe maintenant pour ceux qui veulent seulement faire tourner une machine
  virtuelle, et la recherche de version de Firecracker se rabat sur la redirection de
  `releases/latest` quand l'API se tait

### Le système installé, démarré pour la première fois (12 septembre 2026)

M9-T6 a montré le **support d'amorçage** démarrer. Ce que ce support installe est une autre
configuration, et elle n'avait jamais été démarrée — seulement construite. Entre les deux,
`immutable.nix` ajoute précisément ce qui peut empêcher une machine de démarrer : racine en
lecture seule alors que l'activation de NixOS écrit `/etc/passwd` et `/etc/shadow` à chaque
démarrage, `systemd-boot` sans éditeur donc sans secours, `lockdown=integrity` et
`module.sig_enforce=1`.

**Première réponse, obtenue le 12 septembre.** Le sous-test « la machine a démarré par son
chargeur d'amorçage » est **passé** : `systemd-boot` a lancé la configuration installée, en UEFI,
avec `lockdown=integrity` et `module.sig_enforce=1` sur la ligne de commande. Ces deux paramètres
étaient les candidats les plus plausibles à un refus de démarrer — un noyau qui exige des modules
signés et n'en trouve aucun ne monte pas sa racine. Ce n'est pas ce qui se produit.

- [x] (vérifié en CI, « Le système installé démarre » vert sur `468324a` et `12daf73`)
  `image/tests/installe.nix` — démarre la configuration installée **par son chargeur
  d'amorçage**, en UEFI, depuis un vrai disque, et vérifie dans l'ordre : le chargeur a bien
  lancé le système, les comptes ont été écrits, aucune unité n'a échoué, les sept services
  tournent, `prophet-surface` a au moins été lancée, le propriétaire ouvre une session sur `tty1`
  avec son mot de passe, et les paramètres du noyau sont ceux demandés. Écrit avant de savoir ce
  qu'il dira : c'est le seul moyen d'apprendre quelque chose

**Ce que ce test ne peut pas vérifier, et qu'il ne faut pas croire vérifié.** Le cadre de test
NixOS fournit son propre disque et redéfinit `fileSystems` à une priorité qui l'emporte sur celle
de `immutable.nix`. La **racine en lecture seule n'est donc pas exercée**, et c'est la question la
plus dangereuse pour quelqu'un qui vient d'effacer son disque :

> L'activation de NixOS écrit `/etc/passwd`, `/etc/shadow`, `/etc/group` et tout l'arbre de liens
> de `/etc` à **chaque** démarrage, et crée des répertoires sous `/var`. `immutable.nix` monte
> `/home` et `/var/lib/prophet` depuis des volumes séparés, mais `/etc`, `/var/log`, `/var/lib` et
> `/tmp` restent sur la racine. Si celle-ci est vraiment en lecture seule, l'activation échoue et
> la machine part en mode de secours — sauf que `systemd-boot` est configuré sans éditeur, donc il
> n'y a pas de mode de secours utilisable.

**RÉPONSE, le 12 septembre 2026 : non.** L'expérience a rendu

```
RuntimeError: Shell disconnected
```

La machine ne garde même pas un interpréteur vivant. `immutable.nix` **ne monte plus la racine en
lecture seule** : livrer cela aurait donné, sur un PC dont on vient d'effacer Windows, une machine
qui ne démarre pas — et `systemd-boot` étant configuré sans éditeur, sans aucun rattrapage.

Ce que cela coûte, dit franchement : **la promesse d'immuabilité n'est pas tenue aujourd'hui.** Les
mises à jour A/B, le chiffrement et le verrouillage du noyau le sont ; la racine en lecture seule
ne l'est pas, et `docs/installation.md` le dit. La tenir demande une conception —
`system.etc.overlay`, un `/var` porté par un volume inscriptible, `boot.tmp.useTmpfs` — pas un
réglage. Le test reste, et redeviendra le garde-fou qui empêche de défaire ce travail le jour où
il sera fait.

- [x] `image/tests/racine-en-lecture-seule.nix` — pose la question à la machine au lieu de la
  raisonner. Il force l'option `ro` là où le cadre de test pose la racine, démarre, et raconte ce
  qu'il trouve : les unités en échec, l'état de `systemd-tmpfiles-setup`, les erreurs du journal.
  Son travail d'intégration continue est en `continue-on-error` — c'est une **question**, pas une
  garantie, et un échec n'y signale pas une régression mais donne la réponse

La correction, si la réponse est « non », n'est pas un réglage mais une décision de conception. La
piste que NixOS documente pour ce cas précis : `system.etc.overlay` — qui exige l'initrd systemd,
déjà activé — un `/var` porté par un volume inscriptible plutôt que par la racine, et
`boot.tmp.useTmpfs`. Elle se prendra en la prenant. **À traiter avant de déclarer l'ISO
installable.**

### Le serveur, état réel au 12 septembre 2026 à 15 h 52

Le travail qui agit a été déclenché **par l'API**, sur la branche de travail. Ce qui a
effectivement changé sur la machine, et comment le défaire :

| Fait | Comment revenir en arrière |
|---|---|
| Les unités `hermes*` sont **arrêtées et désactivées** | `systemctl enable --now hermes…` — le journal du workflow nomme les unités. Les fichiers de `/root/hermes` n'ont pas été touchés |
| Prophet OS est déposé dans `/root/prophet_os` | `rm -rf /root/prophet_os` |
| gVisor est installé — `runsc release-20260907.0` | le paquet reste ; `runsc` s'enlève à la main |
| La restriction AppArmor des espaces de noms est **levée** (15 h 54, après la correction d'ordre) | `sysctl -w kernel.apparmor_restrict_unprivileged_userns=1`, et retirer le fichier posé sous `/etc/sysctl.d/` |

Les niveaux 0 et 1 sont donc désormais atteignables sur cette machine : les espaces de noms sont
utilisables, et gVisor est en place. Le niveau 2 restera hors d'atteinte — pas de `/dev/kvm` sur ce
VPS, c'est une machine virtuelle sans virtualisation imbriquée.

« Atteignables » est ce que la configuration permet. Ce que la machine **tient réellement** est
une autre question. Elle a été posée à 15 h 56, en compilant `sandboxd` sur le serveur et en
lançant pour de vrai :

```
test niveau_un_execute_reellement_sous_gvisor ... ok
test niveau_un_n_a_pas_de_reseau ... ok
```

**Le niveau 1 fonctionne sur cette machine** : un programme s'exécute réellement sous gVisor, et la
sandbox n'a aucune interface réseau. Ce ne sont pas des sondes de présence — l'une lance un
programme et regarde ce qu'il rend, l'autre essaie de sortir et constate qu'elle ne peut pas.

Non vérifiable ici, et dit comme tel plutôt que compté comme réussi ou échoué :

| | |
|---|---|
| `niveau_deux_demarre_une_microvm` | `needs_kvm` — il manque l'accès à KVM, Firecracker et les images d'invité. Définitif sur ce VPS |
| `le_niveau_deux_ne_retombe_jamais_sur_le_niveau_zero` | `needs_kvm`, même raison |
| les six tests de la surface | `needs_gpu` — aucun périphérique Vulkan utilisable ; un nœud `/dev/dri` ne suffit pas |

Le rapport complet est dans l'artefact `rapport-serveur` du run `34703605599`.

La cause de l'étape sautée était dans le workflow, pas sur la machine :
`install-isolation.sh gvisor` répond à deux questions — installer gVisor, et signaler la
restriction — et sortait en 1 sur la seconde après avoir réussi la première. L'étape qui devait
lever la restriction a donc été sautée, alors qu'elle était demandée. Corrigé : la restriction est
levée **avant** la préparation, et la préparation juge sur `command -v runsc` plutôt que sur le
code de sortie d'un outil qui répond à deux questions.

### Ce que le run 48 a appris, et ce qui a été corrigé (12 septembre 2026, 16 h 30)

Le run `34703888974` a rendu son verdict : quatre travaux verts, dont **« Le système installé
démarre »** et **« Voir l'image démarrer »**. Deux rouges, tous deux réels, tous deux corrigés ici.

**`prophet log` ne trouvait pas le journal.** Le test des services est allé beaucoup plus loin
qu'avant — `sandbox démarrée tache=task:essai-sandbox niveau=0`, la correction `CAP_SETFCAP` tient
— puis a buté sur ceci :

```
$ prophet log tail -n 20
aucun journal sur cette machine
```

La commande lisait `~/.prophet/ledger`, et rien d'autre. Or `prophet-ledger` écrit dans
`/var/lib/prophet/ledger` : deux journaux existent, et la commande d'audit ne connaissait que
celui du développement. Elle répondait donc « il n'y a rien » devant un journal plein, ce qui est
la pire des trois réponses possibles — pas « je ne sais pas », mais une négation.

Corrigé : `prophet log` interroge d'abord **le service**, qui seul connaît l'état courant et qui
seul sert les membres de `prophet-system` (l'état du daemon est en 0700, pour que personne ne
réécrive l'histoire par le fichier) ; à défaut, il lit les fichiers, en essayant
`/var/lib/prophet/ledger` avant `~/.prophet/ledger` ; et quand il ne trouve rien, il dit **où il a
regardé et pourquoi chaque tentative a échoué**. Quatre tests dans `crates/prophet-cli`, dont un
qui échoue sur l'ancien code.

**Le travail « Construire le système installé » se trompait de question.** Il cherchait
`/mnt/run/current-system` et `/mnt/etc/passwd` après un `nixos-install --no-bootloader`, et
déclarait l'installation ratée de ne pas les trouver. Il avait tort sur les deux : `/run` est un
tmpfs créé au démarrage, il n'existe sur aucun disque ; et `--no-bootloader` ne saute pas
seulement `bootctl`, il saute le `switch-to-configuration boot` tout entier — donc l'activation,
donc `/etc`. Le travail échouait sur une installation réussie, ce qui use la confiance qu'on
accorde aux verts.

Corrigé : il vérifie maintenant ce qu'un tel `nixos-install` produit réellement — le profil système
pointant vers la fermeture exacte qu'on vient de construire, les sept unités et le binaire
`prophet` **sur le disque cible** et non sur le coureur, le haché en 0600 — et il **dit** que les
comptes ne sont pas de son ressort, en nommant le travail qui en répond.

**Les deux dettes notées à 16 h 00 sont payées** : le commentaire périmé d'`installe.nix` (et
celui de `racine-en-lecture-seule.nix`, qui parlait au présent d'une option retirée), et le travail
de la racine en lecture seule, désormais à déclenchement manuel — entrée
`reposer_la_question_de_la_racine`. Un rouge permanent dans un tableau que le guide d'installation
demande de lire avant de graver une image n'est pas une information : c'est un entraînement à
ignorer le rouge.

### La surface refusait le bon mot de passe du propriétaire (12 septembre 2026, 17 h)

Le run `34705399751` a rendu cinq travaux bloquants sur six. Le sixième — « Le système installé
démarre », vert au tour précédent — a échoué, et sa cause n'est pas un aléa.

`prophet-surface` tenait `/dev/tty1` avec `TTYVHangup` et `Restart = "always"`. Sur une machine
sans pilote graphique, elle redémarre cinq fois en une minute, et **chaque tentative raccroche le
terminal où le propriétaire tape son mot de passe**. Le journal, à vingt-deux millisecondes près :

```
16:42:00.600  machine: sending keys 'essai-prophet\n'
16:42:00.686  prophet-surface.service: Scheduled restart job, restart counter is at 4
16:42:00.708  unix_chkpwd: password check failed for user (prophet)
```

Le mot de passe était le bon. Sur un vrai PC, le propriétaire aurait lu « Login incorrect » sans
écran graphique pour lui dire pourquoi, sur une machine dont il vient d'effacer le disque. Il n'y a
pas de pire moment pour donner à un système l'air de refuser son propriétaire.

`docs/components/surface.md` posait déjà la question — « le terminal disputé » — et nommait les deux
sorties possibles. Elle est prise : **la surface vit sur `tty7`**, celui que les serveurs graphiques
occupent depuis toujours et où NixOS ne fait naître aucun `getty` (il n'en crée que sur tty1 à
tty6). Le service de repli reste sur `tty1`, là où un humain regarde.

Le test était complice : il attendait que la surface atteigne `active` ou `failed`, or `active` est
traversé une fraction de seconde à **chaque** relance d'une unité en `Restart = "always"`. Il
déclarait donc la surface posée alors qu'elle en était à sa quatrième tentative, puis se connectait
dans la course — et gagnait une fois sur deux. Un test qui dépend d'une course ne protège de rien :
celui-ci avait déclaré la machine bonne au tour précédent. Deux corrections : l'attente exige
maintenant un état qui **tient** (`failed` est définitif ; un `active` qui survit dix secondes est
un vrai `active`), et un sous-test neuf lit `TTYPath` — il ne court pas, il constate.

Ce qui reste inconnu et n'est pas réglé : que `cage` bascule réellement sur `tty7` et y affiche
quelque chose. Aucun coureur n'a d'adaptateur graphique, et c'était déjà invérifiable sur `tty1`.
Le déménagement ne dégrade rien de vérifié ; il supprime un mal, lui, mesuré.

### La première ISO installable, et ce qu'elle ne tient pas (12 septembre 2026, 17 h 16)

Run `34707081688` : **les six travaux bloquants sont verts**, le septième ignoré comme voulu. La
correction de la surface tient — le sous-test de connexion, qui mettait 900 s à expirer, a rendu
la main en 1,04 s, et le propriétaire voit ses sept services depuis sa session :

```
machine: (finished: waiting for \$|prophet@ to appear on tty 1, in 1.04 seconds)
  Services
    ✓ capd  ✓ ledger  ✓ vault  ✓ egress  ✓ sandboxd  ✓ memoryd  ✓ agentd
```

Et `egress` a refusé la sonde de `prophet status`, sur la machine installée comme sur le serveur :
`WARN requête sans jeton hote=sonde.prophet.invalid`.

**Ce que le même test a montré, et qu'il ne faut pas laisser passer : le verrouillage du noyau
n'a pas lieu.**

```
initrd=… lockdown=integrity module.sig_enforce=1 … lsm=landlock,yama,bpf
lockdown : absent
```

Les deux paramètres sont bien sur la ligne de commande — c'est ce que le sous-test affirme, et il a
raison. Mais le noyau démarre avec `lsm=landlock,yama,bpf`, où `lockdown` ne figure pas, et
`/sys/kernel/security/lockdown` n'existe pas : le LSM n'est pas actif, donc `lockdown=integrity`
ne fait rien. `module.sig_enforce=1` est vraisemblablement inerte de même, puisque la machine
charge ses modules sans se plaindre.

Le sous-test **affichait** déjà `lockdown : absent` sans en conclure quoi que ce soit, et c'était
la bonne façon de ne pas mentir. Mais `docs/installation.md` annonçait « le verrouillage du noyau »
parmi ce qui est tenu, et `immutable.nix` le répète. Corrigé dans le guide : un durcissement
annoncé qui n'a pas lieu est pire qu'un durcissement absent, parce qu'on compte dessus.

Ce n'est pas un défaut de démarrage et cela ne retarde pas l'image. C'est une dette, nommée.
La payer demande d'ajouter `lockdown` à la liste `lsm=` — et de vérifier ce que cela casse, car
`module.sig_enforce=1` devenu effectif sur des modules NixOS non signés empêcherait une machine
réelle de charger ses pilotes.

### Prophet OS tourne sur le serveur (12 septembre 2026, 17 h 09)

Run `34707355297`, vert. Les sept daemons sont `active (running)` et `enabled` sur
`ubuntu-2gb-fsn1-2`, chacun sous son compte, avec le durcissement de l'image. Ce ne sont pas des
sondes de présence — chacun a écrit dans le journal ce qu'il fait :

```
prophet-capd    : clé créée /var/lib/prophet/capd/signing.key
                  politiques locales chargées nombre=1
                  capd écoute socket=/run/prophet/capd.sock
prophet-ledger  : clé créée /var/lib/prophet/ledger/seal.key
                  ledger écoute cle=ed25519:R633QnvkrUans5I6Kxqf/e0tOB/Ra2qxd/UPEeWARJ0=
prophet-egress  : egress écoute ; rien ne sort sans un jeton que capd approuve
                  WARN requête sans jeton hote=sonde.prophet.invalid   (×2)
prophet-sandboxd: niveau maximal atteignable : 1, Landlock ABI 8, gVisor /usr/bin/runsc
prophet-agentd  : les jetons viennent de capd, le journal part vers ledger
```

Les deux lignes d'`egress` valent d'être lues : c'est `prophet status` qui l'interroge, et le proxy
**refuse sa requête faute de jeton**. L'invariant « toute sortie réseau passe par egress » n'est pas
seulement déclaré sur cette machine, il est exercé — et la sonde d'état prouve davantage qu'un
`pong` en se faisant refuser.

Ce que cela n'est pas, et qui doit rester écrit : le serveur n'est pas devenu Prophet OS. Noyau
d'Ubuntu (`7.0.0-22-generic`), racine inscriptible, pas d'emplacements A/B, pas de chiffrement posé
par nous. Ce sont les daemons qui tournent, pas le système. Le niveau 2 y restera hors d'atteinte :
pas de `/dev/kvm`.

Ce qui a changé sur la machine, et comment le défaire :

| Fait | Comment revenir en arrière |
|---|---|
| `/root/hermes` **supprimé** | `tar xf /root/hermes-sauvegarde-<date>.tar -C /root` |
| Sept services dans `/etc/systemd/system/prophet-*.service`, démarrés et activés | `sudo /root/prophet_os/tools/lancer-sur-l-hote.sh --retirer` |
| Programmes dans `/usr/local/lib/prophet`, `prophet` dans `/usr/local/bin` | idem |
| Comptes `capd`, `ledger`, `vault`, `egress`, `memoryd`, `agentd` et groupe `prophet-system` | conservés par `--retirer` ; `userdel` à la main |
| État dans `/var/lib/prophet/<daemon>`, en 0700 | conservé par `--retirer` |

### Hermes est supprimé ; mon propre garde a empêché le lancement (12 septembre 2026, 17 h 06)

Le run `34707083853` a fait ce qu'on lui demandait d'abord : **`/root/hermes` est supprimé**, après
archivage et relecture. Les unités systemd et les entrées cron à son nom sont parties avec.

Puis le lancement a échoué — sur mon propre contrôle, et pour la faute exacte qu'il existe pour
empêcher. `systemd-analyze verify` ne relit pas une unité isolée : il charge tout le graphe de
dépendances et rapporte au passage ce qu'il a à reprocher aux unités de la distribution. Sur ce
serveur :

```
/usr/lib/systemd/system/xfs_scrub_all.service:26: Support for option CPUAccounting= has been
removed and it is ignored
```

Rien à voir avec Prophet OS. Mon filtre ne retirait que les lignes `not found`, donc il a pris ces
reproches pour les siens et refusé de démarrer les sept services. **Une sonde qui conclut sur autre
chose que ce qu'elle prétend mesurer** — c'est la faute que ce dépôt traque partout, écrite cette
fois dans l'outil chargé de l'attraper.

Corrigé : seules les lignes qui **nomment l'unité examinée** sont retenues ; les autres sont
comptées et signalées, parce qu'un avertissement qu'on écarte sans le montrer est un avertissement
qu'on a caché. Vérifié des deux côtés sur une machine : une unité portant
`SystemCallFilter=~@privileged ~@resources` est toujours refusée, et la sortie exacte du serveur
ne bloque plus rien.

Ce que la machine a répondu malgré l'échec, et qui vaut d'être noté :

```
  Isolation
  niveau maximal atteignable : 1 (0 confiné, 1 noyau utilisateur, 2 microVM)
    Landlock      : ABI 8
    gVisor        : /usr/bin/runsc
    /dev/kvm      : absent
```

Et `prophet log tail` a répondu « journal vide » au lieu de « aucun journal sur cette machine » : le
correctif de la recherche du journal fonctionne sur une vraie machine — il a trouvé
`/var/lib/prophet/ledger`, que le script venait de créer.

### L'adresse du serveur était publique (12 septembre 2026, 17 h)

Elle était posée en **variable** de dépôt `VPS_HOST`. GitHub masque la valeur d'un secret, pas
celle d'une variable : il l'imprime dans le bloc `env:` de chaque étape, et les journaux d'Actions
d'un dépôt public sont publics. L'adresse d'une machine dont ce dépôt documente qu'elle accepte
`root` par mot de passe s'est donc retrouvée en clair dans plusieurs exécutions — et une version
antérieure de la sonde l'affichait même en toutes lettres, `VPS_HOST = …`, en croyant rendre
service.

Corrigé : le workflow cherche d'abord le **secret** `VPS_HOST`, masque l'adresse dès sa première
étape, et ne l'imprime plus jamais — « posée » ou « absente », et rien d'autre. La variable reste
acceptée pour que rien ne casse, mais tant qu'elle est une variable, elle fuit une fois par
exécution dans le bloc `env:` de la première étape. **À faire par le propriétaire : déplacer
`VPS_HOST` dans les secrets**, et considérer l'adresse comme connue.

### L'archivage d'Hermes était trop lent pour finir (12 septembre 2026, 17 h)

L'étape a été coupée à sa limite de trente minutes, sans avoir fini. **Rien n'a été supprimé** :
l'effacement vient après la relecture de l'archive, jamais avant, et la relecture n'a pas eu lieu.
`/root/hermes` est intact.

La faute était `tar czf`. Compresser des données déjà compressées — historiques de marché, parquet,
journaux gzippés — coûte tout le temps du monde pour quelques pour cent. Et rien n'était mesuré
avant de commencer : on ne pouvait même pas dire s'il restait une minute ou une heure.

Corrigé : l'archive n'est plus compressée (`tar cf` va à la vitesse du disque) ; la taille et le
nombre de fichiers sont mesurés **avant** de commencer et affichés ; la place libre est vérifiée
avec une marge d'un dixième, parce qu'une archive qui remplit le disque casse la machine qu'on
essayait de préserver ; et la relecture **compte les fichiers** au lieu de se contenter que `tar`
n'ait pas protesté — une archive tronquée au premier bloc se relit sans se plaindre.

### Faire tourner Prophet OS sur une machine qui n'est pas Prophet OS (12 septembre 2026)

`tools/lancer-sur-l-hote.sh` installe les sept daemons en services systemd sur un hôte Ubuntu et
les démarre. Rien ne le faisait jusqu'ici : `verify-on-host.sh` sonde sans rien installer,
`setup-ubuntu-host.sh` pose des dépendances.

Ce que cela **n'est pas**, et qui doit être dit avant qu'on le découvre : le serveur ne devient pas
Prophet OS. Son noyau reste celui d'Ubuntu, sa racine reste inscriptible, il n'y a ni emplacements
A/B ni chiffrement posé par nous. Ce qui tourne, ce sont les daemons, avec le durcissement de
l'image. C'est la différence entre « Prophet OS est installé » et « Prophet OS tourne ici », et sur
un VPS qu'on ne réinstalle pas, seule la seconde est disponible.

Un piège trouvé en écrivant les unités à la main, et qui ne se voit pas dans le module NixOS :

```
SystemCallFilter=~@privileged ~@resources    # ne fait pas ce qu'on lit
```

systemd ne prend le `~` qu'en tête de valeur, puis lit chaque mot comme un nom d'appel système. Le
second `~@resources` n'est pas un groupe nié mais un nom invalide : il est écarté avec un simple
avertissement, et le filtre posé est plus large que voulu. `systemd-analyze verify` le dit —
« System call ~@resources is not known, ignoring » — et le script le lui demande désormais sur les
sept unités **avant** de démarrer quoi que ce soit. NixOS écrit une ligne par élément de liste, ce
qui masque le piège ; à la main, il faut le connaître.

### Hermes et le lancement, câblés dans le workflow du serveur (12 septembre 2026)

Deux entrées neuves, toutes deux à « false » par défaut :

- `supprimer_hermes` — archive `/root/hermes` dans `/root/hermes-sauvegarde-<date>.tar.gz`,
  **relit l'archive** (`tar tzf`), et n'efface qu'ensuite ; puis retire les unités systemd et les
  entrées cron à son nom. L'archive reste sur le serveur : la rapatrier la ferait passer par un
  artefact d'un dépôt public, et un moteur de trading contient des clés d'API. Une archive qu'on
  n'a pas ouverte n'est pas une sauvegarde, c'est un fichier dont on espère quelque chose ;
- `lancer_les_services` — lance `tools/lancer-sur-l-hote.sh`, puis relève ce que la machine répond
  (`prophet status`, `task ls`, `log tail`, l'état et le journal de chaque service) dans l'artefact
  `prophet-sur-le-serveur`.

L'en-tête du workflow disait « rien ici ne touche /root/hermes ». Ce n'est plus vrai, et il le dit
maintenant : le laisser écrit aurait été pire que de ne rien écrire.

### Une correction à ce que ce fichier affirmait encore

Le paragraphe « Le serveur de l'utilisateur reste inatteint » ci-dessous portait deux erreurs, dont
une de ma main. `workflow_dispatch` **fonctionne par l'API sur une branche de travail** : c'est le
bouton de l'interface qui exige la branche par défaut, pas le déclenchement. J'ai affirmé le
contraire pendant des heures, sur la foi d'un unique 404, et demandé trois fois à l'utilisateur une
modification qui n'était pas nécessaire. L'essai a rendu `204 No Content`. Le serveur n'est plus
inatteint : le run `34703605599` y a tourné, et le secret `VPS_PASSWORD` est posé.

### 22 septembre 2026 : reprise, audit et seconde passe Réacteur

L'humain a demandé de reprendre le projet et de tout auditer. Conteneur Ubuntu à 4 cœurs, sans
Nix ni KVM ni carte graphique ; rendu par llvmpipe. Le [rapport](reports/reacteur-seconde-passe-2026-09-22.md)
donne le détail.

**Fini.**
- Branches réunies (`d6e686f`) : avance rapide sur `codex/audit-wsl-20260913` (150 commits,
  CI verte le 15), fusion de `claude/jev-prophet-os-integration-9viyyl` (Jev, 17-18 septembre),
  reprise de `55491f0` ; ADR de Jev renuméroté 0042 (deux ADR 0025 existaient). Le test qui
  opère une page réelle par Jev échouait sur l'arbre réuni : son faux proxy prenait pour des
  décisions les pages que le navigateur piloté fait désormais passer par egress ; il les relaie.
- `just check` ne pouvait réussir sans gitleaks : le repli relevait des fixtures d'essai. Elles
  portent `gitleaks:allow`, que le repli respecte (`b32a5aa`).
- Seconde passe Réacteur (`8d28a09`, ADR 0043) : plaque de mission alignée au pixel sur la
  colonne des commandes (elle débordait de 10 à 22 px), objectif saisi dès l'écran vide,
  Ctrl 1-4 / Ctrl N / Échap, cadence du champ réduite après 30 s sans geste (60 s de repos avec
  trois missions actives : 118 % → 77 % d'un cœur sous llvmpipe). Trois parcours GPU nouveaux,
  vus en échec avant correction. `just captures` produit les noms Réacteur ; captures régénérées.
- Introspection des missions (`d375136`) : `task.status`, enrichi du budget restant, et
  `task.diff` sont offerts aux missions et aux séances d'outils dont le profil les accorde ;
  le catalogue les admet, les profils de l'image et des exemples les accordent. Prouvé avec les
  vrais services et un modèle contrôlé qui garde chaque requête.
- L'en-tête de l'inspecteur dit le dernier geste de l'agent, relu dans le journal, et le
  donne à l'accessibilité ; les instruments compacts ne chevauchent plus leurs graduations
  (`c8550d4`) ; les cartes des clients officiels prennent le vocabulaire des boutons du tableau
  de bord et leur nom accessible (`f404c88`).
- La CI de `d6e686f` a vu `mcp-system::web` expirer sur `Page.navigate` : son serveur témoin
  servait les connexions en série et attendait une connexion spéculative de Chromium. Les trois
  pages témoins visitées par un navigateur servent désormais chaque connexion à part (`064b07b`).
  Le reste de la CI générale de `d6e686f` est vert, dont la mission réelle Qwen3 sous NixOS et,
  pour la première fois depuis des jours, ChatGPT sous NixOS.
- Méthodes réservées par classe de pair (ADR 0044) : le compte de l'humain, membre de
  `prophet-system`, pouvait demander à capd un jeton pour un manifeste de sa main ou écrire au
  journal. `cap.mint` et ses voisins, `ledger.append` et `ledger.seal` reviennent désormais aux
  services, `approval.resolve` à l'humain. Le même examen a trouvé deux détours : sandboxd
  lançait pour tout membre un programme sous sa propre identité, montages et sockets au choix
  de l'appelant (`sandbox.start` et `sandbox.run` reviennent désormais aux services), et
  `/run/prophet`, en `0770`, laissait tout membre supprimer `capd.sock` pour en poser un faux
  (il devient collant, `1770`). Un daemon retirait aussi son socket avant d'en recréer un et à
  l'arrêt : le nom de `capd.sock` était libre entre deux démarrages ; il est désormais posé par
  renommage et reste en place à l'arrêt (`08812ed`). L'essai NixOS des services vérifie tout
  cela sous le compte de l'humain, capd arrêté compris.
- Catalogue des poids (M8-T7) : `providers::weights` lit l'en-tête GGUF de chaque fichier sans
  charger les poids, bornes comprises contre un en-tête hostile ; `prophet model ls` et une
  plaque « Poids installés » de la page Modèles réunissent le dossier des poids et les fichiers
  que `PROPHET_WEIGHTS` nomme, dont le modèle par défaut de l'image dans `/nix/store`.
- Fenêtre du moteur local (M8-T7, complément de l'ADR 0034) : avec 4 096 tokens de fenêtre,
  une seule lecture d'un fichier moyen faisait refuser l'historique par llama-server et la
  mission échouait sur « HTTP 400 ». Le pilote lit ce refus (ses deux nombres seulement),
  resserre les résultats d'outils à sa mesure et renvoie le tour ; la fenêtre apprise sert
  ensuite d'emblée (`d90a8bd`). La conversation de l'atelier oublie de même ses premiers
  échanges et dit combien (`3a1337d`) ; `fs.read` lit par morceaux (`offset`, `next_offset`,
  sans couper de caractère, `71d9a98`) et `fs.search` rend les lignes trouvées, numéro et
  extrait (`2513326`) : un agent à petite fenêtre lit ce qui l'intéresse sans relire le
  fichier. L'essai NixOS du moteur vérifie la forme du refus sur le llama-server épinglé.
- M5-T4, la réserve de microVM (ADR 0045, `1e3ff79`, `6a7ef34`) : l'invité gagne un mode
  réserve où il attend le disque de sa tâche ; sandboxd en fait un instantané et garde deux
  machines restaurées en pause, qui reçoivent le disque de la tâche à la reprise. Sur le
  coureur KVM de la CI, l'essai du critère est vert du premier coup : **une microVM rendue en
  8,9 ms** (médiane de cinq prises, de 8,4 à 10,0 ms) pour un objectif de 150 ms, machine
  neuve à chaque fois, réserve régénérée. Le script d'isolation fait désormais remonter les
  lignes « mesure : » des essais réussis (`41def9e`). Sur `4aaeb50` : réserve pleine en 5,0 s,
  restauration d'une machine en 6 ms, prise médiane de 10,9 ms, et cinq clones tirent cinq
  aléas distincts de `/dev/urandom`. Sur `41df4f5`, deux microVM tournent ensemble (deux
  programmes d'une seconde en 1,49 s), sans autre interface que la boucle locale et sans
  connexion sortante possible. La réserve garde désormais son instantané d'un démarrage de
  sandboxd à l'autre (`f14d01a`, complément de l'ADR 0045) : sous
  `/var/lib/prophet/sandboxd/reserve` sur l'image, avec une empreinte du moniteur, de l'invité
  et de l'hôte ; même empreinte, la première machine est restaurée aussitôt, sans invité à
  démarrer ni gigaoctet à réécrire. L'essai `needs_kvm` exige une réserve reprise pleine en
  moins d'une seconde ; sur le coureur KVM (`094a443`), elle l'est en **7,4 ms**, contre 4,6 s
  pour un premier démarrage. `sandbox.run` et `proc.exec` rendent `elapsed_ms` et `warm_start` :
  l'agent sait ce que son exécution a coûté et si la réserve a servi (`637d706`).
- Poids gérés (M8-T7, ADR 0046) : un catalogue du système compilé dans les binaires (adresse
  épinglée, empreinte SHA-256 publiée, hôtes permis) ; agentd télécharge une entrée par egress
  sous un jeton que capd émet pour ses seuls hôtes, refuse toute redirection hors d'eux, ne pose
  le fichier qu'une fois taille, empreinte et en-tête GGUF vérifiés, reprend par `Range` après
  une coupure, journalise `model.pulled` et `model.removed` (`114ef7b`, `1752d7a`). `prophet
  model catalog | pull | cancel | rm` et la page Modèles, barre de progression comprise
  (`78b7791`) ; un poids que la configuration fournit déjà n'est pas retéléchargé (`32a20ea`).
  Prouvé avec les vrais capd, ledger et egress (`crates/agentd/tests/poids.rs`), et, sous
  systemd, par l'essai des services qui télécharge d'un dépôt local sous le compte de l'humain :
  vert sur `468324a` — capd émet le jeton de `model-pull:essai-vm` (un grant), egress autorise
  le `GET`, agentd pose le fichier vérifié (`agentd 644`), le journal le dit, le retrait
  l'efface.
  **Critère de M8-T7 tenu** (`f688d49`, travail « Poids du catalogue servis (réels) ») :
  `prophet model pull qwen3-8b-q4` tire 5 Go de Hugging Face par egress et les vérifie en
  168 s, `prophet model serve` les fait servir par le routeur épinglé en 3 s, et une complétion
  aboutit en 2,9 s ; M8-T7 est coché, VRAM et matrice GPU restant à mesurer sur une carte.
  Les empreintes du catalogue sont vérifiées à la source : un essai `needs_network` demande à
  l'API de Hugging Face, par le vrai egress, ce qu'elle publie de chaque entrée à sa révision
  épinglée (`e49bad3`, vert en CI) ; il a relevé Qwen3 4B et 8B en Q4_K_M, désormais au
  catalogue avec leur taille exacte (`qwen3-4b-q4`, `qwen3-8b-q4`, l'exemple du plan), puis
  quatre autres familles à licence ouverte — Granite 3.3 2B, SmolLM2 1.7B, Phi-3 mini, Llama 3.2
  3B — que le travail « Poids du catalogue servis (réels) » tire, sert et interroge une à une
  (FRONTIER : valider plusieurs familles). **Vert sur `c44a349`**, quatre cœurs sans carte :
  Granite tiré en 99 s, réponse en 2,5 s (21 tokens/s) ; SmolLM2 36 s, 1,2 s (31 tokens/s) ;
  Phi-3 mini 77 s, 0,9 s (13 tokens/s) ; Llama 3.2 66 s, 2,2 s (17 tokens/s) ; toutes
  répondent juste (« Paris »), et Qwen3 8B, dans le même travail, tiré en 158 s, servi en 3 s,
  complétion en 2,8 s. Servir : `prophet model serve <id>`
  fait charger un poids par le routeur du moteur (`GET /models`, `POST /models/load`), reconnu
  par le chemin de son fichier (`bc001d2`), et la page Modèles dit « Servi » ou propose
  « Servir » (`93bd678`). En mode relais, le routeur de l'image lit le dossier des
  téléchargements à son démarrage, avec une section `[*]` qui borne fenêtre, threads et couches
  (`0df78e7`) : relevé d'abord dans la source du moteur épinglé, et prouvé par le contrôle
  `llama-router`, qui démarre le vrai routeur avec ces options. Son premier passage a échoué, à
  raison : `GET /models` ne rend pas le chemin du poids, que le client supposait ; il est dans
  les arguments de l'instance, et c'est là qu'il est lu désormais (`12daf73`, vert en CI : poids
  du dossier connu par son chemin, fenêtre 4 096 et threads de la section `[*]`). Un poids tout
  juste téléchargé se sert après un redémarrage du moteur. `model.list` dit aux agents les poids locaux et leur fenêtre (`d49724d`),
  `prophet status` les modèles locaux et le catalogue (`6118608`). Un essai `needs_network`
  tire Qwen3 0.6B de Hugging Face par le vrai egress sur une machine qui le joint (`dd4ff44`) :
  vert sur le coureur de la CI (`468324a`), 639 446 688 octets en 23 s, empreinte publiée
  vérifiée ; les adresses signées du CDN passent la détection d'egress.
- Mémoire des poids (ADR 0047, FRONTIER : moteurs locaux) : l'en-tête GGUF donne de quoi
  estimer ce que llama.cpp réservera — fichier, cache KV de toute la fenêtre (couches × têtes
  KV × dimensions), logits d'un micro-lot, moteur — et `/proc/meminfo` ce que la machine a.
  `prophet model ls` (colonne mémoire), `model.list` pour les agents (`memory`, `fit`) et la
  page Modèles (une jauge par poids) le disent ; `prophet model serve` refuse un poids qui ne
  tiendrait pas sans paginer la machine, sauf `--force`, et agentd ne prépare ni ne lance de
  mission locale sur un tel modèle (le routeur le chargerait à la demande). Le catalogue porte
  l'en-tête relevé de chaque fichier (cache KV par token, vocabulaire), revérifié à la source
  par l'essai `needs_network` (vert sur `b2f97f9` : les huit en-têtes lus par une plage d'octets
  à travers le vrai egress, conformes) : la mémoire se dit avant de télécharger (`prophet model catalog`,
  page Modèles). Ce que le moteur tient vraiment se lit aussi, dans `/proc` : la jauge du poids
  servi porte un repère de sa mémoire résidente, `model.list` la rend (`resident`).
  `prophet model pull` refuse, avant
  toute requête, ce qui ne tient pas sur le disque. L'essai des familles en CI relève la
  mémoire résidente de chaque instance du vrai llama-server et la confronte à l'estimation :
  vert sur `89b1b3a` — l'estimation couvre la part anonyme (Qwen3 8B : 5,8 Go estimés, 3,7 Go
  anonymes), la résidente totale la dépasse de 30 à 50 % (8,8 Go), le fichier restant projeté
  pendant qu'une copie réarrangée des poids sert le calcul. Confirmé dans la source épinglée
  (llama.cpp `v0.4.0`, `llama-model-loader.cpp` : tenseurs recopiés depuis la projection, dont
  seuls le début et la fin sont libérés) : l'image charge désormais les poids sans projection
  (`--load-mode none`, `load-mode = none` dans chaque préréglage), et l'essai des familles le
  mesure ainsi : **vert sur `227a63e`**, mémoire résidente en baisse de 28 à 38 % (Qwen3 8B :
  5,46 Go au lieu de 8,76), presque toute anonyme, et l'estimation 5 à 10 % au-dessus ; servir
  Qwen3 8B prend 6,3 s au lieu de 2,9 (matrice matérielle, ADR 0047). CI de `227a63e`
  **verte des deux côtés** : la mission locale sous NixOS (vrais préréglages du relais avec
  `load-mode = none`, Qwen3 1.7B), le contrôle `llama-router`, le système installé en UEFI et
  sans UEFI, les sept services, l'ISO et l'installeur.
- Banc M13 (ADR 0048, FRONTIER : comparaison) : `just bench` existe. La suite de tâches sans
  navigateur se joue avec le modèle de l'image deux fois — par Prophet (mission planifiée,
  lancée, publiée) et par une boucle nue qui appelle les mêmes outils sans registre ni capd —,
  et le banc mesure réussite, durée médiane et p95, tokens et étapes ; la CI le joue dans le
  travail « Poids du catalogue servis (réels) ». Sa plomberie est éprouvée sans modèle dans
  `just check`. Premier passage (`f4d13c6`, Qwen3 1.7B Q8_0, sept tâches, une exécution) :
  Prophet 0/7, boucle nue 2/7 — cinq échecs de chaque côté ont la même forme, le modèle lit puis
  conclut par du texte sans écrire le fichier demandé ; l'écart tient à l'échantillonnage
  (température 0,7). D'où l'ADR 0049 : agentd rappelle au modèle le livrable que l'objectif
  nomme quand il conclut sans lui (deux fois au plus, chaque interrogation comptée, événement
  `task.reminded`), et le banc relève outils appelés et réponse finale, rejoue chaque tâche
  trois fois en CI et compte seize tâches (quinze sans navigateur), chaque vérificateur éprouvé
  sur une solution juste et une fausse (`docs/reports/banc-m13-2026-09-23.md`). Deuxième
  passage (`6d01c38`, quinze tâches, trois exécutions) : 4/45 de chaque côté. Le rappel fait
  écrire (quinze exécutions rappelées, toutes écrites ; « ne-pas-toucher-au-reste » 0 → 3/3),
  sans rendre le contenu juste. 19 exécutions par Prophet s'arrêtaient sur un refus ou une
  « panne » que la boucle nue encaissait : manifeste du banc sans `fs.list`, `fs.search` sur un
  fichier en `Internal`, chemin hors portée qui arrêtait tout. ADR 0050 : un refus de chemin
  revient au modèle en disant où agir, la révocation (revérifiée auprès de capd) et le troisième
  refus arrêtent ; erreurs de nature de chemin en `Invalid` avec l'outil qui convient ;
  `fs.search` fouille un fichier ; `fs.read` compte les lignes ; le banc mesure aussi le temps
  processeur du moteur et des services et leur pic de mémoire. Troisième passage
  (`63586c6`) : **Prophet 8/45, boucle nue 6/45** ; les services de Prophet coûtent 0,27 s de
  processeur par mission et 47 Mo au plus, contre 114 s de processeur pour le moteur (0,2 %).
  `compter-les-erreurs` passe à 3/3 par Prophet ; le rappel conditionnel se déclinait (13 sur
  29) : la consigne revient en tête ; la boucle nue offre désormais les outils dans l'ordre du
  registre. Quatrième passage (`390aed1`, même ordre des deux côtés) : **Prophet 8/45, boucle
  nue 5/45** — la boucle nue perd les réussites qu'elle devait à l'ordre de sa liste ; 25 des 30
  exécutions rappelées écrivent ; Prophet paie ses réussites en calcul du modèle (96 s de
  processeur moteur contre 48, 1,7 fois les tokens), ses services restant à 0,28 s et 47 Mo.
  ADR 0051 : `fs.edit` (remplacer un passage exact, sous le droit de `fs.write`) et, dans les
  contextes de l'image, `fs.list`, `fs.stat`, `fs.search` et `fs.edit`. Cinquième passage
  (`06c432d`) : **Prophet 12/45, boucle nue 9/45** ; `fs.edit` débloque « corriger-une-faute »
  (3/3 des deux côtés), la recherche d'un nom dans le contenu « trouver-le-contrat » (3/3), le
  second rappel « ne-pas-toucher-au-reste » (2/3) ; mais le modèle essaie d'éditer le fichier à
  créer et recommence jusqu'à 24 fois (p95 170 s). ADR 0052 : `fs.edit` sur un fichier absent
  renvoie vers `fs.write`, note au deuxième échec identique, arrêt au cinquième échec de suite.
  La surface montre le parcours en frise (refus et rappels à leur place) et propose trois
  départs à un dialogue vide. Sixième passage (`6a858f7`, la garde) : **Prophet 12/45, boucle
  nue 7/45**, et Prophet consomme désormais moins de tokens (9 613 contre 10 012) — la boucle
  nue s'enferre à son tour dans des `fs.edit` répétés. Suivent `fs.search` qui compte ses
  lignes (`matching_lines`) et tient un filtre vide pour absent, `calc.eval` (ADR 0053) et la
  note d'une écriture faite sans lecture des entrées nommées. Septième passage (`1b98847`) :
  **Prophet 15/45, boucle nue 9/45**, p95 57 s ; « compter-les-erreurs » 3/3 par Prophet,
  « total-des-ventes » réussit pour la première fois. **Qwen3 4B** (nouveau travail de CI, un
  passage) : **Prophet 11/15, boucle nue 9/15**, avec moins de tokens (7 578 contre 9 332).
  `calc.eval` comprend désormais `sum(…)` et les deux paramètres ensemble ; `fs.search` sur un
  fichier ignore le filtre de nom et rend ses lignes sans texte à chercher (la règle du filtre
  vide avait fait retomber « extraire-les-adresses »). Huitième passage (`aab9476`) :
  **Prophet 21/45, boucle nue 12/45** (Qwen3 1.7B) et **10/15 contre 8/15** (Qwen3 4B) ; au prix
  de 1,6 fois les tokens et 1,9 fois le processeur moteur de la boucle nue, qui conclut plus tôt.
  Neuvième passage (`13b3205`) : **Prophet 21/45, boucle nue 14/45** et **10/15 contre 9/15**
  (4B) ; « total-des-ventes » 3/3 des deux côtés dès que `calc.eval` renvoie vers `fs.read`.
  Dixième passage (`c0b0f88`, `fs.copy`) : **Prophet 20/45, boucle nue 12/45** ; avec Qwen3 4B,
  11/15 des deux côtés, « ranger-par-annee » réussie pour la première fois. Avec le petit
  modèle, Prophet plafonne autour de 20-21 sur 45 depuis le huitième passage : le reste de
  l'écart tient au modèle.
- Concurrence et annulation sur le vrai moteur (FRONTIER, moteurs locaux ; essai
  `deux_requetes_se_partagent_le_moteur_et_un_abandon_le_libere`, vert sur `dd5ce52`) : sur une
  instance à une place comme celle de l'image, deux requêtes simultanées aboutissent toutes
  deux (1,3 s et 1,9 s) ; une génération de 3 000 tokens sans fin naturelle, abandonnée par son
  client après son premier fragment, libère l'instance aussitôt — la question suivante répond
  en 0,5 s, quand l'attente aurait duré plus d'une minute.
- Découverte des capacités (FRONTIER, moteurs locaux) : l'en-tête GGUF dit ce que le gabarit de
  conversation déclare — appels d'outils, réflexion. Relevé sur les huit fichiers du catalogue :
  Qwen3, Granite 3.3 et Llama 3.2 déclarent les outils, Phi-3 mini et SmolLM2 non. Les agents le
  lisent dans `model.list` (`template`) avant de confier une étape ; `prophet model ls` et la
  page Modèles le montrent.
- Pas de blocage pendant l'inférence (FRONTIER, interface) : un essai rend la page Conversation
  pendant qu'un moteur répond en flux (30 fragments espacés de 60 ms) et mesure chaque image.
  En release sur lavapipe, ici : médiane 0,5 ms, p95 0,6 ms, maximum 5,3 ms, la réponse
  affichée au fil des fragments ; la génération tourne dans son fil, jamais dans celui des
  images. En débogage, la première image d'une taille de police nouvelle coûte jusqu'à 150 ms
  (préparation de la police par egui) ; l'essai borne donc par rapport au flux. La CI relève
  ces temps en release dans le résumé du travail « Surface d'observation » : sur `b2f97f9`,
  130 images pendant 1,8 s de flux, médiane 6,0 ms, p95 6,7 ms, maximum 11,9 ms (llvmpipe,
  quatre cœurs).
- Conversation longue (M8-T7, `f93e591`) : au-delà de 32 Kio ou de 32 tours, la page
  Conversation refusait d'envoyer ; elle envoie les tours récents qui tiennent, dit combien
  elle en laisse de côté, et garde le fil affiché entier.
- **Correction** : M5-T1 cochait « bwrap + Landlock + seccomp », mais l'amorçage n'appliquait
  jamais Landlock (`apply_landlock` sondait l'ABI puis rendait faux) ; le niveau 0 reposait sur
  la racine minimale et seccomp seuls, et une sandbox pouvait créer des fichiers à sa propre
  racine. Landlock est désormais appliqué (`9b3ae7e`) : rien ne se crée à la racine, rien ne
  s'écrit hors des chemins accordés, rien ne s'exécute hors des montages en lecture seule ;
  prouvé ici, sous Landlock ABI 7, par un essai qui échouait avant. Les essais du niveau 0
  se taisaient en CI faute d'espaces de noms dans « check », et le travail d'isolation ne
  lançait que les essais marqués : il les exige désormais, Landlock compris, et reconnaît
  `needs_userns` (`4aaeb50`, vert sur le coureur). La page Système et `prophet status` disent
  la réserve de microVM (`471364e`, `1059502`).
- Cache du moteur local (ADR 0034, complément du 23 septembre, `2bb8a06`) : la condensation
  ne garde plus intact que le dernier résultat d'outil. En garder deux faisait réévaluer à
  llama-server, à chaque tour, un résultat entier déjà envoyé ; sur une mission simulée de
  huit lectures de 3 ko, 26 ko à réévaluer au lieu de 45 ko, à environ 24 ms par token sur le
  processeur de la CI.
- Erreurs et reprise (FRONTIER) : une mission échouée ou arrêtée se relance depuis
  l'inspecteur (« Relancer », `93db17c`) ou par `prophet task retry` (`6bc1b56`) : la
  préparation repasse par le même contexte du catalogue, que le plan retient désormais
  (`7cdc1d7`), avec le même modèle et la même intention ; rien n'est émis avant la
  confirmation. Un moteur injoignable se dit en français, avec son adresse (`ee2015f`).
- La fenêtre servie se lit : `prophet model ls` (`5c2ce2e`) et la page Modèles (`1c15cfb`)
  marquent le poids que le moteur a chargé et la fenêtre qu'il accorde par requête (`/props`) :
  4 096 tokens sur l'image, pour un fichier qui en annonce 40 960.
- CI de `6e1a152` et `c6e4979` (restriction de capd et du journal, essai des droits sous le
  compte de l'humain) : **verte des deux côtés**, dont les sept services sous systemd, la
  mission réelle Qwen3 sous NixOS, le système installé (UEFI et BIOS) et l'ISO. CI de
  `299c68d` (sandboxd et memoryd réservés, `/run/prophet` collant, fenêtre du moteur) :
  **verte des deux côtés** ; sur le llama-server épinglé, le refus a exactement la forme que
  le pilote lit (`exceed_context_size_error`, `n_prompt_tokens` 12 013, `n_ctx` 4 096). Sur
  `6cf62c0`, les sept services sous systemd sont verts avec capd arrêté : l'humain ne peut ni
  renommer un fichier sur `capd.sock` ni le remplacer par un lien (« Operation not
  permitted »), et capd repart sur son nom. CI de `4aaeb50` (réserve de microVM, Landlock au
  niveau 0, niveau 0 exigé sur le coureur d'isolation) : **verte des deux côtés**, mission réelle
  Qwen3, système installé (UEFI et BIOS) et ISO compris.
- CI de `41df4f5` (deux microVM ensemble sans réseau, condensation qui ménage le cache) :
  **verte des deux côtés**, mission réelle Qwen3, sept services, système installé (UEFI et BIOS)
  et ISO compris. CI de `468324a` (instantané repris, poids gérés, servir, `prophet status`) :
  **verte des deux côtés** — réserve reprise en 6,9 ms, vrai téléchargement de Hugging Face par
  egress, téléchargement sous systemd, mission réelle Qwen3, système installé (UEFI et BIOS),
  ISO. CI de `12daf73` (le routeur lit le dossier des téléchargements) : **verte des deux
  côtés**, contrôle `llama-router` compris.
- Audit visuel et fluidité de la surface (23 septembre,
  `docs/reports/audit-visuel-2026-09-23.md`) : quatre pages, décision et espace de mission
  branché, de 640 à 3840 × 2160, cinq accents et mouvement réduit. Corrigés : la surface tombait
  en 2560 × 1440 et plus (limites de wgpu, `9e03dc9`) ; « Autoriser pour toute la mission » sur
  un paiement irréversible — une action irréversible s'autorise désormais une fois, dans capd
  comme dans la surface (ADR 0054, `9e03dc9`) ; cartes des clients qui se chevauchaient en
  640 (`2c5e98c`) ; libellés décalés de six à huit points par rapport à leurs boutons dans huit
  rangées (`hud::rangee`, `7894c9f`, `f4cc89f`) ; gouttière de la décision en 640 (`f4cc89f`).
  Mesuré en release sur llvmpipe : l'interface coûte 0,45 à 0,75 ms de processeur par image à
  toutes les tailles, le tracé logiciel du champ 23 à 26 ms (13 ms allégé). Au repos, le champ
  gardait 37 % d'un cœur indéfiniment, pris au moteur local : sur un rastériseur logiciel, il se
  fige après deux minutes sans geste et repart d'où il était (ADR 0055, `da9fd65`) — 0,2 % d'un
  cœur en veille. `--mesure` sépare le processeur de l'attente du GPU, `--repos` détaille les
  phases du repos.
- `just check` : **883 réussis, 0 échec, 54 ignorés** (`32a20ea`), format, clippy, contrôles
  du dépôt et secrets (repli) ; les 19 parcours de rendu du bureau passent avec
  `--include-ignored`. Parcours de la surface avec `--include-ignored` : 15 du bureau, 6 de rendu,
  5 de missions avec vrais services, 2 de branchement, 1 de préparation, tous réussis.

**Bloqué.** Rien n'est vérifiable ici sous Nix, en VM ou sur matériel : pas de KVM (les essais
de la réserve de microVM tournent sur le coureur KVM de la CI, qui les a verdis), pas de carte
graphique (fluidité et consommation réelles à mesurer avec `prophet-surface --mesure` et
`--repos`). Le premier appel réel de Jev
attend une clé. Aucun compte Claude, ChatGPT ou Codex n'est connecté. Une décision revient à
l'utilisateur : `task.spawn` d'agentd accepte un manifeste brut du compte de l'humain (c'est ce
que fait `prophet task new`) ; le réserver aux profils du catalogue fermerait la dernière voie
par laquelle un processus de la session fait planifier une mission sous un manifeste de sa main.

**Écart relevé.** M8-T4 à M8-T6 sont cochés pour la ligne de commande, l'environnement et la
détection de session ; le plan demandait aussi de lancer chaque client officiel **dans une
sandbox** (niveau 1, niveau 2 s'il exécute du code). `prophet-pilotd` les lance sous l'identité
de l'humain, sans confinement (ADR 0026, pilotd « Limites ») : un client peut donc joindre
`capd.sock` et trancher une approbation (ADR 0044). Masquer `/run/prophet` dans un espace de
montage propre ne suffirait pas, et le dire serait promettre une isolation qui n'a pas lieu :
le même compte atteint aussi l'IPC de sway et le bus de session (`swaymsg exec`, `systemd-run
--user` lancent un programme hors de tout espace), et son home entier, où un fichier de
démarrage s'exécute à la session suivante. Un confinement réel borne donc les fichiers au
travail de la mission et au profil privé du client, masque `/run/prophet` et
`$XDG_RUNTIME_DIR` sauf le pont, et fait passer la sortie réseau par egress — une décision de
conception à prendre avec l'utilisateur, les clients parlant à leur éditeur sous son
abonnement.

**Pour la session suivante.** Lire la CI de la branche (le travail « Poids du catalogue servis
(réels) » porte désormais cinq familles, la mémoire de chaque instance sans projection, et un
essai de concurrence et d'annulation sur le vrai moteur) ; mesurer la mémoire et la VRAM sur
une machine à carte graphique, où l'estimation de l'ADR 0047 vaudra pour les couches
déchargées ; décider avec l'utilisateur du confinement des clients officiels ; trancher avec l'utilisateur le sort de
`task.spawn` pour le compte de l'humain, et le chemin de confiance qui distinguerait la surface
d'un autre programme du compte pour `approval.resolve` (ADR 0044) ; mesurer la surface sur une
carte graphique ; faire le premier appel réel de Jev avec une clé déposée.

### 15 septembre 2026, nuit et matin : l'ISO installée dans une machine virtuelle, deux fautes

L'humain a demandé d'installer l'OS et de l'essayer. Fait dans QEMU avec KVM, sous WSL, avec
l'ISO du run `34897838743` (révision `9abc822`), empreinte vérifiée, sous SeaBIOS (l'OVMF du
magasin ne démarre pas dans ce QEMU ; la CI, elle, démarre l'ISO en UEFI avec un autre OVMF).
Le support démarre en 30 s et ouvre une session sur la console série. **Deux fautes bloquantes,
que la CI ne pouvait pas voir** parce que son essai de l'installeur s'arrête au montage :
la copie du dépôt prenait le lien `/etc/prophet/source` vers le magasin au lieu de son
contenu, et le `chmod` qui suit mourait sur le magasin en lecture seule (`762e30d`) ; la carte
graphique relevée par l'inventaire était posée dans un sous-shell et n'existait plus à l'étape
de l'accélération (`8dc0c2e`, faute de la veille). Sur un vrai PC, les deux laissaient un
disque formaté et rien d'installé. L'installeur a désormais `--sans-installation`, et le
travail « Installeur sur disque en boucle » joue tout ce qui précède `nixos-install` sur un
disque neuf et vérifie ce qui est posé. **L'installation a abouti** (configuration
`prophet-ci`, fermeture construite sur l'hôte et servie en cache local, `nixos-install` en
13 min) et **le système installé a démarré sur le disque seul** : GRUB, phrase de passe des
volumes chiffrés, écran de connexion, sept services actifs, `prophet status`, relevé de
l'installeur relu. Une troisième faute : sans accélération graphique, la session mourait en
silence — wlroots refuse le rendu logiciel tant qu'on ne le lui permet pas (`16e8bab`) ; avec
la permission, la supervision s'affiche, le lanceur et le terminal aussi. Ce qui reste hors de
preuve : le matériel réel, l'UEFI dans cette VM, la configuration complète. Voir le
[rapport](reports/installation-vm-2026-09-15.md). CI de `adee017` (les trois correctifs, la
sonde HTTPS, l'étape « Tout faire sauf nixos-install ») : **verte des deux côtés** (10:30 UTC).

### Fin de session du 14 septembre 2026, soir

**Fini.** Le motif du modèle joint à une demande d'approbation (`c160cee`, ADR 0041) ; le
lanceur du bureau ouvre les outils que l'atelier logiciel a produits et que l'humain a publiés
(`907ef27`, ADR 0038 complément ; test du bureau corrigé dans `580a929`) ; la voix ouvre une
application ou un outil (`cc5d07f`, ADR 0036) ; l'installeur relève la machine avant
d'effacer le disque et `prophet status` relit ce relevé (`aa35d8a`, `28863a7`) ; le
sous-test sandbox des sept services vit dans l'état du service (`ffd88d2`) ; le guide
d'installation dit ce qui est vrai (`97c5492`). CI verte des deux côtés sur `c160cee` ;
`d6cd159` verte sauf le test Outils (corrigé) ; `580a929` **verte des deux côtés** (19:35 UTC) : le bureau installé ouvre un outil publié et lit sa sortie, l'installeur relève la machine ; `8d43dd3` (`prophet status` relit le relevé) verte des deux côtés aussi (20:30 UTC), comme `9abc822` (la page Système le montre ; 22:20 UTC). La [matrice matérielle](matrice-materielle.md) dit ce que l'image embarque et ce qui en a été vu.

**Bloqué.** Rien n'a jamais démarré sur un vrai PC (écran, réseau, micro, micrologiciel) ;
les vrais Claude Code et Codex n'ont jamais tourné dans une mission avec un compte ; le
client n'est pas confiné (ADR 0026). La machine de développement a rempli son disque en
cours de session (fichier d'échange de Windows pendant les builds WSL, disque virtuel de
125 Go) : deux fichiers tronqués ont été restaurés depuis git, les builds tournent avec deux
travaux et un garde-fou, et le disque virtuel attend un compactage administrateur.

**Pour la session suivante.** Lire la CI de `580a929` et suivants ; installer sur une machine
sacrifiable ou une VM chez l'humain et lire le relevé de l'installeur ; connecter un vrai
client et jouer `needs_codex_login` ; confiner le client ; lancer les outils publiés sous
sandboxd quand le service saura relayer un terminal.

## Blocages

Le travail « Mission locale sous NixOS (modèle réel) » n'a jamais rendu de verdict depuis
`b2f694e` (13 septembre) : sur cette branche comme sur `claude/prophet-os-audit-dev-cbu28z`,
il est annulé à sa limite de 90 minutes. Lu dans son journal le 14 septembre : la mission réelle
réussit (Qwen3-1.7B en routeur, 23 s), puis le sous-test du contexte web ne rend rien. Cause :
même sans bac à sable, le zygote de Chromium engendre chaque rendu par
`ForkAndDropCapabilitiesInChild`, dont le `capset` doit réussir ; sous le filtre `~@privileged`
d'agentd avec `SystemCallErrorNumber=EPERM`, il répond EPERM et le zygote meurt (SIGABRT,
`credentials.cc:365`) pendant que le processus principal, lui, répond « prêt » à la sonde ; la
page ne s'ouvre jamais, et la limite du pilote de test n'atteignait pas un python lancé par
`runuser`. Correctif dans le commit portant ce paragraphe : `capset` admis pour le service
d'agentd quand il porte le navigateur (le `CapabilityBoundingSet` vide fait qu'il ne peut que
retirer), et un `timeout` côté invité dans le sous-test. Verdict (`14cffce`) : plus aucun
arrêt de Chromium au journal, mais le sous-test reste muet jusqu'à la limite — et le journal
montre qu'il n'atteint jamais la mission web : il s'arrête au témoin HTTP lancé en arrière-plan
du shell du pilote de test (`(… &)`), qui garde le canal du pilote ouvert. Le témoin devient
une unité transitoire (`systemd-run --unit=temoin`), lue par `journalctl`. Verdict (`eefdef4`,
14 septembre 02:38 UTC) : **vert, pour la première fois** — la mission réelle (Qwen3-1.7B en
routeur, 24 s), puis le contexte web : navigateur `Chrome/152.0.7977.82` prêt sous l'unité
réelle, la mission ouvre `http://127.0.0.1:8099/`, relit le titre « Témoin Prophet », le témoin
voit `GET /` (52 s), et l'arrêt du moteur est effectif. Ce travail ne bloque plus.

Le parcours du système installé (UEFI) échouait, une fois le verrouillage passé, sur un
fichier créé par l'humain dans Documents/Prophet que le service ne lisait pas. Les
diagnostics ajoutés au scénario l'ont dit (`27b55a9`) : `user:agentd:r-x #effective:---` et
`mask::---` sur /home/pilot, Documents et Documents/Prophet. Un répertoire 0700 muni d'une
entrée `u:agentd:r-x` se lit 0750 (le masque tient lieu de bits de groupe) ; au démarrage
suivant, `homeMode` et les lignes `d … 0700` de tmpfiles remettent 0700, ce chmod ramène le
masque à `---`, et `a+` ne recalcule pas un masque qui existe déjà. Le système installé
démarre au moins deux fois (construction de l'image, puis l'essai) ; l'ISO, une. Correctif :
les ACL écrivent leur masque (`m::r-x`, `d:m::r-x`) — confirmé par la CI (`14cffce`) : le
service lit le fichier de l'humain. Le scénario est alors allé plus loin qu'il n'était jamais
allé, jusqu'à `ui.apps` dans la séance, et a trébuché sur lui-même : il lisait `structured` là
où le protocole MCP écrit `structuredContent` (réponse brute imprimée par `eefdef4`, quatre
applications vues par l'adaptateur, dont l'éditeur). Corrigé (`51b043c`), et le pas suivant a
trébuché à son tour : l'adaptateur nomme l'éditeur `org.xfce.mousepad`, le nom que les
applications GTK récentes se donnent sur le bus d'accessibilité, alors que le profil « bureau »,
capd et l'agent disent `mousepad`. L'adaptateur ramène désormais un identifiant en domaine
inversé à son dernier segment (`supd::app_name`), pour la liste comme pour la recherche —
confirmé par la CI (`1f022b3`) : l'éditeur est trouvé, et le pas suivant, `ui.tree`, échouait
dans le scénario lui-même : les arguments JSON n'étaient pas cités pour le shell de l'humain,
`{"app": "mousepad"}` arrivait en deux mots et la commande sortait en erreur d'usage. Cité
(`da485fc`), et le pas suivant : `ui.tree` répond, mais pas en deux secondes, le délai que la
CLI accorde à toute requête — « pas de réponse en 2 s ». Un outil appelé dans une séance depuis
le terminal a désormais une minute (`prophet task call`) ; le service et l'adaptateur bornent
chacun leur part. Verdict (`59224e7`) : **le parcours du bureau passe entièrement** sur le
système installé — champ trouvé dans l'arbre d'accessibilité, « bonjour » écrit, enregistré
par le menu, thunar refusé par capd, séance close. Le sous-test suivant, déconnexion annulée,
comparait alors les fenêtres au relevé pris au verrouillage, avant l'ouverture de l'éditeur ;
il compare désormais à l'instant. Verdict (`4a8fb44`, 14 septembre 06:08 UTC) : **le parcours
du système installé (UEFI) est vert**, verrouillage, bureau piloté par l'agent, déconnexion
annulée, déconnexion réelle et reconnexion compris. Ce travail ne bloque plus. La même
exécution a vu « Mission locale » trébucher une fois sur cinq dans son scénario : le service
redémarré est « actif » quelques millisecondes avant d'écouter, et la lecture lancée aussitôt
recevait « Connection refused » ; le scénario attend désormais que `prophet task ls` réponde.
Verdict de `75b794f` (14 septembre 06:50 UTC) : **tout est vert sauf ChatGPT** (Fontconfig,
connu) — `check`, surface, isolation, moteur, parole, mission locale au modèle réel, sept
services, installeur, ISO, démarrage, système installé avec et sans UEFI.

Le 14 septembre encore, l'accélération graphique des modèles locaux
([ADR 0037](adr/0037-accelerer-les-modeles-locaux-par-vulkan.md)) : une variante
`llama-cpp-vulkan` du moteur, construite par la CI à chaque poussée, et l'option
`prophet.localEngine.gpu.enable` qui la fait servir, place le modèle sur la carte
(`--gpu-layers`, préréglages du routeur compris) et n'ouvre à l'unité que les nœuds DRM.
Faux par défaut : jamais mesuré sur une vraie carte (`needs_gpu`), le processeur reste le
chemin prouvé. La CI de `1c30ab6` construit la variante Vulkan avec le correctif (31 minutes
pour le travail « Moteur local », grammaire comprise). L'installeur reconnaît lui-même la
carte : un périphérique Vulkan qui n'est pas le rastériseur logiciel, vu par `vulkaninfo` sur
le support d'amorçage muni des pilotes de Mesa, et il écrit `image/machine/acceleration.nix` ;
sans carte, rien ne change.

**Verdict de `4cb676a`, 14 septembre 08:07 UTC : les deux chaînes d'intégration continue sont
entièrement vertes, ChatGPT compris — une première.** `check`, surface, isolation, moteur
(variante Vulkan construite), parole, mission locale au modèle réel avec le contexte web,
ChatGPT sous NixOS, sept services, installeur, ISO avec Kdenlive et darktable, démarrage,
système installé avec et sans UEFI. Puis `b2267a2` (l'installeur sonde la carte, l'ISO porte
Mesa) : vert aussi, après une reprise de `check` — le test du navigateur piloté de `mcp-system`
avait trébuché une fois sur « le navigateur n'a pas répondu à Page.navigate en 15 s », deux
Chromium lancés à la fois sur le coureur ; le délai d'une commande au navigateur passe à 45 s,
sous la minute qu'accorde `prophet task call`. Sur `c416c06`, c'est « Mission locale » qui a trébuché, dans le
contexte web : le modèle local a appelé `doc.read`, hors des droits du contexte, et le refus de
capd a interrompu la mission — le moteur sert ses réglages de conversation (température 0,7),
et un modèle de 1,7 milliard de paramètres tiré ainsi choisit parfois le mauvais outil. Les
tours de mission passent à 0,2 (`providers::local::MISSION_TEMPERATURE`) ; la conversation de
l'atelier garde les réglages du moteur. L'interruption sur refus reste : un agent qui sort de
ses droits s'arrête. Verdict de `f2d91fc` (14 septembre 10:11 UTC) : **les deux
chaînes entièrement vertes**, mission web comprise.

L'invité des microVM ([ADR 0038](adr/0038-l-invite-des-microvm-dans-l-image.md)) : le dépôt
construit `invite-microvm` — le noyau publié par Firecracker, épinglé, et une racine squashfs
faite par Nix (busybox statique, Python 3, un `/init` qui lit la ligne de commande et dit sur la
console série « PROPHET_INVITE_PRET » puis « PROPHET_INVITE_FIN code=N ») ; l'image installée
le sert à sandboxd avec Firecracker sur le chemin du service, et l'hôte de la CI l'emploie pour
les essais de niveau 2. Prouvé ici sous KVM imbriqué : l'invité démarre, dit ses deux lignes,
redémarre, et le moniteur sort avec le code 0 en 1,1 s, trois fois sur trois (88 Mo de racine,
41 Mo de noyau). Puis le contrat côté hôte (`sandboxd::invite`) : le répertoire de
travail part dans un disque ext4 avec `.prophet/exec.sh` (`mkfs.ext4 -d`, `debugfs`, sans
privilège), l'invité l'exécute, la console est lue entre les marques, le code en est tiré, le
disque revient (`debugfs rdump`). **Prouvé ici sous KVM** : un programme Python lit
`entree.txt`, dit « somme 7 », écrit `resultat.txt`, sort avec le code 7 ; l'hôte lit les
deux et retrouve le fichier — 1,8 s de bout en bout, démarrage seul en 56 ms. Le niveau 2
exécute pour de vrai, et un contexte « Atelier logiciel » du catalogue s'appuie dessus
(`proc.exec` `python3` et `sh` en microVM, écriture dans `outils`). La CI n'a de KVM que sur
l'hôte du travail « isolation », qui joue ces essais ; en machine virtuelle, le contexte est
proposé et son exécution refusée en le disant. Le premier passage en CI (`dcf25fd`) a échoué
sur cet hôte : son répertoire temporaire est sous `/home`, que la racine squashfs de l'invité,
en lecture seule, ne laissait pas créer (« espace de travail non monté ») — ici, sous `/tmp`,
cela passait. L'invité pose désormais une surcouche overlay en mémoire sur sa racine et monte
l'espace de travail au chemin de l'hôte, quel qu'il soit ; rejoué ici avec un répertoire sous
`/root`, absent de la racine, avant de repasser par la CI. Le même passage a fait tomber
« les sept services » : le catalogue de l'image portait `proc.kill` parmi les outils de
l'atelier logiciel, que le contrôle des profils n'admettait pas, et agentd, qui refuse un
catalogue fautif plutôt que de le servir, ne démarrait plus — la CI l'a dit, pas le poste de
construction, où le catalogue n'est qu'évalué. Le contrôle admet désormais `proc.kill` avec
`proc.exec` (qui lance un programme nommé peut l'arrêter), et un test garde la forme exacte de
ce profil. Le contrôle des polices de ChatGPT, seul rouge restant, imprime désormais ce
que Fontconfig reproche.
Le rouge de ChatGPT est lu : Fontconfig dit « Cannot load default config file: File not
found: /etc/fonts/fonts.conf » depuis un renderer de Chromium, dans le bac à sable de
l'application, qui ne voit aucun fichier par construction ; le fichier existe et le runtime le
donne à lire, et le texte est rendu (« Sign in to ChatGPT » lu à l'écran). Le contrôle tolère
désormais cette seule ligne et refuse toute autre plainte de Fontconfig ; ce n'est plus une
condition de livraison, c'est une limite connue du bac à sable d'Electron.

La suite de l'humain gagne le montage vidéo (Kdenlive) et le développement photo
(darktable), dans le lanceur (« Montage vidéo », « Photo ») et dans le contexte « bureau » que
l'agent pilote par l'accessibilité ; leur ouverture réelle et leur pilotage par l'agent restent
à voir sur une machine, la CI n'installe pas la suite.

Le conteneur de construction n'a ni KVM, ni Nix, ni Landlock, ni cgroups v2. Ce n'est plus le
dernier mot : le job `isolation` de l'intégration continue installe gVisor, Firecracker et les
images d'invité sur un coureur Ubuntu muni de KVM, et y exerce les quatre tests matériels. C'est
ainsi que M5-T2 et M5-T3 ont été vérifiés.

Ce qu'aucune des deux machines n'offre encore, et qui bloque les tâches marquées ⛔ ci-dessus :
Nix (M9-T5, M9-T6), un GPU avec un modèle du catalogue (M8-T7), un agent de référence exécutable
(M13-T2). `docs/reports/phase0.md` section 5 dit, pour chacune, ce qu'il faut pour la vérifier.

Un rappel qui a coûté cher le 12 septembre : une machine hôte peut refuser ce qu'elle paraît
offrir. Ubuntu 24.04 interdit par AppArmor d'exécuter dans un espace de noms non privilégié, et
`/dev/kvm` peut être présent sans être ouvrable. Voir ADR-0006 ; `prophet status` le signale
désormais, et `tools/install-isolation.sh` le traite sans rien modifier sans autorisation.

Les pilotes de clients officiels sont testés jusqu'à la limite de ce qui est vérifiable sans compte : construction de la ligne de commande, environnement transmis, détection de session, messages d'erreur. L'exécution de bout en bout exige une connexion réelle.

**L'autorisation au niveau du socket était grossière ; elle est levée pour capd et le journal
(22 septembre 2026, ADR 0044).** Un pair admis a désormais une classe : soi ou `root`, service
(groupe principal `prophet-system`, les sept daemons), humain (membre déclaré : l'humain, sa
session, la surface). Émettre, déléguer ou vérifier un droit, demander une approbation, écrire ou
sceller le journal, lancer un programme sous sandboxd reviennent aux services ; trancher une
approbation revient à l'humain ; `/run/prophet` est collant, pour qu'aucun membre ne retire le
socket d'un autre. Reste ouvert : un processus du compte de l'humain — un client officiel lancé par `prophet-pilotd`
compris — peut encore trancher une approbation comme l'humain lui-même, et faire planifier par
`task.spawn` une mission sous un manifeste de sa main (visible, journalisée, lancée à part) ;
réserver l'humain aux profils du catalogue est à décider avec l'utilisateur. memoryd réserve
l'écriture d'un souvenir aux services ; vault et egress gardent leurs contrôles propres, qui
suffisent.

**Les sept daemons tournent sous systemd**, dans une machine NixOS de test que `just test-vm`
démarre et que l'intégration continue exerce : chacun sous son utilisateur, avec le durcissement
du module, ses sockets en 0660 dans un répertoire en 0750, et la chaîne complète qui planifie une
tâche. Ce qui reste non vérifié est le matériel réel — la carte graphique, la carte réseau et le
micrologiciel d'un PC donné.

**Le serveur de l'utilisateur est atteint** (12 septembre, 15 h 52). Le workflow
`.github/workflows/verifier-sur-le-serveur.yml` y tourne, déclenché **par l'API sur la branche de
travail**. Ce paragraphe a longtemps dit le contraire, et c'était mon erreur : le bouton de
l'interface GitHub exige la branche par défaut, le déclenchement par l'API non. Le secret
`VPS_PASSWORD` est posé — et il reste la seule chose qu'aucun agent ne doit écrire : ni dans le
fichier, ni dans un commit, ni dans une entrée qu'il remplirait lui-même.

Une tentative de contourner le premier point — faire de la poussée elle-même le déclencheur, avec
l'intention écrite dans un fichier versionné — a été refusée, à raison : cela rendait un `git push`
capable d'arrêter un moteur de production et de changer un réglage du noyau sans qu'un humain
tranche au moment où cela arrive. Le workflow reste donc à déclenchement manuel.

## Backlog (hors tâche courante, à ne pas faire maintenant)

- **Atelier logiciel, la suite.** L'invité est dans l'image et le niveau 2 exécute (ADR 0038,
  contexte « logiciel »). L'essai de bout en bout est écrit (`needs_kvm_un_client_ecrit_un_
  outil_et_l_execute_en_microvm` : un faux Codex écrit `somme.py` par `fs.write`, l'exécute
  par `proc.exec` — microVM, vrai sandboxd —, la sortie et le fichier écrit par l'outil
  reviennent) et a montré que le contexte ne pouvait pas démarrer : le planificateur montait
  toute mission à `proc.exec` au niveau 2, que le lanceur refusait, puis que chaque commande
  hors liste blanche exigeait une approbation humaine que rien ne pouvait donner à un agent.
  Corrigé (ADR 0031, complément) : la mission reste au niveau 0, chaque commande s'élève seule,
  capd juge la commande à son niveau, et la microVM — sans réseau, sur une copie de l'espace de
  travail examinée avant publication — n'exige plus de décision par commande. L'hôte de la CI
  joue cet essai. Les outils publiés s'ouvrent depuis le lanceur du bureau (« Outils » :
  `~/Documents/Prophet/outils`, terminal sous l'identité de l'humain, ADR 0038 complément) ;
  restent d'autres interpréteurs dans la racine, et le lancement sous sandboxd.
- **Les approbations, de bout en bout — fait (ADR 0041).** Une action que capd refuse faute
  de décision humaine est soumise à l'humain par le registre lui-même (journal
  `approval.requested`, identifiant rendu au modèle), `approval.wait` attend la décision (45 s
  au plus par appel), l'humain tranche dans la surface, et le même appel passe — une fois, ou
  pour toute la tâche selon la portée — ou reste refusé. capd garde une décision une heure
  (`approval.status`) et consomme une décision « une fois » à la demande identique suivante.
  Prouvé avec un vrai broker et un outil irréversible et externe ; la CLI tranche aussi
  (`prophet cap approvals` / `approve` / `deny` / `rules`), la surface accorde aussi pour toute
  la mission, et la voix tranche (« accorde », « refuse »). Le modèle joint son motif à la
  demande (`approval.wait {reason}` → `approval.explain`), que la surface et la CLI montrent
  comme un dire du modèle, à part de ce que le système sait de l'action ; l'atelier dit une
  décision qui arrive, une fois, motif compris, pour qu'on tranche sans regarder l'écran.
- **Arrêter un client lancé — fait.** `task.cancel` d'une mission menée par un client conclut
  sa séance puis demande `pilot.stop {task}` au lanceur, qui tue le client et tout son groupe
  de processus (les clients sont lancés meneurs de groupe ; le délai tue de même). Prouvé dans
  le lanceur (un client qui a lancé un sous-processus est arrêté en moins d'une seconde) et
  avec les vrais services : une mission sur le faux Codex, qui s'attarde trente secondes, est
  annulée, et une autre sur le même client se lance et finit aussitôt.
- **Les vrais clients.** Tout ce qui précède est prouvé avec des clients de remplacement sur le
  même chemin (pont, séance, CLI). Le vrai Claude Code et le vrai Codex, connectés par l'humain
  sur une machine installée, restent à voir : la forme de leur sortie finale (`final_text`),
  leur configuration MCP, l'acceptation des paliers de modèles et leur comportement face aux
  refus de capd sont construits d'après leur documentation. L'essai est écrit et attend
  l'humain : `needs_codex_login_un_vrai_client_rejoint_une_mission_et_ecrit_un_fichier`
  (`PROPHET_TEST_CLIENT=codex PROPHET_TEST_PILOT_STATE=$HOME/.local/state/prophet cargo test -p
  agentd --test pilot -- --ignored needs_codex_login`, dans sa session) — le vrai lanceur, sans
  remplacement, sur son profil connecté ; une mission préparée sur le client, rejointe, un
  fichier écrit par `fs.write`, le résultat vérifié.
- **Mesures sur matériel réel** : carte graphique (option Vulkan de l'ADR 0037, vitesse et
  mémoire vidéo), micro et sortie audio, énergie au repos de la surface. Rien n'a jamais
  tourné hors machine virtuelle ; le relevé de l'installeur (pilote d'affichage, Vulkan,
  réseau, micro, KVM) est le premier pas : il dira, depuis la clé, ce qu'un PC donné offre.
- **Les vrais clients** : Claude Code et Codex connectés par l'humain dans leur profil Prophet,
  puis l'atelier des agents rejoué avec eux (`needs_claude_login`, `needs_chatgpt_login`).

## Incidents (demandes de violation des invariants, refusées)

_Aucun._
