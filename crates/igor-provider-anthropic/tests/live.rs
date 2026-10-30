//! Live check against the real Anthropic Messages API. Never runs in CI.
//!
//! The test is ignored by default and also skips unless `IGOR_LIVE_ANTHROPIC=1` is set.
//!
//! - **Credentials:** an API key in `ANTHROPIC_API_KEY`. It is read through the same
//!   environment-variable secret reference the configuration uses, and is never printed.
//! - **Model:** `IGOR_LIVE_ANTHROPIC_MODEL`, for example the demo model chosen in
//!   `docs/specs/decisions.md`. No model id is built into the code.
//! - **Cost:** two requests, each with `max_tokens` = 1024 and a few hundred input tokens. That
//!   is at most about US$0.10 per run at the most expensive current model's prices, and usually
//!   a few cents.
//! - **Expected result:** the first turn calls the `weather__get_forecast` tool with an
//!   object input and stops with `tool_use`. After the tool result is sent back, the second turn
//!   ends with text. This also proves that thinking blocks are sent back as the API requires.
//!
//! Run:
//!
//! ```sh
//! IGOR_LIVE_ANTHROPIC=1 IGOR_LIVE_ANTHROPIC_MODEL=<model id> ANTHROPIC_API_KEY=<key> \
//!   cargo test -p igor-provider-anthropic --test live -- --ignored --nocapture
//! ```

use std::error::Error;

use igor_config::Config;
use igor_core::CancellationToken;
use igor_core::provider::{
    ContentBlock, Message, ModelRequest, Provider, Role, StopReason, ToolSpec,
};
use igor_provider_anthropic::{AnthropicProvider, AnthropicSettings};
use serde_json::json;

const GATE: &str = "IGOR_LIVE_ANTHROPIC";
const MODEL: &str = "IGOR_LIVE_ANTHROPIC_MODEL";
const TOOL: &str = "weather__get_forecast";

#[tokio::test]
#[ignore = "live check: needs IGOR_LIVE_ANTHROPIC=1, a model and an API key; costs money"]
async fn claude_calls_a_tool_and_answers_with_its_result() -> Result<(), Box<dyn Error>> {
    if std::env::var(GATE).as_deref() != Ok("1") {
        eprintln!("skipped: set {GATE}=1 to run the live check");
        return Ok(());
    }
    let model = std::env::var(MODEL).map_err(|_| format!("{MODEL} is not set"))?;
    let config = Config::from_toml_str(
        &format!(
            "[[providers]]\nid = \"claude\"\nkind = \"anthropic\"\nmodel = {}\napi_key = {{ env = \"ANTHROPIC_API_KEY\" }}\n",
            toml_string(&model)
        ),
        std::path::Path::new("."),
    )?;
    let entry = config.provider("claude").ok_or("provider missing")?;
    let key = entry.api_key.as_ref().ok_or("api key missing")?.resolve()?;
    let provider = AnthropicProvider::new(AnthropicSettings::from_config(entry, key)?)?;

    let mut request = ModelRequest {
        system: "Use the tool to answer. Reply in one sentence.".into(),
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "What is the forecast for Cape Town on 2026-11-03?".into(),
            }],
        }],
        tools: vec![ToolSpec {
            name: TOOL.into(),
            description: "Daily weather forecast for a location and date.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "location": { "type": "string" },
                    "date": { "type": "string", "description": "YYYY-MM-DD" }
                },
                "required": ["location", "date"]
            }),
        }],
        max_tokens: 1024,
    };

    let first = provider
        .complete(request.clone(), CancellationToken::new())
        .await?;
    assert_eq!(first.stop_reason, StopReason::ToolUse);
    let tool_use_id = first
        .content
        .iter()
        .find_map(|block| match block {
            ContentBlock::ToolUse { id, name, input } if name == TOOL && input.is_object() => {
                Some(id.clone())
            }
            _ => None,
        })
        .ok_or("no tool call")?;

    request.messages.push(Message {
        role: Role::Assistant,
        content: first.content,
    });
    request.messages.push(Message {
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_use_id,
            content: r#"{"summary": "Sunny", "max_c": 24}"#.into(),
            is_error: false,
        }],
    });
    let second = provider.complete(request, CancellationToken::new()).await?;
    assert_eq!(second.stop_reason, StopReason::EndTurn);
    let answered = second
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::Text { text } if !text.is_empty()));
    assert!(answered, "expected a text answer");
    Ok(())
}

/// Quotes `value` as a TOML basic string.
fn toml_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
