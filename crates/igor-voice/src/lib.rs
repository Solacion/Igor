//! Speech-to-text for IGOR, embedded in the application.
//!
//! [`Transcriber`] runs OpenAI's Whisper locally through whisper.cpp (the `whisper-rs` crate),
//! on the GPU through Metal on macOS. Nothing is sent over the network and no separate service
//! runs: the host loads a Whisper model file once and passes recorded speech as 16 kHz mono
//! samples. See [`DEFAULT_MODEL_FILE`] for the recommended model.
//!
//! Transcription is blocking, GPU-bound work that takes from a fraction of a second to a few
//! seconds: hosts run it off their async runtime, for example with `spawn_blocking`.
//!
//! Silence is detected before the model runs, and segments Whisper marks as probably not speech
//! are dropped, so a silent recording gives an empty [`Transcript`] instead of a hallucinated
//! phrase.

use std::path::{Path, PathBuf};

use thiserror::Error;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Sample rate [`Transcriber::transcribe`] expects, in hertz.
pub const SAMPLE_RATE: usize = 16_000;

/// Longest recording accepted, in seconds. Push-to-talk requests are far shorter.
pub const MAX_AUDIO_SECONDS: usize = 120;

/// Recommended model: Whisper large-v3-turbo, 5-bit quantised (about 570 MB). Accurate, and fast
/// on Apple silicon. Download it from
/// `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin`.
pub const DEFAULT_MODEL_FILE: &str = "ggml-large-v3-turbo-q5_0.bin";

/// Recordings whose loudest sample is below this level are treated as silence.
const SILENCE_PEAK: f32 = 0.01;

/// Segments with a higher no-speech probability are dropped.
const MAX_NO_SPEECH_PROBABILITY: f32 = 0.6;

/// Speech could not be turned into text.
#[derive(Debug, Error)]
pub enum VoiceError {
    /// The model file does not exist.
    #[error("speech model not found at {0}")]
    ModelMissing(PathBuf),
    /// The model file could not be loaded.
    #[error("cannot load the speech model {path}: {reason}")]
    Load {
        /// Model file.
        path: PathBuf,
        /// What went wrong.
        reason: String,
    },
    /// The recording is empty.
    #[error("the recording is empty")]
    Empty,
    /// The recording is longer than [`MAX_AUDIO_SECONDS`].
    #[error("the recording is longer than {MAX_AUDIO_SECONDS} seconds")]
    TooLong,
    /// A sample is not a finite number.
    #[error("the recording contains invalid samples")]
    InvalidSamples,
    /// Whisper failed while transcribing.
    #[error("transcription failed: {0}")]
    Transcribe(String),
}

/// The result of transcribing one recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    /// The recognised text, trimmed. Empty when no speech was heard.
    pub text: String,
}

impl Transcript {
    /// True when no speech was heard.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// A loaded Whisper model. Load once and reuse: loading takes about a second.
pub struct Transcriber {
    context: WhisperContext,
    language: String,
}

impl std::fmt::Debug for Transcriber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transcriber")
            .field("language", &self.language)
            .finish_non_exhaustive()
    }
}

impl Transcriber {
    /// Loads the model at `model` for English speech. whisper.cpp's own logging is silenced.
    pub fn load(model: &Path) -> Result<Self, VoiceError> {
        if !model.is_file() {
            return Err(VoiceError::ModelMissing(model.to_path_buf()));
        }
        let load_error = |reason: String| VoiceError::Load {
            path: model.to_path_buf(),
            reason,
        };
        whisper_rs::install_logging_hooks();
        let path = model
            .to_str()
            .ok_or_else(|| load_error("the path is not valid UTF-8".to_owned()))?;
        let context = WhisperContext::new_with_params(path, WhisperContextParameters::default())
            .map_err(|err| load_error(err.to_string()))?;
        Ok(Self {
            context,
            language: "en".to_owned(),
        })
    }

    /// Transcribes `audio`: mono samples at [`SAMPLE_RATE`], in the range -1.0 to 1.0.
    pub fn transcribe(&self, audio: &[f32]) -> Result<Transcript, VoiceError> {
        if is_silent(validate(audio)?) {
            return Ok(Transcript {
                text: String::new(),
            });
        }
        let failed = |err: whisper_rs::WhisperError| VoiceError::Transcribe(err.to_string());
        let mut state = self.context.create_state().map_err(failed)?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(&self.language));
        params.set_n_threads(threads());
        params.set_no_context(true);
        params.set_no_timestamps(true);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        state.full(params, audio).map_err(failed)?;
        let mut text = String::new();
        for segment in state.as_iter() {
            if segment.no_speech_probability() > MAX_NO_SPEECH_PROBABILITY {
                continue;
            }
            let part = segment.to_str_lossy().map_err(failed)?;
            let part = part.trim();
            if !part.is_empty() {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(part);
            }
        }
        Ok(Transcript { text })
    }
}

/// Checks `audio` is usable: not empty, not too long, finite samples.
fn validate(audio: &[f32]) -> Result<&[f32], VoiceError> {
    if audio.is_empty() {
        return Err(VoiceError::Empty);
    }
    if audio.len() > MAX_AUDIO_SECONDS * SAMPLE_RATE {
        return Err(VoiceError::TooLong);
    }
    if !audio.iter().all(|sample| sample.is_finite()) {
        return Err(VoiceError::InvalidSamples);
    }
    Ok(audio)
}

fn is_silent(audio: &[f32]) -> bool {
    audio.iter().all(|sample| sample.abs() < SILENCE_PEAK)
}

/// CPU threads for the parts of Whisper that do not run on the GPU.
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
    fn rejects_unusable_audio() {
        assert!(matches!(validate(&[]), Err(VoiceError::Empty)));
        let long = vec![0.0; MAX_AUDIO_SECONDS * SAMPLE_RATE + 1];
        assert!(matches!(validate(&long), Err(VoiceError::TooLong)));
        assert!(matches!(
            validate(&[0.1, f32::NAN]),
            Err(VoiceError::InvalidSamples)
        ));
        assert!(validate(&[0.0, 0.5, -0.5]).is_ok());
    }

    #[test]
    fn quiet_recordings_are_silence() {
        assert!(is_silent(&[0.0, 0.005, -0.009]));
        assert!(!is_silent(&[0.0, 0.2, -0.1]));
    }

    #[test]
    fn a_missing_model_is_reported_with_its_path() {
        let path = Path::new("/nonexistent/igor/ggml-model.bin");
        let result = Transcriber::load(path);
        assert!(
            matches!(&result, Err(VoiceError::ModelMissing(missing)) if missing == path),
            "{result:?}"
        );
    }
}
