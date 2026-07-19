//! Minimal hand-rolled JSON (parse only what LSP needs) — invariant I6.

use std::collections::HashMap;
use std::io::{self, BufRead};

#[derive(Debug, Clone)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(i64),
    Flt(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(HashMap<String, JsonValue>),
}

pub fn parse_json(text: &str) -> Result<JsonValue, ()> {
    let mut p = JsonParser { s: text, i: 0 };
    let v = p.value(0)?;
    p.skip_ws();
    if p.i < p.s.len() {
        return Err(());
    }
    Ok(v)
}

/// Protocol JSON is bounded so hostile LSP/DAP input cannot exhaust the stack.
pub const MAX_JSON_DEPTH: usize = 64;

/// Maximum decoded LSP/DAP message body accepted from a `Content-Length` frame.
pub const MAX_PROTOCOL_MESSAGE_BYTES: usize = 1024 * 1024;

/// Maximum cumulative bytes and field count accepted in an LSP/DAP frame header.
pub const MAX_PROTOCOL_HEADER_BYTES: usize = 8 * 1024;
pub const MAX_PROTOCOL_HEADER_COUNT: usize = 64;

/// Read a bounded LSP/DAP header block and return its `Content-Length`.
/// Each line is capped by the remaining aggregate budget before `read_line`
/// can grow its destination.
pub fn read_protocol_content_length(reader: &mut impl BufRead) -> io::Result<Option<usize>> {
    let mut content_length = None;
    let mut total = 0;
    let mut count = 0;
    loop {
        let remaining = MAX_PROTOCOL_HEADER_BYTES.saturating_sub(total);
        let mut line = String::new();
        let read = std::io::Read::take(&mut *reader, (remaining + 1) as u64)
            .read_line(&mut line)?;
        if read == 0 {
            return Ok(None);
        }
        if read > remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "protocol headers exceed the 8192-byte limit",
            ));
        }
        total += read;
        if line == "\r\n" || line == "\n" {
            return Ok(content_length);
        }
        count += 1;
        if count > MAX_PROTOCOL_HEADER_COUNT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "protocol headers exceed the 64-field limit",
            ));
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse().ok();
        }
    }
}

struct JsonParser<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> JsonParser<'a> {
    fn peek(&self) -> Option<char> {
        self.s[self.i..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.i += c.len_utf8();
        Some(c)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.bump();
        }
    }

    fn value(&mut self, depth: usize) -> Result<JsonValue, ()> {
        self.skip_ws();
        match self.peek() {
            Some('n') => {
                self.expect_literal("null")?;
                Ok(JsonValue::Null)
            }
            Some('t') => {
                self.expect_literal("true")?;
                Ok(JsonValue::Bool(true))
            }
            Some('f') => {
                self.expect_literal("false")?;
                Ok(JsonValue::Bool(false))
            }
            Some('"') => Ok(JsonValue::String(self.string()?)),
            Some('[') => {
                if depth >= MAX_JSON_DEPTH {
                    return Err(());
                }
                self.bump();
                let mut arr = Vec::new();
                self.skip_ws();
                if self.peek() == Some(']') {
                    self.bump();
                    return Ok(JsonValue::Array(arr));
                }
                loop {
                    arr.push(self.value(depth + 1)?);
                    self.skip_ws();
                    match self.bump() {
                        Some(',') => continue,
                        Some(']') => break,
                        _ => return Err(()),
                    }
                }
                Ok(JsonValue::Array(arr))
            }
            Some('{') => {
                if depth >= MAX_JSON_DEPTH {
                    return Err(());
                }
                self.bump();
                let mut obj = HashMap::new();
                self.skip_ws();
                if self.peek() == Some('}') {
                    self.bump();
                    return Ok(JsonValue::Object(obj));
                }
                loop {
                    self.skip_ws();
                    let key = self.string()?;
                    self.skip_ws();
                    if self.bump() != Some(':') {
                        return Err(());
                    }
                    let value = self.value(depth + 1)?;
                    if obj.insert(key, value).is_some() {
                        return Err(());
                    }
                    self.skip_ws();
                    match self.bump() {
                        Some(',') => continue,
                        Some('}') => break,
                        _ => return Err(()),
                    }
                }
                Ok(JsonValue::Object(obj))
            }
            Some(c) if c == '-' || c.is_ascii_digit() => self.number(),
            _ => Err(()),
        }
    }

    fn number(&mut self) -> Result<JsonValue, ()> {
        let start = self.i;
        if self.peek() == Some('-') {
            self.bump();
        }
        match self.peek() {
            Some('0') => {
                self.bump();
                if matches!(self.peek(), Some('0'..='9')) {
                    return Err(());
                }
            }
            Some('1'..='9') => {
                while matches!(self.peek(), Some('0'..='9')) {
                    self.bump();
                }
            }
            _ => return Err(()),
        }

        let mut float = false;
        if self.peek() == Some('.') {
            float = true;
            self.bump();
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err(());
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.bump();
            }
        }
        if matches!(self.peek(), Some('e') | Some('E')) {
            float = true;
            self.bump();
            if matches!(self.peek(), Some('+') | Some('-')) {
                self.bump();
            }
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err(());
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.bump();
            }
        }

        let raw = &self.s[start..self.i];
        if float {
            let value = raw.parse::<f64>().map_err(|_| ())?;
            if !value.is_finite() {
                return Err(());
            }
            Ok(JsonValue::Flt(value))
        } else {
            Ok(JsonValue::Number(raw.parse().map_err(|_| ())?))
        }
    }

    fn expect_literal(&mut self, lit: &str) -> Result<(), ()> {
        if self.s[self.i..].starts_with(lit) {
            self.i += lit.len();
            Ok(())
        } else {
            Err(())
        }
    }

    fn string(&mut self) -> Result<String, ()> {
        if self.bump() != Some('"') {
            return Err(());
        }
        let mut out = String::new();
        while let Some(c) = self.peek() {
            if c == '"' {
                self.bump();
                return Ok(out);
            }
            if c == '\\' {
                self.bump();
                let esc = self.bump().ok_or(())?;
                out.push(match esc {
                    '"' => '"',
                    '\\' => '\\',
                    '/' => '/',
                    'b' => '\x08',
                    'f' => '\x0c',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'u' => self.unicode_escape()?,
                    _ => return Err(()),
                });
            } else if c <= '\u{001f}' {
                return Err(());
            } else {
                self.bump();
                out.push(c);
            }
        }
        Err(())
    }

    /// A `\uXXXX` escape, already past the `u`, combining a high+low surrogate
    /// pair into one code point and rejecting a lone or malformed surrogate.
    fn unicode_escape(&mut self) -> Result<char, ()> {
        let cp = self.hex4()?;
        if (0xD800..=0xDBFF).contains(&cp) {
            if self.bump() != Some('\\') || self.bump() != Some('u') {
                return Err(());
            }
            let lo = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&lo) {
                return Err(());
            }
            let combined = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
            char::from_u32(combined).ok_or(())
        } else if (0xDC00..=0xDFFF).contains(&cp) {
            Err(())
        } else {
            char::from_u32(cp).ok_or(())
        }
    }

    fn hex4(&mut self) -> Result<u32, ()> {
        let hex: String = self.s[self.i..].chars().take(4).collect();
        if hex.len() != 4 {
            return Err(());
        }
        self.i += 4;
        u32::from_str_radix(&hex, 16).map_err(|_| ())
    }
}

