# Voice

IGOR can listen and reply by voice. Everything runs on your Mac: speech is transcribed by an embedded Whisper model and replies are spoken by an embedded Kokoro voice model. No audio leaves the computer and no extra service is needed.

## Speech-to-text model

IGOR uses Whisper large-v3-turbo, 5-bit quantised (`ggml-large-v3-turbo-q5_0.bin`, 574 MB). Until the desktop downloads it on first use, fetch it once:

```sh
mkdir -p "$HOME/Library/Application Support/igor/models"
curl -L -o "$HOME/Library/Application Support/igor/models/ggml-large-v3-turbo-q5_0.bin" \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin
shasum -a 256 "$HOME/Library/Application Support/igor/models/ggml-large-v3-turbo-q5_0.bin"
# 394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2
```

Measured on an Apple M4 Max: loading the model takes about 6 seconds the first time; transcribing a 3 to 4 second request takes 0.3 to 0.6 seconds. Silence gives no text rather than an invented phrase.

Whisper understands about 99 languages, including Afrikaans, German, French and Spanish. By default it detects the language of each recording. On very short clips it can guess wrong, for example reading accented English as another language; fixing the language avoids that. When replies are spoken, IGOR asks the model to answer in the language you used. Igor's own voice speaks English only for now.

## Text-to-speech model

IGOR speaks with Kokoro-82M v1.0 (Apache-2.0), run by sherpa-onnx. It has 28 English voices, American and British, such as `af_heart` (the default), `bf_emma` and `bm_george`. The model is three files, about 354 MB. Until the desktop downloads them on first use, fetch them once:

```sh
DIR="$HOME/Library/Application Support/igor/models/kokoro-multi-lang-v1_0"
REV=f7b96bb6bef5c5da4d3aa4f4e0498fbbf62dc78b
mkdir -p "$DIR"
for f in model.onnx voices.bin tokens.txt; do
  curl -L -o "$DIR/$f" "https://huggingface.co/csukuangfj/kokoro-multi-lang-v1_0/resolve/$REV/$f"
done
shasum -a 256 "$DIR"/model.onnx "$DIR"/voices.bin "$DIR"/tokens.txt
# b40f62b166ac8164b0627ef48a0b358eda0985e272fb03ef5252e7206305da11  model.onnx
# 1c5a5b983d3d50d8586d437a51f3faa2da7919ce76a013c081e65671a3447c29  voices.bin
# 6ebb6bb288f20f3ae8d004d3c2ca27697da27c037d75e81a60e2a6a663f95425  tokens.txt
```

Measured on an Apple M4 Max: loading takes about 0.3 seconds; the first sentence of a reply is ready in about 0.2 seconds, and a reply is produced about seven times faster than it is spoken. IGOR speaks a reply sentence by sentence, so playback starts almost at once. Text is turned into phonemes with espeak-ng, so names and numbers are spoken rather than skipped, though an unusual name may be pronounced the English way. Units written after a number are spoken as words: `km/h` as "kilometres per hour", `°C` as "degrees", `%` as "percent", and likewise for `mph`, `m/s`, `mm`, `cm`, `km`, `kg`, `hPa`, `kW`, `kWh` and `ms`.

To hear a voice:

```sh
cargo run --release -p igor-tts --example say -- "$DIR" bf_emma /tmp/igor.wav "Hello, I am Igor." && afplay /tmp/igor.wav
```

## Approvals stay on screen

When IGOR proposes an action that changes data, it says what the action will do, but you approve or deny it on screen. A spoken "yes" is not accepted: a misheard word must never approve a write.

## Check it

```sh
IGOR_WHISPER_MODEL="$HOME/Library/Application Support/igor/models/ggml-large-v3-turbo-q5_0.bin" \
  cargo test -p igor-voice --test live -- --ignored --nocapture
```

The test speaks two requests with macOS `say`, transcribes them and checks the words.

```sh
MODELS="$HOME/Library/Application Support/igor/models"
IGOR_TTS_MODEL="$MODELS/kokoro-multi-lang-v1_0" IGOR_WHISPER_MODEL="$MODELS/ggml-large-v3-turbo-q5_0.bin" \
  cargo test --release -p igor-tts --test live -- --ignored --nocapture
```

This test speaks a reply with three voices, checks synthesis is faster than playback, and has Whisper transcribe the result to check every word is understood.
