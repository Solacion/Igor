//! Deterministic canonical JSON for hashing.
//!
//! Object keys are sorted by UTF-16 code units and no whitespace is emitted, following
//! RFC 8785 (JCS). Strings are escaped as JCS requires. Numbers use `serde_json` formatting,
//! which matches JCS for integers but not for every floating-point value; full JCS number
//! formatting is needed before canonical arguments are shown to clients (approval binding, #54).

use serde_json::Value;

/// Appends the canonical form of `value` to `out`.
pub(crate) fn write_canonical(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => write_string(out, s),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
            out.push('{');
            for (i, (key, item)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(out, key);
                out.push(':');
                write_canonical(out, item);
            }
            out.push('}');
        }
    }
}

/// Appends `s` as a JSON string literal with JCS escaping.
pub(crate) fn write_string(out: &mut String, s: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let b = u32::from(c);
                out.push_str("\\u00");
                for nibble in [b >> 4, b & 0xF] {
                    out.push(char::from(HEX[nibble as usize]));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Returns the canonical form of `value`.
#[cfg(test)]
pub(crate) fn to_canonical(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(&mut out, value);
    out
}

/// Formats bytes as lowercase hexadecimal.
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 0xF)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn keys_are_sorted_and_whitespace_removed() {
        let value = json!({ "b": [1, true, null], "a": { "d": "x", "c": 2 } });
        assert_eq!(
            to_canonical(&value),
            r#"{"a":{"c":2,"d":"x"},"b":[1,true,null]}"#
        );
    }

    #[test]
    fn key_order_does_not_change_the_result() -> Result<(), serde_json::Error> {
        let a: Value = serde_json::from_str(r#"{"x":1,"y":{"p":1,"q":2}}"#)?;
        let b: Value = serde_json::from_str(r#"{ "y": {"q":2,"p":1}, "x": 1 }"#)?;
        assert_eq!(to_canonical(&a), to_canonical(&b));
        Ok(())
    }

    #[test]
    fn strings_use_jcs_escapes() {
        let value = json!("q\"b\\\u{08}\u{0C}\n\r\t\u{01}\u{1F}/é€");
        assert_eq!(to_canonical(&value), r#""q\"b\\\b\f\n\r\t\u0001\u001f/é€""#);
    }

    #[test]
    fn keys_sort_by_utf16_code_units() {
        // U+1F600 (surrogate pair D83D..) sorts before U+FB01 in UTF-16, after it in UTF-8.
        let value = json!({ "\u{FB01}": 1, "\u{1F600}": 2 });
        assert_eq!(to_canonical(&value), "{\"\u{1F600}\":2,\"\u{FB01}\":1}");
    }

    #[test]
    fn hex_is_lowercase() {
        assert_eq!(to_hex(&[0x00, 0xAB, 0x0F]), "00ab0f");
    }
}
