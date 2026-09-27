//! JSON helpers for ingest and stored telemetry payloads.
//!
//! Typed ingest/query structs stay on `serde_json`. Do not build `Value` trees
//! for stored `attributes`: persist and replay them as raw JSON.

use std::fmt::Write as _;

/// Decode a JSON document into a typed value. Callers must not deserialize
/// stored attributes into `serde_json::Value` when the raw object is enough.
pub fn from_slice<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, serde_json::Error> {
    serde_json::from_slice(body)
}

/// Encode a JSON string literal, including quotes.
pub fn write_string(buffer: &mut String, value: &str) {
    buffer.reserve(value.len() + 2);
    buffer.push('"');
    let mut last = 0;
    for (i, b) in value.bytes().enumerate() {
        let escape = match b {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            0x08 => "\\b",
            0x0c => "\\f",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            0x00..=0x1f => {
                buffer.push_str(&value[last..i]);
                let _ = write!(buffer, "\\u{:04x}", b);
                last = i + 1;
                continue;
            }
            _ => continue,
        };
        buffer.push_str(&value[last..i]);
        buffer.push_str(escape);
        last = i + 1;
    }
    buffer.push_str(&value[last..]);
    buffer.push('"');
}

pub fn write_u64(buffer: &mut String, mut value: u64) {
    if value == 0 {
        buffer.push('0');
        return;
    }
    let mut digits = [0_u8; 20];
    let mut index = digits.len();
    while value > 0 {
        index -= 1;
        digits[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    for digit in &digits[index..] {
        buffer.push(char::from(*digit));
    }
}

pub fn write_usize(buffer: &mut String, value: usize) {
    write_u64(buffer, value as u64);
}

/// Encode a numeric JSON array. Empty slices skip serde.
pub fn encode_u64_array(values: &[u64]) -> Result<String, serde_json::Error> {
    encode_numeric_array(values)
}

/// Encode a finite-float JSON array. Empty slices skip serde.
pub fn encode_f64_array(values: &[f64]) -> Result<String, serde_json::Error> {
    encode_numeric_array(values)
}

fn encode_numeric_array<T: serde::Serialize>(values: &[T]) -> Result<String, serde_json::Error> {
    if values.is_empty() {
        return Ok(String::from("[]"));
    }
    serde_json::to_string(values)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{encode_f64_array, encode_u64_array, from_slice, write_string};
    use crate::domain::telemetry::{Batch, EventInput};

    #[test]
    fn compact_integer_arrays_match_serde_json() {
        assert_eq!(encode_u64_array(&[]).unwrap(), "[]");
        assert_eq!(encode_u64_array(&[1]).unwrap(), "[1]");
        assert_eq!(encode_u64_array(&[0, 10, 1_000]).unwrap(), "[0,10,1000]");
    }

    #[test]
    fn compact_float_arrays_match_serde_json() {
        let values = [0.0, 5.0, 10.5];
        assert_eq!(
            encode_f64_array(&values).unwrap(),
            serde_json::to_string(&values).unwrap()
        );
        assert_eq!(encode_f64_array(&[]).unwrap(), "[]");
    }

    #[test]
    fn from_slice_parses_event_batches() {
        let batch: Batch<EventInput> =
            from_slice(br#"{"items":[{"name":"app_startup","attributes":{"k":1}}]}"#).unwrap();
        assert_eq!(batch.items.len(), 1);
        assert_eq!(batch.items[0].name, "app_startup");
        assert!(!batch.items[0].attributes.is_empty());
    }

    #[test]
    fn write_string_escapes_quotes() {
        let mut buffer = String::new();
        write_string(&mut buffer, r#"a"b"#);
        assert_eq!(buffer, r#""a\"b""#);
    }

    #[test]
    fn write_string_handles_unicode_and_controls() {
        let mut buffer = String::new();
        write_string(&mut buffer, "hello 测\t试 \u{0000} \"world\"\\!");
        assert_eq!(buffer, "\"hello 测\\t试 \\u0000 \\\"world\\\"\\\\!\"");

        let mut empty = String::new();
        write_string(&mut empty, "");
        assert_eq!(empty, "\"\"");
    }
}
