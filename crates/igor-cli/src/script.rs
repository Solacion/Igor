//! Scripted model turns for offline runs.
//!
//! A script is a JSON object with a `turns` array. Each turn has optional `text` and optional
//! `tool_calls`, each `{ "id", "name", "input" }`, where `name` is the host alias such as
//! `sales__get_stock_levels`. A turn with tool calls stops with `tool_use`, otherwise `end_turn`.
//! A turn may instead be `{ "error": "message" }`, which fails that model request.

use std::path::Path;

use igor_core::provider::{
    ContentBlock, ModelResponse, ProviderError, ProviderErrorKind, StopReason,
};
use igor_core::testing::{ScriptedProvider, ScriptedTurn};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScriptFile {
    turns: Vec<TurnFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TurnFile {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCallFile>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolCallFile {
    id: String,
    name: String,
    #[serde(default)]
    input: Value,
}

/// A script file could not be used.
#[derive(Debug, Error)]
pub enum ScriptError {
    /// The file could not be read.
    #[error("cannot read script {path}: {source}")]
    Read {
        /// Path read.
        path: String,
        /// I/O error.
        source: std::io::Error,
    },
    /// The file is not a valid script.
    #[error("invalid script {path}: {message}")]
    Invalid {
        /// Path parsed.
        path: String,
        /// What is wrong.
        message: String,
    },
}

/// Loads a script file into a scripted provider.
pub fn load(path: &Path) -> Result<ScriptedProvider, ScriptError> {
    let display = path.display().to_string();
    let text = std::fs::read_to_string(path).map_err(|source| ScriptError::Read {
        path: display.clone(),
        source,
    })?;
    parse(&text).map_err(|message| ScriptError::Invalid {
        path: display,
        message,
    })
}

/// Parses script JSON into a scripted provider.
pub fn parse(text: &str) -> Result<ScriptedProvider, String> {
    let file: ScriptFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let turns = file
        .turns
        .into_iter()
        .enumerate()
        .map(|(index, turn)| turn_from_file(index, turn))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ScriptedProvider::new(turns))
}

fn turn_from_file(index: usize, turn: TurnFile) -> Result<ScriptedTurn, String> {
    if let Some(message) = turn.error {
        if turn.text.is_some() || !turn.tool_calls.is_empty() {
            return Err(format!(
                "turns[{index}]: `error` cannot be combined with other fields"
            ));
        }
        return Ok(ScriptedTurn::fail(ProviderError {
            kind: ProviderErrorKind::Unavailable,
            message,
            retryable: false,
        }));
    }
    let stop_reason = if turn.tool_calls.is_empty() {
        StopReason::EndTurn
    } else {
        StopReason::ToolUse
    };
    let mut content: Vec<ContentBlock> = turn
        .text
        .into_iter()
        .map(|text| ContentBlock::Text { text })
        .collect();
    content.extend(
        turn.tool_calls
            .into_iter()
            .map(|call| ContentBlock::ToolUse {
                id: call.id,
                name: call.name,
                input: call.input,
            }),
    );
    if content.is_empty() {
        return Err(format!("turns[{index}] needs text, tool_calls or error"));
    }
    Ok(ScriptedTurn::respond(ModelResponse {
        content,
        stop_reason,
    }))
}

#[cfg(test)]
mod tests {
    use igor_core::CancellationToken;
    use igor_core::provider::{ModelRequest, Provider};

    use super::*;

    fn request() -> ModelRequest {
        ModelRequest {
            system: String::new(),
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: 10,
        }
    }

    #[tokio::test]
    async fn replays_tool_calls_then_text() -> Result<(), Box<dyn std::error::Error>> {
        let provider = parse(
            r#"{ "turns": [
                { "tool_calls": [{ "id": "u1", "name": "sales__lookup", "input": { "a": 1 } }] },
                { "text": "done" }
            ] }"#,
        )?;
        let first = provider
            .complete(request(), CancellationToken::new())
            .await?;
        assert_eq!(first.stop_reason, StopReason::ToolUse);
        let second = provider
            .complete(request(), CancellationToken::new())
            .await?;
        assert_eq!(second.stop_reason, StopReason::EndTurn);
        provider.verify()?;
        Ok(())
    }

    #[test]
    fn rejects_invalid_scripts() {
        for bad in [
            r#"{ "turns": [{}] }"#,
            r#"{ "turns": [{ "error": "x", "text": "y" }] }"#,
            r#"{ "turns": [{ "txt": "typo" }] }"#,
            r#"{ "steps": [] }"#,
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
