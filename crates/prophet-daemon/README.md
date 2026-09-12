# `prophet-daemon`

Ce que les sept daemons de Prophet OS font de la même façon : trouver leur socket et leur état,
charger une clé, installer leur journalisation — et surtout **décider à qui ils acceptent de
parler**.

Cette dernière règle est la raison d'être du crate. Recopiée sept fois, elle aurait fini fausse
quelque part, et une règle d'autorisation fausse ne se voit pas : elle laisse simplement passer.
Elle est donc écrite une fois, avec ses tests, dont celui qui dit que `root` ne passe pas par
faveur.

Rien ici ne décide d'un droit — `capd` reste le seul à le faire. Ce qui est décidé ici est plus
modeste et plus ancien : *à qui le noyau dit que je parle*, et *où sont mes fichiers*.

## Ce qu'il porte

- `Pairs` — la règle d'acceptation, par groupe ou par identité propre.
- `clef` — charge ou crée une clé ed25519, et **vérifie le mode 0600 après écriture** plutôt que de
  le supposer : un `umask` hostile ferait mentir la création seule.
- `socket`, `etat` — les chemins conventionnels, surchargeables pour les tests.
- `texte`, `repondre`, `methode_inconnue` — de quoi répondre sans recopier la même erreur.

## `essai` (derrière un drapeau)

Le harnais de test des daemons, qui ne part pas dans l'image : un programme installé n'a aucune
raison de savoir lancer des daemons.

Il porte la seule partie délicate — l'attente. **Attendre que le fichier de socket apparaisse ne
prouve rien** : il apparaît au `bind`, avant que la boucle d'acceptation ne tourne, et un test qui
se contente de le voir passe alors que le daemon ne répond pas encore. On attend donc qu'une
connexion aboutisse *et* qu'un appel revienne.
