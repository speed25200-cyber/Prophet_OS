# `prophet-egress` — proxy de sortie

- **Socket** : `/run/prophet/egress.sock` (protocole HTTP, pas JSON-RPC)
- **Utilisateur** : `egress`, groupe `prophet-system` — le seul service avec une vraie pile réseau
- **Crate** : `crates/egress`

C'est le **seul chemin** par lequel quoi que ce soit sort de cette machine. Les sandboxes n'ont pas
de pile réseau ; ce socket est la seule chose qu'on leur monte.

## Ce qui se passe pour chaque requête

Quatre étapes, dans cet ordre, dont aucune n'est facultative.

1. **Qui demande ?** Le jeton de capacité voyage dans un en-tête `Proxy-Authorization: Prophet
   <base64 du jeton>`, **retiré avant la sortie** — le laisser passer donnerait la capacité de la
   tâche au serveur distant. Sans jeton : `407`.
2. **A-t-il le droit ?** `capd` tranche, sur l'hôte réellement visé. Le proxy ne décide de rien
   lui-même. Refus : `403`, avec le motif rendu par `capd`.
3. **Est-ce une exfiltration ?** Volume, entropie, motifs de secrets dans le corps et l'URL.
   Blocage : `403 ExfiltrationSuspected`, avec l'explication.
4. **Alors seulement**, le relais.

L'hôte contrôlé et l'hôte joint sont **la même variable**. Contrôler `api.exemple.fr` puis se
connecter à ce que dit un autre en-tête serait une passoire avec l'apparence d'un contrôle.

## Lecture et effet

| Classe | Exemple | Défaut |
|---|---|---|
| `GET` sur un domaine autorisé | lire une page | automatique |
| `POST`, `PUT`, `PATCH`, `DELETE` | envoyer, payer, poster, supprimer | **approbation obligatoire** |

Marquer toute sortie comme ayant un effet externe ferait demander une décision humaine pour chaque
lecture ; une approbation qu'on donne cent fois par jour n'est plus une approbation.

## Les hôtes d'interrogation

Une API de décision comme Jev répond à un `POST` sans rien retenir ni rien faire. Demander une
décision humaine à chaque question rendrait une boucle de décision inutilisable. L'administrateur
nomme donc ces hôtes dans `PROPHET_EGRESS_QUERY_HOSTS` (module NixOS `prophet.jev.queryHosts`),
et pour eux, **pour `POST` seulement**, le proxy demande à capd une lecture (`external: false`)
plutôt qu'une action externe. Tout le reste tient : jeton exigé, grant `net.egress` sur l'hôte,
détection d'exfiltration sur le corps, journal. `PUT`, `PATCH` et `DELETE` restent des
modifications partout. Le motif `*` est refusé et arrête le service : « tous les `POST` sont des
lectures » ne doit pas pouvoir s'écrire par accident.

## Le relais TLS

Une requête en forme absolue `https://…` est relayée sous TLS **terminé par le proxy**, avec les
racines de la machine (`/etc/ssl/certs`, ou `SSL_CERT_FILE`). C'est ce qui permet de substituer un
secret dans une requête chiffrée : un tunnel `CONNECT` ne laisse rien voir ni rien remplacer
(ADR-0007). Un certificat que la machine ne reconnaît pas ferme la sortie (`502 TlsFailed`) ;
elle ne se dégrade jamais en clair. Un amont qui n'accepte pas la connexion en quinze secondes
ferme aussi la sortie (`502 Unreachable`), plutôt que de retenir l'appelant jusqu'au délai du
noyau. Le tunnel `CONNECT` reste disponible pour ce qui n'a pas
besoin de secret.

## Quand `capd` n'est pas là

**La sortie se ferme** : `503`, et rien ne part. Un broker injoignable est un « je ne sais pas », et
on ne sort pas sur un « je ne sais pas ». Le défaut inverse — laisser passer en cas de doute —
ouvrirait la machine entière au moment précis où elle ne doit pas l'être.

