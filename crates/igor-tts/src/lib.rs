//! Text-to-speech for IGOR, embedded in the application.
//!
//! [`Synthesizer`] runs the Kokoro-82M voice model locally through sherpa-onnx (ONNX Runtime,
//! linked statically). Nothing is sent over the network and no separate service runs: the host
//! downloads the [`MODEL_FILES`] once into a directory named [`MODEL_DIR`], loads them, and passes
//! text. Text is turned into phonemes with espeak-ng (the `piper-phonemize` crate, which embeds
//! its data), so names, numbers and words outside any dictionary are spoken too.
//!
//! Synthesis is blocking CPU work. On Apple silicon the first sentence takes about 0.2 seconds
//! and a reply is produced about seven times faster than it is spoken. Hosts run it off their
//! async runtime, for example with `spawn_blocking`, and speak a reply sentence by sentence
//! ([`sentences`]) so playback starts quickly. Units written after numbers, such as `km/h` and
//! `°C`, are spoken as words ([`speakable`]).

mod speakable;

pub use speakable::speakable;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use sherpa_onnx::{
    GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsKokoroModelConfig,
    OfflineTtsModelConfig,
};
use thiserror::Error;

/// Directory, under the host's model directory, that holds the [`MODEL_FILES`].
pub const MODEL_DIR: &str = "kokoro-multi-lang-v1_0";

/// One file of the voice model, as published on Hugging Face at a pinned revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFile {
    /// File name inside [`MODEL_DIR`].
    pub name: &'static str,
    /// Download URL, pinned to one revision.
    pub url: &'static str,
    /// Size in bytes.
    pub size: u64,
    /// SHA-256 of the file, lowercase hex. Hosts verify it before using the file.
    pub sha256: &'static str,
}

/// The files [`Synthesizer::load`] needs: Kokoro v1.0 in float32 (about 354 MB in all,
/// Apache-2.0). The int8 build is smaller but about five times slower on Apple silicon.
pub const MODEL_FILES: [ModelFile; 3] = [
    ModelFile {
        name: "model.onnx",
        url: "https://huggingface.co/csukuangfj/kokoro-multi-lang-v1_0/resolve/f7b96bb6bef5c5da4d3aa4f4e0498fbbf62dc78b/model.onnx",
        size: 325_560_556,
        sha256: "b40f62b166ac8164b0627ef48a0b358eda0985e272fb03ef5252e7206305da11",
    },
    ModelFile {
        name: "voices.bin",
        url: "https://huggingface.co/csukuangfj/kokoro-multi-lang-v1_0/resolve/f7b96bb6bef5c5da4d3aa4f4e0498fbbf62dc78b/voices.bin",
        size: 28_200_960,
        sha256: "1c5a5b983d3d50d8586d437a51f3faa2da7919ce76a013c081e65671a3447c29",
    },
    ModelFile {
        name: "tokens.txt",
        url: "https://huggingface.co/csukuangfj/kokoro-multi-lang-v1_0/resolve/f7b96bb6bef5c5da4d3aa4f4e0498fbbf62dc78b/tokens.txt",
        size: 687,
        sha256: "6ebb6bb288f20f3ae8d004d3c2ca27697da27c037d75e81a60e2a6a663f95425",
    },
];

/// Accent of a [`Voice`]; it also selects the pronunciation rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accent {
    /// American English.
    American,
    /// British English.
    British,
}

impl Accent {
    /// espeak-ng voice used to turn text into phonemes. espeak-ng's `en` is British English;
    /// it has no `en-gb` voice.
    fn espeak_voice(self) -> &'static str {
        match self {
            Self::American => "en-us",
            Self::British => "en",
        }
    }
}

/// An English voice of the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Voice {
    /// Stable identifier, the model's own name, such as `af_heart`.
    pub id: &'static str,
    /// Name to show, such as `Heart`.
    pub name: &'static str,
    /// Accent.
    pub accent: Accent,
    /// Speaker number in the model.
    speaker: i32,
}

const fn voice(id: &'static str, name: &'static str, accent: Accent, speaker: i32) -> Voice {
    Voice {
        id,
        name,
        accent,
        speaker,
    }
}

