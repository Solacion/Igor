//! Local Whisper input. Loading and inference never block the webview/runtime thread.
use crate::model_download::{self, DownloadProgress, ModelFile, SpeechCommandError, has_size};
use igor_voice::Transcriber;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;

const MODEL: ModelFile<'static> = ModelFile {
    name: igor_voice::DEFAULT_MODEL_FILE,
    url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-large-v3-turbo-q5_0.bin",
    size: 574_041_195,
    sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
};

#[derive(Debug, Serialize)]
pub struct InputStatus {
    installed: bool,
    download_bytes: u64,
}
#[derive(Debug, Serialize)]
pub struct Transcript {
    text: String,
    language: Option<String>,
}

pub struct VoiceInput {
    path: Option<PathBuf>,
    loaded: Mutex<Option<Transcriber>>,
    downloading: AtomicBool,
}
impl VoiceInput {
    pub fn from_env() -> Self {
        Self::new(
            std::env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .map(|home| {
                    PathBuf::from(home)
                        .join("Library/Application Support/igor/models")
                        .join(MODEL.name)
                }),
        )
    }
    fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            loaded: Mutex::new(None),
            downloading: AtomicBool::new(false),
        }
    }
    fn path(&self) -> Result<&std::path::Path, SpeechCommandError> {
        self.path
            .as_deref()
            .ok_or_else(|| SpeechCommandError::new("no home directory for the speech input model"))
    }
    fn status(&self) -> InputStatus {
        InputStatus {
            installed: self
                .path
                .as_deref()
                .is_some_and(|path| has_size(path, MODEL.size)),
            download_bytes: MODEL.size,
        }
    }
    fn with_model<T>(
        &self,
        apply: impl FnOnce(&mut Transcriber) -> Result<T, SpeechCommandError>,
    ) -> Result<T, SpeechCommandError> {
        let mut loaded = self
            .loaded
            .lock()
            .map_err(|_| SpeechCommandError::new("Speech input is unavailable"))?;
        if loaded.is_none() {
            if !self.status().installed {
                return Err(SpeechCommandError::new(
                    "Download speech input in Settings → Voice before using the microphone.",
                ));
            }
            *loaded = Some(
                Transcriber::load(self.path()?)
                    .map_err(|error| SpeechCommandError::new(error.to_string()))?,
            );
        }
        apply(
            loaded
                .as_mut()
                .ok_or_else(|| SpeechCommandError::new("Speech input could not load"))?,
        )
    }
    fn prepare(&self) -> Result<(), SpeechCommandError> {
        self.with_model(|_| Ok(()))
    }
    fn transcribe(
        &self,
        samples: &[f32],
        language: Option<&str>,
    ) -> Result<Transcript, SpeechCommandError> {
        if samples.is_empty()
            || samples.len() > igor_voice::SAMPLE_RATE * igor_voice::MAX_AUDIO_SECONDS
            || samples
                .iter()
                .any(|sample| !sample.is_finite() || sample.abs() > 1.0)
        {
            return Err(SpeechCommandError::new(
                "Expected up to 120 seconds of finite 16 kHz mono samples between -1 and 1.",
            ));
        }
        self.with_model(|model| {
            model
                .set_language(language)
                .map_err(|error| SpeechCommandError::new(error.to_string()))?;
            model
                .transcribe(samples)
                .map(|result| Transcript {
                    text: result.text,
                    language: result.language,
                })
                .map_err(|error| SpeechCommandError::new(error.to_string()))
        })
    }
    async fn download(
        &self,
        progress: Channel<DownloadProgress>,
    ) -> Result<InputStatus, SpeechCommandError> {
        if self.downloading.swap(true, Ordering::AcqRel) {
            return Err(SpeechCommandError::new(
                "Speech input is already downloading",
            ));
        }
        let result = async {
            let path = self.path()?;
            let dir = path
                .parent()
                .ok_or_else(|| SpeechCommandError::new("no speech input model directory"))?;
            tokio::fs::create_dir_all(dir).await.map_err(|_| {
                SpeechCommandError::new("Cannot create the speech input model directory")
            })?;
            if !has_size(path, MODEL.size) {
                model_download::fetch(&model_download::client()?, &MODEL, path, |received| {
                    let _ = progress.send(DownloadProgress {
                        received,
                        total: MODEL.size,
                    });
                })
                .await?;
            }
            Ok(self.status())
        }
        .await;
        self.downloading.store(false, Ordering::Release);
        result
    }
}
#[derive(Serialize)]
pub struct LanguageView {
    code: &'static str,
    name: &'static str,
}
#[tauri::command]
pub fn speech_languages() -> Vec<LanguageView> {
    igor_voice::languages()
        .into_iter()
        .map(|language| LanguageView {
            code: language.code,
            name: language.name,
        })
        .collect()
}

#[tauri::command]
pub fn transcription_status(input: tauri::State<'_, Arc<VoiceInput>>) -> InputStatus {
    input.status()
}
#[tauri::command]
pub async fn transcription_download(
    progress: Channel<DownloadProgress>,
    input: tauri::State<'_, Arc<VoiceInput>>,
) -> Result<InputStatus, SpeechCommandError> {
    input.download(progress).await
}
#[tauri::command]
pub async fn transcription_prepare(
    input: tauri::State<'_, Arc<VoiceInput>>,
) -> Result<(), SpeechCommandError> {
    let input = Arc::clone(input.inner());
    tauri::async_runtime::spawn_blocking(move || input.prepare())
        .await
        .map_err(|_| SpeechCommandError::new("Speech input could not load"))?
}
#[tauri::command]
pub async fn transcribe(
    samples: Vec<f32>,
    language: Option<String>,
    input: tauri::State<'_, Arc<VoiceInput>>,
) -> Result<Transcript, SpeechCommandError> {
    let input = Arc::clone(input.inner());
    tauri::async_runtime::spawn_blocking(move || input.transcribe(&samples, language.as_deref()))
        .await
        .map_err(|_| SpeechCommandError::new("Speech input could not transcribe"))?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn languages_and_transcript_include_core_metadata() -> Result<(), serde_json::Error> {
        assert!(
            speech_languages()
                .iter()
                .any(|language| language.code == "de" && language.name == "german")
        );
        let response = serde_json::to_value(Transcript {
            text: "Guten Tag".into(),
            language: Some("de".into()),
        })?;
        assert_eq!(
            response,
            serde_json::json!({ "text": "Guten Tag", "language": "de" })
        );
        Ok(())
    }
    #[test]
    fn missing_model_explains_how_to_download() {
        let input = VoiceInput::new(None);
        assert!(!input.status().installed);
        assert!(
            input
                .prepare()
                .is_err_and(|error| error.message.contains("Settings → Voice"))
        );
        assert!(
            input
                .transcribe(&[0.1; 160], None)
                .is_err_and(|error| error.message.contains("Settings → Voice"))
        );
    }
    #[test]
    fn rejects_invalid_audio_before_loading() {
        let input = VoiceInput::new(None);
        for samples in [&[][..], &[f32::NAN], &[f32::INFINITY], &[1.1]] {
            assert!(
                input
                    .transcribe(samples, None)
                    .is_err_and(|error| error.message.contains("16 kHz mono"))
            );
        }
    }
}
