//! Minimal hand-rolled JSON (parse only what LSP needs) — invariant I6.

use crate::DataTree::DataTree;
use std::io::{self, BufRead};

// JSON parsing returns the canonical ordered DataTree directly.  Duplicate object
// keys remain rejected at this protocol boundary.

pub fn parse_json(text: &str) -> Result<DataTree, ()> {
    parse_json_with_limit(text, MAX_PROTOCOL_MESSAGE_BYTES)
}

/// Parse bounded protocol JSON with a caller-selected message ceiling.
///
/// LSP keeps its historical one-megabyte ceiling. DAP has a separate
/// ratified sixteen-megabyte frame ceiling, so the adapter must not widen the
/// shared default for every protocol consumer.
pub fn parse_json_with_limit(text: &str, limit: usize) -> Result<DataTree, ()> {
    if text.len() > limit {
        return Err(());
    }
    parse_json_detailed(text).map_err(|_| ())
}

fn parse_json_detailed(text: &str) -> Result<DataTree, String> {
    let mut p = JSONParser { s: text, i: 0 };
    let v = p.value(0)?;
    p.skip_ws();
    if p.i < p.s.len() {
        return Err("trailing characters after DataTree value".into());
    }
    Ok(v)
}

/// Parse JSON with the provider-facing error text and bounded tree.
pub fn parse(text: &str) -> Result<DataTree, String> {
    if text.len() > MAX_PROTOCOL_MESSAGE_BYTES {
        return Err("JSON input exceeds the 1 MiB limit".into());
    }
    parse_json_detailed(text)
}


/// Protocol JSON is bounded so hostile LSP/DAP input cannot exhaust the stack.
pub const MAX_JSON_DEPTH: usize = 64;

/// Default maximum decoded protocol message body accepted by `parse`.
/// The native DAP adapter supplies its larger 16 MiB limit explicitly.
pub const MAX_PROTOCOL_MESSAGE_BYTES: usize = 1024 * 1024;

/// Maximum cumulative bytes and field count accepted in an LSP/DAP frame header.
pub const MAX_PROTOCOL_HEADER_BYTES: usize = 8 * 1024;
pub const MAX_PROTOCOL_HEADER_COUNT: usize = 64;

/// Read a bounded LSP/DAP header block and return its `Content-Length`.
/// Each line is capped by the remaining aggregate budget before `read_line`
/// can grow its destination.
pub fn read_protocol_content_length(reader: &mut impl BufRead) -> io::Result<Option<usize>> {
    let mut content_length = None;
    let mut saw_content_length = false;
    let mut total = 0;
    let mut count = 0;
    loop {
        let remaining = MAX_PROTOCOL_HEADER_BYTES.saturating_sub(total);
        let mut line = String::new();
        let read =
            std::io::Read::take(&mut *reader, (remaining + 1) as u64).read_line(&mut line)?;
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
            if saw_content_length {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "protocol frame has duplicate Content-Length headers",
                ));
            }
            saw_content_length = true;
            content_length = rest.trim().parse().ok();
        }
    }
}