pub fn json_get<'a>(v: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    match v {
        JsonValue::Object(m) => m.get(key),
        _ => None,
    }
}

pub fn json_str(v: &JsonValue) -> Option<&str> {
    match v {
        JsonValue::String(s) => Some(s),
        _ => None,
    }
}

pub fn json_int(v: &JsonValue) -> Option<i64> {
    match v {
        JsonValue::Number(n) => Some(*n),
        JsonValue::Flt(f) => Some(*f as i64),
        _ => None,
    }
}

pub fn json_u32(v: &JsonValue) -> Option<u32> {
    match v {
        JsonValue::Number(n) => u32::try_from(*n).ok(),
        _ => None,
    }
}

pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_json_accepts_rfc_escapes_unicode_and_bounded_nesting() {
        let parsed = parse_json(r#"{"text":"caf\u00e9 \ud83d\ude80","n":-1.25e+2}"#);
        assert!(parsed.is_ok());

        let deepest = format!("{}0{}", "[".repeat(MAX_JSON_DEPTH), "]".repeat(MAX_JSON_DEPTH));
        assert!(parse_json(&deepest).is_ok());
        let too_deep = format!("{}0{}", "[".repeat(MAX_JSON_DEPTH + 1), "]".repeat(MAX_JSON_DEPTH + 1));
        assert!(parse_json(&too_deep).is_err());
    }

    #[test]
    fn protocol_json_rejects_ambiguous_or_malformed_input_without_panic() {
        for raw in [
            r#"{"a":1,"a":2}"#,
            "{\"text\":\"line\nfeed\"}",
            "01",
            "-",
            "1.",
            "1e",
            "1e9999",
            r#""\ud800""#,
            r#"{"nested":{"method":"fake"}"#,
        ] {
            assert!(parse_json(raw).is_err(), "accepted hostile JSON: {raw:?}");
        }
    }

    #[test]
    fn protocol_positions_require_nonnegative_integer_u32_values() {
        assert_eq!(json_u32(&JsonValue::Number(42)), Some(42));
        assert_eq!(json_u32(&JsonValue::Number(-1)), None);
        assert_eq!(json_u32(&JsonValue::Flt(1.5)), None);
        assert_eq!(json_u32(&JsonValue::Number(i64::MAX)), None);
    }

    #[test]
    fn protocol_headers_reject_an_overlong_line_with_bounded_growth() {
        let frame = format!("X-Fill: {}\r\n", "x".repeat(MAX_PROTOCOL_HEADER_BYTES));
        let error = read_protocol_content_length(&mut std::io::Cursor::new(frame)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "protocol headers exceed the 8192-byte limit"
        );
    }

    #[test]
    fn protocol_headers_reject_too_many_fields() {
        let frame = format!("{}\r\n", "X: y\r\n".repeat(MAX_PROTOCOL_HEADER_COUNT + 1));
        let error = read_protocol_content_length(&mut std::io::Cursor::new(frame)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "protocol headers exceed the 64-field limit");
    }
}
