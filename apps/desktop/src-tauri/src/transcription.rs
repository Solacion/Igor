//! Local microphone transcription; the model is loaded away from the UI thread.
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::State;

#[derive(Default)]
pub struct SpeechInput(Mutex<Option<igor_voice::Transcriber>>);

fn model_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("IGOR_WHISPER_MODEL") {
        return Ok(path.into());
    }
    let home = std::env::var_os("HOME").ok_or("Could not locate the speech model folder.")?;
    Ok(PathBuf::from(home)
        .join("Library/Application Support/igor/models")
        .join(igor_voice::DEFAULT_MODEL_FILE))
}

#[tauri::command]
pub async fn prepare_transcription(state: State<'_, Arc<SpeechInput>>) -> Result<(), String> {
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let mut model = state
            .0
            .lock()
            .map_err(|_| "Speech recognition is unavailable. Restart Igor.")?;
        if model.is_none() {
            *model = Some(
                igor_voice::Transcriber::load(&model_path()?).map_err(|error| error.to_string())?,
            );
        }
        Ok(())
    })
    .await
    .map_err(|_| "Could not prepare speech recognition. Try again.".to_owned())?
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

#[derive(Serialize)]
pub struct TranscriptView {
    text: String,
    language: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AudioRequest {
    samples: Vec<f32>,
    sample_rate: usize,
    channels: usize,
    language: Option<String>,
}

fn validate(request: &AudioRequest) -> Result<(), String> {
    if request.sample_rate != igor_voice::SAMPLE_RATE
        || request.channels != 1
        || request.samples.is_empty()
        || request.samples.len() > igor_voice::SAMPLE_RATE * 60
        || request
            .samples
            .iter()
            .any(|sample| !sample.is_finite() || sample.abs() > 1.0)
    {
        return Err("Invalid microphone recording.".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn transcribe(
    request: AudioRequest,
    state: State<'_, Arc<SpeechInput>>,
) -> Result<TranscriptView, String> {
    validate(&request)?;
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let mut model = state
            .0
            .lock()
            .map_err(|_| "Speech recognition is unavailable. Restart Igor.")?;
        let model = model
            .as_mut()
            .ok_or("Speech recognition is not ready. Try preparing it again.")?;
        model
            .set_language(request.language.as_deref())
            .map_err(|error| error.to_string())?;
        model
            .transcribe(&request.samples)
            .map(|result| TranscriptView {
                text: result.text,
                language: result.language,
            })
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| "Could not transcribe speech. Try again.".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn language_requests_and_transcript_responses_round_trip() -> Result<(), serde_json::Error> {
        for language in [None, Some("de")] {
            let request: AudioRequest = serde_json::from_value(serde_json::json!({
                "samples": [0.2], "sampleRate": 16000, "channels": 1, "language": language,
            }))?;
            assert_eq!(request.language.as_deref(), language);
        }
        let response = serde_json::to_value(TranscriptView {
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
    fn language_picker_uses_core_codes_and_names() {
        assert!(
            speech_languages()
                .iter()
                .any(|language| language.code == "de" && language.name == "german")
        );
    }

    #[test]
    fn rejects_invalid_recordings() {
        let mut audio = AudioRequest {
            samples: vec![0.2; 1600],
            sample_rate: 16000,
            channels: 1,
            language: None,
        };
        assert!(validate(&audio).is_ok());
        audio.sample_rate = 48000;
        assert!(validate(&audio).is_err());
        audio.sample_rate = 16000;
        audio.samples = vec![f32::NAN];
        assert!(validate(&audio).is_err());
        audio.samples = vec![0.0; 16000 * 60 + 1];
        assert!(validate(&audio).is_err());
    }
}
