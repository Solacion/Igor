//! Translation between provider-neutral types and the Messages API wire format.

use igor_provider_http::errors::malformed;
use serde_json::{Map, Value, json};

use igor_core::provider::{
    ContentBlock, ModelRequest, ModelResponse, ProviderError, Role, StopReason,
};

/// Builds the Messages API request body.
pub(crate) fn request_body(model: &str, request: &ModelRequest) -> Value {
    let messages: Vec<Value> = request
        .messages
        .iter()
        .map(|message| {
            let content: Vec<Value> = message.content.iter().map(content_block).collect();
            json!({ "role": role(message.role), "content": content })
        })
        .collect();

    let mut body = Map::new();
    body.insert("model".into(), model.into());
    body.insert("max_tokens".into(), request.max_tokens.into());
    if !request.system.is_empty() {
        body.insert("system".into(), request.system.clone().into());
    }
    body.insert("messages".into(), messages.into());
    if !request.tools.is_empty() {
        let tools = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.input_schema,
                })
            })
            .collect::<Vec<_>>();
        body.insert("tools".into(), tools.into());
    }
    Value::Object(body)
}

fn role(role: Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

fn content_block(block: &ContentBlock) -> Value {
    match block {
        ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
        ContentBlock::ToolUse { id, name, input } => {
            json!({ "type": "tool_use", "id": id, "name": name, "input": input })
        }
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => json!({
            "type": "tool_result",
            "tool_use_id": tool_use_id,
            "content": content,
            "is_error": is_error,
        }),
    }
}

/// Parses a successful Messages API response body.
pub(crate) fn parse_response(body: &[u8]) -> Result<ModelResponse, ProviderError> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|err| malformed(format!("response is not JSON: {err}")))?;
    let raw_content = value
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| malformed("response has no `content` array".into()))?;
    let stop_reason = value
        .get("stop_reason")
        .and_then(Value::as_str)
        .ok_or_else(|| malformed("response has no `stop_reason`".into()))?;

    let mut content = Vec::with_capacity(raw_content.len());
    for (index, block) in raw_content.iter().enumerate() {
        if let Some(block) = parse_block(block)
            .map_err(|problem| malformed(format!("content[{index}]: {problem}")))?
        {
            content.push(block);
        }
    }

    Ok(ModelResponse {
        content,
        stop_reason: stop_reason_from(stop_reason),
    })
}

/// Parses one content block. `Ok(None)` is a thinking block, which is dropped.
fn parse_block(block: &Value) -> Result<Option<ContentBlock>, String> {
    let string = |field: &str| {
        block
            .get(field)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("missing string field `{field}`"))
    };
    match block.get("type").and_then(Value::as_str) {
        Some("text") => Ok(Some(ContentBlock::Text {
            text: string("text")?,
        })),
        Some("tool_use") => {
            let input = block
                .get("input")
                .filter(|input| input.is_object())
                .cloned()
                .ok_or("tool_use `input` is not a JSON object")?;
            Ok(Some(ContentBlock::ToolUse {
                id: string("id")?,
                name: string("name")?,
                input,
            }))
        }
        Some("thinking" | "redacted_thinking") => Ok(None),
        Some(other) => Err(format!("unsupported content block type `{other}`")),
        None => Err("content block has no `type`".into()),
    }
}

fn stop_reason_from(value: &str) -> StopReason {
    match value {
        "end_turn" => StopReason::EndTurn,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        other => StopReason::Other(other.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use igor_core::provider::ProviderErrorKind;
    use igor_core::provider::{Message, ToolSpec};

    use super::*;

    fn request() -> ModelRequest {
        ModelRequest {
            system: "Be brief.".into(),
            messages: vec![
                Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "Forecast?".into(),
                    }],
                },
                Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::ToolUse {
                        id: "toolu_1".into(),
                        name: "weather__get_forecast".into(),
                        input: json!({ "city": "Cape Town" }),
                    }],
                },
                Message {
                    role: Role::User,
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id: "toolu_1".into(),
                        content: "No forecast for that date".into(),
                        is_error: true,
                    }],
                },
            ],
            tools: vec![ToolSpec {
                name: "weather__get_forecast".into(),
                description: "Forecast".into(),
                input_schema: json!({ "type": "object" }),
            }],
            max_tokens: 1024,
        }
    }

    #[test]
    fn request_translates_every_block_type() {
        let body = request_body("test-model", &request());
        assert_eq!(
            body,
            json!({
                "model": "test-model",
                "max_tokens": 1024,
                "system": "Be brief.",
                "messages": [
                    { "role": "user", "content": [{ "type": "text", "text": "Forecast?" }] },
                    { "role": "assistant", "content": [{
                        "type": "tool_use", "id": "toolu_1", "name": "weather__get_forecast",
                        "input": { "city": "Cape Town" }
                    }] },
                    { "role": "user", "content": [{
                        "type": "tool_result", "tool_use_id": "toolu_1",
                        "content": "No forecast for that date", "is_error": true
                    }] }
                ],
                "tools": [{
                    "name": "weather__get_forecast", "description": "Forecast",
                    "input_schema": { "type": "object" }
                }]
            })
        );
    }

    #[test]
    fn empty_system_and_tools_are_omitted() {
        let mut request = request();
        request.system.clear();
        request.tools.clear();
        let body = request_body("m", &request);
        assert!(body.get("system").is_none());
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn stop_reasons_map_to_neutral_values() {
        let cases = [
            ("end_turn", StopReason::EndTurn),
            ("tool_use", StopReason::ToolUse),
            ("max_tokens", StopReason::MaxTokens),
            ("stop_sequence", StopReason::Other("stop_sequence".into())),
            ("pause_turn", StopReason::Other("pause_turn".into())),
            ("refusal", StopReason::Other("refusal".into())),
        ];
        for (wire, expected) in cases {
            let body = json!({ "content": [], "stop_reason": wire }).to_string();
            let parsed = parse_response(body.as_bytes()).map(|p| p.stop_reason);
            assert_eq!(parsed.ok(), Some(expected), "{wire}");
        }
    }

    #[test]
    fn malformed_responses_are_rejected() {
        let cases = [
            "not json",
            r#"{"stop_reason":"end_turn"}"#,
            r#"{"content":[]}"#,
            r#"{"content":[{"type":"tool_use","id":"t","name":"n","input":"{bad"}],"stop_reason":"tool_use"}"#,
            r#"{"content":[{"type":"tool_use","id":"t","input":{}}],"stop_reason":"tool_use"}"#,
            r#"{"content":[{"type":"image"}],"stop_reason":"end_turn"}"#,
            r#"{"content":[{"text":"x"}],"stop_reason":"end_turn"}"#,
        ];
        for body in cases {
            let err = parse_response(body.as_bytes()).err();
            assert_eq!(
                err.map(|e| (e.kind, e.retryable)),
                Some((ProviderErrorKind::MalformedResponse, false)),
                "{body}"
            );
        }
    }

    #[test]
    fn thinking_blocks_are_dropped() -> Result<(), ProviderError> {
        let body = json!({
            "content": [
                { "type": "thinking", "thinking": "", "signature": "sig" },
                { "type": "tool_use", "id": "t", "name": "n", "input": {} }
            ],
            "stop_reason": "tool_use"
        })
        .to_string();
        let parsed = parse_response(body.as_bytes())?;
        assert_eq!(parsed.content.len(), 1);
        assert!(matches!(parsed.content[0], ContentBlock::ToolUse { .. }));
        Ok(())
    }
}
