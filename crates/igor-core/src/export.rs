//! Redacted activity export: one JSON line per runtime event, for any client that offers an
//! activity export (the CLI's `--export`, the desktop).
//!
//! Fields that carry business data or model text are redacted by name wherever they appear, so
//! event variants added later are covered too. Tool arguments keep their keys; the other fields
//! keep only their length. Identifiers, tool names, outcomes and diagnostic reasons are kept.

use std::io::{self, Write};

use serde_json::{Map, Value};

use crate::events::EventEnvelope;

/// Replacement for a redacted value.
pub const REDACTED: &str = "<redacted>";

/// Objects whose values are redacted, keeping their keys.
const REDACT_VALUES: &[&str] = &["arguments"];

/// Strings replaced by their length.
const REDACT_TEXT: &[&str] = &["canonical_arguments", "text", "question"];

/// `envelope` as JSON with business data and model text redacted.
pub fn redacted(envelope: &EventEnvelope) -> serde_json::Result<Value> {
    let mut value = serde_json::to_value(envelope)?;
    redact(&mut value);
    Ok(value)
}

/// Writes `envelope` as one redacted JSON line.
pub fn write_event(envelope: &EventEnvelope, out: &mut dyn Write) -> io::Result<()> {
    serde_json::to_writer(&mut *out, &redacted(envelope)?)?;
    writeln!(out)
}

fn redact(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for (key, field) in fields.iter_mut() {
                if REDACT_VALUES.contains(&key.as_str()) {
                    *field = redact_values(field);
                } else if REDACT_TEXT.contains(&key.as_str()) {
                    if let Value::String(text) = field {
                        *field = Value::String(format!("{REDACTED} ({} bytes)", text.len()));
                    }
                } else {
                    redact(field);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact),
        _ => {}
    }
}

fn redact_values(value: &Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .keys()
                .map(|key| (key.clone(), Value::String(REDACTED.to_owned())))
                .collect::<Map<_, _>>(),
        ),
        _ => Value::String(REDACTED.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::events::RuntimeEvent;
    use crate::tools::{ServerId, ToolId};

    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn exported(event: RuntimeEvent) -> Result<Value, Box<dyn std::error::Error>> {
        let mut out = Vec::new();
        write_event(&EventEnvelope::new(7, event), &mut out)?;
        let line = String::from_utf8(out)?;
        assert!(line.ends_with('\n') && line.matches('\n').count() == 1);
        Ok(serde_json::from_str(&line)?)
    }

    #[test]
    fn redacts_argument_values_and_keeps_keys() -> TestResult {
        let value = exported(RuntimeEvent::ToolCallProposed {
            call_id: "call-1".to_owned(),
            tool: ToolId {
                server: ServerId::new("sales")?,
                name: "create_draft_order".to_owned(),
            },
            arguments: json!({ "supplier_id": "supplier-x", "lines": [{ "quantity": 12 }] }),
        })?;
        assert_eq!(value["sequence"], json!(7));
        assert_eq!(value["event"]["call_id"], json!("call-1"));
        assert_eq!(
            value["event"]["arguments"],
            json!({ "supplier_id": REDACTED, "lines": REDACTED })
        );
        assert!(!value.to_string().contains("supplier-x"));
        Ok(())
    }

    #[test]
    fn redacts_model_text_to_its_length() -> TestResult {
        let value = exported(RuntimeEvent::AssistantText {
            text: "Order 12 cases".to_owned(),
        })?;
        assert_eq!(value["event"]["text"], json!("<redacted> (14 bytes)"));
        let value = exported(RuntimeEvent::ClarificationRequested {
            request_id: "q1".to_owned(),
            question: "Which supplier?".to_owned(),
        })?;
        assert_eq!(value["event"]["request_id"], json!("q1"));
        assert_eq!(value["event"]["question"], json!("<redacted> (15 bytes)"));
        Ok(())
    }
}
