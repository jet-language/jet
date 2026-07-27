//! A tiny std-only JSONValue reader (I6: no external crates).
//!
//! Only as much as Jetpack needs to read `nix build --json` output and write
//! its own small state files. Not a general-purpose library — it parses into a
//! `JSONValue` tree and offers typed accessors that return clear errors, the same
//! shape Forge's `nixbridge` used (see forge-salvage.md).

use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum JSONValue {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Array(Vec<JSONValue>),
    Object(BTreeMap<String, JSONValue>),
}

impl JSONValue {
    pub fn as_array(&self) -> Result<&Vec<JSONValue>, String> {
        match self {
            JSONValue::Array(v) => Ok(v),
            _ => Err("expected a JSONValue array".into()),
        }
    }
    pub fn as_object(&self) -> Result<&BTreeMap<String, JSONValue>, String> {
        match self {
            JSONValue::Object(m) => Ok(m),
            _ => Err("expected a JSONValue object".into()),
        }
    }
    pub fn as_str(&self) -> Result<&str, String> {
        match self {
            JSONValue::Str(s) => Ok(s),
            _ => Err("expected a JSONValue string".into()),
        }
    }
    /// Look up a key in an object, erroring if absent.
    pub fn get<'a>(&'a self, key: &str) -> Result<&'a JSONValue, String> {
        self.as_object()?
            .get(key)
            .ok_or_else(|| format!("missing key `{key}`"))
    }
}

// ── parsing ──────────────────────────────────────────────────────────────

pub fn parse(input: &str) -> Result<JSONValue, String> {
    let mut p = Parser {
        chars: input.chars().collect(),
        pos: 0,
    };
    p.skip_ws();
    let v = p.value()?;
    p.skip_ws();
    if p.pos != p.chars.len() {
        return Err("trailing characters after JSONValue value".into());
    }
    Ok(v)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.pos += 1;
        }
    }

    fn value(&mut self) -> Result<JSONValue, String> {
        self.skip_ws();
        match self.peek() {
            Some('{') => self.object(),
            Some('[') => self.array(),
            Some('"') => Ok(JSONValue::Str(self.string()?)),
            Some('t') | Some('f') => self.boolean(),
            Some('n') => self.null(),
            Some(c) if c == '-' || c.is_ascii_digit() => self.number(),
            Some(c) => Err(format!("unexpected character `{c}`")),
            None => Err("unexpected end of input".into()),
        }
    }

    fn object(&mut self) -> Result<JSONValue, String> {
        self.bump(); // {
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some('}') {
            self.bump();
            return Ok(JSONValue::Object(map));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some('"') {
                return Err("expected a string key in object".into());
            }
            let key = self.string()?;
            self.skip_ws();
            if self.bump() != Some(':') {
                return Err("expected `:` after object key".into());
            }
            let val = self.value()?;
            if map.insert(key.clone(), val).is_some() {
                return Err(format!("duplicate object key `{key}`"));
            }
            self.skip_ws();
            match self.bump() {
                Some(',') => continue,
                Some('}') => break,
                _ => return Err("expected `,` or `}` in object".into()),
            }
        }
        Ok(JSONValue::Object(map))
    }

    fn array(&mut self) -> Result<JSONValue, String> {
        self.bump(); // [
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(']') {
            self.bump();
            return Ok(JSONValue::Array(items));
        }
        loop {
            let val = self.value()?;
            items.push(val);
            self.skip_ws();
            match self.bump() {
                Some(',') => continue,
                Some(']') => break,
                _ => return Err("expected `,` or `]` in array".into()),
            }
        }
        Ok(JSONValue::Array(items))
    }

    fn string(&mut self) -> Result<String, String> {
        self.bump(); // opening "
        let mut out = String::new();
        loop {
            match self.bump() {
                Some('"') => return Ok(out),
                Some('\\') => match self.bump() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('/') => out.push('/'),
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some('b') => out.push('\u{08}'),
                    Some('f') => out.push('\u{0C}'),
                    Some('u') => out.push(self.unicode_escape()?),
                    _ => return Err("invalid escape in string".into()),
                },
                Some(c) if c <= '\u{1f}' => {
                    return Err("unescaped control character in string".into())
                }
                Some(c) => out.push(c),
                None => return Err("unterminated string".into()),
            }
        }
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
        let mut code = 0u32;
        for _ in 0..4 {
            let c = self.bump().ok_or("truncated \\u escape")?;
            let d = c.to_digit(16).ok_or("invalid \\u escape")?;
            code = code * 16 + d;
        }
        Ok(code)
    }

    fn number(&mut self) -> Result<JSONValue, String> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        match self.peek() {
            Some('0') => self.pos += 1,
            Some('1'..='9') => {
                self.pos += 1;
                while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    self.pos += 1;
                }
            }
            _ => return Err("invalid number".into()),
        }
        if matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            return Err("leading zero in number".into());
        }
        if self.peek() == Some('.') {
            self.pos += 1;
            if !matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                return Err("expected digit after decimal point".into());
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                return Err("expected digit in number exponent".into());
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        let slice: String = self.chars[start..self.pos].iter().collect();
        let number = slice
            .parse::<f64>()
            .map_err(|_| format!("invalid number `{slice}`"))?;
        if !number.is_finite() {
            return Err(format!("number `{slice}` is out of range"));
        }
        Ok(JSONValue::Num(number))
    }

    fn boolean(&mut self) -> Result<JSONValue, String> {
        if self.literal("true") {
            Ok(JSONValue::Bool(true))
        } else if self.literal("false") {
            Ok(JSONValue::Bool(false))
        } else {
            Err("invalid literal".into())
        }
    }

    fn null(&mut self) -> Result<JSONValue, String> {
        if self.literal("null") {
            Ok(JSONValue::Null)
        } else {
            Err("invalid literal".into())
        }
    }

    fn literal(&mut self, word: &str) -> bool {
        let end = self.pos + word.len();
        if end <= self.chars.len() && self.chars[self.pos..end].iter().collect::<String>() == word {
            self.pos = end;
            true
        } else {
            false
        }
    }
}

