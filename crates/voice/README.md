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

## Usage

```sh
prophet voice --file phrase.wav --language fr
prophet voice --seconds 8 --prepare documents
prophet --json voice --file phrase.wav
```

## Validation

```sh
nix develop --command cargo test -p voice
PROPHET_WHISPER_MODEL=… PROPHET_WHISPER=… PROPHET_TEST_ESPEAK=… \
  nix develop --command cargo test -p voice -- --include-ignored --nocapture
```

Le second essai synthétise une phrase française avec espeak-ng et la transcrit avec le vrai
modèle ; il exige les trois programmes et n'est pas lancé par `just check`.