/// The model's English voices, by speaker number, as listed in the model's metadata.
pub const VOICES: [Voice; 28] = [
    voice("af_alloy", "Alloy", Accent::American, 0),
    voice("af_aoede", "Aoede", Accent::American, 1),
    voice("af_bella", "Bella", Accent::American, 2),
    voice("af_heart", "Heart", Accent::American, 3),
    voice("af_jessica", "Jessica", Accent::American, 4),
    voice("af_kore", "Kore", Accent::American, 5),
    voice("af_nicole", "Nicole", Accent::American, 6),
    voice("af_nova", "Nova", Accent::American, 7),
    voice("af_river", "River", Accent::American, 8),
    voice("af_sarah", "Sarah", Accent::American, 9),
    voice("af_sky", "Sky", Accent::American, 10),
    voice("am_adam", "Adam", Accent::American, 11),
    voice("am_echo", "Echo", Accent::American, 12),
    voice("am_eric", "Eric", Accent::American, 13),
    voice("am_fenrir", "Fenrir", Accent::American, 14),
    voice("am_liam", "Liam", Accent::American, 15),
    voice("am_michael", "Michael", Accent::American, 16),
    voice("am_onyx", "Onyx", Accent::American, 17),
    voice("am_puck", "Puck", Accent::American, 18),
    voice("am_santa", "Santa", Accent::American, 19),
    voice("bf_alice", "Alice", Accent::British, 20),
    voice("bf_emma", "Emma", Accent::British, 21),
    voice("bf_isabella", "Isabella", Accent::British, 22),
    voice("bf_lily", "Lily", Accent::British, 23),
    voice("bm_daniel", "Daniel", Accent::British, 24),
    voice("bm_fable", "Fable", Accent::British, 25),
    voice("bm_george", "George", Accent::British, 26),
    voice("bm_lewis", "Lewis", Accent::British, 27),
];

/// Voice used when none is chosen: the model's best-rated voice.
pub const DEFAULT_VOICE: &str = "af_heart";

/// The voice with identifier `id`.
pub fn find_voice(id: &str) -> Option<&'static Voice> {
    VOICES.iter().find(|voice| voice.id == id)
}

/// Longest text accepted by one [`Synthesizer::synthesize`] call, in bytes. Hosts pass one
/// sentence or a few at a time.
pub const MAX_TEXT_BYTES: usize = 2_000;

/// Slowest speaking speed accepted.
pub const MIN_SPEED: f32 = 0.5;
/// Fastest speaking speed accepted.
pub const MAX_SPEED: f32 = 2.0;

/// Text could not be turned into speech.
#[derive(Debug, Error)]
pub enum SpeechError {
    /// A model file does not exist.
    #[error("voice model file not found at {0}")]
    ModelMissing(PathBuf),
    /// The model could not be loaded.
    #[error("cannot load the voice model in {0}")]
    Load(PathBuf),
    /// No voice has this identifier.
    #[error("unknown voice {0:?}")]
    UnknownVoice(String),
    /// The text has nothing to say.
    #[error("there is no text to speak")]
    Empty,
    /// The text is longer than [`MAX_TEXT_BYTES`].
    #[error("the text is longer than {MAX_TEXT_BYTES} bytes")]
    TooLong,
    /// The speed is outside [`MIN_SPEED`] to [`MAX_SPEED`].
    #[error("the speed must be between {MIN_SPEED} and {MAX_SPEED}")]
    InvalidSpeed,
    /// The text could not be turned into phonemes.
    #[error("cannot turn the text into phonemes")]
    Phonemize,
    /// The model failed while speaking.
    #[error("speech synthesis failed")]
    Synthesize,
}

/// Synthesised speech: mono samples in the range -1.0 to 1.0.
#[derive(Debug, Clone, PartialEq)]
pub struct Speech {
    /// Samples.
    pub samples: Vec<f32>,
    /// Sample rate in hertz.
    pub sample_rate: u32,
}

impl Speech {
    /// Length in seconds.
    pub fn seconds(&self) -> f32 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.samples.len() as f32 / self.sample_rate as f32
    }

    /// The speech as a 16-bit PCM WAV file, for playing in a web view.
    pub fn to_wav(&self) -> Vec<u8> {
        let data_len = u32::try_from(self.samples.len() * 2).unwrap_or(u32::MAX);
        let mut wav = Vec::with_capacity(44 + self.samples.len() * 2);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&data_len.saturating_add(36).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&self.sample_rate.to_le_bytes());
        wav.extend_from_slice(&self.sample_rate.saturating_mul(2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes()); // bytes per frame
        wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        for sample in &self.samples {
            // Truncation is the conversion: the clamped value fits i16.
            #[allow(clippy::cast_possible_truncation)]
            let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
            wav.extend_from_slice(&value.to_le_bytes());
        }
        wav
    }
}

/// espeak-ng keeps global state, so phonemizing runs one call at a time.
static PHONEMIZER: Mutex<()> = Mutex::new(());

/// A loaded voice model. Load once and reuse: loading takes about a third of a second.
pub struct Synthesizer {
    tts: OfflineTts,
}

impl std::fmt::Debug for Synthesizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Synthesizer").finish_non_exhaustive()
    }
}

