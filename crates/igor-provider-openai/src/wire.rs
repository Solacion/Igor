//! Translation between provider-neutral types and the chat completions wire format.
//!
//! Tool calls are read only from native `tool_calls`. Text that looks like a tool call stays
//! text: the adapter never parses calls out of message content.

use igor_provider_http::errors::malformed;
use serde_json::{Map, Value, json};

use igor_core::provider::{
    ContentBlock, ModelRequest, ModelResponse, ProviderError, Role, StopReason,
};

/// Prefix added to a failed tool result, because chat completions tool messages have no error
/// flag.
pub const TOOL_ERROR_PREFIX: &str = "[tool error] ";

/// Builds the chat completions request body.
pub(crate) fn request_body(model: &str, request: &ModelRequest) -> Value {
    let mut messages = Vec::new();
    if !request.system.is_empty() {
        messages.push(json!({ "role": "system", "content": request.system }));
    }
    for message in &request.messages {
        match message.role {
            Role::User => push_user(&mut messages, &message.content),
            Role::Assistant => messages.push(assistant(&message.content)),
        }
    }

    let mut body = Map::new();
    body.insert("model".into(), model.into());
    body.insert("max_tokens".into(), request.max_tokens.into());
    body.insert("messages".into(), messages.into());
    if !request.tools.is_empty() {
        let tools = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": parameters(&tool.input_schema),
                    }
                })
            })
            .collect::<Vec<_>>();
        body.insert("tools".into(), tools.into());
    }
    Value::Object(body)
}

/// The tool's input schema as `parameters`. An object schema without `properties`, common for
/// MCP tools that take no arguments, gets an empty `properties`: LM Studio rejects the whole
/// request otherwise. Adding it does not change which arguments the schema accepts.
fn parameters(schema: &Value) -> Value {
    match schema {
        Value::Object(fields)
            if fields.get("type").and_then(Value::as_str) == Some("object")
                && !fields.contains_key("properties") =>
        {
            let mut fields = fields.clone();
            fields.insert("properties".into(), Value::Object(Map::new()));
            Value::Object(fields)
        }
        other => other.clone(),
    }
}

/// Tool results become `tool` messages, which must directly follow the assistant's calls; any
/// text follows as one user message.
fn push_user(messages: &mut Vec<Value>, content: &[ContentBlock]) {
    let mut text = Vec::new();
    for block in content {
        match block {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let content = if *is_error {
                    format!("{TOOL_ERROR_PREFIX}{content}")
                } else {
                    content.clone()
                };
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_use_id,
                    "content": content,
                }));
            }
            ContentBlock::Text { text: part } => text.push(part.as_str()),
            ContentBlock::ToolUse { .. } => {}
        }
    }
    if !text.is_empty() {
        messages.push(json!({ "role": "user", "content": text.join("\n") }));
    }
}

fn assistant(content: &[ContentBlock]) -> Value {
    let text: Vec<&str> = content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let calls: Vec<Value> = content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolUse { id, name, input } => Some(json!({
                "id": id,
                "type": "function",
                "function": { "name": name, "arguments": input.to_string() },
            })),
            _ => None,
        })
        .collect();
    let mut message = Map::new();
    message.insert("role".into(), "assistant".into());
    message.insert(
        "content".into(),
        if text.is_empty() {
            Value::Null
        } else {
            text.join("\n").into()
        },
    );
    if !calls.is_empty() {
        message.insert("tool_calls".into(), calls.into());
    }
    Value::Object(message)
}

/// Parses a successful chat completions response body.
pub(crate) fn parse_response(body: &[u8]) -> Result<ModelResponse, ProviderError> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|err| malformed(format!("response is not JSON: {err}")))?;
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or_else(|| malformed("response has no choices".into()))?;
    let message = choice
        .get("message")
        .ok_or_else(|| malformed("choice has no message".into()))?;

    let mut content = Vec::new();
    if let Some(text) = message
        .get("content")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        content.push(ContentBlock::Text {
            text: text.to_owned(),
        });
    }
    // Absent or null means no tool calls; anything other than an array is malformed.
    let calls = match message.get("tool_calls") {
        None | Some(Value::Null) => &[][..],
        Some(Value::Array(calls)) => calls.as_slice(),
        Some(_) => return Err(malformed("`tool_calls` is not an array".into())),
    };
    for (index, call) in calls.iter().enumerate() {
        let block = tool_call(call)
            .map_err(|problem| malformed(format!("tool_calls[{index}]: {problem}")))?;
        content.push(block);
    }
    if content.is_empty() {
        return Err(malformed("response has neither text nor tool calls".into()));
    }

    let finish_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    Ok(ModelResponse {
        content,
        stop_reason: stop_reason_from(finish_reason),
    })
}

