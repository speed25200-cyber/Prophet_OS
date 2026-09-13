# 0025 — Des contextes de mission qui consultent le web, et un navigateur sondé avant la première mission

- **Statut** : accepté
- **Date** : 2026-09-13
- **Tâches liées** : M8 (agentd, préparation), M7-T3 (`http.fetch`), M10-T3 (outils `web.*`), M9 (image)
- **Complète** : ADR 0015 (profils de mission), ADR 0024 (navigateur intégré)

## Contexte

Depuis l'ADR 0024, une mission sait lire une page par egress et naviguer par l'arbre. Mais le
seul chemin qui l'y autorisait était `task.spawn` avec un manifeste écrit à la main : le
catalogue de profils de l'ADR 0015 n'admettait que les outils fichiers natifs. Depuis
« Nouvel objectif », l'humain ne pouvait donc pas confier au système une recherche sur le web.
L'OS des agents promettait un navigateur à l'agent sans qu'aucun contexte ne le lui donne.

Second manque : l'image livrait Chromium au bureau, mais `agentd` n'en nommait aucun. Et
quand bien même : le service tourne sous un durcissement systemd qui tue un navigateur de deux
façons silencieuses (`MemoryDenyWriteExecute` contre le code généré par V8, `SIGSYS` sur
`setrlimit` sous le filtre d'appels commun). Rien ne l'aurait dit avant le premier outil d'une
mission déjà lancée, jeton émis et budget entamé.

## Décision

**Le catalogue admet le web relayé par egress.** Un profil peut porter des hôtes de sortie
(`net.egress`, y compris `*`), l'interface du navigateur piloté (`ui.read` et `ui.act` sur
`browser`, rien d'autre : ni écran, ni fenêtre d'une autre application) et les outils réseau
nommés (`http.fetch`, `web.open`, `web.tree`, `web.act`). Un outil réseau sans aucun hôte est
refusé au chargement : il ne produirait que des refus. Tout le reste du cadre de l'ADR 0015
tient : niveau 0, modèles locaux, périmètres relatifs au home couverts par une lecture.

Un hôte `*` dans un profil n'est pas un droit sans contrôle : chaque requête est tranchée par
capd sur l'hôte réellement joint, inscrite au journal avec sa cible, et toute méthode qui modifie
ou tout envoi de formulaire attend la décision humaine. L'administrateur qui veut une liste
d'hôtes la met dans le profil ; le catalogue de l'image livre un contexte « Recherche sur le
web » sur `*`, parce que l'objectif du système est qu'un agent puisse consulter Internet.

**Le profil dit s'il a besoin du navigateur.** `task.options` rend pour chaque profil `web`,
vrai s'il demande un outil `web.*` (`http.fetch` seul n'a besoin que d'egress). La surface
l'affiche dans le cadre de la mission et ne propose pas de lancer un contexte web sans
navigateur qui répond.

**Le service sonde son navigateur au démarrage, sous ses propres contraintes.** Si
`PROPHET_BROWSER` est nommé, `agentd` le lance une fois sur une page vierge, dans un profil
jetable, sans sortie réseau, et lui demande sa version. `task.options` rend `browser`
(`program`, `ready`, `detail`) : « sonde en cours » jusqu'au verdict, puis la version ou la
raison de l'échec. `task.prepare` refuse un contexte web tant que le navigateur ne répond pas,
avant d'émettre un jeton, avec la raison. Sans `PROPHET_BROWSER`, `browser` est absent et le
refus dit que le service n'en configure aucun.

**L'image nomme le navigateur et relâche exactement deux entraves.** `prophet.navigateur`
(Chromium par défaut, `null` pour retirer les outils web) devient `PROPHET_BROWSER` du service
`prophet-agentd`. Pour lui seul, `MemoryDenyWriteExecute` est levé et `SystemCallErrorNumber`
vaut `EPERM`, de sorte qu'un `setrlimit` refusé est noté par le navigateur au lieu de le tuer.
Le reste du durcissement tient : pas de nouveaux privilèges, pas d'espaces de noms, racine en
lecture seule, capacités vides. Le test des services vérifie que la sonde répond « prêt » sous
l'unité réelle et que le contexte « web » est bien annoncé avec ses droits.

## Conséquences et limites

Le navigateur piloté tourne dans le service `agentd`, pas au niveau 2 (ADR 0024, toujours
ouvert) ; les deux entraves levées le sont pour tout le service, et c'est le prix de ce choix
tant que le navigateur n'a pas son propre confinement. La sonde prouve que le programme
démarre et répond au protocole ; elle ne prouve pas qu'une page réelle se rend, ce que seule
une mission montre. Le catalogue reste une configuration de confiance, sans signature vérifiée.
