//! Igor's own voice: the embedded Kokoro model from `igor-tts`.
//!
//! The model (about 354 MB) is downloaded once, with the operator's consent, into
//! `~/Library/Application Support/igor/models/`. Each file is written to `<name>.part`, checked
//! against the size and SHA-256 pinned in [`igor_tts::MODEL_FILES`], and only then renamed.
//! The synthesizer is loaded once and shared; `speak` returns one sentence as WAV bytes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use igor_tts::{Accent, MODEL_DIR, MODEL_FILES, ModelFile, Synthesizer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::ipc::{Channel, Response};
use tokio::io::AsyncWriteExt;

/// Model directory relative to `HOME`, next to the default configuration file.
const MODELS_PATH: &str = "Library/Application Support/igor/models";

/// Bound on reading one chunk of a model download; the whole download is not bounded.
const CHUNK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// A failed speech command, with a message for the operator.
#[derive(Debug, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct SpeechCommandError {
    message: String,
}

impl SpeechCommandError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl From<igor_tts::SpeechError> for SpeechCommandError {
    fn from(err: igor_tts::SpeechError) -> Self {
        Self::new(err.to_string())
    }
}

#[derive(Debug, Serialize)]
pub struct VoiceView {
    id: &'static str,
    name: &'static str,
    accent: &'static str,
}

#[derive(Debug, Serialize)]
pub struct SpeechStatus {
    /// Every model file is present.
    installed: bool,
    /// Total download size in bytes.
    download_bytes: u64,
    default_voice: &'static str,
    voices: Vec<VoiceView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    received: u64,
    total: u64,
}

#[derive(Debug, Deserialize)]
pub struct SpeakRequest {
    text: String,
    voice: String,
    speed: f32,
}

/// Shared speech state: where the model lives and the loaded synthesizer.
#[derive(Debug)]
pub struct Speech {
    dir: Option<PathBuf>,
    synthesizer: Mutex<Option<Arc<Synthesizer>>>,
    downloading: AtomicBool,
}

impl Speech {
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            synthesizer: Mutex::new(None),
            downloading: AtomicBool::new(false),
        }
    }

    /// The model directory under `HOME`, or `None` without a home directory.
    pub fn from_env() -> Self {
        let dir = std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .map(|home| PathBuf::from(home).join(MODELS_PATH).join(MODEL_DIR));
        Self::new(dir)
    }

    fn dir(&self) -> Result<&Path, SpeechCommandError> {
        self.dir
            .as_deref()
            .ok_or_else(|| SpeechCommandError::new("no home directory for Igor's voice model"))
    }

    pub fn status(&self) -> SpeechStatus {
        SpeechStatus {
            installed: self.dir.as_deref().is_some_and(installed),
            download_bytes: MODEL_FILES.iter().map(|file| file.size).sum(),
            default_voice: igor_tts::DEFAULT_VOICE,
            voices: igor_tts::VOICES
                .iter()
                .map(|voice| VoiceView {
                    id: voice.id,
                    name: voice.name,
                    accent: match voice.accent {
                        Accent::American => "American",
                        Accent::British => "British",
                    },
                })
                .collect(),
        }
    }

    /// The loaded synthesizer, loading it on first use. Blocks: call from a blocking thread.
    fn synthesizer(&self) -> Result<Arc<Synthesizer>, SpeechCommandError> {
        let mut slot = self
            .synthesizer
            .lock()
            .map_err(|_| SpeechCommandError::new("Igor's voice is unavailable"))?;
        if let Some(loaded) = slot.as_ref() {
            return Ok(Arc::clone(loaded));
        }
        let dir = self.dir()?;
        if !installed(dir) {
            return Err(SpeechCommandError::new(
                "Igor's voice is not downloaded yet. Download it in Settings → Voice.",
            ));
        }
        let loaded = Arc::new(Synthesizer::load(dir)?);
        *slot = Some(Arc::clone(&loaded));
        Ok(loaded)
    }

    /// Loads the model ahead of the first reply. Blocks.
    pub fn prepare(&self) -> Result<(), SpeechCommandError> {
        self.synthesizer().map(drop)
    }

    /// One sentence as WAV bytes. Blocks.
    pub fn speak(&self, request: &SpeakRequest) -> Result<Vec<u8>, SpeechCommandError> {
        if igor_tts::find_voice(&request.voice).is_none() {
            return Err(SpeechCommandError::new(format!(
                "unknown voice {:?}",
                request.voice
            )));
        }
        let speech =
            self.synthesizer()?
                .synthesize(&request.text, &request.voice, request.speed)?;
        Ok(speech.to_wav())
    }

    /// Downloads every missing model file, reporting bytes received across all files.
    pub async fn download(
        &self,
        progress: impl Fn(DownloadProgress),
    ) -> Result<(), SpeechCommandError> {
        if self.downloading.swap(true, Ordering::AcqRel) {
            return Err(SpeechCommandError::new(
                "Igor's voice is already downloading",
            ));
        }
        let result = self.download_files(&progress).await;
        self.downloading.store(false, Ordering::Release);
        result
    }

    async fn download_files(
        &self,
        progress: &impl Fn(DownloadProgress),
    ) -> Result<(), SpeechCommandError> {
        let dir = self.dir()?.to_path_buf();
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|err| SpeechCommandError::new(format!("cannot create {dir:?}: {err}")))?;
        let client = reqwest::Client::builder()
            .read_timeout(CHUNK_TIMEOUT)
            .build()
            .map_err(|err| SpeechCommandError::new(format!("cannot start the download: {err}")))?;
        let total = MODEL_FILES.iter().map(|file| file.size).sum();
        let mut done = 0;
        for file in &MODEL_FILES {
            let target = dir.join(file.name);
            if !has_size(&target, file.size) {
                fetch(&client, file, &target, |received| {
                    progress(DownloadProgress {
                        received: done + received,
                        total,
                    });
                })
                .await?;
            }
            done += file.size;
            progress(DownloadProgress {
                received: done,
                total,
            });
        }
        Ok(())
    }
}

