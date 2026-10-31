//! Live transcription check against a real Whisper model. Ignored by default: it needs the model
//! file (about 570 MB) and macOS `say` to synthesise speech.
//!
//! ```sh
//! IGOR_WHISPER_MODEL="$HOME/Library/Application Support/igor/models/ggml-large-v3-turbo-q5_0.bin" \
//!   cargo test -p igor-voice --test live -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use igor_voice::{SAMPLE_RATE, Transcriber};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Synthesises `text` with macOS `say` as 16 kHz mono 32-bit float samples.
fn speak(text: &str, dir: &Path) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let wav = dir.join("speech.wav");
    let status = Command::new("say")
        .args([
            "--file-format=WAVE",
            &format!("--data-format=LEF32@{SAMPLE_RATE}"),
            "-o",
        ])
        .arg(&wav)
        .arg(text)
        .status()?;
    if !status.success() {
        return Err("say failed".into());
    }
    let bytes = std::fs::read(&wav)?;
    // Find the `data` chunk: RIFF header, then chunks of id, little-endian size, payload.
    let mut offset = 12;
    while offset + 8 <= bytes.len() {
        let id = bytes.get(offset..offset + 4).ok_or("truncated chunk id")?;
        let size_bytes: [u8; 4] = bytes
            .get(offset + 4..offset + 8)
            .ok_or("truncated chunk size")?
            .try_into()?;
        let size = usize::try_from(u32::from_le_bytes(size_bytes))?;
        let start = offset + 8;
        if id == b"data" {
            let data = bytes.get(start..start + size).ok_or("truncated data")?;
            return Ok(data
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect());
        }
        offset = start + size + size % 2;
    }
    Err("no data chunk".into())
}

fn model() -> Option<PathBuf> {
    std::env::var_os("IGOR_WHISPER_MODEL").map(PathBuf::from)
}

#[test]
#[ignore = "needs a Whisper model file and macOS say; set IGOR_WHISPER_MODEL"]
fn transcribes_synthesised_requests() -> TestResult {
    let Some(model) = model() else {
        return Err("set IGOR_WHISPER_MODEL to the model file".into());
    };
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("igor-voice-live");
    std::fs::create_dir_all(&dir)?;
    let started = Instant::now();
    let transcriber = Transcriber::load(&model)?;
    println!("model loaded in {:?}", started.elapsed());

    let cases = [
        (
            "Create a draft order for supplier X for delivery on the fifteenth of July.",
            &["draft order", "supplier x", "july"][..],
        ),
        (
            "How many cases of cola does supplier X have in stock?",
            &["cases", "cola", "stock"][..],
        ),
    ];
    for (spoken, expected) in cases {
        let audio = speak(spoken, &dir)?;
        let started = Instant::now();
        let transcript = transcriber.transcribe(&audio)?;
        let seconds = audio.len() as f64 / SAMPLE_RATE as f64;
        println!(
            "{seconds:.1} s of speech in {:?}: {:?}",
            started.elapsed(),
            transcript.text
        );
        let heard = transcript.text.to_lowercase();
        for word in expected {
            assert!(heard.contains(word), "missing {word:?} in {heard:?}");
        }
    }

    let silence = vec![0.0; SAMPLE_RATE * 2];
    assert!(transcriber.transcribe(&silence)?.is_empty());
    Ok(())
}