## Le journal de la mission

Chaque décision s'inscrit au journal (`ledger.append`, acteur `egress`), sous la mission sujet
du jeton — egress est un service, membre principal de `prophet-system`, et le journal ne prend
d'écriture que des services (ADR 0044) :

| Événement | Quand | Contenu |
|---|---|---|
| `net.request` | la sortie a été relayée | `host`, `port`, `method`, `bytes_out`, `bytes_in`, `status` |
| `net.deny` | capd a refusé | `host`, `reason` (le motif de capd) |
| `net.exfil_suspected` | le détecteur a bloqué | `host`, `reason` (l'explication) |

Ni chemin complet, ni en-tête, ni corps : le journal dit **où** la mission est sortie et ce qui
a transité, pas **quoi**. Dans un tunnel `CONNECT`, `status` est celui du tunnel et les octets
sont ceux du flux chiffré. Un jeton dont capd n'a pas pu vérifier la signature n'écrit rien : son
sujet n'est pas établi, et l'inscrire laisserait n'importe qui écrire au journal d'une autre
mission. L'écriture ne retient pas la requête ; une panne du journal se lit dans le journal du
service (`décision de sortie non journalisée`).

C'est ce qui rend visible, dans le parcours de la mission sur la surface, chaque hôte joint par
un client officiel en cage (ADR 0056) — et chaque hôte qui lui manque, refusé.

## La substitution de secrets

Un en-tête qui porte `prophet-secret:<nom>` est complété par le coffre **au tout dernier moment**,
juste avant la sortie — après le contrôle, après la journalisation, après la détection. Ce qui a
été inspecté et journalisé plus haut ne contenait que des références.

Le coffre ne rend une valeur qu'à ce processus, et il le vérifie lui-même par `SO_PEERCRED`. C'est
ce qui fait que « le Vault rend des poignées, jamais des valeurs » tient pour tout le reste du
système, et pas seulement par convention.

Une référence inconnue, interdite pour cet hôte, ou impossible à obtenir **arrête la requête**. La
laisser partir avec le handle littéral serait inoffensif — un handle ne vaut rien sans le coffre —
mais la tâche croirait son secret transmis et ne comprendrait pas l'échec d'authentification qui
suivrait.

Dans un tunnel `CONNECT`, une demande d'injection est **refusée**, pas ignorée (ADR-0007).

## Comment `prophet status` le sonde

Les six autres daemons répondent `pong` à un `ping` JSON-RPC. Celui-ci est un proxy : un `ping`
JSON-RPC est, pour lui, une requête HTTP tronquée, et il attend sagement la ligne vide qui termine
les en-têtes. Elle ne vient jamais. `prophet status` a bloqué ainsi quinze minutes dans le test en
machine virtuelle, sans rien afficher — le proxy n'était pas en faute, la sonde l'était.

La sonde lui parle donc sa langue : une requête **sans jeton**, refusée par `407` avant que rien ne
sorte de la machine. Elle prouve davantage qu'un `pong` — que la règle « rien ne sort d'ici sans
qu'on sache pour qui » est en place. Un `200` à cette requête serait signalé comme une panne, et
non comme un service en bonne santé.

Toutes les sondes de `prophet status` ont un délai de deux secondes. Une commande d'état qui ne
rend pas la main n'apprend rien et bloque le terminal.

## Limites

- Un corps plus grand que 8 Mio est refusé plutôt que relayé sans être regardé : un corps qu'on ne
  peut pas inspecter est exactement celui par lequel on exfiltrerait.
- Dans un tunnel `CONNECT`, le contenu est chiffré de bout en bout : l'hôte est contrôlé et la
  connexion journalisée, mais aucun secret ne peut y être substitué (ADR-0007). Une requête
  `https://` en forme absolue, elle, est relayée avec substitution.