/// Parses one native tool call. `arguments` is a JSON string per the API; an object is also
/// accepted, and an empty string means no arguments. Anything else is malformed.
fn tool_call(call: &Value) -> Result<ContentBlock, String> {
    let id = call
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or("missing `id`")?;
    let function = call.get("function").ok_or("missing `function`")?;
    let name = function
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or("missing `function.name`")?;
    let input = match function.get("arguments") {
        Some(Value::String(text)) if text.trim().is_empty() => Value::Object(Map::new()),
        Some(Value::String(text)) => serde_json::from_str::<Value>(text)
            .map_err(|err| format!("`arguments` is not valid JSON: {err}"))?,
        Some(value @ Value::Object(_)) => value.clone(),
        Some(_) | None => return Err("missing or invalid `arguments`".into()),
    };
    if !input.is_object() {
        return Err("`arguments` is not a JSON object".into());
    }
    Ok(ContentBlock::ToolUse {
        id: id.to_owned(),
        name: name.to_owned(),
        input,
    })
}

fn stop_reason_from(value: &str) -> StopReason {
    match value {
        "stop" => StopReason::EndTurn,
        "tool_calls" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        other => StopReason::Other(other.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use igor_core::provider::ProviderErrorKind;
    use igor_core::provider::{Message, ToolSpec};

    use super::*;

    #[test]
    fn object_schemas_without_properties_get_empty_properties() {
        assert_eq!(
            parameters(&json!({ "type": "object", "additionalProperties": false })),
            json!({ "type": "object", "additionalProperties": false, "properties": {} })
        );
        let with_properties = json!({
            "type": "object",
            "properties": { "date": { "type": "string" } },
            "required": ["date"]
        });
        assert_eq!(parameters(&with_properties), with_properties);
        assert_eq!(parameters(&json!(true)), json!(true));
    }

    #[test]
    fn non_array_tool_calls_are_malformed() {
        let body = json!({
            "choices": [{
                "message": { "role": "assistant", "content": "Hi", "tool_calls": { "id": "x" } },
                "finish_reason": "tool_calls"
            }]
        })
        .to_string();
        let err = parse_response(body.as_bytes()).err();
        assert_eq!(
            err.map(|e| e.kind),
            Some(ProviderErrorKind::MalformedResponse)
        );
    }

    #[test]
    fn request_translates_every_block_type() {
        let request = ModelRequest {
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
                    content: vec![
                        ContentBlock::Text {
                            text: "Checking.".into(),
                        },
                        ContentBlock::ToolUse {
                            id: "call_1".into(),
                            name: "weather__get_forecast".into(),
                            input: json!({ "location": "Cape Town" }),
                        },
                    ],
                },
                Message {
                    role: Role::User,
                    content: vec![
                        ContentBlock::ToolResult {
                            tool_use_id: "call_1".into(),
                            content: "no_forecast".into(),
                            is_error: true,
                        },
                        ContentBlock::Text {
                            text: "And tomorrow?".into(),
                        },
                    ],
                },
            ],
            tools: vec![ToolSpec {
                name: "weather__get_forecast".into(),
                description: "Forecast".into(),
                input_schema: json!({ "type": "object" }),
            }],
            max_tokens: 512,
        };
        assert_eq!(
            request_body("local-model", &request),
            json!({
                "model": "local-model",
                "max_tokens": 512,
                "messages": [
                    { "role": "system", "content": "Be brief." },
                    { "role": "user", "content": "Forecast?" },
                    { "role": "assistant", "content": "Checking.", "tool_calls": [{
                        "id": "call_1", "type": "function",
                        "function": { "name": "weather__get_forecast",
                                      "arguments": "{\"location\":\"Cape Town\"}" }
                    }] },
                    { "role": "tool", "tool_call_id": "call_1",
                      "content": "[tool error] no_forecast" },
                    { "role": "user", "content": "And tomorrow?" }
                ],
                "tools": [{ "type": "function", "function": {
                    "name": "weather__get_forecast", "description": "Forecast",
                    "parameters": { "type": "object", "properties": {} }
                } }]
            })
        );
    }

    #[test]
    fn finish_reasons_map_to_stop_reasons() {
        let cases = [
            ("stop", StopReason::EndTurn),
            ("tool_calls", StopReason::ToolUse),
            ("length", StopReason::MaxTokens),
            ("content_filter", StopReason::Other("content_filter".into())),
        ];
        for (wire, expected) in cases {
            let body = json!({ "choices": [{
                "message": { "role": "assistant", "content": "ok" },
                "finish_reason": wire
            }] })
            .to_string();
            let parsed = parse_response(body.as_bytes()).map(|r| r.stop_reason);
            assert_eq!(parsed.ok(), Some(expected), "{wire}");
        }
    }

    #[test]
    fn arguments_accept_json_string_object_or_empty() -> Result<(), ProviderError> {
        for (arguments, expected) in [
            (json!("{\"a\":1}"), json!({ "a": 1 })),
            (json!({ "a": 1 }), json!({ "a": 1 })),
            (json!(""), json!({})),
        ] {
            let body = json!({ "choices": [{
                "message": { "role": "assistant", "content": null, "tool_calls": [{
                    "id": "c", "type": "function",
                    "function": { "name": "n", "arguments": arguments }
                }] },
                "finish_reason": "tool_calls"
            }] })
            .to_string();
            let response = parse_response(body.as_bytes())?;
            assert!(matches!(
                response.content.first(),
                Some(ContentBlock::ToolUse { input, .. }) if *input == expected
            ));
        }
        Ok(())
    }
}