impl Synthesizer {
    /// Loads the model from `dir`, which holds the [`MODEL_FILES`]. The files are not
    /// re-hashed here: hosts verify them when they download them.
    pub fn load(dir: &Path) -> Result<Self, SpeechError> {
        let path = |name: &str| -> Result<String, SpeechError> {
            let file = dir.join(name);
            if !file.is_file() {
                return Err(SpeechError::ModelMissing(file));
            }
            file.to_str()
                .map(str::to_owned)
                .ok_or_else(|| SpeechError::Load(dir.to_path_buf()))
        };
        let config = OfflineTtsConfig {
            model: OfflineTtsModelConfig {
                kokoro: OfflineTtsKokoroModelConfig {
                    model: Some(path("model.onnx")?),
                    voices: Some(path("voices.bin")?),
                    tokens: Some(path("tokens.txt")?),
                    length_scale: 1.0,
                    ..Default::default()
                },
                num_threads: threads(),
                debug: false,
                ..Default::default()
            },
            ..Default::default()
        };
        let tts =
            OfflineTts::create(&config).ok_or_else(|| SpeechError::Load(dir.to_path_buf()))?;
        Ok(Self { tts })
    }

    /// Speaks `text` with the voice `voice` (a [`Voice::id`]) at `speed`, 1.0 being normal.
    pub fn synthesize(&self, text: &str, voice: &str, speed: f32) -> Result<Speech, SpeechError> {
        let voice = find_voice(voice).ok_or_else(|| SpeechError::UnknownVoice(voice.to_owned()))?;
        let text = validate(text, speed)?;
        let phonemes = phonemize(&speakable(text), voice.accent)?;
        let config = GenerationConfig {
            sid: voice.speaker,
            speed,
            phoneme_codepoints: Some(phonemes),
            ..Default::default()
        };
        // The text was phonemized above, so the model is given none.
        let audio = self
            .tts
            .generate_with_config("", &config, None::<fn(&[f32], f32) -> bool>)
            .ok_or(SpeechError::Synthesize)?;
        let sample_rate =
            u32::try_from(audio.sample_rate()).map_err(|_| SpeechError::Synthesize)?;
        Ok(Speech {
            samples: audio.samples().to_vec(),
            sample_rate,
        })
    }
}

/// Checks `text` and `speed` are usable; returns the trimmed text.
fn validate(text: &str, speed: f32) -> Result<&str, SpeechError> {
    if !(MIN_SPEED..=MAX_SPEED).contains(&speed) {
        return Err(SpeechError::InvalidSpeed);
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err(SpeechError::TooLong);
    }
    let text = text.trim();
    if !text.chars().any(char::is_alphanumeric) {
        return Err(SpeechError::Empty);
    }
    Ok(text)
}

/// Phoneme codepoints of `text`, one list per sentence.
fn phonemize(text: &str, accent: Accent) -> Result<Vec<Vec<i32>>, SpeechError> {
    let _one_at_a_time = PHONEMIZER.lock().unwrap_or_else(PoisonError::into_inner);
    let result =
        piper_phonemize::phonemize(text, accent.espeak_voice()).ok_or(SpeechError::Phonemize)?;
    let sentences: Vec<Vec<i32>> = (0..result.num_sentences())
        .filter_map(|i| result.get_phonemes(i))
        .filter(|phonemes| !phonemes.is_empty())
        .collect();
    if sentences.is_empty() {
        return Err(SpeechError::Empty);
    }
    Ok(sentences)
}

/// Longest piece [`sentences`] returns, in bytes, before it splits at a space.
const MAX_SENTENCE_BYTES: usize = 300;

/// Splits a reply into sentences to speak one at a time, so playback can start while the rest
/// is synthesised. A sentence ends at `.`, `!`, `?` or `;` followed by white space, or at a line
/// break; a dot inside a word, as in `solacion.com` or `1.5`, does not end one. Very long
/// sentences are split at a space. Pieces with nothing to say are dropped.
pub fn sentences(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let next_is_space = chars.peek().is_none_or(|(_, next)| next.is_whitespace());
        let ends = c == '\n' || (matches!(c, '.' | '!' | '?' | ';') && next_is_space);
        if ends {
            let end = i + c.len_utf8();
            pieces.extend(split_long(&text[start..end]));
            start = end;
        }
    }
    pieces.extend(split_long(&text[start..]));
    pieces
        .into_iter()
        .map(str::trim)
        .filter(|piece| piece.chars().any(char::is_alphanumeric))
        .collect()
}

