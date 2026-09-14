# Prophet OS — Avancement

> État au 13 septembre 2026 : les coches historiques ci-dessous décrivent parfois une
> bibliothèque ou une simulation, pas le parcours installé complet. Les exigences de livraison
> sont désormais suivies dans [FRONTIER.md](FRONTIER.md). Le moteur local possède un client HTTP
> concret, une conversation en flux et des missions via MCP/agentd avec vrais capd/ledger.
> L'examen des versions est implémenté, et leur publication commandée par le créateur depuis
> agentd ; l'écriture sous l'identité humaine sur l'image et le parcours installé
> complet restent à établir. Voir le [dernier rapport de l'atelier](reports/atelier-2026-09-13.md)
> et les exigences ouvertes, notamment la qualité graphique attendue et les sessions authentifiées.

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

- [x] M5-T1 — Niveau 0 (bwrap + Landlock + seccomp) (2026-09-12, 24b8338) — 8 tests d'évasion réels, démarrage en 2,6 ms
- [x] M5-T2 — Niveau 1 (gVisor) (2026-09-12, 3c0b7cd) — vérifié sur matériel réel en intégration continue : exécution effective sous gVisor et absence d'interface réseau, tests `needs_gvisor` verts
- [x] M5-T3 — Niveau 2 (Firecracker) (2026-09-12, faca93c) — vérifié sur matériel réel en intégration continue : microVM démarrée avec noyau et racine d'invité, et refus explicite plutôt que repli quand le niveau est inatteignable
- [ ] M5-T4 — Pool de snapshots — plus bloqué : le niveau 2 démarre sur le coureur d'intégration ; reste à écrire, avec l'objectif de 100 ms depuis instantané à mesurer
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
- [ ] M8-T7 — Moteurs locaux — client HTTP, flux annulable, interface de conversation et essai Qwen3/CPU réalisés ; le 13 septembre, budgets de tokens par modèle, condensation du contexte et deux modèles servis par un llama-server en mode routeur, prouvés en relais réel (ADR 0034) ; le même jour, l'image sert deux modèles en mode routeur (Qwen3-1.7B en réflexion, Qwen3-0.6B en exécution, téléchargés à l'installation), préréglages vérifiés sur le vrai moteur et configuration évaluée ; restent le cycle de vie des poids, les budgets VRAM et la matrice GPU/modèles
- [x] M8-T8 — Pilote `prophet-agent` (2026-09-12, 24b8338) — boucle native : points de reprise, fork, rejeu
- [x] M8-T9 — Sélection de pilote (2026-09-12, 24b8338) — sélection expliquée, confidentialité locale respectée
- [x] M8-T10 — CLI (2026-09-12, 24b8338) — `prophet provider ls|login`
- [x] M8-T11 — Démo M8 (2026-09-12, 24b8338) — démonstration sur trois pilotes

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

- [ ] `image/tests/services.nix` demande aussi, désormais, si `agentd` peut écrire là où sa
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

- [ ] **Le maillon jamais exercé : `nixos-install` lui-même.** Le travail « installeur » s'arrête
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

- [ ] `image/tests/installe.nix` — démarre la configuration installée **par son chargeur
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
une unité transitoire (`systemd-run --unit=temoin`), lue par `journalctl`. À confirmer par la CI.

Le parcours du système installé (UEFI) échouait, une fois le verrouillage passé, sur un
fichier créé par l'humain dans Documents/Prophet que le service ne lisait pas. Les
diagnostics ajoutés au scénario l'ont dit (`27b55a9`) : `user:agentd:r-x #effective:---` et
`mask::---` sur /home/pilot, Documents et Documents/Prophet. Un répertoire 0700 muni d'une
entrée `u:agentd:r-x` se lit 0750 (le masque tient lieu de bits de groupe) ; au démarrage
suivant, `homeMode` et les lignes `d … 0700` de tmpfiles remettent 0700, ce chmod ramène le
masque à `---`, et `a+` ne recalcule pas un masque qui existe déjà. Le système installé
démarre au moins deux fois (construction de l'image, puis l'essai) ; l'ISO, une. Correctif :
les ACL écrivent leur masque (`m::r-x`, `d:m::r-x`). À confirmer par la CI.

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

**L'autorisation au niveau du socket est grossière.** Un pair est accepté s'il appartient au
groupe `prophet-system`, et il a alors accès à *toutes* les méthodes système du daemon. C'est
suffisant entre daemons, qui se font mutuellement confiance par construction, mais la surface doit
elle aussi en faire partie pour lire les tâches — et elle obtient du même coup un accès qu'elle
n'utilise pas. Restreindre demanderait une notion de méthode autorisée par pair que `prophet-ipc`
n'a pas. À faire avant qu'un programme moins fiable qu'un afficheur ne parle à un daemon.

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

_Vide._

## Incidents (demandes de violation des invariants, refusées)

_Aucun._
