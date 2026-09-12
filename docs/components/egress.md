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

## Quand `capd` n'est pas là

**La sortie se ferme** : `503`, et rien ne part. Un broker injoignable est un « je ne sais pas », et
on ne sort pas sur un « je ne sais pas ». Le défaut inverse — laisser passer en cas de doute —
ouvrirait la machine entière au moment précis où elle ne doit pas l'être.

## Limites

- Un corps plus grand que 8 Mio est refusé plutôt que relayé sans être regardé : un corps qu'on ne
  peut pas inspecter est exactement celui par lequel on exfiltrerait.
- Dans un tunnel `CONNECT`, le contenu est chiffré de bout en bout : l'hôte est contrôlé et la
  connexion journalisée, mais aucun secret ne peut y être substitué (ADR-0007).
