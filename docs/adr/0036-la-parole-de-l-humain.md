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
   après la transcription ; `--json` rend `{transcript, plan, reply}`. Avec `--reply`, l'OS
   répond à voix haute : ce qu'il a compris, la mission préparée et où l'examiner, ou pourquoi
   rien n'est préparé ; c'est la boucle « je parle, l'OS prépare, l'OS répond ».
3. **L'image.** Le module `voice.nix` installe whisper.cpp et PipeWire dans la session et pose
   les variables ; la configuration de référence télécharge à l'installation le modèle
   `ggml-base` (multilingue, 148 Mo, empreinte publiée par Hugging Face), comme les modèles de
   langue ; la variante d'intégration continue reste sans modèle.

4. **L'OS parle, en local aussi.** `Tools::speak(texte, wav)` synthétise par Piper avec une
   voix `.onnx` locale (`PROPHET_PIPER`, `PROPHET_PIPER_VOICE`), `Tools::play(wav)` joue le son
   par `pw-play` (sinon `aplay`) ; `prophet voice --say "…"` fait les deux, `--out` garde le WAV.
   La configuration de référence installe la voix française « siwis » de Piper (medium), comme
   les autres modèles ; sans voix, l'OS écoute mais ne parle pas, et le dit. La preuve est une
   boucle fermée : l'OS dit une phrase, Whisper la réécoute et retrouve ses mots.

5. **Un mot d'activation, sans bouton.** `prophet voice --listen` écoute par tranches (six
   secondes par défaut) et n'agit que sur une phrase qui commence par le mot d'activation
   (`--wake`, « prophète » par défaut ; casse, accents et ponctuation ignorés) : le reste de la
   phrase est l'intention, traitée comme une dictée, avec `--prepare` et `--reply` s'ils sont
   demandés. Chaque tranche est effacée après transcription, rien n'est conservé ni envoyé ;
   `--rounds` borne l'écoute (essais), sinon Ctrl+C. La reconnaissance du mot est une
   comparaison de mots normalisés en tête de phrase, tolérante à ce que Whisper entend
   réellement (« Profète », « Profette », « prophet » valent « Prophète » ; « prophétie » non) :
   pas de détection acoustique séparée, donc un coût de transcription par tranche, et aucun
   déclenchement au milieu d'une phrase. La voix d'espeak est trop mécanique pour que Whisper y
   attrape le mot ; les essais parlent avec la voix de Piper, et le vrai micro reste à mesurer.

## Conséquences

- Une phrase dite devient un plan de mission à examiner, sans réseau. Preuve : une phrase
  française synthétisée par espeak-ng est transcrite par le vrai whisper.cpp et le vrai modèle
  avec ses mots-clés, langue détectée `fr` ; les tests unitaires couvrent la lecture de la
  sortie de whisper et les refus (durée, fichier, enregistreur absent).
- L'atelier a un bouton « Dicter (6 s) » sous le champ d'objectif, présent seulement si la
  machine a un modèle de parole : le micro est écouté puis transcrit hors du fil graphique, et
  le texte rejoint l'objectif que l'humain relit avant tout envoi ; un brouillon déjà envoyé
  ne reçoit pas de dictée. Le contrôleur est testé avec des réponses de dictée simulées
  (ajout, texte vide, échec nommé).
- Ce qui n'est pas livré : la voix humaine (le test parle avec une voix de synthèse ; la
  qualité sur un vrai micro reste à mesurer), la lecture réelle sur une sortie audio (vérifiée
  par ses refus seulement), le mot d'activation, et la lecture automatique des résultats dans
  l'atelier. Le modèle `base` est un compromis vitesse/qualité sur processeur ; un modèle plus
  grand s'installe par `prophet.voice.model`, une autre voix par `prophet.voice.speaker`.