/// Splits `piece` at spaces into parts of at most [`MAX_SENTENCE_BYTES`] where it can.
fn split_long(mut piece: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    while piece.len() > MAX_SENTENCE_BYTES {
        let mut limit = MAX_SENTENCE_BYTES;
        while !piece.is_char_boundary(limit) {
            limit -= 1;
        }
        let cut = piece[..limit]
            .rfind(' ')
            .filter(|&at| at > 0)
            .unwrap_or(limit);
        parts.push(&piece[..cut]);
        piece = &piece[cut..];
    }
    parts.push(piece);
    parts
}

/// CPU threads for synthesis.
fn threads() -> i32 {
    std::thread::available_parallelism()
        .map_or(4, |n| n.get().min(8))
        .try_into()
        .unwrap_or(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voices_have_unique_ids_and_consecutive_speakers() {
        for (index, voice) in VOICES.iter().enumerate() {
            assert_eq!(
                usize::try_from(voice.speaker).ok(),
                Some(index),
                "{}",
                voice.id
            );
            let british = voice.id.starts_with('b');
            assert_eq!(voice.accent == Accent::British, british, "{}", voice.id);
            assert_eq!(
                VOICES.iter().filter(|other| other.id == voice.id).count(),
                1
            );
        }
        assert!(find_voice(DEFAULT_VOICE).is_some());
        assert!(
            find_voice("ef_dora").is_none(),
            "only English voices are listed"
        );
    }

    #[test]
    fn model_files_are_pinned_and_hashed() {
        for file in MODEL_FILES {
            assert!(file.url.contains("/resolve/f7b96bb6"), "{}", file.name);
            assert!(file.url.ends_with(file.name));
            assert_eq!(file.sha256.len(), 64);
            assert!(
                file.sha256
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            );
        }
    }

    #[test]
    fn rejects_unusable_text_and_speed() {
        assert!(matches!(validate("  ", 1.0), Err(SpeechError::Empty)));
        assert!(matches!(validate("...", 1.0), Err(SpeechError::Empty)));
        assert!(matches!(
            validate(&"a".repeat(MAX_TEXT_BYTES + 1), 1.0),
            Err(SpeechError::TooLong)
        ));
        assert!(matches!(
            validate("hi", 0.4),
            Err(SpeechError::InvalidSpeed)
        ));
        assert!(matches!(
            validate("hi", f32::NAN),
            Err(SpeechError::InvalidSpeed)
        ));
        assert_eq!(validate(" Hi there. ", 1.0).ok(), Some("Hi there."));
    }

    #[test]
    fn splits_replies_into_sentences() {
        let reply = "Hi Pieter. I tested solacion.com: LCP is 1.5 s!\nShall I go on? Yes; fine.";
        assert_eq!(
            sentences(reply),
            [
                "Hi Pieter.",
                "I tested solacion.com: LCP is 1.5 s!",
                "Shall I go on?",
                "Yes;",
                "fine."
            ]
        );
        assert!(sentences(" . \n - ").is_empty());
    }

    #[test]
    fn long_sentences_are_split_at_spaces() {
        let long = "word ".repeat(200);
        let parts = sentences(&long);
        assert!(parts.len() > 1);
        assert!(parts.iter().all(|part| part.len() <= MAX_SENTENCE_BYTES));
        assert_eq!(parts.join(" "), long.trim());
        let unbroken = "é".repeat(400);
        assert!(
            sentences(&unbroken)
                .iter()
                .all(|part| part.len() <= MAX_SENTENCE_BYTES)
        );
    }

    #[test]
    fn wav_has_a_pcm_header() {
        let speech = Speech {
            samples: vec![0.0, 1.0, -1.0, 2.0],
            sample_rate: 24_000,
        };
        let wav = speech.to_wav();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(wav.len(), 44 + 8);
        assert_eq!(&wav[44..], [0, 0, 0xff, 0x7f, 0x01, 0x80, 0xff, 0x7f]);
        assert!((speech.seconds() - 4.0 / 24_000.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_missing_model_is_reported_with_its_path() {
        let dir = Path::new("/nonexistent/igor/kokoro");
        let result = Synthesizer::load(dir);
        assert!(
            matches!(&result, Err(SpeechError::ModelMissing(path)) if path == &dir.join("model.onnx")),
            "{result:?}"
        );
    }

    #[test]
    fn phonemizes_names_and_numbers() -> Result<(), SpeechError> {
        for accent in [Accent::American, Accent::British] {
            let sentences = phonemize("Hi Pieter, it is 154 milliseconds.", accent)?;
            assert_eq!(sentences.len(), 1, "{accent:?}");
            // "one hundred fifty four" is far longer than the digits.
            assert!(sentences[0].len() > 40, "{accent:?}: {sentences:?}");
        }
        Ok(())
    }
}
