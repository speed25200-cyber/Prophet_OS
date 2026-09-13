# La parole

`voice` enregistre le micro de la session (PipeWire, sinon ALSA) et transcrit un fichier audio
en texte par whisper.cpp, en local ; `prophet voice` en fait une commande, et `--prepare
<contexte>` transforme la phrase dite en mission à examiner (ADR 0036). Aucun son ne quitte la
machine.

## Configuration

| Variable | Rôle |
|---|---|
| `PROPHET_WHISPER_MODEL` | Modèle ggml de whisper.cpp, requis (`ggml-base.bin`) |
| `PROPHET_WHISPER` | `whisper-cli` ; sinon cherché sur le chemin |
| `PROPHET_RECORDER` | `pw-record` ou `arecord` ; sinon cherché sur le chemin |
| `PROPHET_PIPER` | `piper`, la synthèse vocale ; sinon cherché sur le chemin |
| `PROPHET_PIPER_VOICE` | Voix de Piper (`*.onnx`, `.onnx.json` à côté) ; sans elle, l'OS ne parle pas |
| `PROPHET_VOICE_LANGUAGE` | Langue de transcription par défaut (`fr`) ; sinon détection automatique |
| `PROPHET_PLAYER` | `pw-play` ou `aplay` ; sinon cherché sur le chemin |

## Usage

```sh
prophet voice --file phrase.wav --language fr
prophet voice --seconds 8 --prepare documents
prophet --json voice --file phrase.wav
prophet voice --say "La note est écrite. Voulez-vous la publier ?"
prophet voice --say "Bonjour" --out bonjour.wav
prophet voice --prepare documents --reply     # écoute, prépare la mission, répond à voix haute
prophet voice --listen --prepare documents --reply   # écoute continue : « Prophète, … » déclenche
```

En écoute continue (`--listen`), seules les phrases qui commencent par le mot d'activation
(`--wake`, « prophète » par défaut, casse, accents et ponctuation ignorés) déclenchent une
action ; le reste de la phrase est l'intention. Deux ordres brefs font exception : « lance la
mission » lance la dernière mission préparée dans cette écoute, « résultat » fait dire son
résultat. Chaque tranche est effacée après transcription.

## Validation

```sh
nix develop --command cargo test -p voice
nix build .#chaine-vocale -o chaine-vocale   # Whisper, Piper, espeak-ng, le modèle et la voix
PROPHET_WHISPER=$PWD/chaine-vocale/bin/whisper-cli PROPHET_PIPER=$PWD/chaine-vocale/bin/piper \
PROPHET_TEST_ESPEAK=$PWD/chaine-vocale/bin/espeak-ng \
PROPHET_WHISPER_MODEL=$PWD/chaine-vocale/share/prophet/ggml-base.bin \
PROPHET_PIPER_VOICE=$PWD/chaine-vocale/share/prophet/voix/fr_FR-siwis-medium.onnx \
  nix develop --command cargo test -p voice -- --include-ignored --nocapture
```

Les essais ignorés synthétisent une phrase française (espeak-ng, ou la voix de Piper) et la
transcrivent avec le vrai modèle ; ils exigent les programmes et les poids, et `just check` ne
les lance pas. Le travail « Parole (Whisper et Piper réels) » de l'intégration continue les
exerce à chaque poussée, ainsi que ceux de la CLI (`prophet voice --prepare`, `--listen`) et
de l'atelier (écoute permanente). Les essais qui synthétisent leurs propres entrées posent
`Tools::deterministic` (ou `PROPHET_PIPER_DETERMINISTIC=1`) : Piper sans bruit rend le même son
pour le même texte, que Whisper entend toujours pareil ; la voix pour l'humain reste naturelle.
