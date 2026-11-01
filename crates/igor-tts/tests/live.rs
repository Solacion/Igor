//! Live speech check against the real Kokoro model, heard back by Whisper. Ignored by default:
//! it needs both models (about 930 MB).
//!
//! ```sh
//! MODELS="$HOME/Library/Application Support/igor/models"
//! IGOR_TTS_MODEL="$MODELS/kokoro-multi-lang-v1_0" \
//! IGOR_WHISPER_MODEL="$MODELS/ggml-large-v3-turbo-q5_0.bin" \
//!   cargo test --release -p igor-tts --test live -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::time::Instant;

use igor_tts::{Synthesizer, sentences};
use igor_voice::{SAMPLE_RATE, Transcriber};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn env_path(name: &str) -> Result<PathBuf, String> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| format!("set {name}"))
}

/// Linear resampling to Whisper's rate; good enough for a recognition check.
fn resample(samples: &[f32], from: u32) -> Vec<f32> {
    let ratio = from as f64 / SAMPLE_RATE as f64;
    let len = (samples.len() as f64 / ratio) as usize;
    (0..len)
        .map(|i| {
            let at = i as f64 * ratio;
            let low = at.floor() as usize;
            let high = (low + 1).min(samples.len().saturating_sub(1));
            let frac = (at - low as f64) as f32;
            samples.get(low).copied().unwrap_or(0.0) * (1.0 - frac)
                + samples.get(high).copied().unwrap_or(0.0) * frac
        })
        .collect()
}

#[test]
#[ignore = "needs the Kokoro and Whisper models; set IGOR_TTS_MODEL and IGOR_WHISPER_MODEL"]
fn speech_is_fast_and_understood() -> TestResult {
    let started = Instant::now();
    let tts = Synthesizer::load(&env_path("IGOR_TTS_MODEL")?)?;
    println!("voice model loaded in {:?}", started.elapsed());
    let whisper = Transcriber::load(&env_path("IGOR_WHISPER_MODEL")?)?;

    let reply = "Hi Pieter. The largest contentful paint is 154 milliseconds. \
                 Shall I draft an order for supplier X?";
    for voice in ["af_heart", "bf_emma", "bm_george"] {
        let mut audio = Vec::new();
        let mut rate = 0;
        let started = Instant::now();
        for (index, sentence) in sentences(reply).into_iter().enumerate() {
            let speech = tts.synthesize(sentence, voice, 1.0)?;
            if index == 0 {
                println!("{voice}: first sentence after {:?}", started.elapsed());
            }
            rate = speech.sample_rate;
            audio.extend(speech.samples);
        }
        let elapsed = started.elapsed();
        let seconds = audio.len() as f64 / f64::from(rate);
        println!("{voice}: {seconds:.1} s of speech in {elapsed:?}");
        assert!(
            elapsed.as_secs_f64() < seconds,
            "synthesis must be faster than playback"
        );

        let heard = whisper
            .transcribe(&resample(&audio, rate))?
            .text
            .to_lowercase();
        println!("{voice}: heard {heard:?}");
        for word in [
            "largest",
            "paint",
            "154",
            "milliseconds",
            "order",
            "supplier",
        ] {
            assert!(
                heard.contains(word),
                "{voice}: missing {word:?} in {heard:?}"
            );
        }
    }
    Ok(())
}
