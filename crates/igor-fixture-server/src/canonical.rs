//! Canonical JSON text used to match recorded tool-call arguments.
//!
//! Follows RFC 8785 (JSON Canonicalization Scheme) for the cases fixtures use: object members are
//! sorted by the UTF-16 code units of their keys, no whitespace is emitted, strings use the
//! minimal JSON escapes, and numbers are compared by value, so `1`, `1.0` and `1e0` are equal.
//! Integral numbers up to 2^53 in magnitude are written without a fraction. Other non-integral
//! numbers use the shortest round-trip form, which can differ from RFC 8785 in exponent
//! notation; fixtures do not depend on that difference.

use std::fmt::Write;

use serde_json::{Map, Number, Value};

/// Largest magnitude at which every integer is exactly representable as an `f64` (2^53).
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_992.0;

/// Returns the canonical JSON text of `value`.
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

/// Returns the canonical JSON text of a JSON object, such as tool-call arguments.
pub fn canonical_object(map: &Map<String, Value>) -> String {
    let mut out = String::new();
    write_object(&mut out, map);
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => write_number(out, n),
        Value::String(s) => write_string(out, s),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => write_object(out, map),
    }
}

fn write_object(out: &mut String, map: &Map<String, Value>) {
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
    out.push('{');
    for (i, (key, item)) in entries.into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_string(out, key);
        out.push(':');
        write_value(out, item);
    }
    out.push('}');
}

// `write!` into a `String` cannot fail, so its `fmt::Result` is ignored here and below.
fn write_number(out: &mut String, n: &Number) {
    if let Some(i) = n.as_i64() {
        let _ = write!(out, "{i}");
    } else if let Some(u) = n.as_u64() {
        let _ = write!(out, "{u}");
    } else if let Some(f) = n.as_f64() {
        if f.fract() == 0.0 && f.abs() <= MAX_SAFE_INTEGER {
            // Exact: the value is integral and within the range where f64 holds every integer.
            let i = f as i64;
            let _ = write!(out, "{i}");
        } else {
            let _ = write!(out, "{f}");
        }
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn sorts_keys_and_removes_whitespace() {
        let value = json!({ "b": 1, "a": { "d": [1, 2], "c": null } });
        assert_eq!(
            canonical_json(&value),
            r#"{"a":{"c":null,"d":[1,2]},"b":1}"#
        );
    }

    #[test]
    fn compares_numbers_by_value() -> Result<(), serde_json::Error> {
        let integer: Value = serde_json::from_str(r#"{"q": 12}"#)?;
        let float: Value = serde_json::from_str(r#"{"q": 12.0}"#)?;
        let exponent: Value = serde_json::from_str(r#"{"q": 1.2e1}"#)?;
        assert_eq!(canonical_json(&integer), canonical_json(&float));
        assert_eq!(canonical_json(&integer), canonical_json(&exponent));
        assert_eq!(canonical_json(&json!(0.5)), "0.5");
        assert_eq!(canonical_json(&json!(-0.0)), "0");
        Ok(())
    }

    #[test]
    fn escapes_strings_minimally() {
        let value = json!("a\"b\\c\n\u{1}é/");
        assert_eq!(canonical_json(&value), "\"a\\\"b\\\\c\\n\\u0001é/\"");
    }

    #[test]
    fn sorts_keys_by_utf16_code_units() {
        // U+1F600 (surrogate 0xD83D) sorts before U+E000 in UTF-16, but after it in UTF-8.
        let value = json!({ "\u{E000}": 2, "\u{1F600}": 1 });
        assert_eq!(canonical_json(&value), "{\"\u{1F600}\":1,\"\u{E000}\":2}");
    }
}
