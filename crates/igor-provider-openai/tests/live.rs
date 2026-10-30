//! Live check against a local LM Studio server. Never runs in CI.
//!
//! The test is ignored by default and also skips unless `IGOR_LIVE_LMSTUDIO=1` is set.
//!
//! - **Server:** LM Studio's local server, running with a model loaded that supports native tool
//!   calling. `IGOR_LIVE_LMSTUDIO_URL` sets the base URL (default `http://127.0.0.1:1234/v1`).
//! - **Model:** `IGOR_LIVE_LMSTUDIO_MODEL`, the model id that LM Studio shows. No model id is
//!   built into the code.
//! - **Credentials and cost:** none. Everything runs locally.
//! - **Expected result:** the test sends a fixed set of prompts that each need the forecast tool,
//!   and prints the model id and the native tool-call success rate. Record that output, with the
//!   model's quantisation and the hardware, as evidence for the known limitation on weak tool
//!   calling (#37). The test fails only if no prompt produces a valid native tool call.
//!
//! Run:
//!
//! ```sh
//! IGOR_LIVE_LMSTUDIO=1 IGOR_LIVE_LMSTUDIO_MODEL=<model id> \
//!   cargo test -p igor-provider-openai --test live -- --ignored --nocapture
//! ```

use std::error::Error;

use igor_core::CancellationToken;
use igor_core::provider::{ContentBlock, Message, ModelRequest, Provider, Role, ToolSpec};
use igor_provider_openai::{OpenAiCompatibleProvider, OpenAiSettings};
use serde_json::json;

const GATE: &str = "IGOR_LIVE_LMSTUDIO";
const MODEL: &str = "IGOR_LIVE_LMSTUDIO_MODEL";
const URL: &str = "IGOR_LIVE_LMSTUDIO_URL";
const DEFAULT_URL: &str = "http://127.0.0.1:1234/v1";
const TOOL: &str = "weather__get_forecast";

const PROMPTS: [&str; 5] = [
    "What is the forecast for Cape Town on 2026-11-03?",
    "Will it rain in Johannesburg on 2026-11-04?",
    "Get me the weather for Durban on 2026-11-05.",
    "I need the forecast for Pretoria, 2026-11-06, please.",
    "How warm will Stellenbosch be on 2026-11-07?",
];

#[tokio::test]
#[ignore = "live check: needs IGOR_LIVE_LMSTUDIO=1 and a running LM Studio server"]
async fn local_model_makes_native_tool_calls() -> Result<(), Box<dyn Error>> {
    if std::env::var(GATE).as_deref() != Ok("1") {
        eprintln!("skipped: set {GATE}=1 to run the live check");
        return Ok(());
    }
    let model = std::env::var(MODEL).map_err(|_| format!("{MODEL} is not set"))?;
    let url = std::env::var(URL).unwrap_or_else(|_| DEFAULT_URL.to_owned());
    let provider = OpenAiCompatibleProvider::new(OpenAiSettings::new(url, &model))?;

    let mut successes = 0;
    for prompt in PROMPTS {
        let request = ModelRequest {
            system: "Use the tool to answer.".into(),
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: prompt.into(),
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
            max_tokens: 512,
        };
        let outcome = provider.complete(request, CancellationToken::new()).await;
        let ok = outcome.as_ref().is_ok_and(|response| {
            response.content.iter().any(|block| {
                matches!(block, ContentBlock::ToolUse { name, input, .. }
                    if name == TOOL && input.get("location").is_some() && input.get("date").is_some())
            })
        });
        if ok {
            successes += 1;
        }
        let detail = outcome.err().map(|err| err.to_string()).unwrap_or_default();
        eprintln!("{} {prompt} {detail}", if ok { "ok  " } else { "FAIL" });
    }
    eprintln!(
        "model {model}: {successes}/{} prompts produced a valid native tool call",
        PROMPTS.len()
    );
    assert!(successes > 0, "no valid native tool call");
    Ok(())
}