struct JSONParser<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> JSONParser<'a> {
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

    fn value(&mut self, depth: usize) -> Result<DataTree, String> {
        self.skip_ws();
        match self.peek() {
            Some('n') => {
                self.expect_literal("null")?;
                Ok(DataTree::Null)
            }
            Some('t') => {
                self.expect_literal("true")?;
                Ok(DataTree::Bool(true))
            }
            Some('f') => {
                self.expect_literal("false")?;
                Ok(DataTree::Bool(false))
            }
            Some('"') => Ok(DataTree::Text(self.string()?)),
            Some('[') => {
                if depth >= MAX_JSON_DEPTH {
                    return Err("JSON exceeds maximum nesting depth".into());
                }
                self.bump();
                let mut arr = Vec::new();
                self.skip_ws();
                if self.peek() == Some(']') {
                    self.bump();
                    return Ok(DataTree::Array(arr));
                }
                loop {
                    arr.push(self.value(depth + 1)?);
                    self.skip_ws();
                    match self.bump() {
                        Some(',') => continue,
                        Some(']') => break,
                        _ => return Err("expected `,` or `]` in array".into()),
                    }
                }
                Ok(DataTree::Array(arr))
            }
            Some('{') => {
                if depth >= MAX_JSON_DEPTH {
                    return Err("JSON exceeds maximum nesting depth".into());
                }
                self.bump();
                let mut obj = Vec::new();
                self.skip_ws();
                if self.peek() == Some('}') {
                    self.bump();
                    return Ok(DataTree::Object(obj));
                }
                loop {
                    self.skip_ws();
                    let key = self.string()?;
                    self.skip_ws();
                    if self.bump() != Some(':') {
                        return Err("expected `:` after object key".into());
                    }
                    let value = self.value(depth + 1)?;
                    if obj.iter().any(|(existing, _)| existing == &key) {
                        return Err(format!("duplicate object key `{key}`"));
                    }
                    obj.push((key, value));
                    self.skip_ws();
                    match self.bump() {
                        Some(',') => continue,
                        Some('}') => break,
                        _ => return Err("expected `,` or `}` in object".into()),
                    }
                }
                Ok(DataTree::Object(obj))
            }
            Some(c) if c == '-' || c.is_ascii_digit() => self.number(),
            Some(c) => Err(format!("unexpected character `{c}`")),
            None => Err("unexpected end of input".into()),
        }
    }

    fn number(&mut self) -> Result<DataTree, String> {
        let start = self.i;
        if self.peek() == Some('-') {
            self.bump();
        }
        match self.peek() {
            Some('0') => {
                self.bump();
                if matches!(self.peek(), Some('0'..='9')) {
                    return Err("leading zero in number".into());
                }
            }
            Some('1'..='9') => {
                while matches!(self.peek(), Some('0'..='9')) {
                    self.bump();
                }
            }
            _ => return Err("invalid number".into()),
        }

        let mut float = false;
        if self.peek() == Some('.') {
            float = true;
            self.bump();
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err("expected digit after decimal point".into());
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
                return Err("expected digit in number exponent".into());
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.bump();
            }
        }

        let raw = &self.s[start..self.i];
        if float {
            let value = raw
                .parse::<f64>()
                .map_err(|_| format!("invalid number `{raw}`"))?;
            if !value.is_finite() {
                return Err(format!("number `{raw}` is out of range"));
            }
            Ok(DataTree::Float(value))
        } else {
            Ok(DataTree::Int(
                raw.parse().map_err(|_| format!("invalid number `{raw}`"))?,
            ))
        }
    }

    fn expect_literal(&mut self, lit: &str) -> Result<(), String> {
        if self.s[self.i..].starts_with(lit) {
            self.i += lit.len();
            Ok(())
        } else {
            Err("invalid literal".into())
        }
    }

    fn string(&mut self) -> Result<String, String> {
        if self.bump() != Some('"') {
            return Err("expected a string key in object".into());
        }
        let mut out = String::new();
        while let Some(c) = self.peek() {
            if c == '"' {
                self.bump();
                return Ok(out);
            }
            if c == '\\' {
                self.bump();
                let esc = self
                    .bump()
                    .ok_or_else(|| "unterminated string".to_string())?;
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
                    _ => return Err("invalid escape in string".into()),
                });
            } else if c <= '\u{001f}' {
                return Err("unescaped control character in string".into());
            } else {
                self.bump();
                out.push(c);
            }
        }
        Err("unterminated string".into())
    }

    /// A `\uXXXX` escape, already past the `u`, combining a high+low surrogate
    /// pair into one code point and rejecting a lone or malformed surrogate.
    fn unicode_escape(&mut self) -> Result<char, String> {
        let cp = self.hex4()?;
        if (0xD800..=0xDBFF).contains(&cp) {
            if self.bump() != Some('\\') || self.bump() != Some('u') {
                return Err("unpaired surrogate in string".into());
            }
            let lo = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&lo) {
                return Err("unpaired surrogate in string".into());
            }
            let combined = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
            char::from_u32(combined).ok_or_else(|| "invalid \\u escape".into())
        } else if (0xDC00..=0xDFFF).contains(&cp) {
            Err("unpaired surrogate in string".into())
        } else {
            char::from_u32(cp).ok_or_else(|| "invalid \\u escape".into())
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let hex: String = self.s[self.i..].chars().take(4).collect();
        if hex.len() != 4 {
            return Err("truncated \\u escape".into());
        }
        self.i += 4;
        u32::from_str_radix(&hex, 16).map_err(|_| "invalid \\u escape".into())
    }
}

pub fn json_get<'a>(v: &'a DataTree, key: &str) -> Option<&'a DataTree> {
    v.get_opt(key)
}

pub fn json_str(v: &DataTree) -> Option<&str> {
    match v {
        DataTree::Text(s) | DataTree::TypedText(s) => Some(s),
        _ => None,
    }
}

