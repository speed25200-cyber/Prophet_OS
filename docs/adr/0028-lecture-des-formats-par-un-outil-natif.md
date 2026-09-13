# ADR 0028 — Lecture des formats par un outil natif (`doc.read`)

Date : 2026-09-13. Statut : accepté.

## Contexte

Un agent doit lire ce que l'humain lui confie : PDF, documents bureautiques, images, vidéos,
sons, pages, archives. `fs.read` rend des octets en UTF-8 approximatif, ce qui ne dit rien d'un
PDF ni d'une image. Laisser l'agent lancer lui-même des programmes de conversion (`proc.exec`)
serait ouvrir l'exécution arbitraire pour un besoin de lecture ; laisser un modèle « regarder »
une image par capture serait contraire au plan.

## Décision

Un outil unique, `doc.read`, sous le droit `fs.read` et les règles de `fs.read` (périmètre,
pas de lien, borne dite). Il reconnaît le format aux octets, jamais au nom, et rend texte et
métadonnées bornés. Ce qui se lit en Rust pur l'est (bureautique par archive et XML, en-têtes
d'images, archives) ; ce qui exige un programme l'emploie s'il est sur le chemin du service
(`pdftotext`, `pdfinfo`, `ffprobe`, `tesseract`), avec un délai, un fichier temporaire privé,
et un mot dans `notes` quand il manque. L'agent ne choisit ni programme ni argument. L'image
met ces programmes sur le chemin d'`agentd`.

## Conséquences

- « Lire vidéo, photo, PDF et tous formats » devient une capacité de l'OS, pas de chaque
  agent, et passe par capd et le journal comme toute lecture.
- Une vidéo se lit par ses mesures et ses étiquettes, pas image par image ; le texte d'une
  image dépend de tesseract et de la qualité de l'image ; un tableur se lit par ses textes et
  ses nombres, pas par ses formules évaluées. Les formats propriétaires sans bibliothèque
  libre (fichiers CAO, projets Photoshop) sont rendus « binaire », avec leurs premiers octets.
- Les programmes tournent sous l'unité durcie d'`agentd`, avec ses interdits ; un programme
  qui en aurait besoin de plus (rendu graphique) n'y a pas sa place, et passera par sandboxd.
