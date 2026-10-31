# Voice

IGOR can listen and reply by voice. Everything runs on your Mac: speech is transcribed by an embedded Whisper model and replies are spoken with the macOS system voices. No audio leaves the computer and no extra service is needed.

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

## Approvals stay on screen

When IGOR proposes an action that changes data, it says what the action will do, but you approve or deny it on screen. A spoken "yes" is not accepted: a misheard word must never approve a write.

## Check it

```sh
IGOR_WHISPER_MODEL="$HOME/Library/Application Support/igor/models/ggml-large-v3-turbo-q5_0.bin" \
  cargo test -p igor-voice --test live -- --ignored --nocapture
```

The test speaks two requests with macOS `say`, transcribes them and checks the words.