pub fn json_int(v: &DataTree) -> Option<i64> {
    match v {
        DataTree::Int(n) => Some(*n),
        DataTree::Float(f) => Some(*f as i64),
        _ => None,
    }
}

pub fn json_u32(v: &DataTree) -> Option<u32> {
    match v {
        DataTree::Int(n) => u32::try_from(*n).ok(),
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

pub fn quote(s: &str) -> String {
    format!("\"{}\"", json_escape(s))
}

pub fn object_of(pairs: &[(&str, &str)]) -> String {
    let mut out = String::from("{\n");
    for (index, (key, value)) in pairs.iter().enumerate() {
        let comma = if index + 1 < pairs.len() { "," } else { "" };
        out.push_str(&format!("  {}: {}{}\n", quote(key), quote(value), comma));
    }
    out.push('}');
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct FilteredJson {
    pub value: DataTree,
    pub noise: Vec<String>,
}

impl FilteredJson {
    pub fn diagnostic(&self, error: impl AsRef<str>) -> String {
        diagnostic_with_noise(error.as_ref(), &self.noise)
    }
}

fn diagnostic_with_noise(error: &str, noise: &[String]) -> String {
    if noise.is_empty() {
        return error.to_string();
    }
    let mut message = format!("{error}\nfiltered provider noise:");
    for line in noise {
        message.push_str("\n  ");
        message.push_str(line);
    }
    message
}

fn is_known_provider_noise(line: &str) -> bool {
    let line = line.trim();
    if line.starts_with("warning:") {
        return true;
    }
    let Some(path) = line
        .strip_prefix('"')
        .and_then(|line| line.strip_suffix("\" has maximum number of links"))
    else {
        return false;
    };
    let Some((store, link)) = path.rsplit_once("/.links/") else {
        return false;
    };
    store.starts_with('/')
        && store.ends_with("/store")
        && !path.contains('"')
        && link.len() == 52
        && link
            .chars()
            .all(|c| "0123456789abcdfghijklmnpqrsvwxyz".contains(c))
}

pub fn parse_lenient(input: &str) -> Result<FilteredJson, String> {
    if input.len() > MAX_PROTOCOL_MESSAGE_BYTES {
        return Err("JSON input exceeds the 1 MiB limit".into());
    }
    let mut filtered = String::with_capacity(input.len());
    let mut noise = Vec::new();
    for line in input.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        if is_known_provider_noise(content) {
            noise.push(content.to_string());
        } else {
            filtered.push_str(line);
        }
    }
    match parse(&filtered) {
        Ok(value) => Ok(FilteredJson { value, noise }),
        Err(error) => Err(diagnostic_with_noise(&error, &noise)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_json_accepts_rfc_escapes_unicode_and_bounded_nesting() {
        let parsed = parse_json(r#"{"text":"caf\u00e9 \ud83d\ude80","n":-1.25e+2}"#);
        assert!(parsed.is_ok());

        let deepest = format!(
            "{}0{}",
            "[".repeat(MAX_JSON_DEPTH),
            "]".repeat(MAX_JSON_DEPTH)
        );
        assert!(parse_json(&deepest).is_ok());
        let too_deep = format!(
            "{}0{}",
            "[".repeat(MAX_JSON_DEPTH + 1),
            "]".repeat(MAX_JSON_DEPTH + 1)
        );
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
    fn lenient_json_rejects_oversized_provider_noise_before_filtering() {
        let input = format!("warning: {}", "x".repeat(MAX_PROTOCOL_MESSAGE_BYTES));
        assert_eq!(
            parse_lenient(&input).unwrap_err(),
            "JSON input exceeds the 1 MiB limit"
        );
    }

    #[test]
    fn protocol_positions_require_nonnegative_integer_u32_values() {
        assert_eq!(json_u32(&DataTree::Int(42)), Some(42));
        assert_eq!(json_u32(&DataTree::Int(-1)), None);
        assert_eq!(json_u32(&DataTree::Float(1.5)), None);
        assert_eq!(json_u32(&DataTree::Int(i64::MAX)), None);
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
        assert_eq!(
            error.to_string(),
            "protocol headers exceed the 64-field limit"
        );
    }

    #[test]
    fn protocol_headers_reject_duplicate_content_length() {
        let frame = "Content-Length: 2\r\nContent-Length: 3\r\n\r\n";
        let error = read_protocol_content_length(&mut std::io::Cursor::new(frame)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "protocol frame has duplicate Content-Length headers"
        );
    }
}
