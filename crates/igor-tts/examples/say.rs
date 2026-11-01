//! Speaks text into a WAV file, to listen to the voices.
//!
//! ```sh
//! cargo run --release -p igor-tts --example say -- \
//!   "$HOME/Library/Application Support/igor/models/kokoro-multi-lang-v1_0" bf_emma out.wav "Hello."
//! afplay out.wav
//! ```

use std::path::Path;

use igor_tts::{Synthesizer, sentences};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [model, voice, out, text] = args.as_slice() else {
        return Err("usage: say <model dir> <voice> <out.wav> <text>".into());
    };
    let model = Path::new(model);
    if !model.is_dir() {
        return Err(format!(
            "no voice model directory at {model:?}; pass the full path to kokoro-multi-lang-v1_0 \
             (is $MODELS set in this shell?)"
        )
        .into());
    }
    let tts = Synthesizer::load(model)?;
    let mut speech = igor_tts::Speech {
        samples: Vec::new(),
        sample_rate: 0,
    };
    for sentence in sentences(text) {
        let part = tts.synthesize(sentence, voice, 1.0)?;
        speech.sample_rate = part.sample_rate;
        speech.samples.extend(part.samples);
    }
    std::fs::write(out, speech.to_wav())?;
    println!("{:.1} s written to {out}", speech.seconds());
    Ok(())
}
