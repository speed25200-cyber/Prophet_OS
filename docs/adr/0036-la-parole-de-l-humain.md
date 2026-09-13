# ADR 0036 — La parole de l'humain : dicter une intention, en local

Date : 2026-09-13. Statut : accepté, première livraison.

## Contexte

Le superviseur humain écrit ses intentions ; il doit aussi pouvoir les dire. Rien n'existait :
ni capture du micro, ni transcription, seulement PipeWire dans le matériel de l'image. Les
invariants s'appliquent : aucun son ne doit quitter la machine, aucune clé d'API, aucun modèle
distant ; et la parole ne donne aucun droit, elle produit un texte que l'humain examine.

## Décision

1. **Un crate `voice`, deux gestes.** `Tools::record(seconds, wav)` enregistre le micro de la
   session en WAV 16 kHz mono par `pw-record` de PipeWire (arrêté par `timeout -s INT`), ou
   `arecord` d'ALSA à défaut ; `Tools::transcribe(audio, langue)` lance `whisper-cli` de
   whisper.cpp sur un modèle ggml local (`-oj`), et rend le texte, la langue et la durée. La
   configuration vient de la session : `PROPHET_WHISPER`, `PROPHET_WHISPER_MODEL`,
   `PROPHET_RECORDER`. Sans modèle, la commande le dit.
2. **`prophet voice`.** `--file` transcrit un fichier ; sans lui, `--seconds` (6 par défaut)
   enregistre le micro ; `--language` impose la langue ; `--prepare <contexte>` prépare une
   mission avec le texte transcrit pour objectif, par le même chemin que `prophet task
   prepare` : le plan est rendu, l'humain le lance séparément. Le son enregistré est effacé
   après la transcription ; `--json` rend `{transcript, plan}`.
3. **L'image.** Le module `voice.nix` installe whisper.cpp et PipeWire dans la session et pose
   les variables ; la configuration de référence télécharge à l'installation le modèle
   `ggml-base` (multilingue, 148 Mo, empreinte publiée par Hugging Face), comme les modèles de
   langue ; la variante d'intégration continue reste sans modèle.

## Conséquences

- Une phrase dite devient un plan de mission à examiner, sans réseau. Preuve : une phrase
  française synthétisée par espeak-ng est transcrite par le vrai whisper.cpp et le vrai modèle
  avec ses mots-clés, langue détectée `fr` ; les tests unitaires couvrent la lecture de la
  sortie de whisper et les refus (durée, fichier, enregistreur absent).
- Ce qui n'est pas livré : la voix humaine (le test parle avec une voix de synthèse ; la
  qualité sur un vrai micro reste à mesurer), le bouton de dictée dans l'atelier (la surface
  n'appelle pas encore `voice`), la réponse parlée de l'OS (aucune synthèse), et le mot
  d'activation. Le modèle `base` est un compromis vitesse/qualité sur processeur ; un modèle
  plus grand s'installe par `prophet.voice.model`.