/// Every model file is present with its pinned size. Hashes are checked when downloading.
fn installed(dir: &Path) -> bool {
    MODEL_FILES
        .iter()
        .all(|file| has_size(&dir.join(file.name), file.size))
}

fn has_size(path: &Path, size: u64) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() == size)
}

/// Downloads `file` to `target` through `<target>.part`, renaming only after the size and
/// SHA-256 match. A failed download leaves no `.part` file behind.
async fn fetch(
    client: &reqwest::Client,
    file: &ModelFile,
    target: &Path,
    progress: impl Fn(u64),
) -> Result<(), SpeechCommandError> {
    let part = target.with_file_name(format!("{}.part", file.name));
    let result = fetch_part(client, file, &part, progress).await;
    match result {
        Ok(()) => tokio::fs::rename(&part, target)
            .await
            .map_err(|err| SpeechCommandError::new(format!("cannot save {}: {err}", file.name))),
        Err(err) => {
            let _ = tokio::fs::remove_file(&part).await;
            Err(err)
        }
    }
}

async fn fetch_part(
    client: &reqwest::Client,
    file: &ModelFile,
    part: &Path,
    progress: impl Fn(u64),
) -> Result<(), SpeechCommandError> {
    let failed = |err: &dyn std::fmt::Display| {
        SpeechCommandError::new(format!("downloading {} failed: {err}", file.name))
    };
    let mut response = client
        .get(file.url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|err| failed(&err))?;
    let mut out = tokio::fs::File::create(part)
        .await
        .map_err(|err| failed(&err))?;
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    while let Some(chunk) = response.chunk().await.map_err(|err| failed(&err))? {
        received += chunk.len() as u64;
        if received > file.size {
            return Err(failed(&"the file is larger than expected"));
        }
        hasher.update(&chunk);
        out.write_all(&chunk).await.map_err(|err| failed(&err))?;
        progress(received);
    }
    out.flush().await.map_err(|err| failed(&err))?;
    if received != file.size {
        return Err(failed(&"the file is incomplete"));
    }
    if hex(&hasher.finalize()) != file.sha256 {
        return Err(failed(&"the file does not match its checksum"));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[tauri::command]
pub fn speech_status(speech: tauri::State<'_, Arc<Speech>>) -> SpeechStatus {
    speech.status()
}

#[tauri::command]
pub async fn speech_download(
    progress: Channel<DownloadProgress>,
    speech: tauri::State<'_, Arc<Speech>>,
) -> Result<SpeechStatus, SpeechCommandError> {
    speech
        .download(|update| {
            let _ = progress.send(update);
        })
        .await?;
    Ok(speech.status())
}

#[tauri::command]
pub async fn speech_prepare(
    speech: tauri::State<'_, Arc<Speech>>,
) -> Result<(), SpeechCommandError> {
    let speech = Arc::clone(speech.inner());
    tauri::async_runtime::spawn_blocking(move || speech.prepare())
        .await
        .map_err(|_| SpeechCommandError::new("Igor's voice could not load"))?
}

#[tauri::command]
pub async fn speak(
    request: SpeakRequest,
    speech: tauri::State<'_, Arc<Speech>>,
) -> Result<Response, SpeechCommandError> {
    let speech = Arc::clone(speech.inner());
    let wav = tauri::async_runtime::spawn_blocking(move || speech.speak(&request))
        .await
        .map_err(|_| SpeechCommandError::new("Igor's voice could not speak"))??;
    Ok(Response::new(wav))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(voice: &str) -> SpeakRequest {
        SpeakRequest {
            text: "Hello.".into(),
            voice: voice.into(),
            speed: 1.0,
        }
    }

    fn empty_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("igor-speech-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn status_lists_every_voice_and_reports_a_missing_model() {
        let status = Speech::new(Some(empty_dir("status"))).status();
        assert!(!status.installed);
        assert_eq!(status.voices.len(), igor_tts::VOICES.len());
        assert_eq!(status.default_voice, "af_heart");
        assert!(status.download_bytes > 300_000_000);
    }

    #[test]
    fn speaking_without_the_model_says_how_to_get_it() {
        let speech = Speech::new(Some(empty_dir("missing")));
        let err = speech.speak(&request("af_heart")).err().map(|e| e.message);
        assert!(err.is_some_and(|message| message.contains("Settings → Voice")));
    }

    #[test]
    fn an_unknown_voice_is_rejected() {
        let speech = Speech::new(Some(empty_dir("unknown")));
        let err = speech
            .speak(&request("Jamie (Premium)"))
            .err()
            .map(|e| e.message);
        assert!(err.is_some_and(|message| message.contains("unknown voice")));
    }

    #[test]
    fn no_home_directory_is_an_error_not_a_panic() {
        let speech = Speech::new(None);
        assert!(!speech.status().installed);
        assert!(speech.speak(&request("af_heart")).is_err());
    }

    #[test]
    fn hex_is_lowercase_and_padded() {
        assert_eq!(hex(&[0x0a, 0xff]), "0aff");
    }
}