/// Strict provider JSONValue plus known noise lines removed before parsing.
#[derive(Debug, Clone, PartialEq)]
pub struct FilteredJson {
    pub value: JSONValue,
    pub noise: Vec<String>,
}

impl FilteredJson {
    /// Add retained provider noise to an error raised while validating the
    /// parsed value's schema.
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
        let _ = write!(message, "\n  {line}");
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
        && link.chars().all(|c| "0123456789abcdfghijklmnpqrsvwxyz".contains(c))
}

/// Filter only known line-oriented Nix noise, then parse the remaining text
/// with [`parse`]. The strict parser requires exactly one complete JSONValue value
/// and whitespace-only EOF. Removed lines are retained for caller diagnostics.
pub fn parse_lenient(input: &str) -> Result<FilteredJson, String> {
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

// ── writing (for Jetpack's own small state files) ────────────────────────

/// Serialize a string as a JSONValue string literal (quotes + escapes).
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if c <= '\u{1f}' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Pretty-print a flat string→string object as JSONValue (used for store metadata).
pub fn object_of(pairs: &[(&str, &str)]) -> String {
    let mut out = String::from("{\n");
    for (i, (k, v)) in pairs.iter().enumerate() {
        let comma = if i + 1 < pairs.len() { "," } else { "" };
        let _ = writeln!(out, "  {}: {}{}", quote(k), quote(v), comma);
    }
    out.push('}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nix_build_shape() {
        let input =
            r#"[{"drvPath":"/nix/store/x.drv","outputs":{"out":"/nix/store/abc-fastfetch-2.0"}}]"#;
        let j = parse(input).unwrap();
        let first = &j.as_array().unwrap()[0];
        let out = first.get("outputs").unwrap().get("out").unwrap();
        assert_eq!(out.as_str().unwrap(), "/nix/store/abc-fastfetch-2.0");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("not json").is_err());
        assert!(parse("").is_err());
        assert!(parse("[1,2").is_err());
        assert!(parse("01").is_err());
        assert!(parse("1.").is_err());
        assert!(parse("1e").is_err());
        assert!(parse("\"raw\nnewline\"").is_err());
        assert!(parse(r#"{"a":1,"a":2}"#).is_err());
        assert!(parse("{}\u{00a0}").is_err());
    }

    #[test]
    fn roundtrips_quote() {
        assert_eq!(quote("a\"b\\c"), "\"a\\\"b\\\\c\"");
        let controls = "\0\u{01}\u{08}\t\n\u{0c}\r\u{1f}";
        assert_eq!(parse(&quote(controls)).unwrap().as_str().unwrap(), controls);
    }

    const LINK_NOISE: &str = "\"/nix/store/.links/1gs2lc42h68lmq8fkcwp96lhnrqcyr3zwmi75k0896nbvc3p4fpc\" has maximum number of links";
    const PAYLOAD: &str =
        r#"[{"drvPath":"/nix/store/x.drv","outputs":{"out":"/nix/store/abc-fastfetch-2.0"}}]"#;

    #[test]
    fn filtered_strict_accepts_noise_before_payload_and_retains_it() {
        let parsed = parse_lenient(&format!("{LINK_NOISE}\n{PAYLOAD}\n")).unwrap();
        assert_eq!(parsed.noise, [LINK_NOISE]);
        let first = &parsed.value.as_array().unwrap()[0];
        let out = first.get("outputs").unwrap().get("out").unwrap();
        assert_eq!(out.as_str().unwrap(), "/nix/store/abc-fastfetch-2.0");
    }

    #[test]
    fn filtered_strict_accepts_noise_between_multiline_payload_lines_and_retains_it() {
        let input = format!(
            "[\n  {{\"drvPath\":\"/nix/store/x.drv\",\nwarning: Nix search path entry was ignored\n  \"outputs\":{{\"out\":\"/nix/store/abc-fastfetch-2.0\"}}}}\n]\n"
        );
        let parsed = parse_lenient(&input).unwrap();
        assert_eq!(parsed.noise, ["warning: Nix search path entry was ignored"]);
        assert_eq!(parsed.value.as_array().unwrap().len(), 1);
    }

    #[test]
    fn filtered_strict_accepts_noise_after_payload_and_retains_it() {
        let parsed = parse_lenient(&format!("{PAYLOAD}\n{LINK_NOISE}\n")).unwrap();
        assert_eq!(parsed.noise, [LINK_NOISE]);
        assert_eq!(parsed.value.as_array().unwrap().len(), 1);
    }

    #[test]
    fn filtered_strict_rejects_duplicate_payloads() {
        assert!(parse_lenient(&format!("{PAYLOAD}\n{PAYLOAD}\n")).is_err());
    }

    #[test]
    fn filtered_strict_rejects_valid_then_forged_payload() {
        assert!(parse_lenient(&format!("{PAYLOAD}\n[]\n")).is_err());
    }

    #[test]
    fn filtered_strict_rejects_malformed_prefix_then_valid_payload() {
        assert!(parse_lenient(&format!("{{malformed\n{PAYLOAD}\n")).is_err());
    }

    #[test]
    fn filtered_strict_does_not_discard_similar_unknown_lines() {
        for unknown in [
            "\"relative/.links/not-nix-noise\" has maximum number of links",
            "\"/evil\\\"/store/.links/1gs2lc42h68lmq8fkcwp96lhnrqcyr3zwmi75k0896nbvc3p4fpc\" has maximum number of links",
        ] {
            assert!(parse_lenient(&format!("{unknown}\n{PAYLOAD}\n")).is_err());
        }
    }

    #[test]
    fn filtered_strict_rejects_garbage_only() {
        assert!(parse_lenient("not json, no payload anywhere").is_err());
    }

    #[test]
    fn filtered_strict_parse_error_retains_removed_noise() {
        let error = parse_lenient(&format!("{LINK_NOISE}\n[1,2\n")).unwrap_err();
        assert!(error.contains("expected `,` or `]` in array"));
        assert!(error.contains(LINK_NOISE));
    }
}
