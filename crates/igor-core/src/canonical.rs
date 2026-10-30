//! Deterministic canonical JSON for hashing.
//!
//! Object keys are sorted by UTF-16 code units and no whitespace is emitted, following
//! RFC 8785 (JCS). Strings are escaped as JCS requires. Numbers use `serde_json` formatting,
//! which matches JCS for integers but not for every floating-point value; full JCS number
//! formatting is not implemented.
//!
//! This is sufficient because canonical forms and their hashes (tool fingerprints, approval action
//! hashes) are created and compared inside one runtime process. They must not be compared with
//! hashes computed by another implementation.

use serde_json::{Map, Value};

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
        Value::Object(map) => write_canonical_object(out, map),
    }
}

/// Appends the canonical form of a JSON object to `out`.
pub(crate) fn write_canonical_object(out: &mut String, map: &Map<String, Value>) {
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

/// Parses 64 hexadecimal digits into a 32-byte digest.
pub(crate) fn parse_hex32(text: &str) -> Option<[u8; 32]> {
    let digits = text.as_bytes();
    if digits.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    let (pairs, _) = digits.as_chunks::<2>();
    for (byte, &[high, low]) in out.iter_mut().zip(pairs) {
        let value = |d: u8| {
            char::from(d)
                .to_digit(16)
                .and_then(|v| u8::try_from(v).ok())
        };
        *byte = (value(high)? << 4) | value(low)?;
    }
    Some(out)
}

/// Defines a SHA-256 digest newtype shown and serialised as lowercase hex.
macro_rules! sha256_digest_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name([u8; 32]);

        impl $name {
            /// Raw digest bytes.
            pub fn as_bytes(&self) -> &[u8; 32] {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&crate::canonical::to_hex(&self.0))
            }
        }

        impl std::str::FromStr for $name {
            type Err = crate::canonical::InvalidDigest;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                crate::canonical::parse_hex32(text)
                    .map(Self)
                    .ok_or(crate::canonical::InvalidDigest)
            }
        }

        impl TryFrom<String> for $name {
            type Error = crate::canonical::InvalidDigest;

            fn try_from(text: String) -> Result<Self, Self::Error> {
                text.parse()
            }
        }

        impl From<$name> for String {
            fn from(digest: $name) -> Self {
                digest.to_string()
            }
        }
    };
}
pub(crate) use sha256_digest_type;

/// Text is not 64 hexadecimal digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("expected a SHA-256 digest as 64 hexadecimal digits")]
pub struct InvalidDigest;

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

    #[test]
    fn hex32_round_trips_and_rejects_bad_input() {
        let bytes: [u8; 32] = core::array::from_fn(|i| u8::try_from(i * 7).unwrap_or(0));
        assert_eq!(parse_hex32(&to_hex(&bytes)), Some(bytes));
        assert_eq!(parse_hex32(&to_hex(&bytes).to_uppercase()), Some(bytes));
        assert_eq!(parse_hex32("00"), None);
        assert_eq!(parse_hex32(&"g".repeat(64)), None);
    }
}
