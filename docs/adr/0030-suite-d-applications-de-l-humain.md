# ADR 0030 — La suite d'applications de l'humain, et sa variante d'intégration continue

Date : 2026-09-13. Statut : accepté.

## Contexte

Le bureau livrait un navigateur, un terminal, un gestionnaire de fichiers, les clients officiels
et, depuis l'ADR 0027, un éditeur de texte. L'humain attend de son système ce qu'il a ailleurs :
traitement de texte et tableur, retouche d'image, dessin vectoriel, 3D, CAO, lecture de PDF et de
vidéo. Les logiciels propriétaires de ces métiers (AutoCAD, Photoshop) n'existent pas sous Linux ;
leurs équivalents libres, si. Et ce que le plan attend des agents, c'est de travailler dans ces
applications par leur arbre d'accessibilité, pas par des pixels.

## Décision

1. **Une suite installée par défaut** (`prophet.desktop.suite.enable`) : LibreOffice, GIMP,
   Inkscape, Blender, FreeCAD, Evince, mpv, avec leurs entrées dans le lanceur et un chemin de
   fichier en argument. Ce sont des paquets de nixpkgs, sans modification.
2. **Ce qui publie une accessibilité se pilote.** Le contexte « bureau » nomme les applications
   GTK et Qt de la suite (`soffice`, `gimp`, `inkscape`, `freecad`, `evince`) à côté de
   l'éditeur ; la session demande aux applications Qt de publier la leur
   (`QT_LINUX_ACCESSIBILITY_ALWAYS_ON`). Blender n'expose rien : l'agent n'y voit rien, et le dit.
3. **L'intégration continue construit la même configuration sans la suite**
   (`nixosConfigurations.prophet-ci`, tests de machine virtuelle avec `suite.enable = false`) :
   elle paie chaque gigaoctet, n'ouvre aucune de ces applications, et ce que l'installeur pose
   reste `prophet`, suite comprise.

## Conséquences

- Le système installé pèse plusieurs gigaoctets de plus, téléchargés depuis le cache binaire à
  l'installation ; une machine à 80 Go les accueille.
- Aucun test n'exerce encore l'agent dans LibreOffice ou FreeCAD ; l'éditeur GTK est prouvé, le
  reste est la même voie (AT-SPI) avec des arbres plus grands et des dialogues modaux (GTK 3)
  dont l'ADR 0027 dit la limite.
- AutoCAD, Photoshop et les logiciels Windows restent hors de portée par cette voie ; le plan
  prévoit une machine virtuelle avec passage de carte graphique, qui n'est pas ici.
